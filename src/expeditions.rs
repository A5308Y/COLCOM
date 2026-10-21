//! Expeditions: squads sent off the map on missions, told mostly in text.
//!
//! Three neighbouring factions (`Faction`) have goodwill towards the colony
//! (-100..100). It sets trade prices and how they take the colony's envoys;
//! the bandits' goodwill also spaces out raids (`raid_spacing`), and friends
//! now and then send a gift.
//!
//! Missions are offered (`Offer`, a few at a time, each for some days). The
//! player picks colonists and packs goods (`World::send_expedition`); the
//! colonists leave the map (kept in `Expedition::members`) for the journey
//! there, an event on the way (some missions), the mission's main event, and
//! the journey home. Each event offers choices (`Choice`) with effects:
//! goods given or gained, goodwill, delays, injuries, someone joining, a
//! trading session, or a fight. A fight is played on a small site map of its
//! own (`Expedition::site`, a whole `World`): the front end swaps it in, the
//! normal battle runs there, and `finish_site` brings the squad (wounds and
//! all) and whatever lies on the site back into the expedition.

use crate::battle::Parley;
use crate::defs::ItemId;
use crate::health::{DamageKind, Part};
use crate::map::{Block, Feature, Map, Pos, Stack};
use crate::pawn::{Pawn, PawnId};
use crate::rng::Rng;
use crate::world::World;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

/// Most mission offers open at once, and how long one stays open (days).
const MAX_OFFERS: usize = 4;
const OFFER_DAYS: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum FactionKind {
    /// Merchants: the best trade.
    Traders,
    /// Farmers and smiths behind palisades.
    Settlers,
    /// Raiders: they're who raids the colony.
    Bandits,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Faction {
    pub name: String,
    pub kind: FactionKind,
    /// -100 (at war) .. 100 (allies).
    pub goodwill: i32,
    /// Days' travel to their home, one way.
    pub days: f32,
}

impl Faction {
    pub fn standing(&self) -> &'static str {
        match self.goodwill {
            g if g >= 50 => "allied",
            g if g >= 15 => "friendly",
            g if g > -15 => "neutral",
            g if g > -50 => "wary",
            _ => "hostile",
        }
    }

    /// Share of an item's value they pay for it.
    pub fn sell_factor(&self) -> f32 {
        let base = if self.kind == FactionKind::Traders { 0.6 } else { 0.5 };
        base + self.goodwill as f32 * 0.002
    }

    /// Share of an item's value they ask for it.
    pub fn buy_factor(&self) -> f32 {
        let base = if self.kind == FactionKind::Traders { 1.35 } else { 1.5 };
        base - self.goodwill as f32 * 0.002
    }
}

/// The colony's neighbours, for a new game (or an old save without them).
pub fn starting_factions() -> Vec<Faction> {
    vec![
        Faction { name: "Riverford Traders".into(), kind: FactionKind::Traders, goodwill: 20, days: 2.0 },
        Faction { name: "Iron Pact".into(), kind: FactionKind::Settlers, goodwill: 0, days: 3.0 },
        Faction { name: "Ash Raiders".into(), kind: FactionKind::Bandits, goodwill: -40, days: 2.0 },
    ]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MissionKind {
    Trade,
    Envoy,
    BanditCamp,
    Ruins,
    Rescue,
    Hunt,
    Prospect,
    Escort,
    PeaceTalks,
    Signal,
}

impl MissionKind {
    pub const ALL: [MissionKind; 10] = [
        MissionKind::Trade,
        MissionKind::Envoy,
        MissionKind::BanditCamp,
        MissionKind::Ruins,
        MissionKind::Rescue,
        MissionKind::Hunt,
        MissionKind::Prospect,
        MissionKind::Escort,
        MissionKind::PeaceTalks,
        MissionKind::Signal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MissionKind::Trade => "Trade caravan",
            MissionKind::Envoy => "Envoy",
            MissionKind::BanditCamp => "Bandit camp",
            MissionKind::Ruins => "Salvage ruins",
            MissionKind::Rescue => "Rescue",
            MissionKind::Hunt => "Hunting trip",
            MissionKind::Prospect => "Prospecting",
            MissionKind::Escort => "Escort duty",
            MissionKind::PeaceTalks => "Peace talks",
            MissionKind::Signal => "Strange signal",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            MissionKind::Trade => "Take goods to their market and trade. Pack what you want to sell.",
            MissionKind::Envoy => "Visit their leaders to win their favour. Gifts help: pack some.",
            MissionKind::BanditCamp => "A raider camp nearby: clear it out, or scare them off. Expect a fight.",
            MissionKind::Ruins => "Old ruins to pick through for components and scrap.",
            MissionKind::Rescue => "Someone is held captive. Free them, and they may join you. Pack a ransom, or come armed.",
            MissionKind::Hunt => "Game has been seen in the hills: bring meat home.",
            MissionKind::Prospect => "Scouts report ore in the hills. Bring some back.",
            MissionKind::Escort => "Guard one of their caravans on the road, for pay and goodwill.",
            MissionKind::PeaceTalks => "Talk to the raiders. Pack tribute if you want them to leave you be.",
            MissionKind::Signal => "Something fell from the sky and is signalling. Whatever it is may be worth having.",
        }
    }

    /// What counts when picking who goes.
    pub fn squad_hint(self) -> &'static str {
        match self {
            MissionKind::Trade => "Bandits may block the road: armed colonists can push through.",
            MissionKind::Envoy => "Nobody needs to fight; one or two envoys will do.",
            MissionKind::BanditCamp | MissionKind::PeaceTalks => {
                "Four or more fit colonists can face the raiders down without a fight; fights take weapons, skill and armour."
            }
            MissionKind::Rescue | MissionKind::Escort => "Expect a fight: weapons, skill and armour count.",
            MissionKind::Ruins | MissionKind::Hunt | MissionKind::Prospect => {
                "Some choices risk injuries to everyone who goes: send only as many as you can spare."
            }
            MissionKind::Signal => "Whatever's at the pod may fight back: bring weapons.",
        }
    }

    /// Goods are worth packing (to sell, give or pay).
    pub fn packs(self) -> bool {
        matches!(self, MissionKind::Trade | MissionKind::Envoy | MissionKind::Rescue | MissionKind::PeaceTalks)
    }

    /// How dangerous it may get, for the offer.
    pub fn risk(self) -> &'static str {
        match self {
            MissionKind::Trade | MissionKind::Envoy | MissionKind::Prospect => "low",
            MissionKind::Hunt | MissionKind::Ruins | MissionKind::PeaceTalks | MissionKind::Signal => "some",
            MissionKind::Rescue | MissionKind::Escort => "a fight is likely",
            MissionKind::BanditCamp => "a fight",
        }
    }

    /// Who would offer it.
    fn fits(self, kind: FactionKind) -> bool {
        match self {
            MissionKind::Trade | MissionKind::Envoy | MissionKind::Escort => kind != FactionKind::Bandits,
            MissionKind::BanditCamp | MissionKind::Rescue | MissionKind::PeaceTalks => kind == FactionKind::Bandits,
            MissionKind::Ruins | MissionKind::Hunt | MissionKind::Prospect | MissionKind::Signal => true,
        }
    }
}

macro_rules! world_id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Identifies ", $what, ". Numbered from `World::next_id` like pawns, but a")]
        /// type of its own so the different ids can't be mixed up.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(std::num::NonZeroU32);

        impl $name {
            /// Ids are handed out by the world; build one directly only in tests.
            pub fn new(raw: u32) -> $name {
                $name(std::num::NonZeroU32::new(raw).expect("ids start at 1"))
            }
        }
    };
}

world_id!(OfferId, "an expedition on offer");
world_id!(ExpeditionId, "an expedition under way");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Offer {
    pub id: OfferId,
    pub kind: MissionKind,
    /// Who it's with or about (index into `World::factions`).
    pub faction: usize,
    /// Days' travel there (and as long back).
    pub days: f32,
    /// Game time it's no longer open.
    pub until: f32,
}

/// What a choice does (all of a choice's effects happen).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    /// Goodwill of the mission's faction changes.
    Goodwill(i32),
    /// Goods gained (brought home).
    Gain(Vec<Stack>),
    /// Goods given out of what was packed (the choice needs them).
    Give(Vec<Stack>),
    /// Everything packed is given (gifts): goodwill by its worth.
    GiveAll,
    /// The journey takes this many days longer.
    Delay(f32),
    /// Each member is hurt with this chance.
    Injure(f32),
    /// Someone joins the colony on the way home.
    Recruit,
    /// A trading session at the faction's market.
    Trade,
    /// The raiders keep away for this many days.
    Truce(f32),
    /// A fight on a site map against this many enemies; `then` the effects of winning.
    Fight { enemies: usize, site: Site, then: Vec<Effect> },
    /// With chance `p` the first effects, else the second.
    Chance { p: f32, yes: Vec<Effect>, no: Vec<Effect> },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Site {
    Road,
    Camp,
    Crash,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub label: String,
    pub effects: Vec<Effect>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub text: String,
    pub choices: Vec<Choice>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Stage {
    /// On the way there; `event` met when the timer runs out (else the main one).
    Out { until: f32, waylaid: bool },
    /// Waiting for the player's choice.
    Decide { event: Event, main: bool },
    /// At the market; `stock` is what they have to sell.
    Trading { stock: Vec<Stack> },
    /// A fight on a site map; `then` if they win. `site` is the site's world,
    /// except while the front end has taken it out to play.
    Fighting { then: Vec<Effect>, main: bool, site: Option<SiteWorld> },
    /// On the way home.
    Back { until: f32 },
}

/// The world of a fight site (its own small map, the squad and the enemy).
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SiteWorld(pub Box<World>);

impl std::fmt::Debug for SiteWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SiteWorld({}x{}, {} pawns)", self.0.map.w, self.0.map.h, self.0.pawns.len())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Expedition {
    pub id: ExpeditionId,
    pub kind: MissionKind,
    pub faction: usize,
    pub days: f32,
    /// The colonists away (taken off the map).
    pub members: Vec<Pawn>,
    /// What they carry: packed goods, and what they get.
    pub cargo: Vec<Stack>,
    pub stage: Stage,
    /// What happened, for the player.
    pub journal: Vec<String>,
    /// Someone to bring home.
    pub recruit: bool,
}

/// Add `s` to a list of stacks (one entry per kind).
fn add_stack(list: &mut Vec<Stack>, s: Stack) {
    if s.count == 0 {
        return;
    }
    match list.iter_mut().find(|o| o.kind == s.kind) {
        Some(o) => o.count += s.count,
        None => list.push(s),
    }
}

/// Take `s` out of a list; false (and nothing taken) if there isn't that much.
fn take_stack(list: &mut Vec<Stack>, s: Stack) -> bool {
    let Some(k) = list.iter().position(|o| o.kind == s.kind && o.count >= s.count) else { return false };
    list[k].count -= s.count;
    if list[k].count == 0 {
        list.remove(k);
    }
    true
}

fn item(id: &str) -> ItemId {
    ItemId::named(id)
}

fn st(id: &str, count: u32) -> Stack {
    Stack { kind: item(id), count }
}

fn stacks_text(goods: &[Stack]) -> String {
    goods.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect::<Vec<_>>().join(", ")
}

/// How many things colonists `members` can carry between them.
pub fn carry_capacity<'a>(members: impl IntoIterator<Item = &'a Pawn>) -> u32 {
    members.into_iter().map(|p| p.carry()).sum()
}

fn worth(goods: &[Stack]) -> f32 {
    goods.iter().map(|s| s.count as f32 * s.kind.def().value).sum()
}

impl Expedition {
    pub fn worth(&self) -> f32 {
        worth(&self.cargo)
    }

    /// Whether the squad can afford a choice (it gives goods they carry),
    /// whichever way its chances and fights turn out.
    pub fn affords(&self, c: &Choice) -> bool {
        self.affords_all(&c.effects)
    }

    fn affords_all(&self, effects: &[Effect]) -> bool {
        effects.iter().all(|e| match e {
            Effect::Give(goods) => goods.iter().all(|g| self.cargo.iter().any(|o| o.kind == g.kind && o.count >= g.count)),
            Effect::GiveAll => !self.cargo.is_empty(),
            Effect::Chance { yes, no, .. } => self.affords_all(yes) && self.affords_all(no),
            Effect::Fight { then, .. } => self.affords_all(then),
            Effect::Goodwill(_)
            | Effect::Gain(_)
            | Effect::Delay(_)
            | Effect::Injure(_)
            | Effect::Recruit
            | Effect::Trade
            | Effect::Truce(_) => true,
        })
    }

    /// How many things they can carry: `CARRY_EACH` each, more with backpacks
    /// (the downed are carried, not carrying).
    pub fn capacity(&self) -> u32 {
        self.members.iter().filter(|p| p.active()).map(|p| p.carry()).sum()
    }

    /// How many things they carry now.
    pub fn load(&self) -> u32 {
        self.cargo.iter().map(|s| s.count).sum()
    }

    /// Add `goods` to the cargo as far as there's room; returns what's left behind.
    fn take_in(&mut self, goods: impl IntoIterator<Item = Stack>) -> Vec<Stack> {
        let mut left = Vec::new();
        for s in goods {
            let room = self.capacity().saturating_sub(self.load());
            add_stack(&mut self.cargo, Stack { kind: s.kind, count: s.count.min(room) });
            add_stack(&mut left, Stack { kind: s.kind, count: s.count.saturating_sub(room) });
        }
        left
    }

    /// Write down what had to be left behind for lack of room, if anything.
    fn note_left(&mut self, left: &[Stack]) {
        if !left.is_empty() {
            self.journal.push(format!("No room to carry {} (backpacks carry more): left behind.", stacks_text(left)));
        }
    }

    /// Able to fight: members not downed or dead.
    pub fn fit(&self) -> usize {
        self.members.iter().filter(|p| p.active()).count()
    }
}

impl World {
    /// Once a second: offers come and go, expeditions travel, friends send gifts.
    pub(crate) fn update_expeditions(&mut self) {
        if self.factions.is_empty() {
            self.factions = starting_factions();
        }
        let now = self.time();
        self.offers.retain(|o| o.until > now);
        // About one new offer a day.
        if self.offers.len() < MAX_OFFERS && self.rng.chance(1.0 / DAY_LEN) {
            self.new_offer();
        }
        for k in 0..self.expeditions.len() {
            match self.expeditions[k].stage {
                Stage::Out { until, waylaid } if now >= until => {
                    let event = if waylaid { self.main_event(k) } else { self.waylay_event(k) };
                    let main = waylaid || event.is_none();
                    let event = event.or_else(|| self.main_event(k));
                    let e = &mut self.expeditions[k];
                    match event {
                        Some(event) => {
                            e.journal.push(event.text.clone());
                            e.stage = Stage::Decide { event, main };
                        }
                        None => e.stage = Stage::Back { until: now + e.days * DAY_LEN },
                    }
                    let name = e.kind.name();
                    self.push_log(format!("Expedition ({}): a decision is waiting.", name));
                }
                _ => {}
            }
        }
        // Home again.
        let back: Vec<usize> =
            (0..self.expeditions.len()).filter(|&k| matches!(self.expeditions[k].stage, Stage::Back { until } if now >= until)).collect();
        for k in back.into_iter().rev() {
            self.come_home(k);
        }
        // Friends remember you now and then (about once in four days each).
        for f in 0..self.factions.len() {
            if self.factions[f].goodwill >= 50 && self.rng.chance(1.0 / (4.0 * DAY_LEN)) {
                let gift = [st("meal", 10), st("medicine", 3), st("steel", 20), st("cloth", 15)][self.rng.below(4)];
                if let Some(at) = self.edge_spot() {
                    self.map.drop_item(at, gift);
                    let msg = format!("The {} sent a gift: {} {}.", self.factions[f].name, gift.count, gift.kind.name().to_lowercase());
                    self.push_log(msg);
                }
            }
        }
    }

    /// How much longer the gaps between raids are: friendlier raiders raid less.
    pub fn raid_spacing(&self) -> f32 {
        let g = self.factions.iter().find(|f| f.kind == FactionKind::Bandits).map_or(-40, |f| f.goodwill);
        (1.0 + (g + 40) as f32 / 100.0).clamp(0.7, 2.0)
    }

    fn new_offer(&mut self) {
        let f = self.rng.below(self.factions.len());
        let kinds: Vec<MissionKind> = MissionKind::ALL.into_iter().filter(|k| k.fits(self.factions[f].kind)).collect();
        let kind = kinds[self.rng.below(kinds.len())];
        self.add_offer(kind, f);
    }

    /// An offer of `kind` now (any kind at random if `None`), from a faction
    /// that would make it, past the usual limit (for testing). `None` if no
    /// faction would.
    pub fn offer_now(&mut self, kind: Option<MissionKind>) -> Option<OfferId> {
        if self.factions.is_empty() {
            self.factions = starting_factions();
        }
        let kind = kind.unwrap_or_else(|| MissionKind::ALL[self.rng.below(MissionKind::ALL.len())]);
        let fits: Vec<usize> = (0..self.factions.len()).filter(|&f| kind.fits(self.factions[f].kind)).collect();
        if fits.is_empty() {
            return None;
        }
        let f = fits[self.rng.below(fits.len())];
        Some(self.add_offer(kind, f))
    }

    /// Expedition `id` reaches the end of its current leg on the next update,
    /// on the road there or home (for testing). False if it isn't travelling.
    pub fn skip_travel(&mut self, id: ExpeditionId) -> bool {
        let now = self.time();
        let Some(e) = self.expeditions.iter_mut().find(|e| e.id == id) else { return false };
        match &mut e.stage {
            Stage::Out { until, .. } | Stage::Back { until } => {
                *until = until.min(now);
                true
            }
            Stage::Decide { .. } | Stage::Trading { .. } | Stage::Fighting { .. } => false,
        }
    }

    fn add_offer(&mut self, kind: MissionKind, f: usize) -> OfferId {
        let days = match kind {
            MissionKind::Trade | MissionKind::Envoy | MissionKind::Escort | MissionKind::PeaceTalks => self.factions[f].days,
            MissionKind::BanditCamp
            | MissionKind::Ruins
            | MissionKind::Rescue
            | MissionKind::Hunt
            | MissionKind::Prospect
            | MissionKind::Signal => self.rng.range_f32(1.0, 2.5),
        };
        let id = OfferId::new(self.take_id());
        self.offers.push(Offer { id, kind, faction: f, days, until: self.time() + OFFER_DAYS * DAY_LEN });
        self.push_log(format!("New expedition: {} ({}).", kind.name(), self.factions[f].name));
        id
    }

    /// Send colonists `ids` on offer `offer`, packing `goods` (taken from the colony).
    pub fn send_expedition(&mut self, offer: OfferId, ids: &[PawnId], goods: &[Stack]) -> Result<(), String> {
        let k = self.offers.iter().position(|o| o.id == offer).ok_or("that offer is gone")?;
        if ids.is_empty() {
            return Err("pick someone to go".into());
        }
        if self.battle.is_some() || self.parley.is_some() {
            return Err("not during a raid".into());
        }
        for &id in ids {
            let p = self.pawn(id).ok_or("no such colonist")?;
            if !p.is_living_colonist() || !p.active() {
                return Err(format!("{} can't go", p.name));
            }
        }
        let have = self.item_totals();
        if goods.iter().any(|g| have[g.kind.index()] < g.count) {
            return Err("not that much in the colony".into());
        }
        let can = carry_capacity(ids.iter().filter_map(|&id| self.pawn(id)));
        let packed: u32 = goods.iter().map(|g| g.count).sum();
        if packed > can {
            return Err(format!("they can carry {} things between them, not {} (backpacks carry more)", can, packed));
        }
        let o = self.offers.remove(k);
        self.take_goods(goods);
        let mut members = Vec::new();
        for &id in ids {
            let i = self.pawn_index(id).unwrap();
            let mut p = self.pawns.remove(i);
            self.end_job(&mut p);
            self.release(id);
            p.path.clear();
            members.push(p);
        }
        let names: Vec<&str> = members.iter().map(|p| p.name.as_str()).collect();
        let msg = format!("{} set out: {} ({}), {:.0} days away.", names.join(", "), o.kind.name(), self.factions[o.faction].name, o.days);
        self.push_log(msg.clone());
        let id = ExpeditionId::new(self.take_id());
        // Halfway there: something on the road (some missions).
        let until = self.time() + o.days * DAY_LEN * 0.5;
        self.expeditions.push(Expedition {
            id,
            kind: o.kind,
            faction: o.faction,
            days: o.days,
            members,
            cargo: goods.to_vec(),
            stage: Stage::Out { until, waylaid: false },
            journal: vec![msg],
            recruit: false,
        });
        Ok(())
    }

    /// Something on the road (None: an easy journey).
    fn waylay_event(&mut self, k: usize) -> Option<Event> {
        let e = &self.expeditions[k];
        let value = e.worth();
        match e.kind {
            MissionKind::Trade => (value > 0.0).then(|| Event {
                text: "A barricade across the road: armed men want a toll of a tenth of your goods to let you pass.".into(),
                choices: vec![
                    Choice { label: "Pay the toll".into(), effects: vec![Effect::Give(self.share_of_cargo(k, 0.1))] },
                    Choice { label: "Take the long way round (a day longer)".into(), effects: vec![Effect::Delay(1.0)] },
                    Choice {
                        label: "Refuse, and push through".into(),
                        effects: vec![Effect::Fight { enemies: 3, site: Site::Road, then: vec![Effect::Gain(vec![st("steel", 10)])] }],
                    },
                ],
            }),
            MissionKind::Escort => Some(Event {
                text: "Raiders burst from the trees and fall on the caravan you're guarding.".into(),
                choices: vec![
                    Choice {
                        label: "Defend the caravan".into(),
                        effects: vec![Effect::Fight {
                            enemies: 4,
                            site: Site::Road,
                            then: vec![Effect::Goodwill(12), Effect::Gain(vec![st("steel", 25), st("medicine", 3)])],
                        }],
                    },
                    Choice { label: "Abandon the cargo and run".into(), effects: vec![Effect::Goodwill(-15)] },
                ],
            }),
            MissionKind::Envoy
            | MissionKind::BanditCamp
            | MissionKind::Ruins
            | MissionKind::Rescue
            | MissionKind::Hunt
            | MissionKind::Prospect
            | MissionKind::PeaceTalks
            | MissionKind::Signal => None,
        }
    }

    /// What the mission comes down to.
    fn main_event(&mut self, k: usize) -> Option<Event> {
        let e = &self.expeditions[k];
        let fit = e.fit();
        let f = &self.factions[e.faction];
        let name = f.name.clone();
        let event = match e.kind {
            MissionKind::Trade => Event {
                text: format!("The market of the {} is busy. Their traders look over your goods.", name),
                choices: vec![Choice { label: "Trade".into(), effects: vec![Effect::Trade] }],
            },
            MissionKind::Envoy => Event {
                text: format!("The leaders of the {} receive your envoys in their hall.", name),
                choices: vec![
                    Choice { label: "Present the gifts you brought".into(), effects: vec![Effect::GiveAll] },
                    Choice { label: "Praise their leader and their people".into(), effects: vec![Effect::Chance { p: 0.6, yes: vec![Effect::Goodwill(10)], no: vec![Effect::Goodwill(2)] }] },
                    Choice {
                        label: "Ask for an alliance".into(),
                        effects: if f.goodwill >= 20 { vec![Effect::Goodwill(20)] } else { vec![Effect::Goodwill(-10)] },
                    },
                ],
            },
            MissionKind::BanditCamp => Event {
                text: "The raiders' camp: a palisade, a few tents, a fire. Nobody has seen you yet.".into(),
                choices: vec![
                    Choice {
                        label: "Storm the camp".into(),
                        effects: vec![Effect::Fight {
                            enemies: 5,
                            site: Site::Camp,
                            then: vec![Effect::Goodwill(-10), Effect::Truce(3.0), Effect::Gain(vec![st("steel", 30), st("mechanical_components", 3), st("medicine", 4)])],
                        }],
                    },
                    Choice {
                        label: "Sneak in at night and take their stores".into(),
                        effects: vec![Effect::Chance {
                            p: 0.5,
                            yes: vec![Effect::Gain(vec![st("steel", 15), st("meal", 10)])],
                            no: vec![Effect::Fight { enemies: 3, site: Site::Camp, then: vec![Effect::Gain(vec![st("steel", 15)])] }],
                        }],
                    },
                    Choice {
                        label: "Show yourselves and tell them to leave".into(),
                        effects: if fit >= 4 {
                            vec![Effect::Goodwill(-5), Effect::Truce(4.0), Effect::Gain(vec![st("steel", 10)])]
                        } else {
                            vec![Effect::Fight { enemies: 5, site: Site::Camp, then: vec![Effect::Truce(3.0)] }]
                        },
                    },
                ],
            },
            MissionKind::Ruins => Event {
                text: "The ruins are bigger than expected. One wing has half collapsed; the rest looks sound.".into(),
                choices: vec![
                    Choice {
                        label: "Search the collapsed wing".into(),
                        effects: vec![
                            Effect::Injure(0.35),
                            Effect::Gain(vec![st("electrical_components", 4), st("optical_components", 2), st("scrap_metal", 20)]),
                        ],
                    },
                    Choice { label: "Search the safe rooms only".into(), effects: vec![Effect::Gain(vec![st("scrap_metal", 12), st("e_waste", 6)])] },
                ],
            },
            MissionKind::Rescue => Event {
                text: "A traveller is tied up in a raider camp. Their guards want goods for them, or blood.".into(),
                choices: vec![
                    Choice {
                        label: "Fight for them".into(),
                        effects: vec![Effect::Fight { enemies: 3, site: Site::Camp, then: vec![Effect::Recruit] }],
                    },
                    Choice { label: "Pay a ransom (worth about 150)".into(), effects: vec![Effect::Give(self.share_of_value(k, 150.0)), Effect::Recruit] },
                    Choice { label: "Leave them".into(), effects: vec![] },
                ],
            },
            MissionKind::Hunt => Event {
                text: "Tracks everywhere: a great elk's, and plenty of small game.".into(),
                choices: vec![
                    Choice { label: "Track the great elk".into(), effects: vec![Effect::Injure(0.3), Effect::Gain(vec![st("raw_food", 45)])] },
                    Choice { label: "Hunt small game".into(), effects: vec![Effect::Gain(vec![st("raw_food", 18)])] },
                ],
            },
            MissionKind::Prospect => Event {
                text: "There's copper here, and the scouts think richer veins lie deeper down.".into(),
                choices: vec![
                    Choice { label: "Dig deep (a day longer)".into(), effects: vec![Effect::Delay(1.0), Effect::Injure(0.15), Effect::Gain(vec![st("copper_ore", 40), st("iron_ore", 20)])] },
                    Choice { label: "Take what lies near the surface".into(), effects: vec![Effect::Gain(vec![st("copper_ore", 15)])] },
                ],
            },
            MissionKind::Escort => Event {
                text: format!("The caravan reaches the {} safely. The merchants pay what they promised.", name),
                choices: vec![Choice { label: "Take the pay and head home".into(), effects: vec![Effect::Goodwill(6), Effect::Gain(vec![st("steel", 15), st("cloth", 10)])] }],
            },
            MissionKind::PeaceTalks => Event {
                text: "The raiders' chiefs listen, hands on their weapons.".into(),
                choices: vec![
                    Choice { label: "Offer tribute (worth about 200)".into(), effects: vec![Effect::Give(self.share_of_value(k, 200.0)), Effect::Goodwill(25), Effect::Truce(5.0)] },
                    Choice {
                        label: "Show them what you're made of".into(),
                        effects: if fit >= 4 {
                            vec![Effect::Goodwill(10), Effect::Truce(2.0)]
                        } else {
                            vec![Effect::Fight { enemies: 4, site: Site::Camp, then: vec![Effect::Goodwill(5)] }]
                        },
                    },
                    Choice { label: "Walk away".into(), effects: vec![] },
                ],
            },
            MissionKind::Signal => Event {
                text: "A pod lies in a smoking crater, its hatch sealed, a light blinking. Footprints lead to it.".into(),
                choices: vec![
                    Choice {
                        label: "Open the pod".into(),
                        effects: vec![Effect::Chance {
                            p: 0.6,
                            yes: vec![Effect::Gain(vec![st("electrical_components", 4), st("optical_components", 3), st("medicine", 3)])],
                            no: vec![Effect::Fight {
                                enemies: 3,
                                site: Site::Crash,
                                then: vec![Effect::Gain(vec![st("electrical_components", 4), st("optical_components", 3)])],
                            }],
                        }],
                    },
                    Choice { label: "Strip the hull and leave the hatch shut".into(), effects: vec![Effect::Gain(vec![st("scrap_metal", 15), st("structural_components", 2)])] },
                ],
            },
        };
        Some(event)
    }

    /// About `share` of what expedition `k` carries (by worth), to hand over.
    fn share_of_cargo(&self, k: usize, share: f32) -> Vec<Stack> {
        self.share_of_value(k, self.expeditions[k].worth() * share)
    }

    /// Goods out of expedition `k`'s cargo worth at least `value` (all of it if not enough).
    fn share_of_value(&self, k: usize, value: f32) -> Vec<Stack> {
        let mut left = value;
        let mut out = Vec::new();
        for s in &self.expeditions[k].cargo {
            if left <= 0.0 {
                break;
            }
            let each = s.kind.def().value.max(0.01);
            let n = ((left / each).ceil() as u32).min(s.count);
            if n > 0 {
                out.push(Stack { kind: s.kind, count: n });
                left -= n as f32 * each;
            }
        }
        if left > 0.0 {
            // Not enough: the choice can't be made.
            return vec![Stack { kind: item("steel"), count: u32::MAX / 2 }];
        }
        out
    }

    /// The player picks choice `c` for the event expedition `id` is waiting on.
    pub fn choose(&mut self, id: ExpeditionId, c: usize) -> Result<(), String> {
        let k = self.expeditions.iter().position(|e| e.id == id).ok_or("no such expedition")?;
        let Stage::Decide { event, main } = self.expeditions[k].stage.clone() else { return Err("nothing to decide".into()) };
        let choice = event.choices.get(c).ok_or("no such choice")?.clone();
        if !self.expeditions[k].affords(&choice) {
            return Err("you don't carry enough for that".into());
        }
        self.expeditions[k].journal.push(format!("> {}", choice.label));
        self.apply(k, &choice.effects, main);
        Ok(())
    }

    /// Carry out `effects` for expedition `k`; then on with the journey (unless a fight or trade starts).
    fn apply(&mut self, k: usize, effects: &[Effect], main: bool) {
        let mut delay = 0.0;
        for e in effects {
            let fname = self.factions[self.expeditions[k].faction].name.clone();
            match e {
                Effect::Goodwill(g) => {
                    let f = self.expeditions[k].faction;
                    self.factions[f].goodwill = (self.factions[f].goodwill + g).clamp(-100, 100);
                    let what = if *g >= 0 { "improved" } else { "soured" };
                    self.expeditions[k].journal.push(format!("Relations with the {} {} ({:+}).", fname, what, g));
                }
                Effect::Gain(goods) => {
                    let e = &mut self.expeditions[k];
                    e.journal.push(format!("Gained {}.", stacks_text(goods)));
                    let left = e.take_in(goods.iter().copied());
                    e.note_left(&left);
                }
                Effect::Give(goods) => {
                    for &s in goods {
                        take_stack(&mut self.expeditions[k].cargo, s);
                    }
                    let text: Vec<String> = goods.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect();
                    self.expeditions[k].journal.push(format!("Handed over {}.", text.join(", ")));
                }
                Effect::GiveAll => {
                    let value = self.expeditions[k].worth();
                    self.expeditions[k].cargo.clear();
                    let g = ((value / 15.0) as i32).clamp(1, 30);
                    let f = self.expeditions[k].faction;
                    self.factions[f].goodwill = (self.factions[f].goodwill + g).clamp(-100, 100);
                    self.expeditions[k].journal.push(format!("The gifts (worth {:.0}) pleased the {} ({:+}).", value, fname, g));
                }
                Effect::Delay(days) => {
                    delay += days;
                    self.expeditions[k].journal.push(format!("{:.0} more day(s) on the road.", days));
                }
                Effect::Injure(p) => {
                    let parts = [Part::LeftArm, Part::RightArm, Part::LeftLeg, Part::RightLeg, Part::Torso];
                    for m in 0..self.expeditions[k].members.len() {
                        if self.rng.chance(*p) {
                            let part = parts[self.rng.below(parts.len())];
                            let amount = self.rng.range_f32(4.0, 12.0);
                            let e = &mut self.expeditions[k];
                            e.members[m].health.injure(part, amount, DamageKind::Bruise, &mut self.rng);
                            e.members[m].health.recompute();
                            let msg = format!("{} was hurt ({}).", e.members[m].name, part.name().to_lowercase());
                            e.journal.push(msg);
                        }
                    }
                }
                Effect::Recruit => {
                    self.expeditions[k].recruit = true;
                    self.expeditions[k].journal.push("Someone new travels with them: they want to join the colony.".into());
                }
                Effect::Truce(days) => {
                    self.story.next_raid = self.story.next_raid.max(self.time() + days * DAY_LEN);
                    self.expeditions[k].journal.push(format!("The raiders will keep away for {:.0} days.", days));
                }
                Effect::Trade => {
                    let stock = self.market_stock(self.expeditions[k].faction);
                    self.expeditions[k].stage = Stage::Trading { stock };
                    return;
                }
                Effect::Fight { enemies, site, then } => {
                    let site_world = self.start_site_fight(k, *enemies, *site);
                    self.expeditions[k].stage = Stage::Fighting { then: then.clone(), main, site: Some(site_world) };
                    return;
                }
                Effect::Chance { p, yes, no } => {
                    let pick = if self.rng.chance(*p) { yes } else { no };
                    let pick = pick.clone();
                    self.apply(k, &pick, main);
                    if !matches!(self.expeditions[k].stage, Stage::Decide { .. }) {
                        return;
                    }
                }
            }
        }
        self.carry_on(k, main, delay);
    }

    /// After an event: on to the destination, or home.
    fn carry_on(&mut self, k: usize, main: bool, delay: f32) {
        let now = self.time();
        let e = &mut self.expeditions[k];
        e.stage = if main {
            Stage::Back { until: now + (e.days + delay) * DAY_LEN }
        } else {
            Stage::Out { until: now + (e.days * 0.5 + delay) * DAY_LEN, waylaid: true }
        };
    }

    /// What a faction's market has to sell.
    fn market_stock(&mut self, f: usize) -> Vec<Stack> {
        let mut stock = vec![st("meal", 20), st("medicine", 6), st("cloth", 30), st("steel", 40)];
        match self.factions[f].kind {
            FactionKind::Traders => stock.extend([
                st("electrical_components", 4),
                st("mechanical_components", 4),
                st("copper", 20),
                st("parka", 2),
                st("rifle", 1),
            ]),
            FactionKind::Settlers => stock.extend([st("raw_food", 60), st("wood", 80), st("stone_blocks", 40), st("steel_armor", 1)]),
            FactionKind::Bandits => stock.extend([st("pistol", 2), st("club", 2), st("scrap_metal", 20)]),
        }
        stock
    }

    /// Trade at the market expedition `id` is at: `sell` from what they carry,
    /// `buy` from the market's stock. What's sold must be worth at least what's bought.
    pub fn trade(&mut self, id: ExpeditionId, sell: &[Stack], buy: &[Stack]) -> Result<(), String> {
        let k = self.expeditions.iter().position(|e| e.id == id).ok_or("no such expedition")?;
        let f = &self.factions[self.expeditions[k].faction];
        let (sf, bf) = (f.sell_factor(), f.buy_factor());
        let Stage::Trading { stock } = &self.expeditions[k].stage else { return Err("not at a market".into()) };
        let paid = worth(sell) * sf;
        let cost = worth(buy) * bf;
        if paid + 0.01 < cost {
            return Err(format!("they want {:.0} more", cost - paid));
        }
        let mut stock = stock.clone();
        let mut cargo = self.expeditions[k].cargo.clone();
        for &s in sell {
            if !take_stack(&mut cargo, s) {
                return Err("you don't carry that much".into());
            }
            add_stack(&mut stock, s);
        }
        for &s in buy {
            if !take_stack(&mut stock, s) {
                return Err("they don't have that much".into());
            }
            add_stack(&mut cargo, s);
        }
        let load: u32 = cargo.iter().map(|s| s.count).sum();
        let can = self.expeditions[k].capacity();
        if load > can && load > self.expeditions[k].load() {
            return Err(format!("they can carry {} things, not {} (backpacks carry more)", can, load));
        }
        let e = &mut self.expeditions[k];
        e.cargo = cargo;
        e.stage = Stage::Trading { stock };
        let text = |l: &[Stack]| l.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect::<Vec<_>>().join(", ");
        e.journal.push(format!("Traded {} for {}.", if sell.is_empty() { "nothing".into() } else { text(sell) }, if buy.is_empty() { "nothing".into() } else { text(buy) }));
        Ok(())
    }

    /// Done at the market: home.
    pub fn leave_market(&mut self, id: ExpeditionId) {
        if let Some(k) = self.expeditions.iter().position(|e| e.id == id && matches!(e.stage, Stage::Trading { .. })) {
            let f = self.expeditions[k].faction;
            self.factions[f].goodwill = (self.factions[f].goodwill + 2).min(100);
            self.carry_on(k, true, 0.0);
        }
    }

    /// Build the site of a fight for expedition `k`: a small map, the squad on
    /// the west side, `enemies` raiders coming from the east.
    fn start_site_fight(&mut self, k: usize, enemies: usize, site: Site) -> SiteWorld {
        let seed = self.rng.next_u64();
        let mut rng = Rng::new(seed);
        let (w, h) = (34, 24);
        let mut map = Map::flat(w, h);
        let z = crate::map::FLAT_Z;
        // Cover to fight from: rocks and trees, or the camp's palisade, or the wreck.
        for _ in 0..40 {
            let (x, y) = (2 + rng.below(w - 4), 1 + rng.below(h - 2));
            let i = map.idx((x, y, z));
            match site {
                Site::Road if rng.chance(0.5) => map.set_feature(i, Feature::plant("tree")),
                Site::Crash if rng.chance(0.3) => map.set_feature(i, Feature::built("wall")),
                _ => map.block[i] = if rng.chance(0.4) { Block::Rock } else { Block::Air },
            }
        }
        if site == Site::Camp {
            for y in 6..18 {
                if y % 4 != 0 {
                    let i = map.idx((24, y, z));
                    map.set_feature(i, Feature::built("fence"));
                }
            }
        }
        // Clear where both sides come in.
        for y in 0..h {
            for x in (0..4).chain(w - 3..w) {
                let i = map.idx((x, y, z));
                map.block[i] = Block::Air;
                map.feature[i] = Feature::None;
            }
        }
        map.recompute_rooms();
        let mut site_world = World::with_map(map, rng);
        site_world.story.next_raid = f32::MAX;
        site_world.story.next_event = f32::MAX;
        site_world.regrow_timer = f32::MAX;
        site_world.share_clock(self);
        site_world.factions = Vec::new();
        // The site carries on the colony's numbering; `finish_site` moves the colony's
        // counter past whatever the site used, so ids stay unique either way.
        site_world.next_id = self.next_id;
        let members = std::mem::take(&mut self.expeditions[k].members);
        let squad = members.len();
        for (n, mut p) in members.into_iter().enumerate() {
            let at = (1 + n % 2, h / 2 - 2 + n / 2, z);
            p.pos = at;
            (p.x, p.y, p.z) = (at.0 as f32, at.1 as f32, at.2 as f32);
            p.job = None;
            p.path.clear();
            site_world.pawns.push(p);
        }
        let squads = crate::battle::raider_squads(enemies, &mut site_world.rng);
        // They fight as well equipped as the raiders the colony faces.
        let tier = self.raid_tier();
        site_world.start_battle(Parley { squads, entry: (w - 2, h / 2, z), demand: Vec::new(), tier });
        self.expeditions[k].journal.push(format!("A fight: {} against {}.", squad, enemies));
        SiteWorld(Box::new(site_world))
    }

    /// The fight at expedition `id`'s site is over (`site` is the site world
    /// after the battle): the squad comes back into the expedition, with
    /// whatever lies about if they won.
    pub fn finish_site(&mut self, id: ExpeditionId, mut site: World) {
        self.next_id = self.next_id.max(site.next_id);
        let Some(k) = self.expeditions.iter().position(|e| e.id == id) else { return };
        let won = site.pawns.iter().any(|p| p.is_colonist() && p.active());
        let members: Vec<Pawn> = site.pawns.drain(..).filter(|p| p.is_colonist()).collect();
        let lost: Vec<String> = members.iter().filter(|p| p.is_dead()).map(|p| p.name.clone()).collect();
        self.expeditions[k].members = members.into_iter().filter(|p| !p.is_dead()).collect();
        for name in &lost {
            self.expeditions[k].journal.push(format!("{} died in the fight.", name));
            self.push_log(format!("{} died on an expedition.", name));
        }
        let Stage::Fighting { then, main, .. } = &self.expeditions[k].stage else { return };
        let (then, main) = (then.clone(), *main);
        if self.expeditions[k].members.is_empty() {
            self.push_log(format!("The {} expedition was lost.", self.expeditions[k].kind.name().to_lowercase()));
            self.expeditions.remove(k);
            return;
        }
        if won {
            // What the fallen left on the field, as much as they can carry.
            let mut loot = Vec::new();
            for i in 0..site.map.len() {
                for s in site.map.take_all(i) {
                    add_stack(&mut loot, s);
                }
            }
            // The most valuable first.
            loot.sort_by(|a, b| b.kind.def().value.total_cmp(&a.kind.def().value));
            let e = &mut self.expeditions[k];
            let left = e.take_in(loot);
            e.note_left(&left);
            self.expeditions[k].journal.push("They won the fight.".into());
            self.apply(k, &then, main);
        } else {
            self.expeditions[k].journal.push("They were beaten, and carried their wounded home.".into());
            self.carry_on(k, true, 0.0);
        }
    }

    /// A cell at the edge of the map to arrive at (where the colony can be walked to).
    fn edge_spot(&mut self) -> Option<Pos> {
        self.random_edge_tile()
    }

    /// Expedition `k` is back: its people walk in at the edge, its goods are put down there.
    fn come_home(&mut self, k: usize) {
        let e = self.expeditions.remove(k);
        let at = self.edge_spot().unwrap_or((self.map.w / 2, self.map.h / 2, 1));
        let names: Vec<String> = e.members.iter().map(|p| p.name.clone()).collect();
        for mut p in e.members {
            p.pos = at;
            (p.x, p.y, p.z) = (at.0 as f32, at.1 as f32, at.2 as f32);
            p.job = None;
            p.path.clear();
            self.pawns.push(p);
        }
        for s in e.cargo {
            self.map.drop_item(at, s);
        }
        if e.recruit {
            let id = self.spawn_colonist(at);
            let name = self.pawn(id).map(|p| p.name.clone()).unwrap_or_default();
            self.push_log(format!("{} joined the colony.", name));
        }
        self.push_log(format!("{} are back from the {} expedition.", names.join(", "), e.kind.name().to_lowercase()));
    }

    /// The fight site waiting to be played, if any (taken out by the front end).
    pub fn take_site(&mut self) -> Option<(ExpeditionId, World)> {
        self.expeditions.iter_mut().find_map(|e| match &mut e.stage {
            Stage::Fighting { site, .. } => site.take().map(|s| (e.id, *s.0)),
            Stage::Out { .. } | Stage::Decide { .. } | Stage::Trading { .. } | Stage::Back { .. } => None,
        })
    }

    /// Put a site back (for saving while it's played).
    pub fn put_site(&mut self, id: ExpeditionId, site: World) {
        if let Some(Stage::Fighting { site: slot, .. }) = self.expeditions.iter_mut().find(|e| e.id == id).map(|e| &mut e.stage) {
            *slot = Some(SiteWorld(Box::new(site)));
        }
    }

    /// Colonists away on expeditions.
    pub fn away(&self) -> usize {
        self.expeditions.iter().map(|e| e.members.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLAT_Z as Z;
    use crate::testing::{colonist, item, quiet_world, run};

    /// A colony of three with goods in store, and an offer of `kind` from faction `f`.
    fn colony(kind: MissionKind, f: usize) -> (World, Vec<PawnId>, OfferId) {
        let mut w = quiet_world(24, 16, 3);
        let ids = vec![colonist(&mut w, (5, 5, Z)), colonist(&mut w, (6, 5, Z)), colonist(&mut w, (7, 5, Z))];
        w.map.drop_item((3, 3, Z), st("steel", 60));
        w.map.drop_item((3, 4, Z), st("meal", 20));
        let days = w.factions[f].days;
        w.offers.push(Offer { id: OfferId::new(999), kind, faction: f, days, until: w.time() + DAY_LEN });
        (w, ids, OfferId::new(999))
    }

    /// Run until expedition `id` waits for something (a choice, the market, a fight) or is home.
    fn travel(w: &mut World, id: ExpeditionId) {
        for _ in 0..(10.0 * DAY_LEN) as usize {
            run(w, 1.0);
            match w.expeditions.iter().find(|e| e.id == id) {
                Some(e) if matches!(e.stage, Stage::Out { .. } | Stage::Back { .. }) => {}
                _ => return,
            }
        }
        panic!("the expedition never got anywhere");
    }

    fn event(w: &World, id: ExpeditionId) -> Event {
        match &w.expeditions.iter().find(|e| e.id == id).unwrap().stage {
            Stage::Decide { event, .. } => event.clone(),
            s => panic!("not deciding: {:?}", s),
        }
    }

    #[test]
    fn every_mission_has_a_decision() {
        for kind in MissionKind::ALL {
            let f = (0..3).find(|&f| kind.fits(starting_factions()[f].kind)).unwrap();
            let (mut w, ids, offer) = colony(kind, f);
            w.send_expedition(offer, &ids, &[st("steel", 40)]).unwrap();
            let e = w.expeditions[0].id;
            let way = w.waylay_event(0).map_or(0, |ev| ev.choices.len());
            let main = w.main_event(0).unwrap().choices.len();
            assert!(way >= 2 || main >= 2 || kind == MissionKind::Trade, "{:?} offers no choice", kind);
            let _ = e;
        }
    }

    #[test]
    fn missions_can_be_offered_and_hurried_for_testing() {
        let (mut w, ids, _) = colony(MissionKind::Trade, 0);
        for kind in MissionKind::ALL {
            let id = w.offer_now(Some(kind)).unwrap();
            let o = w.offers.iter().find(|o| o.id == id).unwrap();
            assert_eq!(o.kind, kind);
            assert!(kind.fits(w.factions[o.faction].kind), "{:?} from the wrong faction", kind);
        }
        assert!(w.offers.len() > MAX_OFFERS, "testing offers ignore the limit");
        assert!(w.offer_now(None).is_some());

        let offer = w.offers.iter().find(|o| o.kind == MissionKind::Hunt).unwrap().id;
        w.send_expedition(offer, &ids, &[]).unwrap();
        let e = w.expeditions[0].id;
        assert!(w.skip_travel(e));
        run(&mut w, 1.5);
        assert!(matches!(w.expeditions[0].stage, Stage::Decide { .. }), "the event comes straight away");
        assert!(!w.skip_travel(e), "nothing to skip while deciding");
    }

    #[test]
    fn a_trade_caravan_goes_pays_trades_and_comes_home() {
        let (mut w, ids, offer) = colony(MissionKind::Trade, 0);
        assert!(w.send_expedition(offer, &ids[..2], &[st("steel", 100)]).is_err(), "only what the colony has");
        w.send_expedition(offer, &ids[..2], &[st("steel", 50)]).unwrap();
        let id = w.expeditions[0].id;
        assert_eq!(w.pawns.len(), 1, "they left the map");
        assert_eq!(w.item_totals()[item("steel").index()], 10, "with the goods");
        assert_eq!(w.away(), 2);

        // A toll on the road: take the long way.
        travel(&mut w, id);
        let ev = event(&w, id);
        assert_eq!(ev.choices.len(), 3);
        let start = w.time();
        w.choose(id, 1).unwrap();
        travel(&mut w, id);
        assert!(w.time() - start > 1.9 * DAY_LEN, "a day longer");

        // The market.
        w.choose(id, 0).unwrap();
        assert!(matches!(w.expeditions[0].stage, Stage::Trading { .. }));
        assert!(w.trade(id, &[st("steel", 1)], &[st("medicine", 6)]).is_err(), "too little for that");
        assert!(w.trade(id, &[st("steel", 60)], &[]).is_err(), "they only have 50");
        w.trade(id, &[st("steel", 50)], &[st("medicine", 2)]).unwrap();
        assert!(w.expeditions[0].cargo.contains(&st("medicine", 2)));
        w.leave_market(id);
        travel(&mut w, id);
        assert!(w.expeditions.is_empty(), "home");
        assert_eq!(w.pawns.len(), 3);
        assert_eq!(w.item_totals()[item("medicine").index()], 2, "with what they bought");
    }

    #[test]
    fn gifts_need_goods_and_win_goodwill() {
        let (mut w, ids, offer) = colony(MissionKind::Envoy, 1);
        w.send_expedition(offer, &ids[..1], &[st("steel", 30)]).unwrap();
        let id = w.expeditions[0].id;
        travel(&mut w, id);
        let before = w.factions[1].goodwill;
        w.choose(id, 0).unwrap();
        assert!(w.factions[1].goodwill > before);
        assert!(w.expeditions[0].cargo.is_empty(), "the gifts were given");
        // Nothing left to give: a ransom can't be paid.
        let ransom = Choice { label: String::new(), effects: vec![Effect::Give(w.share_of_value(0, 150.0))] };
        assert!(!w.expeditions[0].affords(&ransom));
    }

    #[test]
    fn storming_a_camp_is_fought_on_a_site_and_the_squad_comes_back() {
        let (mut w, ids, offer) = colony(MissionKind::BanditCamp, 2);
        for &id in &ids {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("assault_rifle"));
        }
        w.send_expedition(offer, &ids, &[]).unwrap();
        let id = w.expeditions[0].id;
        travel(&mut w, id);
        w.choose(id, 0).unwrap();
        assert!(matches!(w.expeditions[0].stage, Stage::Fighting { .. }));
        let (sid, mut site) = w.take_site().unwrap();
        assert_eq!(sid, id);
        assert_eq!(site.pawns.iter().filter(|p| p.is_colonist()).count(), 3, "the squad is at the site");
        assert_eq!(site.pawns.iter().filter(|p| !p.is_colonist()).count(), 5, "and the raiders");
        site.battle.as_mut().unwrap().auto_colony = true;
        for _ in 0..20_000 {
            if site.battle.is_none() {
                break;
            }
            site.tick(crate::TICK);
        }
        assert!(site.battle.is_none(), "the fight ended");
        let site_ids = site.next_id;
        assert!(site_ids > w.next_id, "the site numbered its raiders after the colony's ids");
        w.finish_site(id, site);
        assert_eq!(w.next_id, site_ids, "and the colony carries on after them");
        if let Some(e) = w.expeditions.first() {
            assert!(matches!(e.stage, Stage::Back { .. }), "{:?}", e.stage);
            assert!(!e.members.is_empty());
            travel(&mut w, id);
        }
        assert!(w.expeditions.is_empty());
        assert!(w.pawns.iter().filter(|p| p.is_colonist()).count() >= 1 || w.colony_lost());
    }

    #[test]
    fn a_choice_is_only_affordable_if_every_outcome_is() {
        let (mut w, ids, offer) = colony(MissionKind::Trade, 0);
        w.send_expedition(offer, &ids, &[st("steel", 5)]).unwrap();
        let e = &w.expeditions[0];
        let give = |n| vec![Effect::Give(vec![st("steel", n)])];
        let choice = |effects| Choice { label: String::new(), effects };
        assert!(e.affords(&choice(give(5))));
        assert!(!e.affords(&choice(give(9))));
        // Nested in a chance or a fight's aftermath, it still has to be paid.
        assert!(!e.affords(&choice(vec![Effect::Chance { p: 0.5, yes: Vec::new(), no: give(9) }])));
        assert!(!e.affords(&choice(vec![Effect::Fight { enemies: 2, site: Site::Camp, then: give(9) }])));
        assert!(e.affords(&choice(vec![Effect::Chance { p: 0.5, yes: give(5), no: Vec::new() }])));
    }

    #[test]
    fn expeditions_carry_only_so_much_and_backpacks_carry_more() {
        let (mut w, ids, offer) = colony(MissionKind::Trade, 0);
        w.map.drop_item((4, 3, Z), st("steel", 60));
        let err = w.send_expedition(offer, &ids[..1], &[st("steel", 100)]).unwrap_err();
        assert!(err.contains("carry"), "{}", err);
        let i = w.pawn_index(ids[0]).unwrap();
        w.pawns[i].wear(item("field_pack"));
        w.send_expedition(offer, &ids[..1], &[st("steel", 100)]).unwrap();
        let e = &mut w.expeditions[0];
        assert_eq!((e.load(), e.capacity()), (100, crate::pawn::CARRY_EACH + 90));
        // More than fits: fill the cargo and return the excess to the caller.
        let left = e.take_in([st("copper_ore", 80)]);
        assert_eq!(left, vec![st("copper_ore", 30)]);
        assert_eq!(e.load(), e.capacity());
        assert!(e.cargo.contains(&st("copper_ore", 50)));
    }

    #[test]
    fn friendlier_raiders_raid_less_and_truces_hold() {
        let mut w = quiet_world(10, 10, 1);
        let hostile = w.raid_spacing();
        w.factions[2].goodwill = 40;
        assert!(w.raid_spacing() > hostile * 1.5);
        let (mut w, ids, offer) = colony(MissionKind::PeaceTalks, 2);
        w.send_expedition(offer, &ids[..1], &[st("steel", 60)]).unwrap();
        let id = w.expeditions[0].id;
        travel(&mut w, id);
        w.story.next_raid = w.time() + 10.0;
        w.choose(id, 0).unwrap();
        assert!(w.story.next_raid > w.time() + 4.0 * DAY_LEN, "tribute buys peace");
        assert!(w.factions[2].goodwill > -40);
    }

    #[test]
    fn everyone_away_is_not_a_lost_colony_and_saves_keep_them() {
        let (mut w, ids, offer) = colony(MissionKind::Hunt, 0);
        w.send_expedition(offer, &ids, &[]).unwrap();
        assert_eq!(w.colonists_alive(), 0);
        assert!(!w.colony_lost());
        let back = World::load_json(&w.save_json().unwrap()).unwrap();
        assert_eq!(back.away(), 3);
        assert_eq!(back.factions.len(), 3);
    }

    #[test]
    fn offers_come_and_go() {
        let mut w = quiet_world(10, 10, 5);
        colonist(&mut w, (2, 2, Z));
        // About one a day, each open for three: over a week there's soon something on offer.
        let mut seen = false;
        for _ in 0..7 * 24 {
            run(&mut w, DAY_LEN / 24.0);
            seen |= !w.offers.is_empty();
            assert!(w.offers.len() <= MAX_OFFERS);
        }
        assert!(seen);
        assert!(w.offers.iter().all(|o| o.kind.fits(w.factions[o.faction].kind)));
    }
}
