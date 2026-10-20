//! Turn-based battles (phase 12), in the style of MENACE.
//!
//! A raid opens a parley (`Parley`): pay the raiders' demand, or fight. A
//! battle stops the whole colony. Units are squads (a leader and the
//! colonists assigned to them, see `squads.rs`), lone fighters, raider
//! squads, and civilians sheltering in the safe zone (who never act but can
//! be attacked). The sides take turns activating one unit each; every unit
//! acts once per round, spending action points on moving, attacking and
//! hunkering down. Each round lets `TURN_SECS` of time pass for wounds and
//! hunger. Squad members who were far from their leader arrive a few
//! rounds in, already in their combat gear.

use crate::defs::{defs, ItemId, UtilityEffect};
use crate::health::CyberSlot;
use crate::map::{adjacent, Pos, Stack, Zone};
use crate::pathing::{bfs, bfs_avoiding, DistField};
use crate::pawn::{attack_with, Attack, Faction, Life, Memory, Pawn, PawnId, Role, Skill};
use crate::world::{Effect, Step, World, HUNGER_DRAIN, REST_DRAIN};
use serde::{Deserialize, Serialize};

/// Action points per unit and round, and what things cost.
pub const AP_MAX: i32 = 100;
pub const AP_MOVE: i32 = 10;
pub const AP_ATTACK: i32 = 40;
pub const AP_HUNKER: i32 = 20;
/// Deploying a squad (down on one knee) and standing it up again.
pub const AP_DEPLOY: i32 = 30;
/// A deployed squad aims better, and is harder to hit.
const DEPLOYED_AIM: f32 = 1.25;
const DEPLOYED_TARGET: f32 = 0.8;
/// Suppression from being attacked, and from each hit.
const SUPPRESS_ATTACK: f32 = 15.0;
const SUPPRESS_HIT: f32 = 10.0;
/// Melee units move this much faster in battle (fewer AP a step).
pub const MELEE_PACE: f32 = 1.15;
/// An attack in which everyone uses their sidearm (quick to draw and fire).
pub const AP_SIDEARM_ATTACK: i32 = 25;
/// Long guns hit this share as often at someone right next to them (pistols and
/// blows don't): a ranged squad caught in melee draws its pistols.
pub const CLOSE_QUARTERS: f32 = 0.2;
/// Suppression from a grenade going off in the unit.
const SUPPRESS_BLAST: f32 = 30.0;
/// Game seconds that pass per round (for bleeding, healing and hunger).
pub const TURN_SECS: f32 = 6.0;
/// Cells a squad member covers per round on the way to their leader (as
/// far as a unit can run in a round).
pub const CELLS_PER_ROUND: usize = (AP_MAX / AP_MOVE) as usize;
/// Members within this many cells of their leader start the battle with them.
const WITH_LEADER: usize = 2;
/// Suppression from which a unit is pinned down (less AP, worse aim, and
/// moving costs `PINNED_MOVE` times as much).
pub const PINNED_MOVE: i32 = 2;
/// Suppression from which a unit is pinned down (less AP, worse aim).
pub const PINNED: f32 = 50.0;
/// Morale below which a unit breaks: raiders flee, colonists cower.
pub const BROKEN: f32 = 25.0;
/// Largest raider squad.
const RAIDER_SQUAD: usize = 4;
/// An attack: seconds to take aim; a squad's shots go off together, spread
/// over `VOLLEY_SPREAD` (so they overlap), a volley every `VOLLEY_GAP` (rifles
/// fire two); blows come one after another, `MELEE_GAP` apart; then
/// `AFTER_ATTACK` to let the last one sink in.
const AIM_TIME: f32 = 0.6;
const VOLLEY_SPREAD: f32 = 0.15;
const VOLLEY_GAP: f32 = 0.4;
const MELEE_GAP: f32 = 0.8;
const AFTER_ATTACK: f32 = 0.7;
/// Rounds without an attack and without the sides getting any closer
/// after which the raiders give up.
pub const STALEMATE: u32 = 10;
/// How many times faster a walk plays out once the player hurries it (clicks again).
pub const HURRY: u32 = 5;
/// What a raid demands per raider, in item value.
pub const DEMAND_PER_RAIDER: f32 = 40.0;

pub use crate::defs::CoverKind as Cover;

impl Cover {
    pub fn name(self) -> &'static str {
        match self {
            Cover::None => "no cover",
            Cover::Light => "light cover",
            Cover::Medium => "medium cover",
            Cover::Heavy => "heavy cover",
        }
    }

    /// One step less: what cover is worth against a shot from above.
    fn weaker(self) -> Cover {
        match self {
            Cover::Heavy => Cover::Medium,
            Cover::Medium => Cover::Light,
            Cover::Light | Cover::None => Cover::None,
        }
    }

    /// How much it scales the chance of being hit. (Heavy cover blocks the
    /// view; this is for shots that get past it at an angle.)
    pub fn hit_factor(self) -> f32 {
        match self {
            Cover::None => 1.0,
            Cover::Light => 0.8,
            Cover::Medium => 0.6,
            Cover::Heavy => 0.45,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Unit {
    pub side: Faction,
    /// Pawn ids still fighting. The first is where the unit stands and moves from
    /// (a squad's leader while they stand).
    pub members: Vec<PawnId>,
    pub kind: UnitKind,
    /// Everyone who has fought in it, the fallen too (to show losses).
    #[serde(default)]
    pub roster: Vec<PawnId>,
    pub ap: i32,
    /// AP at the start of this round (switching units is allowed until some is spent).
    pub ap_start: i32,
    pub acted: bool,
    pub stance: Stance,
    pub suppression: f32,
    pub morale: f32,
}

/// How a unit holds itself between orders.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Stance {
    #[default]
    Standing,
    /// Heads down: harder to hit until the unit's next turn (civilians always).
    Hunkered,
    /// A squad down on one knee: better aim, harder to hit, can't move.
    Deployed,
}

/// What a unit is, fixed when the battle starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum UnitKind {
    /// One fighter on their own.
    Lone,
    /// A squad, led until its leader falls; it then fights on leaderless.
    Squad { leader: Option<PawnId> },
    /// Civilians sheltering in the safe zone: never act, but can be attacked.
    Civilians,
}

impl Unit {
    fn new(side: Faction, members: Vec<PawnId>, kind: UnitKind, morale: f32) -> Unit {
        Unit {
            side,
            roster: members.clone(),
            members,
            kind,
            ap: AP_MAX,
            ap_start: AP_MAX,
            acted: false,
            stance: Stance::Standing,
            suppression: 0.0,
            morale,
        }
    }

    pub fn is_squad(&self) -> bool {
        matches!(self.kind, UnitKind::Squad { .. })
    }

    /// A squad's leader, while they still stand.
    pub fn leader(&self) -> Option<PawnId> {
        match self.kind {
            UnitKind::Squad { leader } => leader,
            UnitKind::Lone | UnitKind::Civilians => None,
        }
    }

    pub fn is_civilians(&self) -> bool {
        self.kind == UnitKind::Civilians
    }

    pub fn is_hunkered(&self) -> bool {
        self.stance == Stance::Hunkered
    }

    pub fn is_deployed(&self) -> bool {
        self.stance == Stance::Deployed
    }

    /// Nobody left in it.
    pub fn out(&self) -> bool {
        self.members.is_empty()
    }

    pub fn pinned(&self) -> bool {
        self.suppression >= PINNED
    }

    /// Started this round pinned down (fewer AP, slower going), whatever
    /// its suppression has dropped to since. (A radio's bonus never makes up
    /// the pinned unit's loss: it's less than `AP_MAX` * 4/10.)
    pub fn held_down(&self) -> bool {
        self.ap_start < AP_MAX
    }

    pub fn broken(&self) -> bool {
        self.morale < BROKEN
    }

    /// Takes part in the turn order.
    pub fn can_act(&self) -> bool {
        !self.out() && !self.is_civilians()
    }
}

/// Someone walking to their unit's new cell (a squad's members all go
/// together, at the pace of the slowest).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Mover {
    pub pawn: PawnId,
    pub goal: Pos,
}

/// A shot or blow still to come in an attack, `delay` seconds from now.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Strike {
    pub attacker: PawnId,
    /// Who they aim at to begin with (the actual target is picked when it fires).
    pub target: PawnId,
    /// The unit attacked.
    #[serde(default)]
    pub unit: usize,
    pub delay: f32,
}

/// A squad member on the way to their leader.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Arrival {
    pub pawn: PawnId,
    pub leader: PawnId,
    pub rounds: u32,
}

/// A raid waiting for the player's answer: pay the demand, or fight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Parley {
    /// Raiders per squad.
    pub squads: Vec<usize>,
    pub entry: Pos,
    /// What they want; empty if the colony doesn't have enough.
    pub demand: Vec<Stack>,
    /// How well they're equipped: an index into `raider_tiers`.
    #[serde(default)]
    pub tier: usize,
}

/// Split `raiders` into squads, largest first. Small raids come mostly as
/// lone raiders (3: usually 1-1-1), bigger ones in pairs (4: usually 2-2,
/// else 2-1-1), and a full squad of `RAIDER_SQUAD` shows up at about 10.
pub fn raider_squads(raiders: usize, rng: &mut crate::rng::Rng) -> Vec<usize> {
    if raiders == 0 {
        return Vec::new();
    }
    // Average squad size: about 1 up to 3 raiders, rising to 3.4 at 10.
    let n = raiders as f32;
    let size = if n <= 3.0 { 1.1 } else { (1.7 + (n - 4.0) * 0.283).min(3.5) };
    // That many squads, rounded up or down at random (the nearer, the likelier).
    let count = n / size;
    let mut groups = count.floor() as usize + rng.chance(count.fract()) as usize;
    groups = groups.clamp(raiders.div_ceil(RAIDER_SQUAD), raiders);
    // As even as possible.
    (0..groups).map(|k| raiders / groups + (k < raiders % groups) as usize).collect()
}

impl Parley {
    pub fn raiders(&self) -> usize {
        self.squads.iter().sum()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Won,
    Lost,
}

/// The unit taking its turn, and what it has done in it so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activation {
    pub unit: usize,
    /// It has moved during this activation.
    pub moved: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Battle {
    pub round: u32,
    pub units: Vec<Unit>,
    pub arrivals: Vec<Arrival>,
    pub side_to_act: Faction,
    /// The unit taking its activation.
    pub active: Option<Activation>,
    /// Seconds the last attack still plays out.
    pub busy: f32,
    /// Pawns walking to their unit's new cell.
    pub moving: Vec<Mover>,
    /// What the raiders came for (taken if they win).
    pub demand: Vec<Stack>,
    /// The AI plays the colony's units too (for tests).
    pub auto_colony: bool,
    /// What happened, newest last.
    pub events: Vec<String>,
    /// Rounds in a row without an attack or the sides closing in; raiders
    /// give up after `STALEMATE`.
    #[serde(default)]
    pub quiet_rounds: u32,
    /// Closest the sides have been (cells), to tell closing in from a stalemate.
    #[serde(default)]
    pub closest: Option<usize>,
    /// The colony passes for the rest of the round: its remaining units end
    /// their turns as they come up.
    #[serde(default)]
    pub colony_passes: bool,
    /// The attack playing out: shots and blows still to come.
    #[serde(default)]
    pub strikes: Vec<Strike>,
    /// The walk under way plays out `HURRY` times faster (until it ends).
    #[serde(default)]
    pub hurry: bool,
}

impl Battle {
    /// Waiting for an attack or a move to play out.
    pub fn busy(&self) -> bool {
        self.busy > 0.0 || !self.moving.is_empty() || !self.strikes.is_empty()
    }

    /// The unit pawn `id` fights in.
    pub fn active_unit(&self) -> Option<usize> {
        self.active.map(|a| a.unit)
    }

    pub fn unit_of(&self, id: PawnId) -> Option<usize> {
        self.units.iter().position(|u| u.members.contains(&id))
    }

    fn has_turn_left(&self, side: Faction) -> bool {
        self.units.iter().any(|u| u.side == side && u.can_act() && !u.acted)
    }
}

/// An order for the unit whose turn it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BattleOrder {
    /// Pick which unit acts (before it spends any AP).
    Activate(usize),
    /// Walk the unit (its leader) to a cell; members gather around it.
    Move(Pos),
    /// Shoot at, or strike, a unit: everyone in the unit who can reach it.
    Attack(usize),
    /// Take cover for the rest of the round (ends the activation).
    Hunker,
    /// A squad goes down on one knee, or stands up again (`AP_DEPLOY` each way).
    Deploy,
    /// Someone in the unit throws a grenade at a unit: everyone in it is caught.
    Throw(usize),
    /// Someone in the unit uses a medkit on everyone's wounds.
    Medkit,
    End,
}

/// Straight-line distance between two cells.
fn cell_dist(a: Pos, b: Pos) -> f32 {
    let d = |p: usize, q: usize| p as f32 - q as f32;
    (d(a.0, b.0).powi(2) + d(a.1, b.1).powi(2) + d(a.2, b.2).powi(2)).sqrt()
}

fn place(p: &mut Pawn, c: Pos) {
    p.pos = c;
    (p.x, p.y, p.z) = (c.0 as f32, c.1 as f32, c.2 as f32);
    p.path.clear();
    p.path_goal = None;
}

fn other(side: Faction) -> Faction {
    match side {
        Faction::Colony => Faction::Raider,
        Faction::Raider => Faction::Colony,
    }
}

/// How likely `a` is to hit, before the situation (cover, hunkering, height,
/// being pinned): shooting at `d` cells, or a blow in melee. Shots: 55% +
/// 2.5% per Shooting level - 2.5% per cell; blows: 60% + 2% per Melee level.
/// Bad eyes or hands make either worse; goggles and implants add to it
/// (`Pawn::aim_bonus`), and so do bionic eyes and arms working better than
/// natural ones (up to a fifth more each).
pub fn base_hit_chance(a: &Pawn, ranged: bool, d: f32) -> f32 {
    let skill = if ranged { Skill::Shooting } else { Skill::Melee };
    let level = a.skill(skill) as f32;
    let c = if ranged { 0.55 + level * 0.025 - d * 0.025 } else { 0.6 + level * 0.02 };
    let caps = &a.health.caps;
    (c + a.aim_bonus()) * (0.4 + 0.6 * caps.sight.min(1.2)) * (0.5 + 0.5 * caps.manipulation.min(1.2))
}

impl World {
    /// The battle under way. Orders and ticks return early without one, so
    /// the helpers below are only ever reached while it runs.
    pub(crate) fn current_battle(&self) -> &Battle {
        self.battle.as_ref().expect("battle helpers run only during a battle")
    }

    pub(crate) fn current_battle_mut(&mut self) -> &mut Battle {
        self.battle.as_mut().expect("battle helpers run only during a battle")
    }

    /// Whether unit `u` can go down on one knee (or, deployed, stand up):
    /// squads that fight at range (a whole squad fights one way, like its leader).
    pub fn can_deploy(&self, u: usize) -> bool {
        let Some(unit) = self.battle.as_ref().and_then(|b| b.units.get(u)) else { return false };
        unit.is_squad() && (unit.is_deployed() || unit.members.first().and_then(|&id| self.pawn(id)).is_some_and(|p| p.main_attack().ranged))
    }

    // ---------- parley ----------

    /// Goods worth about `value` from what's lying in the colony, most
    /// valuable first; empty if there isn't that much.
    pub fn goods_worth(&self, value: f32) -> Vec<Stack> {
        let totals = self.item_totals();
        let mut kinds: Vec<ItemId> = ItemId::all().filter(|k| totals[k.index()] > 0).collect();
        kinds.sort_by(|a, b| b.def().value.total_cmp(&a.def().value));
        let mut left = value;
        let mut out = Vec::new();
        for k in kinds {
            if left <= 0.0 {
                break;
            }
            let v = k.def().value.max(0.01);
            let have = self.map.count_items(k);
            let count = ((left / v).ceil() as u32).min(have);
            if count > 0 {
                out.push(Stack { kind: k, count });
                left -= count as f32 * v;
            }
        }
        if left > 0.0 {
            Vec::new()
        } else {
            out
        }
    }

    /// Remove goods from wherever they lie in the colony.
    pub(crate) fn take_goods(&mut self, goods: &[Stack]) {
        for g in goods {
            let mut left = g.count;
            for i in 0..self.map.len() {
                if left == 0 {
                    break;
                }
                if let Some(s) = self.map.take_item(i, g.kind, left) {
                    left -= s.count;
                }
            }
        }
    }

    /// Raiders arrive and make their demand; the colony stops until answered.
    pub(crate) fn open_parley(&mut self, raiders: usize, entry: Pos, tier: usize) {
        let squads = raider_squads(raiders, &mut self.rng);
        // Better equipped raiders want more.
        let demand = self.goods_worth(raiders as f32 * DEMAND_PER_RAIDER * (1.0 + 0.25 * tier as f32));
        self.parley = Some(Parley { squads, entry, demand, tier });
    }

    /// Equip raider `id` as raider tier `tier` says (see `RaiderTier`): a
    /// weapon (of one kind, if `ranged` says), apparel, a sidearm, belt
    /// items and cyberware, each by its chance; better tiers fight better too.
    pub fn equip_raider(&mut self, id: PawnId, tier: usize, ranged: Option<bool>) {
        let tiers = &defs().raider_tiers;
        let tier = tier.min(tiers.len() - 1);
        let t = &tiers[tier];
        let Some(i) = self.pawn_index(id) else { return };
        let rng = &mut self.rng;
        let p = &mut self.pawns[i];
        p.weapon = Pawn::raider_weapon(rng, tier, ranged).or(p.weapon);
        p.apparel.clear();
        for (kind, chance) in &t.apparel {
            if rng.chance(*chance) {
                p.wear(kind.id());
            }
        }
        // Riflemen carry pistols (a ranged squad fights at range, close in
        // too); melee raiders may carry anything, a pistol to shoot on the way in.
        let ranged = p.main_attack().ranged;
        let fits = |k: ItemId| Some(k) != p.weapon && (!ranged || attack_with(Some(k)).ranged);
        p.sidearm = t.sidearms.iter().find(|(k, chance)| fits(k.id()) && rng.chance(*chance)).map(|(k, _)| k.id());
        // Shields only for melee raiders (they'd stop a rifleman's shots).
        let shield = |k: ItemId| k.def().utility.as_ref().is_some_and(|u| matches!(u.effect, UtilityEffect::Shield { .. }));
        p.utility = t
            .belt
            .iter()
            .find(|(k, _, chance)| !(ranged && shield(k.id())) && rng.chance(*chance))
            .map(|(k, count, _)| Stack { kind: k.id(), count: *count });
        for (kind, chance) in &t.cyberware {
            let Some(c) = kind.id().def().cyberware.as_ref() else { continue };
            let free: Vec<CyberSlot> = c.fits.iter().copied().filter(|&s| p.health.implant(s).is_none()).collect();
            if !free.is_empty() && rng.chance(*chance) {
                let slot = free[rng.below(free.len())];
                p.health.install(slot, kind.id(), c.efficiency);
            }
        }
        for skill in [Skill::Shooting, Skill::Melee] {
            p.skills[skill] = (p.skills[skill] + 2 * tier as u8).min(20);
        }
    }

    /// Hand over what the raiders demand; they leave. False if there's
    /// nothing to pay with (they want a fight).
    pub fn pay_tribute(&mut self) -> bool {
        let Some(p) = self.parley.take() else { return false };
        if p.demand.is_empty() {
            self.parley = Some(p);
            return false;
        }
        self.take_goods(&p.demand);
        let paid: Vec<String> = p.demand.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect();
        self.push_log(format!("You paid the raiders {} and they left.", paid.join(", ")));
        true
    }

    /// Turn the raiders down: battle.
    pub fn refuse_tribute(&mut self) {
        if let Some(p) = self.parley.take() {
            self.start_battle(p);
        }
    }

    // ---------- starting and ending ----------

    pub(crate) fn start_battle(&mut self, parley: Parley) {
        let mut b = Battle {
            round: 1,
            units: Vec::new(),
            arrivals: Vec::new(),
            side_to_act: Faction::Colony,
            active: None,
            busy: 0.0,
            moving: Vec::new(),
            hurry: false,
            demand: parley.demand.clone(),
            auto_colony: false,
            events: Vec::new(),
            quiet_rounds: 0,
            closest: None,
            colony_passes: false,
            strikes: Vec::new(),
        };
        // Everyone drops what they're doing.
        for i in 0..self.pawns.len() {
            if self.pawns[i].is_colonist() && !self.pawns[i].is_dead() {
                let mut p = self.pawns[i].clone();
                self.end_job(&mut p);
                self.pawns[i] = p;
            }
        }

        // Raiders come in squads near where they entered.
        let df = bfs(&self.map, parley.entry);
        // Each squad stands on one cell, a few cells from the next.
        let spots: Vec<Pos> = df.order.iter().map(|&i| self.map.pos(i)).collect();
        for (k, &size) in parley.squads.iter().enumerate() {
            let Some(&at) = spots.get(k * 6).or(spots.last()) else { break };
            let mut members: Vec<PawnId> = Vec::new();
            for _ in 0..size {
                let name = crate::story::raider_name(&mut self.rng);
                let id = self.spawn_raider(name, at);
                // A squad fights one way: armed like the first of them.
                let ranged = members.first().map(|&first| self.pawn(first).unwrap().main_attack().ranged);
                self.equip_raider(id, parley.tier, ranged);
                members.push(id);
            }
            if !members.is_empty() {
                let kind = if members.len() > 1 { UnitKind::Squad { leader: Some(members[0]) } } else { UnitKind::Lone };
                b.units.push(Unit::new(Faction::Raider, members, kind, 60.0));
            }
        }

        // The colony: squads around their leaders, lone fighters where they stand.
        let ready = |p: &Pawn| p.is_colonist() && p.active();
        let colonists: Vec<(PawnId, Role, Pos)> =
            self.pawns.iter().filter(|p| ready(p)).map(|p| (p.id, p.role, p.pos)).collect();
        let leaders: Vec<(PawnId, Pos)> =
            colonists.iter().filter(|c| c.1 == Role::Leader).map(|c| (c.0, c.2)).collect();
        let ranged = |w: &World, id: PawnId| w.pawn(id).is_some_and(|p| p.main_attack().ranged);
        // Members armed the other way than their squad (say their gun wasn't
        // in storage) fight alone this time.
        let mut misfits = Vec::new();
        for &(leader, at) in &leaders {
            let df = bfs(&self.map, at);
            let mut members = vec![leader];
            for &(id, role, pos) in &colonists {
                if role != Role::Member(leader) {
                    continue;
                }
                if ranged(self, id) != ranged(self, leader) {
                    let (p, l) = (self.pawn(id).unwrap(), self.pawn(leader).unwrap());
                    let msg = format!(
                        "{} has a {} weapon, but {}'s squad is a {} squad: they fight alone.",
                        p.name,
                        crate::squads::style_name(!ranged(self, leader)),
                        l.name,
                        crate::squads::style_name(ranged(self, leader))
                    );
                    b.events.push(msg.clone());
                    self.push_log(msg);
                    misfits.push(id);
                    continue;
                }
                let steps = df.dist(pos).map_or(pos.0.abs_diff(at.0) + pos.1.abs_diff(at.1), |d| d as usize);
                if steps <= WITH_LEADER {
                    // A squad holds one cell: the leader's.
                    let i = self.pawn_index(id).unwrap();
                    place(&mut self.pawns[i], at);
                    members.push(id);
                } else {
                    let rounds = steps.saturating_sub(WITH_LEADER).div_ceil(CELLS_PER_ROUND).max(1) as u32;
                    b.arrivals.push(Arrival { pawn: id, leader, rounds });
                }
            }
            let morale = self.unit_morale(&members) + 10.0;
            b.units.push(Unit::new(Faction::Colony, members, UnitKind::Squad { leader: Some(leader) }, morale));
        }
        for &(id, role, _) in &colonists {
            let alone = match role {
                Role::Fighter => true,
                // A member whose leader can't fight fights alone.
                Role::Member(l) => !leaders.iter().any(|&(lid, _)| lid == l) || misfits.contains(&id),
                _ => false,
            };
            if alone {
                let morale = self.unit_morale(&[id]);
                b.units.push(Unit::new(Faction::Colony, vec![id], UnitKind::Lone, morale));
            }
        }
        // Civilians run to the safe zone (if there is one) and keep their heads down.
        let mut safe: Vec<Pos> =
            (0..self.map.len()).filter(|&i| self.map.zone[i] == Zone::Safe).map(|i| self.map.pos(i)).collect();
        safe.retain(|&c| self.map.standable(c));
        for &(id, role, pos) in &colonists {
            if role != Role::Civilian {
                continue;
            }
            if let Some(k) = (0..safe.len()).min_by_key(|&k| safe[k].0.abs_diff(pos.0) + safe[k].1.abs_diff(pos.1)) {
                let c = safe.swap_remove(k);
                let i = self.pawn_index(id).unwrap();
                place(&mut self.pawns[i], c);
            }
            let mut u = Unit::new(Faction::Colony, vec![id], UnitKind::Civilians, 50.0);
            u.stance = Stance::Hunkered;
            b.units.push(u);
        }
        // Everyone runs while the battle lasts.
        for p in self.pawns.iter_mut() {
            p.running = true;
        }
        let raiders = parley.raiders();
        let who = if raiders == 1 { "1 raider attacks".to_string() } else { format!("{} raiders attack", raiders) };
        b.events.push(format!("Battle! {}.", who));
        // A field radio in a unit: more action points from the first round.
        for u in b.units.iter_mut() {
            let bonus = self.ap_bonus(&u.members);
            u.ap += bonus;
            u.ap_start += bonus;
        }
        self.battle = Some(b);
        self.push_log(format!("Battle! {} the colony.", who));
    }

    /// Starting morale: how the members feel.
    fn unit_morale(&self, members: &[PawnId]) -> f32 {
        let moods: Vec<f32> = members.iter().filter_map(|&id| self.pawn(id)).map(|p| p.mood).collect();
        let avg = if moods.is_empty() { 50.0 } else { moods.iter().sum::<f32>() / moods.len() as f32 };
        avg.clamp(35.0, 85.0)
    }

    /// Take battle gear off and put their own things back on: only for
    /// battles saved while battle gear was still put on at their start.
    fn gear_down(&mut self, id: PawnId) {
        let Some(i) = self.pawn_index(id) else { return };
        let stash: Vec<(ItemId, Option<ItemId>)> = self.pawns[i].stash.drain(..).rev().collect();
        for (taken, prev) in stash {
            let p = &mut self.pawns[i];
            let at = p.pos;
            if p.take_off(taken) {
                self.map.drop_item(at, Stack { kind: taken, count: 1 });
            }
            if let Some(prev) = prev {
                for extra in self.pawns[i].wear(prev) {
                    self.map.drop_item(at, Stack { kind: extra, count: 1 });
                }
            }
        }
    }

    fn end_battle(&mut self, outcome: Outcome) {
        let Some(b) = self.battle.take() else { return };
        for p in self.pawns.iter_mut() {
            p.running = false;
            p.sidearm_drawn = false;
            // Shields are full again by the next fight.
            p.shield_drain = 0.0;
        }
        let colonists: Vec<PawnId> = self.pawns.iter().filter(|p| p.is_living_colonist()).map(|p| p.id).collect();
        for &id in &colonists {
            self.gear_down(id);
        }
        let memory = match outcome {
            Outcome::Won => Memory::WonBattle,
            Outcome::Lost => Memory::LostBattle,
        };
        for p in self.pawns.iter_mut().filter(|p| p.is_living_colonist()) {
            p.add_memory(memory);
        }
        match outcome {
            Outcome::Won => self.push_log(format!("The raid was beaten off after {} rounds.", b.round)),
            Outcome::Lost => {
                let loot = self.goods_worth(b.demand.iter().map(|s| s.count as f32 * s.kind.def().value).sum::<f32>() * 1.5);
                self.take_goods(&loot);
                let taken: Vec<String> = loot.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect();
                let what = if taken.is_empty() { "what they could find".to_string() } else { taken.join(", ") };
                self.push_log(format!("The raiders won. They took {} and left.", what));
            }
        }
        // Raiders still standing leave; the downed stay where they fell.
        for p in self.pawns.iter_mut().filter(|p| !p.is_colonist() && p.active()) {
            p.life = Life::Left;
        }
        self.pawns.retain(|p| p.is_colonist() || !p.has_left());
    }

    // ---------- the turn order ----------

    /// The player's order for the colony's unit; refused when it isn't the
    /// colony's turn or the order doesn't make sense.
    pub fn battle_order(&mut self, order: BattleOrder) -> Result<(), String> {
        let b = self.battle.as_ref().ok_or("there's no battle")?;
        if b.side_to_act != Faction::Colony || b.auto_colony {
            return Err("it's the raiders' turn".into());
        }
        self.do_order(order)
    }

    pub(crate) fn do_order(&mut self, order: BattleOrder) -> Result<(), String> {
        let b = self.battle.as_ref().ok_or("there's no battle")?;
        if b.busy() {
            return Err("wait for the last action to finish".into());
        }
        if let BattleOrder::Activate(u) = order {
            let unit = b.units.get(u).ok_or("no such unit")?;
            if unit.side != b.side_to_act || unit.acted || !unit.can_act() {
                return Err("that unit can't act now".into());
            }
            if let Some(a) = b.active_unit() {
                if a != u && b.units[a].ap < b.units[a].ap_start {
                    return Err("finish with the active unit first".into());
                }
            }
            let b = self.current_battle_mut();
            b.active = Some(Activation { unit: u, moved: false });
            // Hunkering lasts until the unit's next turn; deploying until it stands up.
            if b.units[u].is_hunkered() {
                b.units[u].stance = Stance::Standing;
            }
            return Ok(());
        }
        let u = b.active_unit().ok_or("pick a unit first")?;
        let ap = b.units[u].ap;
        match order {
            BattleOrder::Activate(_) => unreachable!(),
            BattleOrder::End => {
                self.end_activation();
                Ok(())
            }
            BattleOrder::Deploy => {
                if !self.can_deploy(u) {
                    return Err("only ranged squads deploy".into());
                }
                if ap < AP_DEPLOY {
                    return Err("not enough action points".into());
                }
                let b = self.current_battle_mut();
                b.units[u].stance = if b.units[u].is_deployed() { Stance::Standing } else { Stance::Deployed };
                b.units[u].ap -= AP_DEPLOY;
                Ok(())
            }
            BattleOrder::Hunker => {
                if ap < AP_HUNKER {
                    return Err("not enough action points".into());
                }
                let b = self.current_battle_mut();
                // Taking cover means getting up off one knee.
                b.units[u].stance = Stance::Hunkered;
                b.units[u].ap = 0;
                self.end_activation();
                Ok(())
            }
            BattleOrder::Move(to) => {
                if self.current_battle().units[u].is_deployed() {
                    return Err(format!("deployed: stand up first ({} AP)", AP_DEPLOY));
                }
                let cost = self.move_cost(u, to)?;
                if cost > ap {
                    return Err("too far for the action points left".into());
                }
                self.start_move(u, to);
                let b = self.current_battle_mut();
                b.units[u].ap -= cost;
                b.active = Some(Activation { unit: u, moved: true });
                Ok(())
            }
            BattleOrder::Attack(t) => {
                let cost = self.attack_cost(u, t);
                if ap < cost {
                    return Err("not enough action points".into());
                }
                self.attack_unit(u, t)?;
                // The attack may have ended the battle.
                if let Some(b) = self.battle.as_mut() {
                    b.units[u].ap -= cost;
                }
                Ok(())
            }
            BattleOrder::Throw(t) => {
                let (thrower, kind) = self.throw_option(u, t).ok_or("nobody can throw a grenade that far")?;
                let cost = kind.def().utility.as_ref().map_or(AP_ATTACK, |d| d.ap);
                if ap < cost {
                    return Err("not enough action points".into());
                }
                self.throw_grenade(thrower, t);
                if let Some(b) = self.battle.as_mut() {
                    b.units[u].ap -= cost;
                }
                Ok(())
            }
            BattleOrder::Medkit => {
                let (medic, kind) = self.medkit_option(u).ok_or("no medkit, or nobody needs one")?;
                let cost = kind.def().utility.as_ref().map_or(AP_ATTACK, |d| d.ap);
                if ap < cost {
                    return Err("not enough action points".into());
                }
                self.use_medkit(medic, u);
                let b = self.current_battle_mut();
                b.units[u].ap -= cost;
                Ok(())
            }
        }
    }

    /// Extra action points a unit of `members` gets each round: the best
    /// field radio anyone standing in it carries (radios don't add up).
    pub fn ap_bonus(&self, members: &[PawnId]) -> i32 {
        members
            .iter()
            .filter_map(|&id| self.pawn(id))
            .filter(|p| p.active())
            .flat_map(|p| p.apparel.iter().filter_map(|k| k.def().apparel.as_ref()).map(|a| a.ap))
            .max()
            .unwrap_or(0)
    }

    // ---------- belt items ----------

    /// Who in unit `u` could throw a grenade at unit `t` (someone there in
    /// range and sight): the thrower and the grenade.
    pub fn throw_option(&self, u: usize, t: usize) -> Option<(PawnId, ItemId)> {
        let b = self.battle.as_ref()?;
        let (unit, target) = (b.units.get(u)?, b.units.get(t)?);
        if unit.side == target.side || target.out() {
            return None;
        }
        unit.members.iter().filter_map(|&m| self.pawn(m)).filter(|p| p.active()).find_map(|p| {
            let s = p.utility?;
            let Some(UtilityEffect::Grenade { range, .. }) = s.kind.def().utility.as_ref().map(|d| d.effect) else { return None };
            let reach = target.members.iter().filter_map(|&o| self.pawn(o)).any(|o| {
                o.active() && cell_dist(p.pos, o.pos) <= range && battle_sight(self, p.pos, o.pos)
            });
            reach.then_some((p.id, s.kind))
        })
    }

    /// Someone in unit `u` with a medkit, if anyone there has wounds to tend.
    pub fn medkit_option(&self, u: usize) -> Option<(PawnId, ItemId)> {
        let unit = self.battle.as_ref()?.units.get(u)?;
        let members: Vec<&Pawn> = unit.members.iter().filter_map(|&m| self.pawn(m)).collect();
        if !members.iter().any(|p| p.health.needs_tending()) {
            return None;
        }
        members.iter().filter(|p| p.active()).find_map(|p| {
            let s = p.utility?;
            matches!(s.kind.def().utility.as_ref()?.effect, UtilityEffect::Medkit { .. }).then_some((p.id, s.kind))
        })
    }

    /// `thrower` throws a grenade at unit `t`: it goes off among them,
    /// wounding everyone (armour helps, cover doesn't) and keeping heads down.
    fn throw_grenade(&mut self, thrower: PawnId, t: usize) {
        let Some(ai) = self.pawn_index(thrower) else { return };
        let Some(kind) = self.pawns[ai].use_utility() else { return };
        let Some(UtilityEffect::Grenade { damage, .. }) = kind.def().utility.as_ref().map(|d| d.effect) else { return };
        let members = self.battle.as_ref().map_or(Vec::new(), |b| b.units[t].members.clone());
        let Some(at) = members.iter().filter_map(|&m| self.pawn(m)).map(|p| (p.x, p.y, p.z)).next() else { return };
        let from = (self.pawns[ai].x, self.pawns[ai].y, self.pawns[ai].z);
        self.pawns[ai].cooldown = 1.0;
        self.effects.push(Effect { from, to: at, ttl: 0.4, hit: true });
        self.sound("rock_break", Some(self.pawns[ai].pos));
        let by = format!("{}'s {}", self.pawns[ai].name, kind.name().to_lowercase());
        for m in members {
            let Some(ti) = self.pawn_index(m).filter(|&i| self.pawns[i].active()) else { continue };
            let dmg = self.rng.range_f32(damage.0, damage.1);
            self.shot_damage(ti, dmg, crate::health::DamageKind::Cut, &by);
        }
        if let Some(b) = self.battle.as_mut() {
            b.units[t].suppression += SUPPRESS_BLAST;
            b.quiet_rounds = 0;
            b.busy = AFTER_ATTACK;
        }
        self.refresh_units();
    }

    /// `medic` uses a medkit on everyone in unit `u`.
    fn use_medkit(&mut self, medic: PawnId, u: usize) {
        let Some(mi) = self.pawn_index(medic) else { return };
        let Some(kind) = self.pawns[mi].use_utility() else { return };
        let Some(UtilityEffect::Medkit { quality }) = kind.def().utility.as_ref().map(|d| d.effect) else { return };
        let skill = self.pawns[mi].skill(Skill::Medicine) as f32;
        let quality = (quality + 0.02 * skill + self.pawns[mi].tend_bonus()).min(1.0);
        self.pawns[mi].learn(Skill::Medicine, 2.0);
        let name = self.pawns[mi].name.clone();
        self.sound("rustle", Some(self.pawns[mi].pos));
        let members = self.battle.as_ref().map_or(Vec::new(), |b| b.units[u].members.clone());
        let mut tended = 0;
        for m in members {
            let Some(i) = self.pawn_index(m) else { continue };
            tended += self.pawns[i].health.tend(quality, 3);
            self.pawns[i].health.recompute();
        }
        self.push_log(format!("{} used a {}: {} wounds tended.", name, kind.name().to_lowercase(), tended));
        if let Some(b) = self.battle.as_mut() {
            b.busy = AFTER_ATTACK;
        }
    }

    /// End the turns of all the colony's units still to act this round (the
    /// raiders still take theirs in between).
    pub fn end_colony_turns(&mut self) -> Result<(), String> {
        let b = self.battle.as_mut().ok_or("there's no battle")?;
        if b.busy() {
            return Err("wait for the last action to finish".into());
        }
        b.colony_passes = true;
        Ok(())
    }

    /// AP to walk unit `u` to `to`, if it can stand there.
    pub fn move_cost(&self, u: usize, to: Pos) -> Result<i32, String> {
        let b = self.battle.as_ref().ok_or("there's no battle")?;
        let unit = b.units.get(u).ok_or("no such unit")?;
        let lead = self.pawn(*unit.members.first().ok_or("no one left")?).ok_or("no one left")?;
        if !self.map.standable(to) {
            return Err("can't stand there".into());
        }
        if self.pawns.iter().any(|p| p.active() && p.pos == to && !unit.members.contains(&p.id)) {
            return Err("someone is standing there".into());
        }
        let d = self.battle_field(unit.side, lead.pos).dist(to).ok_or("can't get there")?;
        if d == 0 {
            return Err("already there".into());
        }
        Ok(d * self.step_ap(u))
    }

    /// AP unit `u` spends per cell it moves: `AP_MOVE` at the slowest
    /// member's pace (more if they limp, less in powered armour), and
    /// `PINNED_MOVE` times as much while pinned down.
    pub fn step_ap(&self, u: usize) -> i32 {
        let Some(unit) = self.battle.as_ref().and_then(|b| b.units.get(u)) else { return AP_MOVE };
        let speed = unit.members.iter().filter_map(|&id| self.pawn(id)).map(|p| p.pace()).fold(f32::MAX, f32::min);
        let speed = if speed == f32::MAX { 1.0 } else { speed.max(0.25) };
        let speed = speed * self.melee_pace(&unit.members);
        let pinned = if unit.held_down() { PINNED_MOVE } else { 1 };
        ((AP_MOVE as f32 / speed).round() as i32).max(1) * pinned
    }

    /// How much faster a unit of `members` moves for fighting in melee
    /// (`MELEE_PACE` if all of them do, lightly equipped; else 1).
    pub fn melee_pace(&self, members: &[PawnId]) -> f32 {
        let melee = !members.is_empty() && members.iter().filter_map(|&id| self.pawn(id)).all(|p| !p.main_attack().ranged);
        if melee {
            MELEE_PACE
        } else {
            1.0
        }
    }

    /// Walking distances from `from` for side `side` in the battle: nobody
    /// gets past an enemy by walking through their cell.
    pub fn battle_field(&self, side: Faction, from: Pos) -> DistField {
        let blocked: std::collections::HashSet<usize> = self
            .battle
            .iter()
            .flat_map(|b| b.units.iter().filter(|u| u.side != side).flat_map(|u| u.members.iter()))
            .filter_map(|&id| self.pawn(id))
            .filter(|p| p.active())
            .map(|p| self.map.idx(p.pos))
            .collect();
        bfs_avoiding(&self.map, from, |i| blocked.contains(&i))
    }

    /// The whole unit sets off for `to` together.
    fn start_move(&mut self, u: usize, to: Pos) {
        let unit = &self.current_battle().units[u];
        let (members, side) = (unit.members.clone(), unit.side);
        // Plan each walk around the enemy (`go_to` keeps a path planned for the same goal).
        for &id in &members {
            let Some(i) = self.pawn_index(id) else { continue };
            let path = self.battle_field(side, self.pawns[i].pos).path_to(to);
            let p = &mut self.pawns[i];
            p.path = path;
            p.path_goal = Some(to);
        }
        let moving = members.iter().map(|&pawn| Mover { pawn, goal: to }).collect();
        let b = self.current_battle_mut();
        b.moving = moving;
        b.hurry = false;
    }

    /// Play the walk under way (anyone's) `HURRY` times faster until it ends.
    /// Returns false if nobody is walking.
    pub fn hurry_move(&mut self) -> bool {
        match self.battle.as_mut() {
            Some(b) if !b.moving.is_empty() => {
                b.hurry = true;
                true
            }
            _ => false,
        }
    }

    /// The cell unit `u` holds (its leader's).
    pub fn unit_cell(&self, u: usize) -> Option<Pos> {
        let b = self.battle.as_ref()?;
        self.pawn(*b.units.get(u)?.members.first()?).map(|p| p.pos)
    }

    /// Who in unit `u` can attack whom in unit `t`: (attacker, target) pawn ids.
    pub fn attack_options(&self, u: usize, t: usize) -> Vec<(PawnId, PawnId)> {
        let Some(b) = &self.battle else { return Vec::new() };
        let (Some(unit), Some(target)) = (b.units.get(u), b.units.get(t)) else { return Vec::new() };
        if unit.side == target.side {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &m in &unit.members {
            let Some(a) = self.pawn(m) else { continue };
            for &tm in &target.members {
                let Some(o) = self.pawn(tm) else { continue };
                if self.can_attack(a, o) {
                    out.push((m, tm));
                }
            }
        }
        out
    }

    /// In reach: within range and in sight for guns, next to them for blows.
    pub fn can_attack(&self, a: &Pawn, o: &Pawn) -> bool {
        self.can_attack_from(a, a.pos, o)
    }

    /// Whether `a` could reach `o` if standing at `from` (with either weapon).
    pub fn can_attack_from(&self, a: &Pawn, from: Pos, o: &Pawn) -> bool {
        self.choose_weapon(a, from, o).is_some()
    }

    /// Whether attack `w` reaches `o` from `from`: in range and in sight for
    /// guns, next to them for blows.
    fn reaches(&self, w: &Attack, from: Pos, o: &Pawn) -> bool {
        if w.ranged {
            cell_dist(from, o.pos) <= w.range && battle_sight(self, from, o.pos)
        } else {
            adjacent(from, o.pos) && from.2 == o.pos.2
        }
    }

    /// Whether `a` keeps their sidearm holstered: in a ranged squad (in a
    /// battle, or by their role outside one) a melee sidearm isn't used, as
    /// a squad fights one way. Melee squads may draw a pistol; lone fighters anything.
    pub fn sidearm_barred(&self, a: &Pawn) -> bool {
        let in_squad = match self.battle.as_ref().and_then(|b| b.unit_of(a.id).map(|u| &b.units[u])) {
            Some(unit) => unit.is_squad(),
            None => matches!(a.role, Role::Leader | Role::Member(_)),
        };
        in_squad && a.main_attack().ranged && !attack_with(a.sidearm).ranged
    }

    /// The weapons each attacker in unit `u` would use on unit `t` (on their first target in reach).
    fn planned_weapons(&self, u: usize, t: usize) -> Vec<(PawnId, Option<ItemId>)> {
        let options = self.attack_options(u, t);
        let mut out: Vec<(PawnId, Option<ItemId>)> = Vec::new();
        for &(a, o) in &options {
            if out.iter().any(|x| x.0 == a) {
                continue;
            }
            let (Some(pa), Some(po)) = (self.pawn(a), self.pawn(o)) else { continue };
            out.push((a, self.choose_weapon(pa, pa.pos, po).flatten()));
        }
        out
    }

    /// AP for unit `u` to attack unit `t`: `AP_SIDEARM_ATTACK` if everyone
    /// who can reach them would use their sidearm, else `AP_ATTACK`.
    pub fn attack_cost(&self, u: usize, t: usize) -> i32 {
        let planned = self.planned_weapons(u, t);
        let quick = !planned.is_empty()
            && planned.iter().all(|&(a, k)| self.pawn(a).is_some_and(|p| k.is_some() && k == p.sidearm && k != p.weapon));
        if quick {
            AP_SIDEARM_ATTACK
        } else {
            AP_ATTACK
        }
    }

    /// The weapon `a` would use on `o` from `from` (`Some(None)`: fists):
    /// of the main weapon and the sidearm, the one that reaches and does
    /// the most harm, likely hits counted. `None` if neither reaches.
    pub fn choose_weapon(&self, a: &Pawn, from: Pos, o: &Pawn) -> Option<Option<ItemId>> {
        let mut best: Option<(f32, Option<ItemId>)> = None;
        let barred = self.sidearm_barred(a);
        for kind in a.weapons().filter(|&k| !(barred && k.is_some() && k == a.sidearm && k != a.weapon)) {
            let w = attack_with(kind);
            if !self.reaches(&w, from, o) {
                continue;
            }
            let shots = kind.and_then(|k| k.def().weapon.as_ref()).map_or(1, |d| d.shots.max(1)) as f32;
            let harm = (w.damage.0 + w.damage.1) / 2.0 * shots * self.hit_chance_with(a, &w, from, o);
            if best.is_none_or(|(b, _)| harm > b) {
                best = Some((harm, kind));
            }
        }
        best.map(|b| b.1)
    }

    /// Line up an attack: each member who can reach someone in unit `t`
    /// takes aim, then they all fire at once (blows land one after another).
    fn attack_unit(&mut self, u: usize, t: usize) -> Result<(), String> {
        let options = self.attack_options(u, t);
        if options.is_empty() {
            return Err("out of reach (range, sight, or not next to them)".into());
        }
        let mut attackers: Vec<PawnId> = options.iter().map(|o| o.0).collect();
        attackers.dedup();
        // Shots go round the unit: everyone's first, then the second shots (rifles fire two).
        // Each with the weapon they'd use on their first target in reach.
        let chosen = self.planned_weapons(u, t);
        let shots: Vec<(PawnId, u32)> =
            chosen.iter().map(|&(a, k)| (a, k.and_then(|k| k.def().weapon.as_ref()).map_or(1, |w| w.shots.max(1)))).collect();
        let most = shots.iter().map(|s| s.1).max().unwrap_or(1);
        let mut strikes = Vec::new();
        let mut at = AIM_TIME;
        for round in 0..most {
            // Shots this volley go off together; blows take turns.
            let (mut blows, mut volley) = (0.0, false);
            for &(a, _) in shots.iter().filter(|s| s.1 > round) {
                let Some(ai) = self.pawn_index(a) else { continue };
                let Some(target) = self.pick_target(ai, t) else { continue };
                let kind = chosen.iter().find(|c| c.0 == a).and_then(|c| c.1);
                let delay = if attack_with(kind).ranged {
                    volley = true;
                    at + self.rng.range_f32(0.0, VOLLEY_SPREAD)
                } else {
                    blows += MELEE_GAP;
                    at + blows - MELEE_GAP
                };
                strikes.push(Strike { attacker: a, target, unit: t, delay });
            }
            at += if volley { VOLLEY_GAP.max(blows) } else { blows };
        }
        strikes.sort_by(|a, b| a.delay.total_cmp(&b.delay));
        let b = self.current_battle_mut();
        b.strikes = strikes;
        b.quiet_rounds = 0;
        // Coming under attack keeps heads down, however many shots it is.
        b.units[t].suppression += SUPPRESS_ATTACK;
        Ok(())
    }

    /// One shot or blow of an attack. If the target is already down, the
    /// attacker picks someone else from that unit in reach.
    fn fire_strike(&mut self, s: Strike) {
        let Some(ai) = self.pawn_index(s.attacker).filter(|&i| self.pawns[i].active()) else { return };
        let target = self.pick_target(ai, s.unit);
        let Some(ti) = target.and_then(|t| self.pawn_index(t)) else { return };
        // Draw the sidearm if it suits better (or put it away again).
        let (a, o) = (&self.pawns[ai], &self.pawns[ti]);
        let kind = self.choose_weapon(a, a.pos, o).unwrap_or(a.held());
        self.pawns[ai].sidearm_drawn = kind.is_some() && kind == self.pawns[ai].sidearm && kind != self.pawns[ai].weapon;
        let chance = self.hit_chance(&self.pawns[ai], &self.pawns[ti]);
        let hit = self.rng.chance(chance);
        let w = self.pawns[ai].attack();
        let skill = if w.ranged { Skill::Shooting } else { Skill::Melee };
        let (from, to) = ((self.pawns[ai].x, self.pawns[ai].y, self.pawns[ai].z), (self.pawns[ti].x, self.pawns[ti].y, self.pawns[ti].z));
        {
            let p = &mut self.pawns[ai];
            // A jump in cooldown is what the front end animates as a shot or blow.
            p.cooldown = 1.0;
            p.learn(skill, 2.0);
        }
        self.sound(w.sound, Some(self.pawns[ai].pos));
        if w.ranged {
            let to = if hit { to } else { (to.0 + self.rng.range_f32(-1.0, 1.0), to.1 + self.rng.range_f32(-1.0, 1.0), to.2) };
            self.effects.push(Effect { from, to, ttl: 0.15, hit });
        }
        let target_id = self.pawns[ti].id;
        if let Some(b) = self.battle.as_mut() {
            if let Some(tu) = b.unit_of(target_id) {
                if hit {
                    b.units[tu].suppression += SUPPRESS_HIT;
                }
            }
        }
        if hit {
            let (lo, hi) = w.damage;
            let dmg = self.rng.range_f32(lo, hi);
            let by = self.pawns[ai].name.clone();
            if w.ranged {
                self.shot_damage(ti, dmg, w.kind, &by);
            } else {
                self.damage(ti, dmg, w.kind, &by);
            }
        }
        self.refresh_units();
    }

    /// Someone in unit `t` the pawn at index `ai` can reach, at random; a
    /// squad's leader only once nobody else in it is left standing.
    fn pick_target(&mut self, ai: usize, t: usize) -> Option<PawnId> {
        let b = self.battle.as_ref()?;
        let unit = b.units.get(t)?;
        let mut targets: Vec<PawnId> = unit
            .members
            .iter()
            .copied()
            .filter(|&o| self.pawn(o).is_some_and(|o| o.active() && self.can_attack(&self.pawns[ai], o)))
            .collect();
        if let Some(leader) = unit.leader().filter(|_| targets.len() > 1) {
            targets.retain(|&o| o != leader);
        }
        (!targets.is_empty()).then(|| targets[self.rng.below(targets.len())])
    }

    /// Who pawn `id` is aiming at in the attack playing out, if anyone.
    pub fn aim_target(&self, id: PawnId) -> Option<PawnId> {
        self.battle.as_ref()?.strikes.iter().find(|s| s.attacker == id).map(|s| s.target)
    }

    /// Chance that `a`'s shot or blow hits `o`.
    pub fn hit_chance(&self, a: &Pawn, o: &Pawn) -> f32 {
        self.hit_chance_from(a, a.pos, o)
    }

    /// Chance that `a`'s shot or blow hits `o` if `a` stood at `from`, with
    /// the weapon they'd use there (or the one in hand, if none reaches).
    pub fn hit_chance_from(&self, a: &Pawn, from: Pos, o: &Pawn) -> f32 {
        let w = attack_with(self.choose_weapon(a, from, o).unwrap_or(a.held()));
        self.hit_chance_with(a, &w, from, o)
    }

    /// Chance that `a`'s shot or blow with attack `w` hits `o` from `from`.
    pub(crate) fn hit_chance_with(&self, a: &Pawn, w: &Attack, from: Pos, o: &Pawn) -> f32 {
        let d = cell_dist(from, o.pos);
        let mut c = base_hit_chance(a, w.ranged, d);
        if w.long_gun && d < 1.5 {
            c *= CLOSE_QUARTERS;
        }
        let b = self.battle.as_ref();
        let unit = |id: PawnId| b.and_then(|b| b.unit_of(id).map(|u| &b.units[u]));
        if w.ranged {
            c *= self.cover(from, o.pos).hit_factor();
            if unit(o.id).is_some_and(|u| u.is_hunkered()) {
                c *= 0.75;
            }
            if unit(o.id).is_some_and(|u| u.is_deployed()) {
                c *= DEPLOYED_TARGET;
            }
            if unit(a.id).is_some_and(|u| u.is_deployed()) {
                c *= DEPLOYED_AIM;
            }
            if from.2 > o.pos.2 {
                c *= 1.15;
            }
        }
        if unit(a.id).is_some_and(|u| u.pinned()) {
            c *= 0.7;
        }
        c.clamp(0.05, 0.95)
    }

    /// Cover someone at `at` has from shots coming from `from`: what stands
    /// on the side facing the shooter. Shooting down weakens it.
    pub fn cover(&self, from: Pos, at: Pos) -> Cover {
        let m = &self.map;
        let dx = (from.0 as i32 - at.0 as i32).signum();
        let dy = (from.1 as i32 - at.1 as i32).signum();
        let mut best = Cover::None;
        for (cx, cy) in [(dx, 0), (0, dy)] {
            if (cx, cy) == (0, 0) {
                continue;
            }
            let Some(c) = m.at(at.0 as i32 + cx, at.1 as i32 + cy, at.2 as i32) else { continue };
            best = best.max(self.cover_at(c));
        }
        if from.2 > at.2 {
            best.weaker()
        } else {
            best
        }
    }

    /// How much cover the cell `c` gives: rock and soil heavy; structures
    /// and plants as their definition says (see `BuildDef::cover`).
    pub fn cover_at(&self, c: Pos) -> Cover {
        let m = &self.map;
        let i = m.idx(c);
        if m.block[i].solid() {
            return Cover::Heavy;
        }
        match m.feature[i] {
            crate::map::Feature::Built { kind, .. } => kind.def().cover_kind(),
            crate::map::Feature::Plant { kind, .. } => kind.def().cover_kind(),
            // Blueprints are only planned; nothing stands there yet.
            crate::map::Feature::None | crate::map::Feature::Blueprint { .. } => Cover::None,
        }
    }

    fn end_activation(&mut self) {
        let Some(b) = self.battle.as_mut() else { return };
        if let Some(a) = b.active.take() {
            b.units[a.unit].acted = true;
        }
        let (side, next) = (b.side_to_act, other(b.side_to_act));
        if b.has_turn_left(next) {
            b.side_to_act = next;
        } else if !b.has_turn_left(side) {
            self.new_round();
        }
    }

    fn new_round(&mut self) {
        {
            let mut shields = Vec::new();
            let bonus: Vec<i32> = self.current_battle().units.iter().map(|u| self.ap_bonus(&u.members)).collect();
            let b = self.current_battle_mut();
            b.round += 1;
            for (u, bonus) in b.units.iter_mut().zip(bonus) {
                u.acted = false;
                for &id in &u.members {
                    shields.push(id);
                }
                u.ap = if u.pinned() { AP_MAX * 6 / 10 } else { AP_MAX } + bonus;
                u.ap_start = u.ap;
                if !u.pinned() {
                    u.morale = (u.morale + 5.0).min(90.0);
                }
                u.suppression *= 0.5;
            }
            b.side_to_act = Faction::Colony;
            b.colony_passes = false;
            // Shields recharge a little each round.
            for id in shields {
                if let Some(i) = self.pawns.iter().position(|p| p.id == id) {
                    let p = &mut self.pawns[i];
                    let recharge = p.shield().map_or(0.0, |s| s.1);
                    p.shield_drain = (p.shield_drain - recharge).max(0.0);
                }
            }
            let b = self.current_battle_mut();
            for a in b.arrivals.iter_mut() {
                a.rounds = a.rounds.saturating_sub(1);
            }
            b.quiet_rounds += 1;
        }
        // Closing in counts as progress too.
        let gap = self.gap_between_sides();
        let b = self.current_battle_mut();
        if gap.is_some_and(|g| b.closest.is_none_or(|c| g < c)) {
            b.closest = gap;
            b.quiet_rounds = 0;
        }
        if self.battle.as_ref().is_some_and(|b| b.quiet_rounds >= STALEMATE) {
            self.push_log("The raiders gave up and left.".into());
            self.end_battle(Outcome::Won);
            return;
        }
        // Time passes for wounds and hunger.
        self.advance_clock(TURN_SECS);
        for i in 0..self.pawns.len() {
            let mut p = self.pawns[i].clone();
            if p.present() {
                self.update_health(&mut p, TURN_SECS);
                if p.is_colonist() {
                    p.hunger = (p.hunger - HUNGER_DRAIN * TURN_SECS).max(0.0);
                    p.rest = (p.rest - REST_DRAIN * TURN_SECS).max(0.0);
                }
            }
            self.pawns[i] = p;
        }
        self.arrive();
        // Wounds may have decided it.
        self.refresh_units();
        if let Some(b) = self.battle.as_mut() {
            if !b.has_turn_left(Faction::Colony) {
                b.side_to_act = Faction::Raider;
            }
        }
    }

    /// Fewest cells between any fighter of the colony and any raider.
    fn gap_between_sides(&self) -> Option<usize> {
        let b = self.battle.as_ref()?;
        let side = |s: Faction| -> Vec<Pos> {
            b.units
                .iter()
                .filter(|u| u.side == s && !u.is_civilians())
                .flat_map(|u| u.members.iter().filter_map(|&id| self.pawn(id)).map(|p| p.pos))
                .collect()
        };
        let (ours, theirs) = (side(Faction::Colony), side(Faction::Raider));
        ours.iter()
            .flat_map(|a| theirs.iter().map(move |b| a.0.abs_diff(b.0) + a.1.abs_diff(b.1) + a.2.abs_diff(b.2)))
            .min()
    }

    /// Squad members whose time is up join their leader.
    fn arrive(&mut self) {
        let b = self.current_battle_mut();
        let (due, waiting): (Vec<Arrival>, Vec<Arrival>) = b.arrivals.drain(..).partition(|a| a.rounds == 0);
        b.arrivals = waiting;
        for a in due {
            let Some(pi) = self.pawn_index(a.pawn).filter(|&i| self.pawns[i].active()) else { continue };
            let b = self.current_battle();
            let leader_unit = b.unit_of(a.leader).filter(|&u| !b.units[u].out());
            let name = self.pawns[pi].name.clone();
            match leader_unit {
                Some(u) => {
                    let members = b.units[u].members.clone();
                    let at = self.pawn(members[0]).unwrap().pos;
                    place(&mut self.pawns[pi], at);
                    let b = self.current_battle_mut();
                    b.units[u].members.push(a.pawn);
                    b.units[u].roster.push(a.pawn);
                    b.units[u].morale = (b.units[u].morale + 5.0).min(90.0);
                    let leader = self.pawn(members[0]).map_or(String::new(), |l| l.name.clone());
                    self.battle_event(format!("{} joins {}'s squad.", name, leader));
                }
                None => {
                    let morale = self.unit_morale(&[a.pawn]);
                    let b = self.current_battle_mut();
                    b.units.push(Unit::new(Faction::Colony, vec![a.pawn], UnitKind::Lone, morale));
                    self.battle_event(format!("{} arrives; their leader is down, so they fight alone.", name));
                }
            }
        }
    }

    fn battle_event(&mut self, msg: String) {
        if let Some(b) = self.battle.as_mut() {
            b.events.push(msg);
            if b.events.len() > 30 {
                b.events.remove(0);
            }
        }
    }

    /// Drop members who are down, dead or gone; losses shake morale, losing
    /// the leader most. Ends the battle when a side has no one left.
    fn refresh_units(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let mut events = Vec::new();
        let mut units = b.units.clone();
        for u in units.iter_mut() {
            let before = u.members.clone();
            let was_broken = u.broken();
            u.members.retain(|&id| self.pawn(id).is_some_and(|p| p.active()));
            let lost = before.len() - u.members.len();
            if lost == 0 {
                continue;
            }
            u.morale -= 20.0 * lost as f32;
            // Losing the leader hurts once; whoever is left fights on leaderless.
            if let Some(leader) = u.leader().filter(|l| !u.members.contains(l)) {
                u.kind = UnitKind::Squad { leader: None };
                u.morale -= 20.0;
                if let (Some(l), false) = (self.pawn(leader), u.out()) {
                    events.push(format!("{}'s squad lost its leader.", l.name));
                }
            }
            if !was_broken && u.broken() && !u.out() && !u.is_civilians() {
                let who = self.pawn(u.members[0]).map_or(String::new(), |p| p.name.clone());
                events.push(match u.side {
                    Faction::Raider => format!("{}'s group loses its nerve and runs.", who),
                    Faction::Colony => format!("{} is shaken and takes cover.", who),
                });
            }
        }
        let b = self.current_battle_mut();
        b.units = units;
        for e in events {
            self.battle_event(e);
        }
        self.check_battle_end();
    }

    fn check_battle_end(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let raiders = b.units.iter().any(|u| u.side == Faction::Raider && !u.out());
        let colony = b.units.iter().any(|u| u.side == Faction::Colony && u.can_act()) || !b.arrivals.is_empty();
        if !raiders {
            self.end_battle(Outcome::Won);
        } else if !colony {
            self.end_battle(Outcome::Lost);
        }
    }

    /// Advance a battle: play out moves and attacks, then let the AI act
    /// (or wait for the player).
    pub(crate) fn battle_tick(&mut self, dt: f32) {
        for p in self.pawns.iter_mut() {
            p.cooldown = (p.cooldown - dt).max(0.0);
        }
        let Some(b) = self.battle.as_mut() else { return };
        if b.busy > 0.0 {
            b.busy -= dt;
            return;
        }
        if !b.strikes.is_empty() {
            for s in b.strikes.iter_mut() {
                s.delay -= dt;
            }
            let (due, later): (Vec<Strike>, Vec<Strike>) = b.strikes.drain(..).partition(|s| s.delay <= 0.0);
            b.strikes = later;
            for s in due {
                if self.battle.is_none() {
                    return;
                }
                self.fire_strike(s);
            }
            if let Some(b) = self.battle.as_mut() {
                if b.strikes.is_empty() {
                    b.busy = AFTER_ATTACK;
                }
            }
            return;
        }
        if !b.moving.is_empty() {
            let moving = b.moving.clone();
            // Hurried: several ordinary steps per tick, so it's just the same walk, faster.
            let steps = if b.hurry { HURRY } else { 1 };
            // Everyone keeps to the pace of the slowest, so the squad stays together.
            let pace = moving.iter().filter_map(|m| self.pawn(m.pawn)).map(|p| p.move_speed()).fold(f32::MAX, f32::min);
            let ids: Vec<PawnId> = moving.iter().map(|m| m.pawn).collect();
            let pace = pace * self.melee_pace(&ids);
            let mut still = Vec::new();
            for m in moving {
                let Some(i) = self.pawn_index(m.pawn) else { continue };
                let mut p = self.pawns[i].clone();
                let step = dt * pace / p.move_speed().max(0.01);
                let walking = (0..steps).all(|_| matches!(self.go_to(&mut p, m.goal, false, step), Step::Moving));
                if walking {
                    still.push(m);
                }
                self.pawns[i] = p;
            }
            let done = still.is_empty();
            let b = self.current_battle_mut();
            b.moving = still;
            b.hurry &= !done;
            if done {
                self.escape_at_edges();
                self.refresh_units();
            }
            return;
        }
        let b = self.current_battle();
        if let Some(u) = b.active_unit() {
            if b.units[u].out() || b.units[u].ap < self.step_ap(u) {
                self.end_activation();
                return;
            }
        }
        let b = self.current_battle();
        if b.side_to_act == Faction::Raider || b.auto_colony {
            self.ai_step();
        } else if b.colony_passes {
            // Passing for the round: end the active unit's turn, or the next one's.
            let next = b.active_unit().or_else(|| b.units.iter().position(|u| u.side == Faction::Colony && u.can_act() && !u.acted));
            if let Some(u) = next {
                if b.active.is_none() {
                    let _ = self.do_order(BattleOrder::Activate(u));
                }
                let _ = self.do_order(BattleOrder::End);
            }
        } else if b.active.is_none() {
            // Shaken colonists sit their turn out on their own.
            let cowering = b
                .units
                .iter()
                .position(|u| u.side == Faction::Colony && u.can_act() && !u.acted && u.broken());
            let others = b.units.iter().any(|u| u.side == Faction::Colony && u.can_act() && !u.acted && !u.broken());
            if let (Some(u), false) = (cowering, others) {
                let _ = self.do_order(BattleOrder::Activate(u));
                let _ = self.do_order(BattleOrder::Hunker);
            }
        }
    }

    /// Fleeing raiders who reach the edge of the map are gone.
    fn escape_at_edges(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let (w, h) = (self.map.w, self.map.h);
        let fleeing: Vec<PawnId> = b
            .units
            .iter()
            .filter(|u| u.side == Faction::Raider && u.broken())
            .flat_map(|u| u.members.iter().copied())
            .collect();
        for id in fleeing {
            let Some(i) = self.pawn_index(id) else { continue };
            let p = &mut self.pawns[i];
            if p.pos.0 == 0 || p.pos.1 == 0 || p.pos.0 == w - 1 || p.pos.1 == h - 1 {
                p.life = Life::Left;
                let name = p.name.clone();
                self.battle_event(format!("{} fled.", name));
            }
        }
    }
}

/// Line of sight for battles: a clear line between the two cells (see
/// `clear_line`). Heavy cover (rock, walls, doors) blocks it; there's no
/// peeking past it.
pub fn battle_sight(w: &World, a: Pos, b: Pos) -> bool {
    clear_line(&w.map, a, b)
}

/// Air that doesn't block the view: no rock, soil, wall or door.
fn see_through(m: &crate::map::Map, c: Pos) -> bool {
    let i = m.idx(c);
    !m.block[i].solid() && !m.feature[i].blocks_sight()
}

/// A straight line at chest height from the middle of cell `a` to the
/// middle of cell `b`, through every column of cells it crosses. Rock,
/// soil, walls and doors block it (the two ends' own cells aren't checked),
/// and so does a built floor wherever the line passes through it from one
/// level to another. Squeezing past a corner is blocked only if both cells
/// beside the corner block.
fn clear_line(m: &crate::map::Map, a: Pos, b: Pos) -> bool {
    let (x0, y0, x1, y1) = (a.0 as f32 + 0.5, a.1 as f32 + 0.5, b.0 as f32 + 0.5, b.1 as f32 + 0.5);
    let (z0, z1) = (a.2 as f32 + 0.6, b.2 as f32 + 0.6);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let (sx, sy) = (dx.signum() as i32, dy.signum() as i32);
    // Line parameter t (0..1) at the next border in x and in y, and per cell.
    let border = |c: usize, s: i32, o: f32, d: f32| if s == 0 { f32::MAX } else { ((c as i32 + (s > 0) as i32) as f32 - o) / d };
    let (mut tx, mut ty) = (border(a.0, sx, x0, dx), border(a.1, sy, y0, dy));
    let (stepx, stepy) = (if sx == 0 { f32::MAX } else { 1.0 / dx.abs() }, if sy == 0 { f32::MAX } else { 1.0 / dy.abs() });
    let height = |t: f32| z0 + (z1 - z0) * t;
    // Is the line blocked in column (x, y) between t0 and t1?
    let blocked = |x: i32, y: i32, t0: f32, t1: f32| -> bool {
        let (h0, h1) = (height(t0), height(t1));
        let (lo, hi) = (h0.min(h1).floor() as i32, h0.max(h1).floor() as i32);
        for z in lo..=hi {
            let Some(c) = m.at(x, y, z) else { continue };
            if z > lo && (m.floor[m.idx(c)] || m.roof[m.idx(c)]) {
                return true;
            }
            if c != a && c != b && !see_through(m, c) {
                return true;
            }
        }
        false
    };
    let (mut cx, mut cy, mut t) = (a.0 as i32, a.1 as i32, 0.0);
    let target = (b.0 as i32, b.1 as i32);
    for _ in 0..1000 {
        let exit = tx.min(ty).min(1.0);
        if blocked(cx, cy, t, exit) {
            return false;
        }
        if (cx, cy) == target {
            return true;
        }
        if (tx - ty).abs() < 1e-5 {
            // Through a corner: the cells either side only touch the line.
            if blocked(cx + sx, cy, tx, tx) && blocked(cx, cy + sy, tx, tx) {
                return false;
            }
            (cx, cy, t) = (cx + sx, cy + sy, tx);
            tx += stepx;
            ty += stepy;
        } else if tx < ty {
            (cx, t) = (cx + sx, tx);
            tx += stepx;
        } else {
            (cy, t) = (cy + sy, ty);
            ty += stepy;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{content_colonist as colonist, item, quiet_world};
    use crate::map::{Feature, FLAT_Z as Z};
    use crate::TICK;


    fn world(w: usize, h: usize, seed: u64) -> World {
        quiet_world(w, h, seed)
    }


    fn fight(w: &mut World, raiders: usize, entry: Pos) {
        w.start_battle(Parley { squads: vec![raiders], entry, demand: Vec::new(), tier: 0 });
    }

    #[test]
    fn squads_fight_one_way() {
        let mut w = world(30, 30, 4);
        let (lead, mate, odd) = (colonist(&mut w, (5, 10, Z)), colonist(&mut w, (5, 11, Z)), colonist(&mut w, (6, 10, Z)));
        for (id, gun) in [(lead, "rifle"), (mate, "pistol"), (odd, "pistol")] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item(gun));
        }
        w.set_role(lead, Role::Leader).unwrap();
        w.set_role(mate, Role::Member(lead)).unwrap();
        w.set_role(odd, Role::Member(lead)).unwrap();
        // Afterwards their gun goes, and they pick up a club.
        let i = w.pawn_index(odd).unwrap();
        w.pawns[i].weapon = Some(item("club"));
        fight(&mut w, 8, (25, 25, Z));
        let b = battle(&w);
        let squad = &b.units[b.unit_of(lead).unwrap()];
        assert_eq!(squad.members, vec![lead, mate]);
        let alone = &b.units[b.unit_of(odd).unwrap()];
        assert!(!alone.is_squad() && alone.members == vec![odd], "the clubber fights alone");
        assert_eq!(alone.kind, UnitKind::Lone, "the mismatched member is a lone fighter");
        // Raider squads are armed alike too.
        for u in b.units.iter().filter(|u| u.side == Faction::Raider) {
            let kinds: Vec<bool> = u.members.iter().map(|&id| w.pawn(id).unwrap().main_attack().ranged).collect();
            assert!(kinds.windows(2).all(|k| k[0] == k[1]), "{:?}", kinds);
        }
    }

    fn battle(w: &World) -> &Battle {
        w.battle.as_ref().expect("a battle is on")
    }

    /// Tick until the battle is over (or give up).
    fn play_out(w: &mut World, secs: f32) {
        play_out_observing(w, secs, |_| {});
    }

    fn play_out_observing(w: &mut World, secs: f32, mut observe: impl FnMut(&World)) {
        for _ in 0..(secs / TICK) as usize {
            if w.battle.is_none() {
                return;
            }
            observe(w);
            w.tick(TICK);
        }
    }

    fn raiders(w: &World) -> Vec<PawnId> {
        w.pawns.iter().filter(|p| !p.is_colonist()).map(|p| p.id).collect()
    }

    #[test]
    fn paying_the_raiders_sends_them_away() {
        let mut w = world(20, 20, 1);
        colonist(&mut w, (5, 5, Z));
        w.map.drop_item((3, 3, Z), Stack { kind: item("steel"), count: 30 });
        w.map.drop_item((4, 3, Z), Stack { kind: item("wood"), count: 50 });
        w.open_parley(3, (19, 10, Z), 0);
        let demand = w.parley.as_ref().unwrap().demand.clone();
        let value: f32 = demand.iter().map(|s| s.count as f32 * s.kind.def().value).sum();
        assert!(value >= 3.0 * DEMAND_PER_RAIDER, "they want enough: {:?}", demand);
        assert_eq!(demand[0].kind, item("steel"), "the most valuable first");

        // The colony stands still while you decide.
        let t = w.time();
        w.tick(TICK);
        assert_eq!(w.time(), t);

        assert!(w.pay_tribute());
        assert!(w.parley.is_none() && w.battle.is_none());
        assert!(raiders(&w).is_empty(), "nobody attacks");
        assert_eq!(w.map.count_items(item("steel")), 30 - demand[0].count);
    }

    #[test]
    fn a_poor_colony_has_to_fight() {
        let mut w = world(20, 20, 1);
        colonist(&mut w, (5, 5, Z));
        w.open_parley(3, (19, 10, Z), 0);
        assert!(w.parley.as_ref().unwrap().demand.is_empty());
        assert!(!w.pay_tribute(), "nothing to pay with");
        let squads = w.parley.as_ref().unwrap().squads.len();
        w.refuse_tribute();
        assert_eq!(battle(&w).units.iter().filter(|u| u.side == Faction::Raider).count(), squads, "a unit per squad");
        assert_eq!(raiders(&w).len(), 3);
    }

    #[test]
    fn squads_form_around_their_leaders() {
        let mut w = world(40, 20, 2);
        let leader = colonist(&mut w, (5, 5, Z));
        let near = colonist(&mut w, (6, 5, Z));
        let far = colonist(&mut w, (17, 5, Z));
        let lone = colonist(&mut w, (5, 15, Z));
        let civ = colonist(&mut w, (30, 15, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(near, Role::Member(leader)).unwrap();
        w.set_role(far, Role::Member(leader)).unwrap();
        w.set_role(civ, Role::Civilian).unwrap();
        for x in 2..4 {
            let i = w.map.idx((x, 2, Z));
            w.map.zone[i] = Zone::Safe;
        }
        fight(&mut w, 2, (39, 10, Z));
        let b = battle(&w);
        let squad = &b.units[b.unit_of(leader).unwrap()];
        assert_eq!(squad.members, vec![leader, near], "the near one is with the leader");
        assert_eq!(w.pawn(near).unwrap().pos, w.pawn(leader).unwrap().pos, "a squad holds one cell");
        assert!(squad.is_squad());
        // 12 cells away: 10 beyond the leader's side, run in one round.
        assert_eq!(b.arrivals.len(), 1);
        assert_eq!((b.arrivals[0].pawn, b.arrivals[0].rounds), (far, 1));
        assert_eq!(b.units[b.unit_of(lone).unwrap()].members, vec![lone], "fights alone");
        let civilians = &b.units[b.unit_of(civ).unwrap()];
        assert!(civilians.is_civilians() && !civilians.can_act());
        assert_eq!(w.map.zone[w.map.idx(w.pawn(civ).unwrap().pos)], Zone::Safe, "ran to the safe zone");

        // A round later, the far member stands beside the leader.
        w.new_round();
        let b = battle(&w);
        assert!(b.arrivals.is_empty());
        assert!(b.units[b.unit_of(leader).unwrap()].members.contains(&far));
        let (a, l) = (w.pawn(far).unwrap().pos, w.pawn(leader).unwrap().pos);
        assert_eq!(a, l, "arrived on the leader's cell");
    }

    #[test]
    fn sides_take_turns_one_unit_at_a_time() {
        let mut w = world(40, 20, 4);
        let a = colonist(&mut w, (5, 5, Z));
        let b2 = colonist(&mut w, (5, 15, Z));
        fight(&mut w, 1, (39, 10, Z));
        let unit = |w: &World, id: PawnId| battle(w).unit_of(id).unwrap();
        let (ua, ub) = (unit(&w, a), unit(&w, b2));
        let raider = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        assert_eq!(battle(&w).side_to_act, Faction::Colony, "the colony goes first");
        assert!(w.battle_order(BattleOrder::Activate(raider)).is_err(), "not their turn");
        w.battle_order(BattleOrder::Activate(ua)).unwrap();
        w.battle_order(BattleOrder::End).unwrap();
        assert_eq!(battle(&w).side_to_act, Faction::Raider);
        assert!(w.battle_order(BattleOrder::Activate(ub)).is_err(), "the raiders are up");
        w.do_order(BattleOrder::Activate(raider)).unwrap();
        w.do_order(BattleOrder::End).unwrap();
        // The raiders are done for the round; the colony plays its other unit.
        assert_eq!(battle(&w).side_to_act, Faction::Colony);
        assert!(w.battle_order(BattleOrder::Activate(ua)).is_err(), "already acted");
        w.battle_order(BattleOrder::Activate(ub)).unwrap();
        let t = w.time();
        w.battle_order(BattleOrder::End).unwrap();
        let b = battle(&w);
        assert_eq!(b.round, 2, "everyone has acted: a new round");
        assert!(b.units.iter().all(|u| !u.acted));
        assert_eq!(w.time(), t + TURN_SECS, "time passes per round");
    }

    #[test]
    fn squads_move_together() {
        let mut w = world(40, 20, 16);
        let leader = colonist(&mut w, (5, 10, Z));
        let a = colonist(&mut w, (5, 11, Z));
        let b2 = colonist(&mut w, (6, 10, Z));
        w.set_role(leader, Role::Leader).unwrap();
        for m in [a, b2] {
            w.set_role(m, Role::Member(leader)).unwrap();
        }
        // One of them limps: the squad keeps to their pace.
        let i = w.pawn_index(b2).unwrap();
        w.pawns[i].health.caps.moving = 0.6;
        fight(&mut w, 1, (39, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        assert_eq!(w.unit_cell(u), Some((5, 10, Z)));
        // Another unit's cell can't be shared.
        let lone = colonist(&mut w, (9, 10, Z));
        w.battle.as_mut().unwrap().units.push(Unit::new(Faction::Colony, vec![lone], UnitKind::Lone, 50.0));
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert!(w.move_cost(u, (9, 10, Z)).is_err(), "taken by another unit");
        // The limp costs AP too: 17 a step at 60% (too far to (10, 12), 7 cells).
        assert_eq!(w.step_ap(u), 17);
        assert!(w.battle_order(BattleOrder::Move((10, 12, Z))).is_err());
        w.battle_order(BattleOrder::Move((8, 11, Z))).unwrap();
        for _ in 0..(0.7 / TICK) as usize {
            w.tick(TICK);
            let at = |id: PawnId| {
                let p = w.pawn(id).unwrap();
                (p.x, p.y)
            };
            let (l, x, y) = (at(leader), at(a), at(b2));
            assert!((l.0 - x.0).abs() + (l.1 - x.1).abs() < 0.01 && (l.0 - y.0).abs() + (l.1 - y.1).abs() < 0.01, "together");
        }
        for _ in 0..(8.0 / TICK) as usize {
            w.tick(TICK);
        }
        for id in [leader, a, b2] {
            assert_eq!(w.pawn(id).unwrap().pos, (8, 11, Z), "everyone on the new cell");
        }
        assert!(!battle(&w).busy());
    }

    #[test]
    fn action_points_limit_what_a_unit_does() {
        let mut w = world(40, 20, 5);
        let id = colonist(&mut w, (5, 10, Z));
        fight(&mut w, 1, (39, 10, Z));
        let u = battle(&w).unit_of(id).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert_eq!(w.move_cost(u, (8, 10, Z)), Ok(3 * AP_MOVE));
        assert!(w.battle_order(BattleOrder::Move((17, 10, Z))).is_err(), "12 cells is more than 100 AP");
        w.battle_order(BattleOrder::Move((8, 10, Z))).unwrap();
        assert!(w.battle_order(BattleOrder::End).is_err(), "busy walking");
        for _ in 0..60 {
            w.tick(TICK);
        }
        assert_eq!(w.pawn(id).unwrap().pos, (8, 10, Z));
        assert_eq!(battle(&w).units[u].ap, AP_MAX - 3 * AP_MOVE);
        let raider = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        assert!(w.battle_order(BattleOrder::Attack(raider)).is_err(), "AP left, but 25 cells is out of range");
        assert!(w.battle_order(BattleOrder::Activate(raider)).is_err());
    }

    #[test]
    fn small_raids_come_in_small_squads() {
        let mut rng = crate::rng::Rng::new(5);
        // How often each split comes up, out of 1000 raids of `n`.
        let mut splits = |n: usize| {
            let mut seen: std::collections::HashMap<Vec<usize>, u32> = Default::default();
            for _ in 0..1000 {
                let s = raider_squads(n, &mut rng);
                assert_eq!(s.iter().sum::<usize>(), n);
                assert!(s.iter().all(|&g| (1..=RAIDER_SQUAD).contains(&g)));
                assert!(s.windows(2).all(|w| w[0] >= w[1]), "largest first");
                *seen.entry(s).or_default() += 1;
            }
            seen
        };
        assert_eq!(splits(1).get(&vec![1]), Some(&1000));
        let three = splits(3);
        assert!(three[&vec![1, 1, 1]] > 600, "mostly alone: {:?}", three);
        let four = splits(4);
        assert!(four[&vec![2, 2]] > 500 && four[&vec![2, 1, 1]] > 200, "pairs: {:?}", four);
        assert!(!four.contains_key(&vec![4]), "no full squad");
        let mut full = |n| splits(n).iter().filter(|(s, _)| s[0] == RAIDER_SQUAD).map(|(_, &c)| c).sum::<u32>();
        assert!(full(6) < 100, "rarely a full squad at 6");
        assert!(full(10) > 900, "usually one at 10");
    }

    #[test]
    fn enemies_block_the_way() {
        let mut w = world(30, 20, 3);
        // A dead-end corridor along y = 10 between walls, x 4..=16, closed at x = 17.
        let mut walls = vec![(17, 10)];
        for x in 4..=17 {
            walls.extend([(x, 9), (x, 11)]);
        }
        for (x, y) in walls {
            let i = w.map.idx((x, y, Z));
            w.map.set_feature(i, crate::map::Feature::built("wall"));
        }
        let id = colonist(&mut w, (5, 10, Z));
        fight(&mut w, 1, (29, 10, Z));
        // The raider stands in the middle of it.
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].pos = (10, 10, Z);
        (w.pawns[ri].x, w.pawns[ri].y) = (10.0, 10.0);
        let u = battle(&w).unit_of(id).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert_eq!(w.move_cost(u, (8, 10, Z)), Ok(3 * AP_MOVE), "up to them is fine");
        assert!(w.move_cost(u, (11, 10, Z)).is_err(), "not past them");
        // A way round (gaps in the wall either side of them): the long way it is.
        for x in [9, 11] {
            let i = w.map.idx((x, 11, Z));
            w.map.set_feature(i, crate::map::Feature::None);
        }
        let cost = w.move_cost(u, (11, 10, Z)).unwrap();
        assert_eq!(cost, 10 * AP_MOVE, "round them, not the 6 steps through");
        w.battle_order(BattleOrder::Move((11, 10, Z))).unwrap();
        let mut through = false;
        for _ in 0..(10.0 / TICK) as usize {
            w.tick(TICK);
            through |= w.pawn(id).unwrap().pos == (10, 10, Z);
        }
        assert!(!through, "never stepped into the raider's cell");
        assert_eq!(w.pawn(id).unwrap().pos, (11, 10, Z), "got there");
    }

    #[test]
    fn clicking_again_hurries_a_walk() {
        // Ticks for a lone colonist to walk 8 cells, hurried or not.
        let walk = |hurry: bool| {
            let mut w = world(40, 20, 5);
            let id = colonist(&mut w, (5, 10, Z));
            fight(&mut w, 1, (39, 10, Z));
            let u = battle(&w).unit_of(id).unwrap();
            assert!(!w.hurry_move(), "nobody walking yet");
            w.battle_order(BattleOrder::Activate(u)).unwrap();
            w.battle_order(BattleOrder::Move((13, 10, Z))).unwrap();
            if hurry {
                w.tick(TICK);
                assert!(w.hurry_move());
            }
            let mut ticks = 0;
            while !battle(&w).moving.is_empty() {
                w.tick(TICK);
                ticks += 1;
                assert!(ticks < 10_000, "never arrived");
            }
            assert_eq!(w.pawn(id).unwrap().pos, (13, 10, Z));
            assert!(!battle(&w).hurry, "only until the walk ends");
            ticks
        };
        let (slow, fast) = (walk(false), walk(true));
        assert!(fast * 4 < slow && fast * 6 > slow, "{} ticks hurried vs {}", fast, slow);
    }

    #[test]
    fn cover_and_height_change_the_odds() {
        let mut w = world(30, 30, 6);
        let shooter = colonist(&mut w, (5, 15, Z));
        let target = colonist(&mut w, (12, 15, Z));
        let chance = |w: &World| w.hit_chance(w.pawn(shooter).unwrap(), w.pawn(target).unwrap());
        let open = chance(&w);
        let beside = w.map.idx((11, 15, Z));
        w.map.feature[beside] = Feature::built("fence");
        assert_eq!(w.cover((5, 15, Z), (12, 15, Z)), Cover::Light);
        let light = chance(&w);
        w.map.feature[beside] = Feature::built("shelf");
        assert_eq!(w.cover((5, 15, Z), (12, 15, Z)), Cover::Medium);
        let medium = chance(&w);
        assert!(open > light && light > medium, "{} > {} > {}", open, light, medium);
        assert_eq!(w.cover((12, 25, Z), (12, 15, Z)), Cover::None, "no cover on the open side");

        // From a level up, medium cover is only light, and height helps aim.
        let i = w.pawn_index(shooter).unwrap();
        w.pawns[i].pos.2 = Z + 1;
        w.pawns[i].z = (Z + 1) as f32;
        assert_eq!(w.cover((5, 15, Z + 1), (12, 15, Z)), Cover::Light);
        assert!(chance(&w) > medium);

        // Heavy cover (a wall) blocks the view: no shot at all.
        w.pawns[i].pos.2 = Z;
        w.pawns[i].z = Z as f32;
        w.map.feature[beside] = Feature::built("wall");
        assert_eq!(w.cover_at((11, 15, Z)), Cover::Heavy);
        assert!(!battle_sight(&w, (5, 15, Z), (12, 15, Z)), "nobody shoots through a wall");
    }

    #[test]
    fn suppression_pins_and_losses_break_morale() {
        let mut w = world(40, 20, 7);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (6, 10, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        fight(&mut w, 1, (39, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        w.battle.as_mut().unwrap().units[u].suppression = 80.0;
        w.new_round();
        assert_eq!(battle(&w).units[u].ap, AP_MAX * 6 / 10, "pinned: fewer action points");
        assert_eq!(w.step_ap(u), AP_MOVE * PINNED_MOVE, "and moving costs more");

        let before = battle(&w).units[u].morale;
        let i = w.pawn_index(leader).unwrap();
        let mut p = w.pawns[i].clone();
        w.kill(&mut p, "was shot");
        w.pawns[i] = p;
        w.refresh_units();
        let b = battle(&w);
        assert_eq!(b.units[u].members, vec![member], "the squad fights on");
        assert_eq!(b.units[u].morale, before - 40.0, "one casualty plus the leader penalty");
        assert_eq!(b.units[u].kind, UnitKind::Squad { leader: None });
    }

    #[test]
    fn broken_raiders_run_off_the_map() {
        let mut w = world(30, 12, 8);
        let id = colonist(&mut w, (3, 6, Z));
        fight(&mut w, 1, (27, 6, Z));
        let r = raiders(&w)[0];
        let ru = battle(&w).unit_of(r).unwrap();
        w.battle.as_mut().unwrap().units[ru].morale = 5.0;
        let u = battle(&w).unit_of(id).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::End).unwrap();
        play_out(&mut w, 30.0);
        assert!(w.battle.is_none(), "with the raider gone, the battle is won");
        assert!(w.pawn(r).is_none(), "fled the map");
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::WonBattle));
    }

    #[test]
    fn a_strong_colony_wins() {
        let mut w = world(40, 24, 9);
        for k in 0..3 {
            let id = colonist(&mut w, (6, 9 + 3 * k, Z));
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].skills[Skill::Shooting] = 15;
            w.pawns[i].weapon = Some(item("rifle"));
        }
        // Out of range at first, so it takes a few rounds.
        fight(&mut w, 1, (39, 12, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("knife"));
        w.battle.as_mut().unwrap().auto_colony = true;
        let t = w.time();
        play_out(&mut w, 300.0);
        assert!(w.battle.is_none(), "over in time");
        assert!(w.pawn(r).is_none_or(|p| !p.active()), "the raider is down, dead or gone");
        assert!(w.time() > t, "rounds took time");
    }

    #[test]
    fn a_weak_colony_loses_its_goods() {
        let mut w = world(40, 24, 10);
        let id = colonist(&mut w, (6, 12, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = None;
        w.pawns[i].skills[Skill::Melee] = 0;
        w.map.drop_item((3, 3, Z), Stack { kind: item("steel"), count: 60 });
        w.open_parley(4, (30, 12, Z), 0);
        let demand = w.parley.as_ref().unwrap().demand.clone();
        assert!(!demand.is_empty());
        w.refuse_tribute();
        for r in raiders(&w) {
            let ri = w.pawn_index(r).unwrap();
            w.pawns[ri].weapon = Some(item("rifle"));
            w.pawns[ri].skills[Skill::Shooting] = 15;
        }
        w.battle.as_mut().unwrap().auto_colony = true;
        play_out(&mut w, 600.0);
        assert!(w.battle.is_none(), "over in time");
        assert!(!w.pawn(id).unwrap().active(), "the colonist went down");
        assert!(w.map.count_items(item("steel")) < 60, "the raiders took goods");
        assert!(raiders(&w).iter().all(|&r| !w.pawn(r).unwrap().active()), "the rest left");
    }

    #[test]
    fn raiders_closing_in_from_afar_dont_give_up() {
        let mut w = world(150, 12, 12);
        let id = colonist(&mut w, (2, 6, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("knife"));
        fight(&mut w, 1, (147, 6, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("knife"));
        w.battle.as_mut().unwrap().auto_colony = true;
        let mut reached_melee = false;
        play_out_observing(&mut w, 900.0, |w| {
            reached_melee |= w.gap_between_sides().is_some_and(|gap| gap <= 1);
        });
        assert!(w.battle.is_none(), "over in time");
        assert!(reached_melee, "they closed the distance instead of abandoning the approach");
    }

    #[test]
    fn raiders_who_cant_get_at_anyone_give_up() {
        let mut w = world(30, 12, 13);
        let id = colonist(&mut w, (4, 6, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("knife"));
        // Walled in, no door: nobody gets in or out.
        for y in 4..=8 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 4 || y == 8 {
                    let c = w.map.idx((x, y, Z));
                    w.map.feature[c] = Feature::built("wall");
                }
            }
        }
        fight(&mut w, 1, (27, 6, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("knife"));
        w.battle.as_mut().unwrap().auto_colony = true;
        let mut last_quiet_rounds = 0;
        play_out_observing(&mut w, 900.0, |w| {
            last_quiet_rounds = battle(w).quiet_rounds;
        });
        assert!(w.battle.is_none(), "over in time");
        assert_eq!(last_quiet_rounds, STALEMATE - 1, "the next quiet round ends the raid");
        let p = w.pawn(id).unwrap();
        assert!(p.active() && p.health.hediffs.is_empty(), "no one reached the colonist");
        assert!(p.memories.iter().any(|m| m.kind == Memory::WonBattle));
        assert!(w.pawn(r).is_none(), "the still-healthy raider left");
    }

    #[test]
    fn everyone_runs_in_battle_and_walks_after() {
        let mut w = world(30, 12, 14);
        let id = colonist(&mut w, (4, 6, Z));
        assert!(!w.pawn(id).unwrap().running);
        let walk = w.pawn(id).unwrap().move_speed();
        fight(&mut w, 1, (27, 6, Z));
        assert!(w.pawns.iter().all(|p| p.running));
        assert!(w.pawn(id).unwrap().move_speed() > 2.0 * walk);
        w.end_battle(Outcome::Won);
        assert!(w.pawns.iter().all(|p| !p.running));
    }

    #[test]
    fn attacks_play_out_in_volleys() {
        let mut w = world(40, 20, 15);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (5, 11, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        for id in [leader, member] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("rifle"));
        }
        fight(&mut w, 1, (14, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        let raider = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Attack(raider)).unwrap();
        let strikes = battle(&w).strikes.clone();
        assert_eq!(strikes.len(), 4, "rifles fire twice each");
        let (first, second) = strikes.split_at(2);
        let mut volley: Vec<PawnId> = first.iter().map(|s| s.attacker).collect();
        volley.sort();
        let mut both = vec![leader, member];
        both.sort();
        assert_eq!(volley, both, "everyone's first shot, then the second");
        assert!(first[1].delay - first[0].delay <= VOLLEY_SPREAD, "together: {:?}", first);
        assert!(second[0].delay - first[1].delay >= VOLLEY_GAP - VOLLEY_SPREAD, "then the next volley: {:?}", strikes);
        let leaders = strikes.iter().find(|s| s.attacker == leader).unwrap();
        assert_eq!(w.aim_target(leader), Some(leaders.target), "aiming first");
        assert!(w.battle_order(BattleOrder::End).is_err(), "busy while the attack plays out");
        let shots = |w: &mut World| w.take_sounds().iter().filter(|s| s.name == "rifle").count();
        shots(&mut w);
        for _ in 0..(0.3 / TICK) as usize {
            w.tick(TICK);
        }
        assert_eq!(shots(&mut w), 0, "still taking aim");
        // Shots heard per tick.
        let mut fired = Vec::new();
        for _ in 0..(3.0 / TICK) as usize {
            if w.battle.is_none() {
                break;
            }
            w.tick(TICK);
            fired.push(shots(&mut w));
        }
        let start = fired.iter().position(|&n| n > 0).expect("shots were fired");
        let first: usize = fired[start..(start + 6).min(fired.len())].iter().sum();
        assert_eq!(first, 2, "both first shots within a fifth of a second: {:?}", fired);
        assert!(w.battle.as_ref().is_none_or(|b| b.strikes.is_empty() && !b.busy()), "and it's over");
    }

    #[test]
    fn squad_leaders_are_hit_last_and_losses_are_remembered() {
        let mut w = world(40, 20, 17);
        let shooter = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(shooter).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        fight(&mut w, 3, (14, 10, Z));
        let t = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        let leader = battle(&w).units[t].members[0];
        assert!(battle(&w).units[t].is_squad());
        for _ in 0..200 {
            let picked = w.pick_target(i, t).unwrap();
            assert_ne!(picked, leader, "not the leader while others stand");
        }
        // Down the other two: now the leader is the only target.
        let others: Vec<PawnId> = battle(&w).units[t].members[1..].to_vec();
        for o in &others {
            let oi = w.pawn_index(*o).unwrap();
            let mut p = w.pawns[oi].clone();
            w.kill(&mut p, "was shot");
            w.pawns[oi] = p;
        }
        w.refresh_units();
        assert_eq!(w.pick_target(i, t), Some(leader));
        let unit = &battle(&w).units[t];
        assert_eq!(unit.members, vec![leader]);
        assert_eq!(unit.roster.len(), 3, "the fallen are still on the roster");
    }

    #[test]
    fn a_fallen_leader_is_not_replaced() {
        let mut w = world(40, 20, 17);
        let shooter = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(shooter).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        fight(&mut w, 3, (14, 10, Z));
        let t = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        let ids = battle(&w).units[t].members.clone();
        assert_eq!(battle(&w).units[t].leader(), Some(ids[0]));
        let down = |w: &mut World, id: PawnId| {
            let k = w.pawn_index(id).unwrap();
            let mut p = w.pawns[k].clone();
            w.kill(&mut p, "was shot");
            w.pawns[k] = p;
            w.refresh_units();
        };
        let morale = battle(&w).units[t].morale;
        down(&mut w, ids[0]);
        assert_eq!(battle(&w).units[t].leader(), None, "nobody takes command");
        assert_eq!(battle(&w).units[t].morale, morale - 40.0, "a member and the leader");
        assert!((0..200).any(|_| w.pick_target(i, t) == Some(ids[1])), "the new first member isn't spared like a leader");
        // The next one to fall is just a member: no second leader penalty.
        down(&mut w, ids[1]);
        assert_eq!(battle(&w).units[t].morale, morale - 60.0, "only the member's 20");
    }

    #[test]
    fn walls_between_you_block_the_view() {
        let mut w = world(20, 11, 18);
        // A long wall: two people pressed against it from either side can't see each other.
        for y in 0..11 {
            let i = w.map.idx((6, y, Z));
            w.map.feature[i] = Feature::built("wall");
        }
        assert!(!battle_sight(&w, (5, 5, Z), (7, 5, Z)), "not through the wall");
        assert!(!battle_sight(&w, (2, 5, Z), (12, 8, Z)));

        // Inside a walled house, from right outside it.
        let mut w = world(20, 20, 18);
        for y in 5..=9 {
            for x in 5..=9 {
                if x == 5 || x == 9 || y == 5 || y == 9 {
                    let i = w.map.idx((x, y, Z));
                    w.map.feature[i] = Feature::built("wall");
                }
            }
        }
        assert!(!battle_sight(&w, (4, 7, Z), (6, 7, Z)), "not into the house");
        assert!(!battle_sight(&w, (12, 7, Z), (8, 7, Z)));
    }

    #[test]
    fn heavy_cover_cannot_be_peeked_past() {
        let mut w = world(20, 12, 19);
        for y in 3..=5 {
            let i = w.map.idx((6, y, Z));
            w.map.feature[i] = Feature::built("wall");
        }
        assert!(!battle_sight(&w, (5, 5, Z), (12, 4, Z)), "no leaning out past a wall");
        assert!(battle_sight(&w, (5, 6, Z), (12, 4, Z)), "step out past its end to see them");
    }

    #[test]
    fn corners_only_block_when_both_sides_do() {
        let mut w = world(12, 12, 20);
        let wall = |w: &mut World, x: usize, y: usize| {
            let i = w.map.idx((x, y, Z));
            w.map.feature[i] = Feature::built("wall");
        };
        wall(&mut w, 6, 5);
        assert!(clear_line(&w.map, (5, 5, Z), (8, 8, Z)), "grazing one corner is fine");
        wall(&mut w, 5, 6);
        assert!(!clear_line(&w.map, (5, 5, Z), (8, 8, Z)), "between two corners is not");
    }

    #[test]
    fn floors_block_the_view_between_levels() {
        let mut w = world(16, 8, 21);
        // A roof (floor on the level above) from x = 6 to 9.
        for x in 6..=9 {
            let i = w.map.idx((x, 4, Z + 1));
            w.map.set_floor(i, true);
        }
        assert!(!clear_line(&w.map, (5, 4, Z), (8, 4, Z + 1)), "the slab is in the way");
        assert!(clear_line(&w.map, (3, 4, Z), (6, 4, Z + 1)), "someone at the roof's edge can be seen");
    }

    #[test]
    fn squads_deploy_on_one_knee() {
        let mut w = world(40, 20, 22);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (5, 11, Z));
        let lone = colonist(&mut w, (5, 15, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        for id in [leader, member] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("rifle"));
        }
        fight(&mut w, 1, (16, 10, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("rifle"));
        let u = battle(&w).unit_of(leader).unwrap();
        let ours = |w: &World| w.hit_chance(w.pawn(leader).unwrap(), w.pawn(r).unwrap());
        let theirs = |w: &World| w.hit_chance(w.pawn(r).unwrap(), w.pawn(leader).unwrap());
        let (aim, exposed) = (ours(&w), theirs(&w));

        let lu = battle(&w).unit_of(lone).unwrap();
        w.battle_order(BattleOrder::Activate(lu)).unwrap();
        assert!(w.battle_order(BattleOrder::Deploy).is_err(), "only squads deploy");
        w.battle_order(BattleOrder::End).unwrap();
        // The raider's turn.
        let ru = battle(&w).unit_of(r).unwrap();
        w.do_order(BattleOrder::Activate(ru)).unwrap();
        w.do_order(BattleOrder::End).unwrap();

        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Deploy).unwrap();
        assert!(battle(&w).units[u].is_deployed());
        assert_eq!(battle(&w).units[u].ap, AP_MAX - AP_DEPLOY);
        assert!(ours(&w) > aim, "better aim deployed");
        assert!(theirs(&w) < exposed, "harder to hit");
        assert!(w.battle_order(BattleOrder::Move((6, 12, Z))).is_err(), "deployed: stand up first");
        w.battle_order(BattleOrder::Deploy).unwrap();
        assert!(!battle(&w).units[u].is_deployed());
        assert_eq!(battle(&w).units[u].ap, AP_MAX - 2 * AP_DEPLOY, "standing up costs too");
        assert!(w.battle_order(BattleOrder::Deploy).is_ok(), "30 AP left is enough");
        assert!(w.battle_order(BattleOrder::Deploy).is_err(), "but not again");
    }

    #[test]
    fn hunkering_stands_a_deployed_squad_up() {
        let mut w = world(40, 20, 22);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (5, 11, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        for id in [leader, member] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("rifle"));
        }
        fight(&mut w, 1, (16, 10, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        w.pawns[ri].weapon = Some(item("rifle"));
        let u = battle(&w).unit_of(leader).unwrap();
        let theirs = |w: &World| w.hit_chance(w.pawn(r).unwrap(), w.pawn(leader).unwrap());
        let exposed = theirs(&w);
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Deploy).unwrap();
        w.battle_order(BattleOrder::Hunker).unwrap();
        assert_eq!(battle(&w).units[u].stance, Stance::Hunkered);
        assert!((theirs(&w) - exposed * 0.75).abs() < 1e-5, "cover from hunkering alone, not stacked on deploying");
    }

    #[test]
    fn melee_squads_dont_deploy() {
        let mut w = world(40, 20, 22);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (5, 11, Z));
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        for id in [leader, member] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("club"));
        }
        fight(&mut w, 1, (16, 10, Z));
        let u = battle(&w).unit_of(leader).unwrap();
        assert!(battle(&w).units[u].is_squad() && !w.can_deploy(u));
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert!(w.battle_order(BattleOrder::Deploy).is_err(), "a melee squad has nothing to aim");
    }

    #[test]
    fn one_attack_does_not_pin_a_unit() {
        let mut w = world(40, 20, 23);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        w.pawns[i].skills[Skill::Shooting] = 20;
        fight(&mut w, 1, (12, 10, Z));
        let u = battle(&w).unit_of(id).unwrap();
        let t = battle(&w).units.iter().position(|u| u.side == Faction::Raider).unwrap();
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Attack(t)).unwrap();
        for _ in 0..(3.0 / TICK) as usize {
            w.tick(TICK);
        }
        if let Some(b) = &w.battle {
            assert!(!b.units[t].pinned(), "one volley doesn't pin: {}", b.units[t].suppression);
        }
    }

    #[test]
    fn powered_armour_moves_faster_but_a_squad_keeps_together() {
        let mut w = world(40, 20, 7);
        let leader = colonist(&mut w, (5, 10, Z));
        let member = colonist(&mut w, (6, 10, Z));
        let lone = colonist(&mut w, (5, 15, Z));
        for id in [leader, lone] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].wear(item("powered_armor"));
        }
        let walk = w.pawn(member).unwrap().move_speed();
        assert!((w.pawn(lone).unwrap().move_speed() - walk * 1.25).abs() < 1e-4, "a quarter faster on foot");
        w.set_role(leader, Role::Leader).unwrap();
        w.set_role(member, Role::Member(leader)).unwrap();
        fight(&mut w, 1, (39, 10, Z));
        let (squad, alone) = (battle(&w).unit_of(leader).unwrap(), battle(&w).unit_of(lone).unwrap());
        assert_eq!(w.step_ap(alone), 8, "fewer AP a step");
        assert_eq!(w.step_ap(squad), AP_MOVE, "the squad goes at its slowest member's pace");
        // Limping at half speed: twice the AP a step; in powered armour, a bit less than that.
        let i = w.pawn_index(member).unwrap();
        w.pawns[i].health.caps.moving = 0.5;
        assert_eq!(w.step_ap(squad), 2 * AP_MOVE);
        let i = w.pawn_index(lone).unwrap();
        w.pawns[i].health.caps.moving = 0.5;
        assert_eq!(w.step_ap(alone), 16);
    }

    /// Colonist `id` alone against one raider standing at `at`: (their unit, the raider's unit, the raider).
    fn duel(w: &mut World, id: PawnId, at: Pos) -> (usize, usize, PawnId) {
        fight(w, 1, (w.map.w - 1, 10, Z));
        let r = raiders(w)[0];
        let ri = w.pawn_index(r).unwrap();
        place(&mut w.pawns[ri], at);
        let b = battle(w);
        (b.unit_of(id).unwrap(), b.unit_of(r).unwrap(), r)
    }

    #[test]
    fn a_sidearm_is_drawn_when_the_main_weapon_cant_reach() {
        let mut w = world(30, 20, 31);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("club"));
        let (u, t, r) = duel(&mut w, id, (10, 10, Z));
        let (a, o) = (w.pawn(id).unwrap(), w.pawn(r).unwrap());
        assert_eq!(w.choose_weapon(a, a.pos, o), None, "a club doesn't reach five cells");
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].equip_sidearm(item("pistol"));
        let (a, o) = (w.pawn(id).unwrap(), w.pawn(r).unwrap());
        assert_eq!(w.choose_weapon(a, a.pos, o), Some(Some(item("pistol"))));
        assert!(!w.pawn(id).unwrap().main_attack().ranged, "they still fight in melee (squads go by the main weapon)");
        assert_eq!(w.attack_cost(u, t), AP_SIDEARM_ATTACK, "a quick sidearm attack");
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Attack(t)).unwrap();
        assert_eq!(battle(&w).units[u].ap, AP_MAX - AP_SIDEARM_ATTACK);
        let mut drawn = false;
        for _ in 0..(3.0 / TICK) as usize {
            if w.battle.is_none() {
                break;
            }
            w.tick(TICK);
            drawn |= w.pawn(id).unwrap().held() == Some(item("pistol"));
        }
        assert!(drawn, "the pistol was in hand for the shot");
        assert!(w.take_sounds().iter().any(|s| s.name == "pistol"));
    }

    #[test]
    fn rifles_are_clumsy_up_close_so_riflemen_draw_pistols() {
        let mut w = world(30, 20, 35);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        let (u, t, r) = duel(&mut w, id, (6, 10, Z));
        let (a, o) = (w.pawn(id).unwrap(), w.pawn(r).unwrap());
        let rifle = w.hit_chance_with(a, &attack_with(Some(item("rifle"))), a.pos, o);
        let pistol = w.hit_chance_with(a, &attack_with(Some(item("pistol"))), a.pos, o);
        assert!(rifle < pistol * 0.6, "rifle {} vs pistol {}", rifle, pistol);
        assert_eq!(w.choose_weapon(a, a.pos, o), Some(Some(item("rifle"))), "without a sidearm, the rifle it is");
        assert_eq!(w.attack_cost(u, t), AP_ATTACK);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].equip_sidearm(item("pistol"));
        let (a, o) = (w.pawn(id).unwrap(), w.pawn(r).unwrap());
        assert_eq!(w.choose_weapon(a, a.pos, o), Some(Some(item("pistol"))), "up close, the pistol");
        assert_eq!(w.attack_cost(u, t), AP_SIDEARM_ATTACK);
        // Further off, the rifle again.
        let ri = w.pawn_index(r).unwrap();
        place(&mut w.pawns[ri], (12, 10, Z));
        let (a, o) = (w.pawn(id).unwrap(), w.pawn(r).unwrap());
        assert_eq!(w.choose_weapon(a, a.pos, o), Some(Some(item("rifle"))));
    }

    #[test]
    fn ranged_squads_keep_melee_sidearms_holstered() {
        let mut w = world(30, 20, 36);
        let (lead, mate) = (colonist(&mut w, (5, 10, Z)), colonist(&mut w, (5, 11, Z)));
        w.set_role(lead, Role::Leader).unwrap();
        w.set_role(mate, Role::Member(lead)).unwrap();
        for id in [lead, mate] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("rifle"));
            // (Given directly: the loadout would refuse it.)
            w.pawns[i].equip_sidearm(item("knife"));
        }
        fight(&mut w, 1, (29, 10, Z));
        let r = raiders(&w)[0];
        let ri = w.pawn_index(r).unwrap();
        place(&mut w.pawns[ri], (6, 10, Z));
        let (a, o) = (w.pawn(lead).unwrap(), w.pawn(r).unwrap());
        assert!(w.sidearm_barred(a));
        assert_eq!(w.choose_weapon(a, a.pos, o), Some(Some(item("rifle"))), "a ranged squad fights at range, even up close");
    }

    #[test]
    fn melee_units_move_faster() {
        let mut w = world(30, 20, 37);
        let (gun, blade) = (colonist(&mut w, (5, 8, Z)), colonist(&mut w, (5, 12, Z)));
        for (id, weapon) in [(gun, "rifle"), (blade, "knife")] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item(weapon));
        }
        fight(&mut w, 1, (29, 10, Z));
        let b = battle(&w);
        let (gu, bu) = (b.unit_of(gun).unwrap(), b.unit_of(blade).unwrap());
        assert_eq!(w.step_ap(gu), AP_MOVE);
        assert!(w.step_ap(bu) < AP_MOVE, "{} AP a step", w.step_ap(bu));
        assert!(AP_MAX / w.step_ap(bu) > AP_MAX / w.step_ap(gu), "more cells a round");
    }

    #[test]
    fn a_personal_shield_takes_shots_but_stops_its_wearers_own() {
        let mut w = world(30, 20, 38);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        w.pawns[i].take_utility(Stack { kind: item("personal_shield"), count: 1 });
        let p = w.pawn(id).unwrap();
        assert!(!p.main_attack().ranged && p.held().is_none(), "no shooting through it: fists");
        assert!(!crate::squads::fights_at_range(p), "they count as melee");
        assert_eq!(p.shield_energy(), 40.0);
        // Shots: the shield takes them until it breaks; blows go straight through.
        let before = w.pawn(id).unwrap().health.hediffs.len();
        w.shot_damage(i, 15.0, crate::health::DamageKind::Gunshot, "a test");
        assert_eq!(w.pawns[i].health.hediffs.len(), before, "absorbed");
        assert_eq!(w.pawns[i].shield_energy(), 25.0);
        w.pawns[i].apparel.clear();
        w.shot_damage(i, 35.0, crate::health::DamageKind::Gunshot, "a test");
        assert_eq!(w.pawns[i].shield_energy(), 0.0);
        assert!(w.pawns[i].health.hediffs.len() > before, "the rest broke through");
        // It recharges each round, and is full after the battle.
        fight(&mut w, 1, (29, 10, Z));
        w.battle.as_mut().unwrap().auto_colony = true;
        let round = battle(&w).round;
        while w.battle.as_ref().is_some_and(|b| b.round == round) {
            w.tick(TICK);
        }
        if w.battle.is_some() {
            assert!(w.pawn(id).unwrap().shield_energy() >= 10.0, "recharged a little");
        }
    }

    #[test]
    fn grenades_catch_everyone_in_a_squad() {
        let mut w = world(30, 20, 32);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        w.pawns[i].take_utility(Stack { kind: item("frag_grenade"), count: 5 });
        assert_eq!(w.pawn(id).unwrap().utility, Some(Stack { kind: item("frag_grenade"), count: 3 }), "they carry three");
        fight(&mut w, 2, (29, 10, Z));
        let rs = raiders(&w);
        for (n, &r) in rs.iter().enumerate() {
            let ri = w.pawn_index(r).unwrap();
            place(&mut w.pawns[ri], (9, 9 + n, Z));
            w.pawns[ri].apparel.clear();
        }
        let b = battle(&w);
        let (u, t) = (b.unit_of(id).unwrap(), b.unit_of(rs[0]).unwrap());
        assert!(w.throw_option(u, t).is_some());
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        w.battle_order(BattleOrder::Throw(t)).unwrap();
        assert_eq!(w.pawn(id).unwrap().utility.map(|s| s.count), Some(2), "one used up");
        let b = battle(&w);
        assert_eq!(b.units[u].ap, AP_MAX - 40);
        assert!(b.units[t].suppression >= SUPPRESS_BLAST);
        for &r in &b.units[t].roster {
            let p = w.pawn(r).unwrap();
            assert!(p.is_dead() || !p.health.hediffs.is_empty(), "{} was caught in the blast", p.name);
        }
    }

    #[test]
    fn a_medkit_patches_up_the_unit() {
        let mut w = world(30, 20, 33);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].take_utility(Stack { kind: item("field_medkit"), count: 1 });
        let (u, _, _) = duel(&mut w, id, (25, 10, Z));
        w.battle_order(BattleOrder::Activate(u)).unwrap();
        assert!(w.battle_order(BattleOrder::Medkit).is_err(), "nobody is hurt");
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].health.injure(crate::health::Part::LeftArm, 8.0, crate::health::DamageKind::Cut, &mut crate::rng::Rng::new(2));
        w.pawns[i].health.recompute();
        assert!(w.pawn(id).unwrap().health.bleed_rate() > 0.0);
        w.battle_order(BattleOrder::Medkit).unwrap();
        let p = w.pawn(id).unwrap();
        assert_eq!(p.health.bleed_rate(), 0.0, "tended");
        assert!(p.utility.is_none(), "used up");
    }

    #[test]
    fn goggles_and_bionic_eyes_aim_better() {
        let mut w = world(30, 20, 34);
        let id = colonist(&mut w, (5, 5, Z));
        let plain = w.pawn(id).unwrap().clone();
        let mut goggles = plain.clone();
        goggles.wear(item("tactical_goggles"));
        let mut eyes = plain.clone();
        eyes.health.install(crate::health::CyberSlot::Eyes, item("bionic_eyes"), 1.25);
        let chance = |p: &Pawn| base_hit_chance(p, true, 6.0);
        assert!(chance(&goggles) > chance(&plain) + 0.03, "{} vs {}", chance(&goggles), chance(&plain));
        assert!(chance(&eyes) > chance(&plain) * 1.1, "{} vs {}", chance(&eyes), chance(&plain));
    }

    #[test]
    fn raiders_come_better_equipped_as_the_threat_grows() {
        let mut w = world(40, 20, 41);
        colonist(&mut w, (5, 10, Z));
        assert_eq!(w.raid_tier(), 0, "a new colony meets scavengers");
        w.set_time(30.0 * crate::DAY_LEN);
        let top = defs().raider_tiers.len() - 1;
        assert_eq!(w.raid_tier(), top, "a month on, the elite");
        w.difficulty.raid_gear = Some(0);
        assert_eq!(w.raid_tier(), 0, "unless the setting says otherwise");
        let raiders = |tier: usize, seed: u64| {
            let mut w = world(40, 20, seed);
            colonist(&mut w, (5, 10, Z));
            w.open_parley(8, (39, 10, Z), tier);
            w.refuse_tribute();
            w.pawns.into_iter().filter(|p| !p.is_colonist()).collect::<Vec<Pawn>>()
        };
        let weak = raiders(0, 42);
        assert!(weak.iter().all(|p| p.health.implants().count() == 0 && p.utility.is_none()));
        assert!(weak.iter().all(|p| p.weapon.is_none_or(|k| k != item("laser_rifle"))));
        let elite: Vec<Pawn> = (0..3).flat_map(|s| raiders(top, 50 + s)).collect();
        assert!(elite.iter().filter(|p| p.weapon == Some(item("laser_rifle"))).count() * 2 > elite.len(), "mostly laser rifles");
        assert!(elite.iter().filter(|p| p.health.implants().count() > 0).count() * 2 > elite.len(), "mostly with cyberware");
        assert!(elite.iter().any(|p| p.apparel.contains(&item("powered_armor"))));
        let mercs: Vec<Pawn> = (0..4).flat_map(|s| raiders(3, 60 + s)).collect();
        for p in weak.iter().chain(&elite).chain(&mercs) {
            assert!(!p.main_attack().ranged || p.sidearm.is_none_or(|k| attack_with(Some(k)).ranged), "{}: a rifleman with a melee sidearm", p.name);
        }
        assert!(elite.iter().map(|p| p.armor()).sum::<f32>() > weak.iter().map(|p| p.armor()).sum::<f32>() * 3.0);
    }

    #[test]
    fn a_field_radio_gives_the_unit_more_action_points() {
        let mut w = world(30, 20, 43);
        let (lead, mate) = (colonist(&mut w, (5, 10, Z)), colonist(&mut w, (5, 11, Z)));
        w.set_role(lead, Role::Leader).unwrap();
        w.set_role(mate, Role::Member(lead)).unwrap();
        for id in [lead, mate] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = Some(item("rifle"));
            w.pawns[i].wear(item("radio_pack"));
        }
        fight(&mut w, 1, (29, 10, Z));
        let u = battle(&w).unit_of(lead).unwrap();
        assert_eq!(battle(&w).units[u].ap, AP_MAX + 10, "radios don't add up");
        assert!(!battle(&w).units[u].held_down());
    }

    #[test]
    fn the_base_hit_chance_is_the_battle_one_in_the_open() {
        let mut w = world(30, 20, 3);
        let a = colonist(&mut w, (5, 5, Z));
        let o = colonist(&mut w, (15, 5, Z));
        let (pa, po) = (w.pawn(a).unwrap().clone(), w.pawn(o).unwrap().clone());
        let ranged = pa.attack().ranged;
        let d = cell_dist(pa.pos, po.pos);
        assert!((w.hit_chance(&pa, &po) - base_hit_chance(&pa, ranged, d).clamp(0.05, 0.95)).abs() < 1e-5);
        // Melee: skill only, 60% + 2% a level.
        let mut fist = pa.clone();
        fist.weapon = None;
        fist.skills[Skill::Melee] = 10;
        assert!((base_hit_chance(&fist, false, 1.0) - 0.8).abs() < 1e-5);
    }

    #[test]
    fn hit_chances_can_be_previewed_from_another_cell() {
        let mut w = world(40, 20, 24);
        let id = colonist(&mut w, (5, 10, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        fight(&mut w, 1, (30, 10, Z));
        let r = raiders(&w)[0];
        let dest = (16, 10, Z);
        let (a, o) = (w.pawn(id).unwrap(), w.pawn(r).unwrap());
        assert!(!w.can_attack(a, o), "out of reach from here");
        assert!(w.can_attack_from(a, dest, o), "but not from there");
        let preview = w.hit_chance_from(a, dest, o);
        place(&mut w.pawns[i], dest);
        let actual = w.hit_chance(w.pawn(id).unwrap(), w.pawn(r).unwrap());
        assert!((preview - actual).abs() < 1e-6, "{} == {}", preview, actual);
    }

    #[test]
    fn the_colony_can_pass_for_the_round() {
        let mut w = world(40, 20, 25);
        for y in [5, 10, 15] {
            colonist(&mut w, (5, y, Z));
        }
        fight(&mut w, 1, (39, 10, Z));
        w.end_colony_turns().unwrap();
        for _ in 0..(6.0 / TICK) as usize {
            if battle(&w).round > 1 {
                break;
            }
            w.tick(TICK);
        }
        let b = battle(&w);
        assert_eq!(b.round, 2, "all three passed and the raiders acted: a new round");
        assert!(!b.colony_passes, "passing is only for one round");
    }

    #[test]
    fn battles_survive_saving_and_loading() {
        let mut w = world(40, 20, 11);
        colonist(&mut w, (5, 10, Z));
        fight(&mut w, 2, (39, 10, Z));
        w.battle.as_mut().unwrap().units[0].suppression = 30.0;
        let loaded = crate::save::from_json(&crate::save::to_json(&w).unwrap()).unwrap();
        let (a, b) = (battle(&w), loaded.battle.as_ref().unwrap());
        assert_eq!(a.units.len(), b.units.len());
        assert_eq!(a.units[0].members, b.units[0].members);
        assert_eq!(b.units[0].suppression, 30.0);
        assert_eq!(a.side_to_act, b.side_to_act);
    }
}
