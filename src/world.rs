//! The whole simulation state and its fixed-step update.

use crate::battle::{Battle, Parley};
use crate::health::TickCtx;
use crate::defs::{defs, BuildId, PlantId, Special};
use crate::map::{Block, Desig, Dir, Feature, Map, Pos, Stack, Zone};
use crate::moving::Move;
use crate::pawn::{Faction, Job, Life, Memory, Pawn, PawnId, WorkPriority, WorkType};
use crate::pathing::{bfs, DistField};
use crate::production::{Bench, Stockpile};
use crate::rooms::RoomStats;
use crate::rng::Rng;
use crate::story::Storyteller;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

pub const MAP_W: usize = 100;
pub const MAP_H: usize = 70;
pub const MAP_D: usize = 16;

const COLONIST_NAMES: [&str; 16] = [
    "Ada", "Bram", "Cleo", "Dax", "Eve", "Finn", "Gus", "Hana", "Ivo", "Juno", "Kai", "Lia", "Milo", "Nell",
    "Otto", "Pia",
];

/// A short-lived visual (gunshot tracer), in cell coordinates (x, y, level).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Effect {
    pub from: (f32, f32, f32),
    pub to: (f32, f32, f32),
    pub ttl: f32,
    pub hit: bool,
}

/// Something worth hearing, for the front end: a sound name from
/// `assets/sounds.ron` and where it happened (cell coordinates; `None` is
/// heard everywhere, like an alert).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundEvent {
    pub name: &'static str,
    pub at: Option<(f32, f32, f32)>,
}

/// Every sound name the simulation uses (checked against the sound table by
/// the front end's tests); definitions add their own (harvest, weapons).
pub const SIM_SOUNDS: [&str; 10] =
    ["thud", "pods", "joined", "level_up", "dig", "rock_break", "built", "hit", "glance", "death"];

/// Sound events kept until the front end takes them; older ones are dropped.
const MAX_SOUNDS: usize = 128;

/// Something the player paints onto tiles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Desig(Desig),
    /// Grow zones (with their crop) and clearing zones.
    Zone(Zone),
    /// Stockpile cells: join a stockpile beside them or start a new one.
    Stockpile,
    /// Quarry columns: join a quarry beside them or start a new one (see `quarry.rs`).
    Quarry,
    Build(BuildId),
    Cancel,
}

/// Where to get to for a goal (see `World::go_to_reach`).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Reach {
    /// The goal cell itself.
    On,
    /// Beside it (`Map::around`).
    Beside,
    /// Somewhere to build it from (`Map::build_around`).
    Build,
}

pub(crate) enum Step {
    Arrived,
    Moving,
    Unreachable,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct World {
    pub map: Map,
    pub pawns: Vec<Pawn>,
    /// Per-tile reservations by pawn (None = free): work targets, item stacks
    /// being picked up, and stockpile cells being delivered to.
    pub res_target: crate::reservations::Reservations,
    pub res_item: crate::reservations::Reservations,
    pub res_cell: crate::reservations::Reservations,
    /// (patient, doctor) pairs, so two doctors don't treat the same patient.
    #[serde(default)]
    pub res_patient: Vec<(PawnId, PawnId)>,
    /// Game seconds since the start. Kept in f64 and private: an f32 sum of
    /// 1/30 s ticks drifts within months and stops advancing after a few
    /// years of game time. Read it with `time()`.
    #[serde(rename = "time")]
    clock: f64,
    pub rng: Rng,
    pub next_id: u32,
    pub log: Vec<String>,
    pub story: Storyteller,
    /// Raids on or off, and how big (see `story.rs`).
    #[serde(default)]
    pub difficulty: crate::story::Difficulty,
    pub effects: Vec<Effect>,
    pub colonist_names_used: usize,
    pub regrow_timer: f32,
    /// Workbenches with their bills and delivered ingredients.
    pub benches: Vec<Bench>,
    /// Stockpile settings, indexed by the id in `Zone::Stockpile`.
    pub stockpiles: Vec<Stockpile>,
    /// Furniture being moved (see `moving.rs`).
    #[serde(default)]
    pub moves: Vec<Move>,
    /// Each cell's room temperature as last worked out (NaN outside rooms).
    /// Rooms warm and cool over time, so this is saved. See `temperature.rs`.
    #[serde(with = "crate::save::rle_nan")]
    pub cell_temp: Vec<f32>,
    /// Charge in the colony's batteries (watt-hours; see `power.rs`).
    #[serde(default)]
    pub stored_wh: f32,
    /// Cells of the heaters and coolers running this second, and how the grid
    /// did (saved: what had power depends on the batteries at the time).
    pub running: std::collections::BTreeSet<u32>,
    pub power: crate::power::PowerState,
    /// Areas dug out level by level (see `quarry.rs`).
    #[serde(default)]
    pub quarries: Vec<crate::quarry::Quarry>,
    /// Per quarry, how many of the cells marked to dig nobody can get to
    /// (counted once a second in `update_quarries`; saved with the rest of
    /// that second's results, so loading doesn't change what happens next).
    pub quarry_unreachable: Vec<usize>,
    /// Those cells (indices): left standing for now, so the layers go on around them.
    pub quarry_out_of_reach: std::collections::BTreeSet<usize>,
    /// Counts down to the next once-a-second pass over benches and rooms.
    pub colony_timer: f32,
    /// Quality per room id (index 0 unused). Derived, so not saved.
    #[serde(skip)]
    pub room_stats: Vec<RoomStats>,
    /// A raid waiting for an answer: pay, or fight (the colony stops meanwhile).
    #[serde(default)]
    pub parley: Option<Parley>,
    /// The battle going on, if any (the colony stops meanwhile).
    #[serde(default)]
    pub battle: Option<Battle>,
    /// Sounds since the front end last took them (see `take_sounds`).
    #[serde(skip)]
    pub sounds: Vec<SoundEvent>,
    /// Colonists who can't walk to the edge of the map (shut in by
    /// furniture, walls or doors), checked once a second. They climb out.
    pub trapped: Vec<PawnId>,
    /// The neighbouring factions, the missions on offer, and squads away (see `expeditions`).
    #[serde(default)]
    pub factions: Vec<crate::expeditions::Faction>,
    #[serde(default)]
    pub offers: Vec<crate::expeditions::Offer>,
    #[serde(default)]
    pub expeditions: Vec<crate::expeditions::Expedition>,
}

impl World {
    pub fn new(seed: u64) -> World {
        let mut rng = Rng::new(seed);
        let map = Map::generate(MAP_W, MAP_H, MAP_D, &mut rng);
        let mut w = World::with_map(map, rng);
        let (cx, cy) = (MAP_W / 2, MAP_H / 2);
        // The landing zone is flat, so one level works for all of it.
        let z = w.map.surface(cx, cy).unwrap_or(1);
        // Each lands with a different weapon (`start_weapons`).
        let weapons: Vec<_> = defs().start_weapons.iter().map(|r| r.id()).collect();
        for k in 0..3 {
            let id = w.spawn_colonist((cx - 1 + k, cy, z));
            if !weapons.is_empty() {
                let i = w.pawn_index(id).unwrap();
                w.pawns[i].weapon = Some(weapons[k % weapons.len()]);
            }
        }
        let mut stockpile = Vec::new();
        let store = w.add_stockpile(Stockpile::new(1));
        for y in cy + 2..cy + 5 {
            for x in cx - 4..cx + 2 {
                let i = w.map.idx((x, y, z));
                w.map.zone[i] = Zone::Stockpile(store);
                stockpile.push((x, y, z));
            }
        }
        // Starting supplies, a few cells apart in the stockpile.
        for (k, a) in defs().start_items.iter().enumerate() {
            let at = stockpile[(k * 7) % stockpile.len()];
            w.map.drop_item(at, Stack { kind: a.item, count: a.count });
        }
        w.push_log("Three colonists crash-landed. Press H for help.".into());
        w
    }

    /// An empty world around an existing map (also used by tests).
    pub fn with_map(map: Map, rng: Rng) -> World {
        let n = map.len();
        World {
            map,
            pawns: Vec::new(),
            res_target: crate::reservations::Reservations::new(n),
            res_item: crate::reservations::Reservations::new(n),
            res_cell: crate::reservations::Reservations::new(n),
            res_patient: Vec::new(),
            clock: (DAY_LEN * 0.25) as f64,
            rng,
            next_id: 1,
            log: Vec::new(),
            story: Storyteller::new(),
            difficulty: Default::default(),
            effects: Vec::new(),
            colonist_names_used: 0,
            regrow_timer: REGROW_SECS,
            benches: Vec::new(),
            stockpiles: Vec::new(),
            moves: Vec::new(),
            quarries: Vec::new(),
            stored_wh: 0.0,
            running: Default::default(),
            power: Default::default(),
            cell_temp: vec![f32::NAN; n],
            quarry_unreachable: Vec::new(),
            quarry_out_of_reach: Default::default(),
            colony_timer: 0.0,
            room_stats: Vec::new(),
            parley: None,
            battle: None,
            sounds: Vec::new(),
            trapped: Vec::new(),
            factions: crate::expeditions::starting_factions(),
            offers: Vec::new(),
            expeditions: Vec::new(),
        }
    }

    // ---------- queries ----------

    /// The nearest cell `from` can walk to, within `max` steps, that passes `test`.
    pub(crate) fn nearest_cell(&self, from: Pos, max: i32, test: impl FnMut(usize) -> bool) -> Option<Pos> {
        crate::pathing::nearest(&self.map, from, max, test).map(|i| self.map.pos(i))
    }

    /// The nearest cell in `df` (within `max` steps) holding a stack that
    /// passes `test(cell, stack)`, looking at every stack of a shelf.
    pub(crate) fn nearest_stack(&self, df: &DistField, max: i32, test: impl Fn(usize, Stack) -> bool) -> Option<Pos> {
        df.nearest(max, |i| self.map.stacks(i).any(|s| test(i, s))).map(|i| self.map.pos(i))
    }

    /// The next number for a new pawn, offer or expedition (each wraps it in
    /// its own id type).
    pub(crate) fn take_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Game seconds since the start.
    pub fn time(&self) -> f32 {
        self.clock as f32
    }

    /// Jump the clock (scenarios, tests and loading old saves).
    pub fn set_time(&mut self, time: f32) {
        self.clock = time as f64;
    }

    pub(crate) fn advance_clock(&mut self, dt: f32) {
        self.clock += dt as f64;
    }

    /// Run on the same clock as `other` (an expedition's fight site).
    pub(crate) fn share_clock(&mut self, other: &World) {
        self.clock = other.clock;
    }

    pub fn day(&self) -> u32 {
        (self.time() / DAY_LEN) as u32 + 1
    }

    pub fn hour(&self) -> f32 {
        (self.time() % DAY_LEN) / DAY_LEN * 24.0
    }

    pub fn is_night(&self) -> bool {
        let h = self.hour();
        !(DAWN..DUSK).contains(&h)
    }

    /// 0 at midnight, 1 at noon.
    pub fn daylight(&self) -> f32 {
        let t = (self.time() % DAY_LEN) / DAY_LEN;
        0.5 - 0.5 * (t * std::f32::consts::TAU).cos()
    }

    pub fn pawn_index(&self, id: PawnId) -> Option<usize> {
        self.pawns.iter().position(|p| p.id == id)
    }

    pub fn pawn(&self, id: PawnId) -> Option<&Pawn> {
        self.pawns.iter().find(|p| p.id == id)
    }

    pub fn colonists_alive(&self) -> usize {
        self.pawns.iter().filter(|p| p.is_living_colonist()).count()
    }

    pub fn colony_lost(&self) -> bool {
        self.colonists_alive() == 0 && self.away() == 0
    }

    /// Warnings for the player, `true` = urgent.
    pub fn alerts(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        let raiders = self.pawns.iter().filter(|p| p.faction == Faction::Raider && p.active()).count();
        if raiders > 0 {
            out.push((format!("Raid: {} raiders", raiders), true));
        }
        for e in &self.expeditions {
            match e.stage {
                crate::expeditions::Stage::Decide { .. } => out.push((format!("{}: a decision is waiting (Expeditions)", e.kind.name()), true)),
                crate::expeditions::Stage::Trading { .. } => out.push((format!("{}: at the market (Expeditions)", e.kind.name()), false)),
                crate::expeditions::Stage::Out { .. }
                | crate::expeditions::Stage::Fighting { .. }
                | crate::expeditions::Stage::Back { .. } => {}
            }
        }
        for p in self.trapped.iter().filter_map(|&id| self.pawn(id)) {
            out.push((format!("{} is trapped (climbing out)", p.name), true));
        }
        let colonists: Vec<&Pawn> = self.pawns.iter().filter(|p| p.is_living_colonist()).collect();
        let mut needs_tending = false;
        for p in &colonists {
            let h = &p.health;
            if let Some(secs) = h.time_to_bleed_out() {
                let hours = secs / DAY_LEN * 24.0;
                out.push((format!("{} bleeding out (~{:.0}h)", p.name, hours.max(1.0)), true));
            } else if p.is_downed() && !self.in_bed(p) {
                out.push((format!("{} needs rescue", p.name), true));
            } else if p.is_downed() {
                out.push((format!("{} is downed", p.name), true));
            } else if h.malnutrition > 0.0 && p.hunger <= 0.0 {
                out.push((format!("{} is starving", p.name), true));
            } else if p.break_timer > 0.0 {
                out.push((format!("{}: mental break", p.name), false));
            }
            for d in h.hediffs.iter().filter(|d| d.is_disease()) {
                let urgent = d.severity > d.immunity().unwrap_or(0.0);
                out.push((format!("{}: {} {:.0}%", p.name, d.label().to_lowercase(), d.severity * 100.0), urgent));
            }
            needs_tending |= h.needs_tending();
        }
        let medicine = self.map.count_items_where(|k| defs().is_medicine(k));
        if needs_tending && medicine == 0 {
            out.push(("No medicine".into(), true));
        } else if medicine < 3 {
            out.push((format!("Low medicine ({} left)", medicine), false));
        }
        if !colonists.iter().any(|p| p.active() && p.priorities[WorkType::Doctor].is_enabled()) {
            out.push(("No doctor available".into(), needs_tending));
        }
        for p in colonists.iter().filter(|p| p.health.hypothermia > 0.05) {
            out.push((format!("{} is freezing (hypothermia {:.0}%)", p.name, p.health.hypothermia * 100.0), true));
        }
        if self.power.unpowered > 0 {
            out.push((format!("Not enough power: {} heater(s)/cooler(s) off", self.power.unpowered), false));
        }
        let doomed = self.crops_frost_will_kill();
        if doomed > 0 && self.season_left() < 1.5 * DAY_LEN {
            let hours = (self.season_left() / DAY_LEN * 24.0).round();
            out.push((format!("Frost in {:.0}h: {} crop(s) won't be ripe", hours, doomed), false));
        }
        let food = self.map.count_items_where(|k| defs().is_food(k));
        if (food as usize) < colonists.len() * 4 {
            out.push((format!("Low food ({} left)", food), false));
        }
        let beds = self.map.feature.iter().filter(|f| f.is_bed() && !f.structure().is_some_and(|d| d.medical)).count();
        if beds < colonists.len() {
            out.push((format!("Need beds ({}/{})", beds, colonists.len()), false));
        }
        if !self.map.zone.iter().any(|z| z.stockpile().is_some()) {
            out.push(("No stockpile zone".into(), false));
        }
        let gaps = self.map.roof_gaps.len();
        if gaps > 0 {
            out.push((format!("Roof can't reach {} cell(s): build a wall within 3", gaps), false));
        }
        let unburied = self.pawns.iter().filter(|p| p.is_colonist() && p.is_dead() && !p.is_buried()).count();
        if unburied > 0 {
            let graves = self.empty_graves().len();
            let text = if graves >= unburied {
                format!("{} unburied colonist(s)", unburied)
            } else {
                format!("{} unburied colonist(s): dig a grave", unburied)
            };
            out.push((text, false));
        }
        let idle = colonists
            .iter()
            .filter(|p| p.active() && matches!(p.job, None | Some(Job::Wander { .. })))
            .count();
        if idle > 0 {
            out.push((format!("{} idle colonist(s)", idle), false));
        }
        out
    }

    /// Note a sound at a cell (`None`: everywhere).
    pub(crate) fn sound(&mut self, name: &'static str, at: Option<Pos>) {
        if self.sounds.len() >= MAX_SOUNDS {
            self.sounds.remove(0);
        }
        let at = at.map(|p| (p.0 as f32, p.1 as f32, p.2 as f32));
        self.sounds.push(SoundEvent { name, at });
    }

    /// The sounds since the last call, for the front end to play.
    pub fn take_sounds(&mut self) -> Vec<SoundEvent> {
        std::mem::take(&mut self.sounds)
    }

    pub fn push_log(&mut self, msg: String) {
        self.log.push(msg);
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }

    // ---------- spawning ----------

    pub fn spawn_colonist(&mut self, pos: Pos) -> PawnId {
        let name = COLONIST_NAMES
            .get(self.colonist_names_used)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Colonist {}", self.colonist_names_used + 1));
        self.colonist_names_used += 1;
        let id = PawnId::new(self.take_id());
        let p = Pawn::new_colonist(id, name, pos, &mut self.rng);
        self.pawns.push(p);
        id
    }

    pub fn spawn_raider(&mut self, name: String, pos: Pos) -> PawnId {
        let id = PawnId::new(self.take_id());
        let p = Pawn::new_raider(id, name, pos, &mut self.rng);
        self.pawns.push(p);
        id
    }

    // ---------- simulation ----------

    pub fn tick(&mut self, dt: f32) {
        // A raid stops the colony: while the parley is open, and during the battle.
        if self.parley.is_some() {
            return;
        }
        if self.battle.is_some() {
            self.battle_tick(dt);
            self.age_effects(dt);
            return;
        }
        self.advance_clock(dt);
        self.check_season(self.time() - dt);
        if self.map.rooms_dirty {
            self.map.recompute_rooms();
            self.mark_roofs();
            self.update_room_stats();
        }
        if self.map.support_dirty {
            self.settle_unsupported();
        }
        self.colony_timer -= dt;
        if self.colony_timer <= 0.0 {
            self.colony_timer = 1.0;
            self.sync_containers();
            self.update_production();
            self.update_moves();
            self.update_decor();
            self.update_quarries();
            self.update_room_stats();
            self.update_temperatures(1.0);
            self.update_power(1.0);
            self.update_squads();
            self.update_graves();
            self.update_trapped();
            self.update_expeditions();
            // Whole-map passes: once a second rather than every tick.
            self.grow_plants(1.0);
            let fade = 1.0 / (1.5 * DAY_LEN);
            for b in self.map.blood.iter_mut() {
                *b = (*b - fade).max(0.0);
            }
        }
        self.storyteller_tick();

        // Each pawn is updated on a copy so it can freely read and modify the
        // rest of the world (including other pawns) while it thinks.
        for i in 0..self.pawns.len() {
            let mut p = self.pawns[i].clone();
            self.update_pawn(&mut p, dt);
            self.pawns[i] = p;
        }
        self.pawns.retain(|p| p.is_colonist() || p.present());

        self.age_effects(dt);
    }

    fn age_effects(&mut self, dt: f32) {
        for e in self.effects.iter_mut() {
            e.ttl -= dt;
        }
        self.effects.retain(|e| e.ttl > 0.0);
    }

    pub(crate) fn grow_plants(&mut self, dt: f32) {
        let season = self.season();
        // Only in daylight, and not under a roof (or anything else overhead);
        // sown crops as fast as the season lets them.
        if !self.is_night() {
            for i in 0..self.map.len() {
                let Feature::Plant { kind, growth } = self.map.feature[i] else { continue };
                if growth >= 1.0 || self.map.covered(self.map.pos(i)) {
                    continue;
                }
                let t = kind.def().grow_time;
                let rate = if kind.def().sow.is_some() { season.crop_growth() } else { 1.0 };
                let growth = if t > 0.0 { (growth + dt * rate / t).min(1.0) } else { 1.0 };
                self.map.feature[i] = Feature::Plant { kind, growth };
                self.map.mark_changed();
            }
        }
        self.regrow_timer -= dt;
        // Wild plants don't come back in winter.
        if self.regrow_timer <= 0.0 && season.regrowth() {
            self.regrow_timer = REGROW_SECS;
            let (x, y) = (self.rng.below(self.map.w), self.rng.below(self.map.h));
            let m = &self.map;
            let Some(z) = m.surface(x, y) else { return };
            let i = m.idx((x, y, z));
            let free = m.feature[i] == Feature::None
                && m.block[m.idx((x, y, z - 1))] == Block::Soil
                && m.zone[i] == Zone::None
                && m.items[i].is_none()
                && !m.covered((x, y, z))
                // Nothing grows up against buildings.
                && !m.beside_wall((x, y, z))
                && !self.pawns.iter().any(|p| m.idx(p.pos) == i);
            if free {
                // Only up to how much of the land it covered when the map was made.
                if let Some(kind) = self.pick_regrowing().filter(|&k| self.wild_count(k) < self.wild_cap(k)) {
                    self.map.set_feature(i, Feature::Plant { kind, growth: 1.0 });
                }
            }
        }
    }

    /// How many plants of `kind` grow on the map.
    pub fn wild_count(&self, kind: PlantId) -> usize {
        self.map.feature.iter().filter(|f| matches!(f, Feature::Plant { kind: k, .. } if *k == kind)).count()
    }

    /// Most wild plants of `kind` the map regrows to: its share (`density`)
    /// of the columns with open soil on top, as when the map was made.
    pub fn wild_cap(&self, kind: PlantId) -> usize {
        let Some(w) = kind.def().wild.as_ref() else { return 0 };
        let m = &self.map;
        // Columns whose topmost solid ground is soil with open air above (whatever stands on it).
        let soil_top = |x: usize, y: usize| {
            let top = (0..m.d).rev().find(|&z| m.block[m.idx((x, y, z))].solid());
            top.is_some_and(|z| m.block[m.idx((x, y, z))] == Block::Soil && z + 1 < m.d)
        };
        let soil = (0..m.h).flat_map(|y| (0..m.w).map(move |x| (x, y))).filter(|&(x, y)| soil_top(x, y)).count();
        (w.density * soil as f32).ceil() as usize
    }

    /// A wild plant, weighted by how readily each regrows.
    fn pick_regrowing(&mut self) -> Option<PlantId> {
        let wild: Vec<(PlantId, f32)> =
            PlantId::all().filter_map(|k| k.def().wild.as_ref().map(|w| (k, w.regrow))).collect();
        let total: f32 = wild.iter().map(|w| w.1).sum();
        let mut r = self.rng.f32() * total;
        for &(kind, weight) in &wild {
            if r < weight {
                return Some(kind);
            }
            r -= weight;
        }
        wild.last().map(|w| w.0)
    }

    /// Items drop to the next support; plants that lost their ground die.
    fn settle_unsupported(&mut self) {
        self.map.support_dirty = false;
        for i in 0..self.map.len() {
            let p = self.map.pos(i);
            if p.2 == 0 || self.map.block[i] != Block::Air || self.map.supported(p) {
                continue;
            }
            if matches!(self.map.feature[i], Feature::Plant { .. }) {
                self.map.set_feature(i, Feature::None);
            }
            let land = self.map.landing(p);
            self.map.move_items(i, land);
        }
    }

    /// A pawn left standing on nothing drops to the next support. Falls of
    /// two levels or more hurt.
    fn fall_if_unsupported(&mut self, p: &mut Pawn) {
        let i = self.map.idx(p.pos);
        if p.pos.2 == 0 || self.map.block[i] != Block::Air || self.map.supported(p.pos) {
            return;
        }
        // Climbing out of a pit up its wall (see `way_out`): holding on.
        if !p.path.is_empty() && self.map.cling(p.pos) {
            return;
        }
        let land = self.map.landing(p.pos);
        let levels = p.pos.2 - land.2;
        p.pos = land;
        p.x = land.0 as f32;
        p.y = land.1 as f32;
        p.z = land.2 as f32;
        p.path.clear();
        p.path_goal = None;
        self.sound("thud", Some(land));
        if levels >= 2 {
            for _ in 0..levels - 1 {
                p.health.apply_damage(7.0, crate::health::DamageKind::Bruise, |_| 0.0, &mut self.rng);
            }
            p.health.recompute();
            self.push_log(format!("{} fell {} levels.", p.name, levels));
        }
    }

    fn update_pawn(&mut self, p: &mut Pawn, dt: f32) {
        if !p.present() {
            return;
        }
        self.fall_if_unsupported(p);
        p.cooldown = (p.cooldown - dt).max(0.0);
        self.update_health(p, dt);
        if p.is_dead() {
            return;
        }
        if p.is_colonist() {
            self.update_needs(p, dt);
        }
        if let Life::Downed { time } = &mut p.life {
            *time += dt;
            // Nobody treats raiders; left lying long enough, they die.
            if p.faction == Faction::Raider && *time > 120.0 {
                self.kill(p, "succumbed to their wounds");
            }
            return;
        }
        // Raiders only act in battles (`battle.rs`).
        if p.faction == Faction::Colony {
            p.getting_up = (p.getting_up - dt).max(0.0);
            let was_seated = self.seated(p);
            self.colonist_think(p, dt);
            self.run_job(p, dt);
            // Off a chair: standing up takes a moment.
            if self.seated(p) {
                p.getting_up = 0.0;
            } else if was_seated {
                p.getting_up = crate::recreation::GET_UP;
            }
        }
    }

    pub(crate) fn update_health(&mut self, p: &mut Pawn, dt: f32) {
        let lying = p.lying();
        // A sleeping spot heals no better than lying anywhere.
        let in_bed = lying && self.map.feature[self.map.idx(p.pos)].structure().is_some_and(|d| d.proper_bed());
        // A hospital bed heals faster.
        let care = if in_bed { self.room_effects(p.pos).map_or(1.0, |(_, e)| e.heal) } else { 1.0 };
        // Raiders come dressed for the weather.
        let felt = if p.is_colonist() { p.felt } else { 20.0 };
        let ctx = TickCtx { in_bed, resting: lying, fed: !p.is_colonist() || p.hunger > 0.0, care, felt };
        let events = p.health.tick(dt, ctx, &mut self.rng);
        p.health.recompute();
        for e in events {
            self.push_log(format!("{} {}", p.name, e));
        }
        if let Some(cause) = p.health.death_cause() {
            self.kill(p, &cause);
            return;
        }
        // Bleeding leaves a trail.
        let bleed = p.health.bleed_rate();
        if bleed > 0.0 && self.rng.chance(bleed * dt * 0.5) {
            let i = self.map.idx(p.pos);
            self.map.blood[i] = (self.map.blood[i] + 0.5).min(1.0);
        }
        match (p.life, p.health.downed_reason()) {
            (Life::Up, Some(reason)) => {
                self.end_job(p);
                p.life = Life::Downed { time: 0.0 };
                self.push_log(format!("{} collapsed ({}).", p.name, reason));
                self.sound("thud", Some(p.pos));
            }
            (Life::Downed { .. }, None) => {
                p.life = Life::Up;
                self.push_log(format!("{} got back up.", p.name));
            }
            (Life::Up, None) | (Life::Downed { .. }, Some(_)) | (Life::Dead { .. } | Life::Left, _) => {}
        }
    }

    fn update_needs(&mut self, p: &mut Pawn, dt: f32) {
        p.felt = crate::temperature::felt(self.temperature_at(p.pos), p.warmth());
        if !p.sleeping && p.is_colonist() {
            p.joy = (p.joy - crate::recreation::JOY_DRAIN * dt).max(0.0);
        }
        p.hunger = (p.hunger - HUNGER_DRAIN * dt).max(0.0);
        if !p.sleeping {
            p.rest = (p.rest - REST_DRAIN * dt).max(0.0);
        }

        for m in p.memories.iter_mut() {
            m.left -= dt;
        }
        p.memories.retain(|m| m.left > 0.0);
        let target = p.mood_target();
        p.mood += (target - p.mood) * (dt * 0.05).min(1.0);

        if p.break_timer > 0.0 {
            p.break_timer -= dt;
            if p.break_timer <= 0.0 {
                p.add_memory(Memory::Catharsis);
                self.push_log(format!("{} has recovered from their mental break.", p.name));
            }
        } else if p.mood < 20.0 && !p.is_downed() && self.rng.chance(dt * 0.01) {
            self.end_job(p);
            p.break_timer = 40.0;
            self.push_log(format!("{} is having a mental break and wanders off in a daze.", p.name));
        }
    }

    /// Whether someone at `p` can walk to the edge of the map.
    fn reaches_edge(&self, p: Pos) -> bool {
        let m = &self.map;
        self.nearest_cell(p, crate::pathing::ANY_DISTANCE, |i| {
            let (x, y, _) = m.pos(i);
            (x == 0 || y == 0 || x == m.w - 1 || y == m.h - 1) && m.standable(m.pos(i))
        })
        .is_some()
    }

    /// Once a second: who is shut in.
    pub(crate) fn update_trapped(&mut self) {
        self.trapped = if self.battle.is_some() {
            Vec::new()
        } else {
            let edges = crate::pathing::edge_reach(&self.map);
            self.pawns
                .iter()
                .filter(|p| p.is_colonist() && p.active())
                .filter(|p| !edges.contains(&self.map, p.pos))
                .map(|p| p.id)
                .collect()
        };
    }

    /// For a trapped pawn at `p`: the nearest spot over the furniture, or up
    /// the wall of a pit, from which the edge of the map can be walked to.
    fn way_out(&self, p: Pos) -> Option<Pos> {
        let m = &self.map;
        let inside = bfs(m, p);
        let climbing = crate::pathing::bfs_climbing(m, p);
        let out = climbing
            .nearest_first(crate::pathing::ANY_DISTANCE)
            .map(|(i, _)| m.pos(i))
            .filter(|&c| inside.dist(c).is_none() && m.standable(c))
            .take(6)
            .find(|&c| self.reaches_edge(c));
        out
    }

    fn colonist_think(&mut self, p: &mut Pawn, dt: f32) {
        if p.break_timer > 0.0 {
            // In a daze, but not so far gone as to starve.
            if p.hunger < HUNGER_WAKE && !matches!(p.job, Some(Job::Eat { .. })) && self.try_eat(p) {
                return;
            }
            if p.job.is_none() {
                self.start_wander(p, 10);
            }
            return;
        }
        p.think_timer -= dt;
        if p.think_timer > 0.0 {
            return;
        }
        p.think_timer = 0.5;

        // Shut in: climb out over the furniture first.
        if self.trapped.contains(&p.id) && !matches!(p.job, Some(Job::Goto { .. })) {
            if let Some(out) = self.way_out(p.pos) {
                self.end_job(p);
                self.start_job(p, Job::Goto { to: out });
                return;
            }
        }

        // Urgent needs interrupt whatever they're doing.
        let on_need = matches!(p.job, Some(Job::Eat { .. }) | Some(Job::Sleep { .. }));
        if !on_need {
            if p.hunger < HUNGER_URGENT && self.try_eat(p) {
                return;
            }
            if p.rest < 8.0 && !matches!(p.job, Some(Job::Patient { .. })) {
                self.end_job(p);
                let bed = self.find_bed(p);
                self.start_job(p, Job::Sleep { bed });
                return;
            }
        }
        // Freezing: off to somewhere warm (unless eating, sleeping or laid up).
        let warming = matches!(p.job, Some(Job::WarmUp { .. }));
        if !on_need && !warming && !matches!(p.job, Some(Job::Patient { .. })) && self.needs_warmth(p) {
            if let Some(to) = self.warm_spot(p) {
                self.end_job(p);
                self.start_job(p, Job::WarmUp { to });
                return;
            }
        }
        // Sleepers wake up to eat when they'd otherwise starve.
        if matches!(p.job, Some(Job::Sleep { .. })) && p.hunger < HUNGER_WAKE && self.try_eat(p) {
            return;
        }
        let tending_self = matches!(p.job, Some(Job::Tend { patient, .. }) if patient == p.id);
        let resting = matches!(p.job, Some(Job::Patient { .. }));
        if !on_need && !tending_self && p.health.wants_bed_rest() {
            // With no one else to treat them, doctors patch themselves up first.
            if p.health.needs_tending()
                && p.priorities[WorkType::Doctor].is_enabled()
                && !self.other_doctor_available(p.id)
            {
                let job = self.self_tend_job(p);
                self.end_job(p);
                self.start_job(p, job);
                return;
            }
            if !resting {
                self.end_job(p);
                let bed = self.find_patient_bed(p);
                self.start_job(p, Job::Patient { bed });
                return;
            }
        }
        if p.job.is_some() {
            return;
        }
        if p.hunger < HUNGER_IDLE && self.try_eat(p) {
            return;
        }
        // Tired, or (a little) in their hours for sleep.
        let block = self.block_now(p);
        if p.rest < 30.0 || (block == crate::recreation::Block::Sleep && p.rest < 90.0) {
            let bed = self.find_bed(p);
            self.start_job(p, Job::Sleep { bed });
            return;
        }
        // Carry on where the player sent them.
        if let Some(job) = self.forced_job(p) {
            self.take_over(p.id, job);
            self.start_job(p, job);
            return;
        }
        // An ordered operation, what it needs and a doctor: onto the operating table.
        let ops = std::mem::take(&mut p.surgeries);
        p.surgeries = ops.into_iter().filter(|&o| Self::still_to_do(p, o)).collect();
        if let Some(table) = self.operation_table(p) {
            self.start_job(p, Job::Operation { table });
            return;
        }
        // Their hours for fun, or bored in hours for anything.
        if let Some(job) = self.fun_now(p) {
            self.start_job(p, job);
            return;
        }
        // No work in fun hours.
        if block != crate::recreation::Block::Fun {
            if let Some(job) = self.find_work(p) {
                self.start_job(p, job);
                return;
            }
        }
        if self.rng.chance(0.25) {
            // Now and then they sit down on a chair nearby instead.
            if self.rng.chance(0.4) {
                if let Some(at) = self.free_chair(p, 12) {
                    let until = self.time() + self.rng.range_f32(15.0, 30.0);
                    self.start_job(p, Job::Sit { at, until });
                    return;
                }
            }
            self.start_wander(p, 5);
        }
    }

    // ---------- jobs, reservations, movement ----------

    pub(crate) fn release(&mut self, id: PawnId) {
        self.res_target.release(id);
        self.res_item.release(id);
        self.res_cell.release(id);
        self.res_patient.retain(|&(_, doctor)| doctor != id);
    }

    pub(crate) fn start_job(&mut self, p: &mut Pawn, job: Job) {
        let id = p.id;
        match job {
            Job::Work { target, .. } => {
                let i = self.map.idx(target);
                self.res_target.set(i, Some(id));
            }
            Job::Haul { from, to, deliver, .. } => {
                let (fi, ti) = (self.map.idx(from), self.map.idx(to));
                self.res_item.set(fi, Some(id));
                if deliver {
                    self.res_target.set(ti, Some(id));
                } else {
                    self.res_cell.set(ti, Some(id));
                }
            }
            Job::Eat { from, .. } | Job::Equip { from, .. } => {
                let i = self.map.idx(from);
                self.res_item.set(i, Some(id));
            }
            Job::Relax { at, .. } => {
                let i = self.map.idx(at);
                self.res_target.set(i, Some(id));
            }
            Job::Sleep { bed: Some(b) } => p.bed = Some(b),
            // Hospital beds and operating tables stay nobody's.
            Job::Patient { bed: Some(b) } if !self.care_bed(b) => p.bed = Some(b),
            Job::Surgery { patient, prosthetic, .. } => {
                if let Some(at) = prosthetic {
                    let i = self.map.idx(at);
                    self.res_item.set(i, Some(id));
                }
                self.res_patient.push((patient, id));
            }
            Job::Tend { patient, medicine, .. } => {
                if let Some(m) = medicine {
                    let i = self.map.idx(m);
                    self.res_item.set(i, Some(id));
                }
                if patient != id {
                    self.res_patient.push((patient, id));
                }
            }
            Job::Rescue { patient, bed, .. } => {
                self.res_patient.push((patient, id));
                let care = self.care_bed(bed);
                if let Some(pi) = self.pawn_index(patient).filter(|_| !care) {
                    self.pawns[pi].bed = Some(bed);
                }
            }
            Job::Feed { patient, food, .. } => {
                let i = self.map.idx(food);
                self.res_item.set(i, Some(id));
                self.res_patient.push((patient, id));
            }
            Job::Bury { corpse, grave, .. } => {
                let i = self.map.idx(grave);
                self.res_target.set(i, Some(id));
                self.res_patient.push((corpse, id));
            }
            _ => {}
        }
        p.job = Some(job);
        p.job_progress = 0.0;
        p.path.clear();
        p.path_goal = None;
    }

    /// Stop the current job, freeing reservations and dropping anything carried.
    pub(crate) fn end_job(&mut self, p: &mut Pawn) {
        self.release(p.id);
        if let Some(c) = p.carrying.take() {
            self.map.drop_item(p.pos, c);
        }
        p.job = None;
        p.job_progress = 0.0;
        p.path.clear();
        p.path_goal = None;
        p.sleeping = false;
        p.resting = false;
    }

    /// Walk towards `goal` (or next to it if `adjacent`), planning a path as needed.
    pub(crate) fn go_to(&self, p: &mut Pawn, goal: Pos, adjacent: bool, dt: f32) -> Step {
        self.go_to_reach(p, goal, if adjacent { Reach::Beside } else { Reach::On }, dt)
    }

    pub(crate) fn go_to_reach(&self, p: &mut Pawn, goal: Pos, reach: Reach, dt: f32) -> Step {
        // What can't be stepped onto (a shelf) is reached from beside it.
        let reach = if reach == Reach::On && p.pos != goal && self.map.storage_furniture(self.map.idx(goal)) { Reach::Beside } else { reach };
        // Shut in: over the furniture (or up a pit's wall) if need be, and on up once climbing.
        let climb = self.trapped.contains(&p.id) || self.map.cling(p.pos);
        if p.path_goal != Some(goal) {
            p.path.clear();
            p.path_goal = Some(goal);
            let at_goal = match reach {
                Reach::On => p.pos == goal,
                Reach::Beside => p.pos == goal || crate::map::adjacent(p.pos, goal),
                Reach::Build => self.map.build_around(goal, self.map.overhead_work(goal)).any(|c| c == p.pos),
            };
            if !at_goal {
                let m = &self.map;
                let stand = |c: Pos| c == p.pos || !m.storage_furniture(m.idx(c));
                let goals: Vec<Pos> = match reach {
                    Reach::Build => m.build_around(goal, m.overhead_work(goal))
                        .filter(|&c| !m.storage_furniture(m.idx(c))).collect(),
                    Reach::On => vec![goal],
                    Reach::Beside => std::iter::once(goal).chain(m.around(goal)).filter(|&c| stand(c)).collect(),
                };
                match crate::pathing::path_to_any(m, p.pos, &goals, climb) {
                    Some(path) => p.path = path,
                    None => {
                        p.path_goal = None;
                        return Step::Unreachable;
                    }
                }
            }
        }
        if let Some(&next) = p.path.last() {
            // Still getting up from a chair.
            if p.getting_up > 0.0 {
                return Step::Moving;
            }
            let over = climb && (self.map.climbable(next) || self.map.cling(next));
            if !self.map.passable(next) && !over {
                // Something was built or grew in the way; plan again next tick.
                p.path.clear();
                p.path_goal = None;
                return Step::Moving;
            }
            // Climbing over furniture is slow going.
            let dt = if over { dt * CLIMB_PACE } else { dt };
            if move_towards(p, next, dt) {
                p.pos = next;
                p.path.pop();
            }
            return Step::Moving;
        }
        // Path done: settle onto the tile centre before acting.
        let here = p.pos;
        if move_towards(p, here, dt) {
            Step::Arrived
        } else {
            Step::Moving
        }
    }

    // ---------- player orders ----------

    /// Whether `order` makes sense for cell `p`.
    pub fn order_applies(&self, p: Pos, order: Order) -> bool {
        let m = &self.map;
        let i = m.idx(p);
        let f = m.feature[i];
        match order {
            Order::Desig(Desig::Mine) => m.mineable(p),
            Order::Desig(Desig::Fill) => m.fillable(p, true) && m.desig[i] != Some(Desig::Fill),
            // A filled grave stays.
            Order::Desig(Desig::Deconstruct) => m.deconstructable(p) && self.grave_occupant(p).is_none(),
            Order::Desig(d) => f.harvest_desig() == Some(d),
            // A shelf's own storage goes only with the shelf. Quarries go too.
            Order::Zone(Zone::None) => m.zone[i] != Zone::None && !self.container_at(i) || m.quarry_at(p.0, p.1).is_some(),
            Order::Zone(Zone::Stockpile(_)) => false,
            // Safe zones go indoors too, among the furniture (but not over shelves).
            Order::Zone(Zone::Safe) => m.standable(p) && m.zone[i] != Zone::Safe && !self.container_at(i),
            // Crops need open sky.
            Order::Zone(Zone::Grow(_)) if m.covered(p) => false,
            Order::Zone(z) => m.standable(p) && (f == Feature::None || f.is_crop()) && m.zone[i] != z,
            Order::Stockpile => {
                m.standable(p) && (f == Feature::None || f.is_crop()) && m.zone[i].stockpile().is_none()
            }
            Order::Quarry => self.quarry_applies(p),
            // Solar panels need open sky.
            Order::Build(kind) if kind.def().open_sky && m.covered(p) => false,
            // Things for walls go into a wall's slots (`order_decor`).
            Order::Build(kind) if kind.def().on_wall => false,
            // A wall, door or window can be built over another: the old one stands
            // until the new one replaces it.
            Order::Build(kind) if f.structure().is_some_and(|d| d.blocks_room) => {
                let old = f.structure_id().unwrap();
                let planned = matches!(f, Feature::Blueprint { kind: k, .. } if k == kind);
                kind.def().blocks_room && kind != old && !planned
            }
            Order::Build(kind) => {
                // A blueprint of something else is simply replaced.
                let replacing = matches!(f, Feature::Blueprint { kind: other, .. } if other != kind);
                let empty = f == Feature::None || replacing;
                match kind.def().special {
                    // Over a roof too: the floor replaces it.
                    Special::Floor => {
                        empty && m.block[i] == Block::Air && !m.floor[i] && !m.on_ground(p) && m.floor_supported(p)
                    }
                    // Roofs come by themselves (`mark_roofs`).
                    Special::Roof => false,
                    Special::Stairs => {
                        m.standable(p) && empty && m.above(p).is_some_and(|a| m.block[m.idx(a)] == Block::Air)
                    }
                    // Dug into natural soil.
                    Special::Grave => {
                        m.standable(p) && empty && !m.floor[i] && m.below(p).is_some_and(|b| m.block[m.idx(b)] == Block::Soil)
                    }
                    // Walls, doors and windows also stack straight onto walls, doors and
                    // windows below them, built or planned (`stacked_on_plan`).
                    _ => {
                        empty
                            && (m.standable(p)
                            || kind.def().blocks_room
                                && m.open(p)
                                    && m.below(p).is_some_and(|b| m.feature[m.idx(b)].planned().is_some_and(|d| d.holds_floors)))
                    }
                }
            }
            // Roof blueprints would only come back.
            Order::Cancel => {
                m.desig[i].is_some()
                    || matches!(f, Feature::Blueprint { .. }) && !m.roof_blueprint(i)
                    || self.move_from(p).is_some()
            }
        }
    }

    /// Apply a player order to one cell. Returns false if it doesn't apply there.
    pub fn apply_order(&mut self, p: Pos, order: Order) -> bool {
        if !self.order_applies(p, order) {
            return false;
        }
        let i = self.map.idx(p);
        self.map.mark_changed();
        match order {
            Order::Desig(d) => self.map.set_desig(i, Some(d)),
            Order::Zone(Zone::None) if self.map.quarry_at(p.0, p.1).is_some() => {
                self.remove_quarry_column(p.0, p.1);
                if !self.container_at(i) {
                    self.map.zone[i] = Zone::None;
                }
            }
            Order::Zone(z) => self.map.zone[i] = z,
            Order::Quarry => self.add_quarry_column(p),
            Order::Stockpile => {
                let id = self.stockpile_for(p);
                self.map.zone[i] = Zone::Stockpile(id);
            }
            Order::Build(kind) => {
                // Materials already delivered to a replaced blueprint are left for the haulers.
                if let Feature::Blueprint { kind: old, delivered, .. } = self.map.feature[i] {
                    let at = self.map.landing(p);
                    for (item, count) in old.def().delivered_split(delivered) {
                        self.map.drop_item(at, Stack { kind: item, count });
                    }
                }
                let stairs = |f: Feature| f.planned().is_some_and(|d| d.special == Special::Stairs);
                if stairs(self.map.feature[i]) || kind.def().special == Special::Stairs {
                    // Planned stairs keep the roof off (see `recompute_rooms`).
                    self.map.rooms_dirty = true;
                }
                // Built over a standing wall (door, window), or one already being built over.
                let over = match self.map.feature[i] {
                    Feature::Built { kind, dir } => Some((kind, dir)),
                    Feature::Blueprint { over, .. } => over,
                    _ => None,
                };
                self.map.feature[i] = Feature::Blueprint { kind, delivered: 0, dir: crate::map::Dir::PlusX, over };
            }
            Order::Cancel => {
                self.map.set_desig(i, None);
                self.cancel_move(p);
                self.cancel_blueprint(p);
            }
        }
        true
    }

    /// Take away the blueprint at `p`, dropping what was delivered to it, and
    /// any walls planned on top of it that are now left hanging. What it was
    /// to be built over simply stays.
    fn cancel_blueprint(&mut self, mut p: Pos) {
        let mut i = self.map.idx(p);
        while let Feature::Blueprint { kind, delivered, over, .. } = self.map.feature[i] {
            if kind.def().special == Special::Stairs {
                self.map.rooms_dirty = true;
            }
            self.map.feature[i] = over.map_or(Feature::None, |(kind, dir)| Feature::Built { kind, dir });
            self.map.mark_changed();
            let at = self.map.landing(p);
            for (item, count) in kind.def().delivered_split(delivered) {
                self.map.drop_item(at, Stack { kind: item, count });
            }
            if over.is_some() {
                break;
            }
            // Nothing holds up a wall planned on top any more.
            let m = &self.map;
            let hanging = |a: Pos| matches!(m.feature[m.idx(a)], Feature::Blueprint { .. }) && !m.standable(a);
            match self.map.above(p) {
                Some(a) if hanging(a) => {
                    p = a;
                    i = self.map.idx(a);
                }
                _ => break,
            }
        }
    }

    /// A blueprint of a wall (door, window) planned on top of one that isn't
    /// built yet. It waits for the one below, and goes when that is cancelled.
    pub(crate) fn stacked_on_plan(&self, p: Pos) -> bool {
        let m = &self.map;
        matches!(m.feature[m.idx(p)], Feature::Blueprint { .. })
            && !m.standable(p)
            && m.below(p).is_some_and(|b| matches!(m.feature[m.idx(b)], Feature::Blueprint { over: None, .. }))
    }

    /// Testing: drop `count` of `kind` at `at` (spilling onto the cells around).
    pub fn spawn_items(&mut self, at: Pos, kind: crate::defs::ItemId, count: u32) {
        if count == 0 {
            return;
        }
        self.map.drop_item(at, Stack { kind, count });
        self.push_log(format!("Dropped {} {} (testing).", count, kind.name().to_lowercase()));
    }

    /// Turn the blueprint at `p` to face `dir` once built. Returns false if there's none.
    pub fn turn_blueprint(&mut self, p: Pos, dir: Dir) -> bool {
        let i = self.map.idx(p);
        match &mut self.map.feature[i] {
            Feature::Blueprint { dir: d, .. } => {
                *d = dir;
                self.map.mark_changed();
                true
            }
            _ => false,
        }
    }

    /// Put roof blueprints where rooms want roofs (`Map::roof_wanted`), and
    /// take away those no longer wanted (the room was opened up), dropping
    /// what was delivered to them.
    pub fn mark_roofs(&mut self) {
        let Some(roof) = defs().roof() else { return };
        let m = &mut self.map;
        let mut wanted = vec![false; m.len()];
        let mut changed = false;
        for &i in &m.roof_wanted {
            wanted[i] = true;
            if m.feature[i] == Feature::None {
                m.feature[i] = Feature::blueprint(roof, 0, crate::map::Dir::PlusX);
                changed = true;
            }
        }
        for (i, &keep) in wanted.iter().enumerate() {
            if let Feature::Blueprint { kind, delivered, .. } = m.feature[i] {
                if kind == roof && !keep {
                    m.feature[i] = Feature::None;
                    changed = true;
                    let at = m.landing(m.pos(i));
                    for (item, count) in kind.def().delivered_split(delivered) {
                        m.drop_item(at, Stack { kind: item, count });
                    }
                }
            }
        }
        if changed { m.mark_changed(); }
    }

    /// Run `f` on a copy of pawn `id` with full access to the world.
    pub(crate) fn with_pawn(&mut self, id: PawnId, f: impl FnOnce(&mut World, &mut Pawn)) {
        if let Some(i) = self.pawn_index(id) {
            let mut p = self.pawns[i].clone();
            f(self, &mut p);
            self.pawns[i] = p;
        }
    }

    /// Have colonist `id` pick up the weapon, apparel or belt item lying at `at` and use
    /// it, keeping it even when something better turns up.
    pub fn order_equip(&mut self, id: PawnId, at: Pos) -> bool {
        let i = self.map.idx(at);
        // The first weapon or piece of apparel here (a shelf may hold several things).
        let Some(s) = self.map.stacks(i).find(|s| s.kind.def().is_gear()) else { return false };
        if self.res_item[i].is_some_and(|holder| holder != id) {
            return false;
        }
        let kind = s.kind;
        let mut ordered = false;
        self.with_pawn(id, |w, p| {
            if p.is_colonist() && p.active() && p.break_timer <= 0.0 {
                w.end_job(p);
                w.start_job(p, Job::Equip { from: at, chosen: true, sidearm: false });
                ordered = true;
            }
        });
        // It's theirs to use from now on: in their loadout.
        if ordered {
            self.set_loadout_item(id, kind);
        }
        ordered
    }

    pub fn set_priority(&mut self, id: PawnId, wt: WorkType, prio: WorkPriority) {
        if let Some(i) = self.pawn_index(id) {
            self.pawns[i].priorities[wt] = prio;
        }
    }

    // ---------- save / load ----------

    /// A versioned save file (see `save`).
    pub fn save_json(&self) -> Result<String, String> {
        crate::save::to_json(self)
    }

    /// Load a save file, refusing other format versions with a readable message.
    pub fn load_json(s: &str) -> Result<World, String> {
        let mut w = crate::save::from_json(s)?;
        // Rebuild only what's derived from saved state; nothing saved changes.
        w.index_quarries();
        w.map.recompute_rooms();
        w.mark_roofs();
        w.sync_containers();
        w.update_room_stats();
        for p in w.pawns.iter_mut() {
            p.health.recompute();
        }
        Ok(w)
    }
}

/// Hours of the day between which it's light (plants grow, people work).
pub const DAWN: f32 = 6.0;
pub const DUSK: f32 = 21.0;

/// Hunger lost per second (100 full, 0 starving): a full stomach lasts a
/// little over a day.
pub const HUNGER_DRAIN: f32 = 0.2;
/// Rest lost per second awake.
pub(crate) const REST_DRAIN: f32 = 0.165;
/// Rest gained per second asleep on the ground (a bed adds half again).
pub(crate) const SLEEP_GAIN: f32 = 0.4;

/// Hunger below which colonists eat: between jobs, dropping what they're
/// doing, and waking up or snapping out of a mental break. With room for a
/// long walk to the food.
const HUNGER_IDLE: f32 = 50.0;
const HUNGER_URGENT: f32 = 30.0;
const HUNGER_WAKE: f32 = 15.0;

/// Seconds between attempts to grow back a wild plant.
const REGROW_SECS: f32 = 8.0;

/// Share of walking pace while climbing over furniture.
const CLIMB_PACE: f32 = 0.3;

/// Step `p` towards tile `t`; returns true once it is exactly there.
fn move_towards(p: &mut Pawn, t: Pos, dt: f32) -> bool {
    let step = p.move_speed() * dt;
    let (tx, ty, tz) = (t.0 as f32, t.1 as f32, t.2 as f32);
    let (dx, dy, dz) = (tx - p.x, ty - p.y, tz - p.z);
    let d = (dx * dx + dy * dy + dz * dz).sqrt();
    if d <= step {
        p.x = tx;
        p.y = ty;
        p.z = tz;
        true
    } else {
        p.x += dx / d * step;
        p.y += dy / d * step;
        p.z += dz / d * step;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{colonist, item, quiet_world, run};
    use crate::TICK;

    const Z: usize = crate::map::FLAT_Z;


    fn test_world(w: usize, h: usize) -> World {
        let mut world = quiet_world(w, h, 7);
        world.stockpiles.push(Stockpile::new(1));
        world
    }

    /// Never thinks of anything to do, so stays where they are.
    fn hold_still(w: &mut World, id: PawnId) {
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].think_timer = f32::MAX;
    }



    #[test]
    fn hauling_moves_items_into_the_stockpile() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        for x in 2..5 {
            let i = w.map.idx((x, 10, Z));
            w.map.zone[i] = Zone::Stockpile(crate::map::StockpileId::new(0));
        }
        let src = w.map.idx((12, 12, Z));
        w.map.items[src] = Some(Stack { kind: item("wood"), count: 20 });
        run(&mut w, 120.0);
        assert!(w.map.items[src].is_none(), "wood should have been picked up");
        let stored: u32 = (2..5).filter_map(|x| w.map.items[w.map.idx((x, 10, Z))]).map(|s| s.count).sum();
        assert_eq!(stored, 20);
    }

    #[test]
    fn construction_delivers_materials_and_builds() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let stone = w.map.idx((3, 8, Z));
        w.map.items[stone] = Some(Stack { kind: item("stone"), count: 4 });
        w.apply_order((10, 10, Z), Order::Build(BuildId::named("wall")));
        run(&mut w, 40.0);
        assert_eq!(w.map.feature[w.map.idx((10, 10, Z))], Feature::built("wall"));
        assert_eq!(w.map.count_items(item("stone")), 0);
    }

    #[test]
    fn chopping_drops_wood() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let t = w.map.idx((8, 8, Z));
        w.map.feature[t] = Feature::plant("tree");
        w.apply_order((8, 8, Z), Order::Desig(Desig::Chop));
        run(&mut w, 20.0);
        assert_eq!(w.map.feature[t], Feature::None);
        assert!(w.map.count_items(item("wood")) > 0);
    }

    #[test]
    fn hungry_colonists_eat_from_the_ground() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        // Hungry enough to drop everything (one raw food then does).
        w.pawns[i].hunger = 25.0;
        let f = w.map.idx((6, 6, Z));
        w.map.items[f] = Some(Stack { kind: item("raw_food"), count: 3 });
        // Walk over (about 8 s), eat, and check before hunger drains again.
        run(&mut w, 20.0);
        assert!(w.pawn(id).unwrap().hunger > 30.0, "ate: {}", w.pawn(id).unwrap().hunger);
        assert_eq!(w.map.count_items(item("raw_food")), 2);
    }

    #[test]
    fn grow_zone_gets_planted() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let i = w.map.idx((6, 6, Z));
        w.apply_order((6, 6, Z), Order::Zone(Zone::Grow(defs().sown().unwrap())));
        run(&mut w, 10.0);
        assert!(w.map.feature[i].is_crop());
    }

    #[test]
    fn trapped_colonists_climb_out() {
        let mut w = test_world(20, 20);
        // A room (walls 5..=11) whose door at (5, 9) opens onto a cell boxed in
        // by a lamp and a shelf: the rest of the room is shut off.
        for y in 5..=11 {
            for x in 5..=11 {
                if x == 5 || x == 11 || y == 5 || y == 11 {
                    let i = w.map.idx((x, y, Z));
                    w.map.set_feature(i, Feature::built(if (x, y) == (5, 9) { "door" } else { "wall" }));
                }
            }
        }
        for (at, id) in [((6, 8), "lamp"), ((7, 9), "shelf"), ((6, 10), "lamp")] {
            let i = w.map.idx((at.0, at.1, Z));
            w.map.set_feature(i, Feature::built(id));
        }
        let nell = colonist(&mut w, (9, 7, Z));
        let i = w.pawn_index(nell).unwrap();
        w.pawns[i].hunger = 5.0;
        w.map.drop_item((15, 15, Z), Stack { kind: item("raw_food"), count: 10 });
        w.update_trapped();
        assert_eq!(w.trapped, vec![nell]);
        let mut climbed = false;
        for _ in 0..(60.0 / TICK) as usize {
            w.tick(TICK);
            let p = w.pawn(nell).unwrap();
            climbed |= w.map.climbable(p.pos);
        }
        assert!(climbed, "over the furniture");
        assert!(w.trapped.is_empty(), "out");
        assert!(w.pawn(nell).unwrap().hunger > 30.0, "and fed");
    }

    #[test]
    fn dug_holes_can_be_filled_in() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        // A hill two levels high (with a step up to it) and a shaft dug down through it.
        raise(&mut w, 5, 5, 13, 13, 1);
        raise(&mut w, 6, 6, 12, 12, 2);
        let (low, high) = ((9, 9, Z), (9, 9, Z + 1));
        for p in [low, high] {
            let i = w.map.idx(p);
            w.map.set_block(i, Block::Air);
        }
        w.map.drop_item(low, Stack { kind: item("wood"), count: 3 });
        // The top can only be marked once the bottom is.
        assert!(!w.apply_order(high, Order::Desig(Desig::Fill)), "nothing under it yet");
        assert!(w.apply_order(low, Order::Desig(Desig::Fill)));
        assert!(!w.apply_order(low, Order::Desig(Desig::Fill)), "already marked");
        assert!(w.apply_order(high, Order::Desig(Desig::Fill)), "waits for the one below");
        // Not into the sky, nor where something is built.
        assert!(!w.apply_order((15, 15, Z + 1), Order::Desig(Desig::Fill)), "in mid-air");
        let wall = w.map.idx((3, 3, Z));
        w.map.set_feature(wall, Feature::built("wall"));
        assert!(!w.apply_order((3, 3, Z), Order::Desig(Desig::Fill)));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].priorities = crate::pawn::PerWork::filled(crate::pawn::WorkPriority::Never);
        w.pawns[i].priorities[WorkType::Mining] = crate::pawn::WorkPriority::First;
        run(&mut w, 40.0);
        for p in [low, high] {
            assert_eq!(w.map.block[w.map.idx(p)], Block::Soil, "{:?} filled", p);
            assert_eq!(w.map.desig[w.map.idx(p)], None);
        }
        assert_eq!(w.map.count_items(item("wood")), 3, "the wood was pushed out, not buried");
        assert!(w.map.standable((9, 9, Z + 2)), "level with the hilltop again");
    }

    #[test]
    fn wild_plants_regrow_only_to_their_share() {
        let mut w = test_world(30, 30);
        let tree = PlantId::named("tree");
        let cap = w.wild_cap(tree);
        assert_eq!(cap, (0.11f32 * 900.0).ceil() as usize, "11% of the open soil");
        // Many regrowth chances: far more than would fill the map without a limit.
        w.regrow_timer = 0.0;
        for _ in 0..20_000 {
            w.grow_plants(4.0);
            w.regrow_timer = 0.0;
        }
        let trees = w.wild_count(tree);
        assert!(trees <= cap && trees > cap * 3 / 4, "{} trees, cap {}", trees, cap);
        let bushes = w.wild_count(PlantId::named("berry_bush"));
        assert!(bushes <= w.wild_cap(PlantId::named("berry_bush")), "{} bushes, cap {}", bushes, w.wild_cap(PlantId::named("berry_bush")));
        // Chopped down, they grow back.
        for f in w.map.feature.iter_mut() {
            if matches!(f, Feature::Plant { kind, .. } if *kind == tree) {
                *f = Feature::None;
            }
        }
        for _ in 0..5_000 {
            w.grow_plants(4.0);
            w.regrow_timer = 0.0;
        }
        assert!(w.wild_count(tree) > 0, "a forest comes back");
    }

    #[test]
    fn nothing_grows_back_beside_walls() {
        let mut w = test_world(12, 12);
        // A walled hut in the middle, with a door and a window; the rest open soil.
        house(&mut w, 3, 3, 8, 8);
        for (at, id) in [((5, 3, Z), "door"), ((8, 5, Z), "window")] {
            let i = w.map.idx(at);
            w.map.set_feature(i, Feature::built(id));
        }
        let ring: Vec<Pos> = (0..12)
            .flat_map(|y| (0..12).map(move |x| (x, y, Z)))
            .filter(|&p| w.map.beside_wall(p) && w.map.feature[w.map.idx(p)] == Feature::None)
            .collect();
        assert!(ring.contains(&(4, 2, Z)) && ring.contains(&(9, 9, Z)), "sides and corners");
        for _ in 0..20_000 {
            w.grow_plants(4.0);
            w.regrow_timer = 0.0;
        }
        assert!(w.map.feature.iter().any(|f| matches!(f, Feature::Plant { .. })), "plants grow elsewhere");
        for p in ring {
            assert_eq!(w.map.feature[w.map.idx(p)], Feature::None, "{:?} is beside a wall", p);
        }
    }

    #[test]
    fn work_priorities_decide_what_comes_first() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        w.set_priority(id, WorkType::PlantCutting, WorkPriority::Fourth);
        w.set_priority(id, WorkType::Mining, WorkPriority::First);
        // The tree is closer, but mining has the higher priority.
        let tree = w.map.idx((3, 2, Z));
        w.map.feature[tree] = Feature::plant("tree");
        w.apply_order((3, 2, Z), Order::Desig(Desig::Chop));
        let rock = w.map.idx((15, 15, Z));
        w.map.block[rock] = Block::Rock;
        w.apply_order((15, 15, Z), Order::Desig(Desig::Mine));
        run(&mut w, 1.5);
        let job = w.pawn(id).unwrap().job;
        assert!(matches!(job, Some(Job::Work { task: crate::pawn::Task::Mine, .. })), "{:?}", job);
    }

    fn wound(w: &mut World, id: PawnId, part: crate::health::Part, amount: f32, kind: crate::health::DamageKind) {
        let i = w.pawn_index(id).unwrap();
        let mut rng = Rng::new(99);
        w.pawns[i].health.injure(part, amount, kind, &mut rng);
        w.pawns[i].health.recompute();
    }

    #[test]
    fn doctor_tends_a_wounded_colonist_with_medicine() {
        use crate::health::{DamageKind, Part};
        let mut w = test_world(20, 20);
        let doctor = colonist(&mut w, (2, 2, Z));
        let patient = colonist(&mut w, (10, 10, Z));
        w.set_priority(patient, WorkType::Doctor, WorkPriority::Never);
        let med = w.map.idx((5, 5, Z));
        w.map.items[med] = Some(Stack { kind: item("medicine"), count: 2 });
        wound(&mut w, patient, Part::LeftLeg, 10.0, DamageKind::Gunshot);
        run(&mut w, 25.0);
        let p = w.pawn(patient).unwrap();
        assert!(!p.health.needs_tending(), "patient should have been tended");
        assert_eq!(p.health.bleed_rate(), 0.0);
        assert_eq!(w.map.count_items(item("medicine")), 1);
        assert!(w.pawn(doctor).unwrap().job.is_none_or(|j| !matches!(j, Job::Tend { .. })));
    }

    #[test]
    fn downed_colonists_are_carried_to_bed() {
        use crate::health::{DamageKind, Part};
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let patient = colonist(&mut w, (12, 12, Z));
        w.set_priority(patient, WorkType::Doctor, WorkPriority::Never);
        let bed = w.map.idx((4, 15, Z));
        w.map.feature[bed] = Feature::built("bed");
        // Both legs gone: conscious, but unable to walk.
        wound(&mut w, patient, Part::LeftLeg, 100.0, DamageKind::Bruise);
        wound(&mut w, patient, Part::RightLeg, 100.0, DamageKind::Bruise);
        run(&mut w, 30.0);
        let p = w.pawn(patient).unwrap();
        assert!(p.is_downed());
        assert_eq!(p.pos, (4, 15, Z), "patient should be in the bed");
    }

    #[test]
    fn the_clock_keeps_time_in_long_games() {
        // An f32 sum of ticks ran ~2x fast around day 1100 and stopped by day 2200.
        for day in [1.0, 300.0, 1100.0, 2200.0, 5000.0] {
            let mut w = test_world(10, 10);
            w.set_time(day * DAY_LEN);
            let start = w.clock;
            for _ in 0..30 {
                w.tick(TICK);
            }
            assert!((w.clock - start - 1.0).abs() < 1e-6, "day {day}: one second took {}", w.clock - start);
        }
    }

    #[test]
    fn collapsing_never_revives_the_dead_or_recalls_those_who_left() {
        use crate::health::{DamageKind, Part};
        for gone in [Life::Dead { buried: false }, Life::Left] {
            let mut w = test_world(20, 20);
            let id = colonist(&mut w, (5, 5, Z));
            wound(&mut w, id, Part::LeftLeg, 100.0, DamageKind::Bruise);
            wound(&mut w, id, Part::RightLeg, 100.0, DamageKind::Bruise);
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].life = gone;
            let mut p = w.pawns[i].clone();
            assert!(p.health.downed_reason().is_some(), "wounds that would down a living pawn");
            w.update_health(&mut p, 0.1);
            assert_eq!(p.life, gone);
        }
    }

    #[test]
    fn a_lone_doctor_treats_themselves() {
        use crate::health::{DamageKind, Part};
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (5, 5, Z));
        wound(&mut w, id, Part::LeftArm, 6.0, DamageKind::Cut);
        run(&mut w, 15.0);
        assert!(!w.pawn(id).unwrap().health.needs_tending());
    }

    /// Raise a rectangle of ground by `levels` (natural soil).
    fn raise(w: &mut World, x0: usize, y0: usize, x1: usize, y1: usize, levels: usize) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                for z in Z..Z + levels {
                    let i = w.map.idx((x, y, z));
                    w.map.block[i] = Block::Soil;
                }
            }
        }
        w.map.recompute_rooms();
    }

    #[test]
    fn colonists_climb_a_hill_to_haul() {
        let mut w = test_world(20, 20);
        raise(&mut w, 10, 0, 19, 19, 1);
        colonist(&mut w, (2, 2, Z));
        for x in 2..5 {
            let i = w.map.idx((x, 10, Z));
            w.map.zone[i] = Zone::Stockpile(crate::map::StockpileId::new(0));
        }
        let src = w.map.idx((15, 15, Z + 1));
        w.map.items[src] = Some(Stack { kind: item("wood"), count: 20 });
        run(&mut w, 160.0);
        assert!(w.map.items[src].is_none(), "wood on the hill should have been fetched");
        assert_eq!(w.map.count_items(item("wood")), 20);
    }

    #[test]
    fn cliffs_keep_colonists_out() {
        let mut w = test_world(20, 20);
        raise(&mut w, 10, 0, 19, 19, 2);
        colonist(&mut w, (2, 2, Z));
        for x in 2..5 {
            let i = w.map.idx((x, 10, Z));
            w.map.zone[i] = Zone::Stockpile(crate::map::StockpileId::new(0));
        }
        let src = w.map.idx((15, 15, Z + 2));
        w.map.items[src] = Some(Stack { kind: item("wood"), count: 20 });
        run(&mut w, 30.0);
        assert!(w.map.items[src].is_some(), "nobody can climb a two-level cliff");
    }

    #[test]
    fn mining_digs_a_tunnel_into_rock() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 5, Z));
        for x in 8..14 {
            for y in 2..9 {
                for z in Z..Z + 3 {
                    let i = w.map.idx((x, y, z));
                    w.map.block[i] = Block::Rock;
                }
            }
        }
        w.map.recompute_rooms();
        for x in 8..11 {
            assert!(w.apply_order((x, 5, Z), Order::Desig(Desig::Mine)));
        }
        run(&mut w, 60.0);
        for x in 8..11 {
            assert_eq!(w.map.block[w.map.idx((x, 5, Z))], Block::Air, "({}, 5) should be dug out", x);
        }
        assert!(w.map.standable((10, 5, Z)), "the tunnel floor can be walked on");
        assert!(w.map.covered((10, 5, Z)), "rock overhead");
        assert!(!w.map.roofed((10, 5, Z)), "but open at one end, so not an enclosed room");
        assert!(w.map.count_items(item("stone")) >= 18);
    }

    #[test]
    fn a_generated_colony_runs_for_half_a_day() {
        let mut w = World::new(5);
        for p in &w.pawns {
            assert!(w.map.standable(p.pos), "{} starts on solid ground", p.name);
        }
        run(&mut w, DAY_LEN / 2.0);
        assert_eq!(w.colonists_alive(), 3);
        for p in w.pawns.iter().filter(|p| !p.is_dead()) {
            assert!(w.map.standable(p.pos) || p.path.is_empty(), "{} is somewhere sensible", p.name);
        }
    }

    /// A test world on soil `ground` levels deep, `d` levels in total.
    fn deep_world(w: usize, h: usize, d: usize, ground: usize) -> World {
        let mut m = Map::empty(w, h, d);
        for y in 0..h {
            for x in 0..w {
                for z in 0..ground {
                    let i = m.idx((x, y, z));
                    m.block[i] = Block::Soil;
                }
            }
        }
        m.recompute_rooms();
        let mut world = World::with_map(m, Rng::new(7));
        world.story.next_raid = f32::MAX;
        world.story.next_event = f32::MAX;
        world.regrow_timer = f32::MAX;
        world
    }

    #[test]
    fn digging_down_drops_whoever_stands_there() {
        let mut w = deep_world(12, 12, 6, 3);
        let digger = colonist(&mut w, (2, 2, 3));
        let bystander = colonist(&mut w, (6, 6, 3));
        w.set_priority(bystander, WorkType::Mining, WorkPriority::Never);
        // Keep the bystander standing still on the spot being dug out.
        hold_still(&mut w, bystander);
        assert!(w.apply_order((6, 6, 2), Order::Desig(Desig::Mine)), "soil can be dug");
        run(&mut w, 30.0);
        assert_eq!(w.map.block[w.map.idx((6, 6, 2))], Block::Air);
        assert_eq!(w.pawn(bystander).unwrap().pos, (6, 6, 2), "fell into the hole");
        assert!(w.pawn(digger).is_some());
    }

    #[test]
    fn long_falls_hurt() {
        let mut w = deep_world(8, 8, 8, 1);
        let ledge = w.map.idx((4, 4, 5));
        w.map.floor[ledge] = true;
        let id = colonist(&mut w, (4, 4, 5));
        hold_still(&mut w, id);
        let i = w.map.idx((4, 4, 5));
        w.map.set_floor(i, false);
        run(&mut w, 0.5);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.pos, (4, 4, 1));
        assert!(!p.health.hediffs.is_empty(), "a four-level fall should bruise");
    }

    #[test]
    fn colonists_floor_over_a_room_from_inside() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (7, 7, Z));
        house(&mut w, 5, 5, 9, 9);
        let mut placed = 0;
        for y in 5..=9 {
            for x in 5..=9 {
                placed += w.apply_order((x, y, Z + 1), Order::Build(BuildId::named("floor"))) as usize;
            }
        }
        assert_eq!(placed, 25, "every cell over the room has wall support nearby");
        w.map.drop_item((7, 7, Z), Stack { kind: item("wood"), count: 50 });
        run(&mut w, 480.0);
        let built = (5..=9).flat_map(|y| (5..=9).map(move |x| (x, y))).filter(|&(x, y)| w.map.floor[w.map.idx((x, y, Z + 1))]).count();
        assert_eq!(built, 25, "all floors built");
        assert!(w.map.roofed((7, 7, Z)), "now it's a room");
    }

    /// Walls from (x0, y0) to (x1, y1) with a door in the west wall, rooms recomputed and roofs marked.
    fn house(w: &mut World, x0: usize, y0: usize, x1: usize, y1: usize) {
        let door = (x0, (y0 + y1) / 2);
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x == x0 || x == x1 || y == y0 || y == y1 {
                    let i = w.map.idx((x, y, Z));
                    w.map.set_feature(i, if (x, y) == door { Feature::built("door") } else { Feature::built("wall") });
                }
            }
        }
        w.map.recompute_rooms();
        w.mark_roofs();
    }

    /// Cells at level `z` with a roof blueprint.
    fn roof_blueprints(w: &World, z: usize) -> Vec<(usize, usize)> {
        let m = &w.map;
        (0..m.h).flat_map(|y| (0..m.w).map(move |x| (x, y))).filter(|&(x, y)| m.roof_blueprint(m.idx((x, y, z)))).collect()
    }

    #[test]
    fn enclosed_rooms_get_roof_blueprints() {
        let mut w = test_world(16, 16);
        house(&mut w, 5, 5, 9, 9);
        let marked = roof_blueprints(&w, Z + 1);
        let inner: Vec<(usize, usize)> = (6..=8).flat_map(|y| (6..=8).map(move |x| (x, y))).collect();
        assert_eq!(marked, inner, "over every inner cell, not over the walls or outside");
        assert!(roof_blueprints(&w, Z + 2).is_empty(), "nothing over the roof");
        // Natural hollows don't get roofs: a pit in a hill.
        raise(&mut w, 0, 11, 4, 15, 1);
        let pit = w.map.idx((2, 13, Z));
        w.map.set_block(pit, Block::Air);
        w.map.recompute_rooms();
        w.mark_roofs();
        assert_eq!(roof_blueprints(&w, Z + 1), inner, "no roof over the pit");
    }

    #[test]
    fn sleeping_spots_cost_nothing_and_count_as_beds() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        assert_eq!(w.find_bed(w.pawn(id).unwrap()), None, "no bed yet");
        let spot = BuildId::named("sleeping_spot");
        assert!(spot.def().free());
        assert!(w.apply_order((8, 8, Z), Order::Build(spot)));
        run(&mut w, 20.0);
        let i = w.map.idx((8, 8, Z));
        assert_eq!(w.map.feature[i], Feature::built("sleeping_spot"), "built without materials");
        assert_eq!(w.find_bed(w.pawn(id).unwrap()), Some((8, 8, Z)), "a place to sleep");
    }

    #[test]
    fn items_can_be_dropped_for_testing() {
        let mut w = test_world(16, 16);
        w.spawn_items((8, 8, Z), item("copper"), 120);
        assert_eq!(w.map.count_items(item("copper")), 120);
        let stacks: Vec<_> = w.map.items.iter().flatten().filter(|s| s.kind == item("copper")).collect();
        assert_eq!(stacks.len(), 3, "spread over cells of 50");
        assert!(stacks.iter().all(|s| s.count <= crate::map::STACK_MAX));
    }

    #[test]
    fn benches_are_built_from_several_materials_in_order() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        let bench = BuildId::named("machinist_bench");
        let (structural, steel) = (item("structural_components"), item("steel"));
        assert!(w.apply_order((8, 8, Z), Order::Build(bench)));
        let i = w.map.idx((8, 8, Z));
        // Only the steel around: the structural components come first, so nothing happens.
        w.map.drop_item((3, 3, Z), Stack { kind: steel, count: 30 });
        run(&mut w, 20.0);
        assert!(matches!(w.map.feature[i], Feature::Blueprint { delivered: 0, .. }), "waits for the first material");
        w.map.drop_item((3, 6, Z), Stack { kind: structural, count: 8 });
        run(&mut w, 90.0);
        assert_eq!(w.map.feature[i], Feature::built("machinist_bench"), "then built from both");
        assert_eq!(w.map.count_items(steel) + w.map.count_items(structural), 0);

        // Called off half-way: exactly what was brought comes back.
        assert!(w.apply_order((12, 12, Z), Order::Build(bench)));
        let j = w.map.idx((12, 12, Z));
        w.map.feature[j] = Feature::blueprint(bench, 8 + 10, crate::map::Dir::PlusX);
        assert!(w.apply_order((12, 12, Z), Order::Cancel));
        assert_eq!((w.map.count_items(structural), w.map.count_items(steel)), (8, 10));
    }

    #[test]
    fn solar_panels_need_open_sky() {
        let mut w = test_world(16, 16);
        let roof = w.map.idx((8, 8, Z + 1));
        w.map.set_roof(roof, true);
        assert!(!w.apply_order((8, 8, Z), Order::Build(BuildId::named("pv_system"))), "not under a roof");
        assert!(w.apply_order((8, 8, Z), Order::Build(BuildId::named("home_battery"))), "a battery is fine");
        assert!(w.apply_order((4, 4, Z), Order::Build(BuildId::named("pv_system"))), "in the open");
        let pv = BuildId::named("pv_system").def();
        assert_eq!(pv.power_text().as_deref(), Some("Makes up to 1 kW in daylight"));
    }

    #[test]
    fn copper_is_mined_and_raiders_leave_scrap() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        w.set_priority(id, WorkType::Mining, WorkPriority::First);
        let i = w.map.idx((6, 6, Z));
        w.map.set_block(i, Block::Copper);
        assert!(w.apply_order((6, 6, Z), Order::Desig(Desig::Mine)));
        run(&mut w, 30.0);
        assert_eq!(w.map.count_items(item("copper_ore")), defs().copper_drops.count);

        let r = w.spawn_raider("Vex".into(), (10, 10, Z));
        let ri = w.pawn_index(r).unwrap();
        let mut raider = w.pawns[ri].clone();
        w.kill(&mut raider, "died");
        assert!((1..=2).contains(&w.map.count_items(item("scrap_metal"))), "scrap where they fell");
    }

    #[test]
    fn laser_burns_dont_bleed() {
        use crate::health::DamageKind;
        let mut h = crate::health::Health::new();
        h.injure(crate::health::Part::LeftArm, 10.0, DamageKind::Burn, &mut crate::rng::Rng::new(1));
        h.recompute();
        assert_eq!(h.bleed_rate(), 0.0, "seared shut");
        let mut cut = crate::health::Health::new();
        cut.injure(crate::health::Part::LeftArm, 10.0, DamageKind::Cut, &mut crate::rng::Rng::new(1));
        cut.recompute();
        assert!(h.pain > cut.pain, "but it hurts more");
        assert_eq!(item("laser_rifle").def().weapon.as_ref().map(|w| w.kind), Some(DamageKind::Burn));
    }

    #[test]
    fn things_are_built_turned_as_placed() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        let chair = BuildId::named("chair");
        w.map.drop_item((3, 3, Z), Stack { kind: chair.def().cost[0].item, count: 10 });
        assert!(w.apply_order((8, 8, Z), Order::Build(chair)));
        assert!(w.turn_blueprint((8, 8, Z), crate::map::Dir::MinusY));
        assert!(!w.turn_blueprint((9, 9, Z), crate::map::Dir::MinusX), "no blueprint there");
        // Stairs with a platform to the east, turned west (where they can't be climbed): still built as placed.
        let stairs = BuildId::named("stairs");
        for x in 13..16 {
            for y in 9..12 {
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_floor(i, true);
            }
        }
        let s = w.map.idx((12, 10, Z));
        w.map.feature[s] = Feature::blueprint(stairs, stairs.def().total_cost(), crate::map::Dir::MinusX);
        run(&mut w, 60.0);
        assert_eq!(w.map.feature[w.map.idx((8, 8, Z))], Feature::Built { kind: chair, dir: crate::map::Dir::MinusY });
        assert_eq!(w.map.feature[s], Feature::Built { kind: stairs, dir: crate::map::Dir::MinusX }, "as the blueprint shows");
    }

    #[test]
    fn stairs_get_no_roof_over_them() {
        let mut w = test_world(16, 16);
        let i = w.map.idx((7, 7, Z));
        w.map.set_feature(i, Feature::Built { kind: BuildId::named("stairs"), dir: crate::map::Dir::PlusX });
        house(&mut w, 5, 5, 9, 9);
        let marked = roof_blueprints(&w, Z + 1);
        assert!(!marked.contains(&(7, 7)), "the way up stays open");
        assert!(!marked.contains(&(6, 7)), "and so does the way onto them");
        assert_eq!(marked.len(), 7);
        // Roofed everywhere else, it's still a room (the stairwell counts as covered).
        for (x, y) in marked {
            let a = w.map.idx((x, y, Z + 1));
            w.map.feature[a] = Feature::None;
            w.map.set_roof(a, true);
        }
        w.map.recompute_rooms();
        assert!(w.map.roofed((6, 6, Z)));
    }

    #[test]
    fn planned_stairs_get_no_roof_either() {
        let mut w = test_world(16, 16);
        house(&mut w, 5, 5, 9, 9);
        assert!(roof_blueprints(&w, Z + 1).contains(&(7, 7)));
        assert!(w.apply_order((7, 7, Z), Order::Build(BuildId::named("stairs"))));
        run(&mut w, 1.0);
        assert!(!roof_blueprints(&w, Z + 1).contains(&(7, 7)), "the roof plan goes");
        assert_eq!(roof_blueprints(&w, Z + 1).len(), 7, "over the cell in front too");
        assert!(w.apply_order((7, 7, Z), Order::Cancel));
        run(&mut w, 1.0);
        assert!(roof_blueprints(&w, Z + 1).contains(&(7, 7)), "and comes back without the stairs");
    }

    #[test]
    fn stairs_built_under_a_roof_open_it() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        house(&mut w, 5, 5, 9, 9);
        for y in 6..=8 {
            for x in 6..=8 {
                let a = w.map.idx((x, y, Z + 1));
                w.map.feature[a] = Feature::None;
                w.map.set_roof(a, true);
            }
        }
        w.map.recompute_rooms();
        let i = w.map.idx((7, 7, Z));
        w.map.feature[i] = Feature::blueprint(BuildId::named("stairs"), BuildId::named("stairs").def().total_cost(), crate::map::Dir::PlusX);
        run(&mut w, 60.0);
        assert!(w.map.feature[i].is_special(Special::Stairs), "built");
        assert!(!w.map.roof[w.map.idx((7, 7, Z + 1))], "no roof over the stairs");
        let foot = w.map.stairs_foot((7, 7, Z)).unwrap();
        assert!(!w.map.roof[w.map.idx((foot.0, foot.1, Z + 1))], "nor over the way onto them");
        assert!(roof_blueprints(&w, Z + 1).is_empty(), "and none comes back");
        assert!(w.map.roofed((6, 6, Z)), "still a room");
    }

    #[test]
    fn plants_dont_grow_under_a_roof() {
        let mut w = test_world(16, 16);
        house(&mut w, 5, 5, 9, 9);
        let a = w.map.idx((7, 7, Z + 1));
        w.map.feature[a] = Feature::None;
        w.map.set_roof(a, true);
        let crop = defs().sown().unwrap();
        assert!(!w.apply_order((7, 7, Z), Order::Zone(Zone::Grow(crop))), "no grow zone under a roof");
        assert!(w.apply_order((2, 2, Z), Order::Zone(Zone::Grow(crop))));
        for at in [(7, 7, Z), (2, 2, Z)] {
            let i = w.map.idx(at);
            w.map.feature[i] = Feature::Plant { kind: crop, growth: 0.0 };
        }
        w.set_time(DAY_LEN / 2.0);
        run(&mut w, 10.0);
        let growth = |w: &World, at: Pos| match w.map.feature[w.map.idx(at)] {
            Feature::Plant { growth, .. } => growth,
            _ => -1.0,
        };
        assert_eq!(growth(&w, (7, 7, Z)), 0.0, "under the roof");
        assert!(growth(&w, (2, 2, Z)) > 0.0, "in the open");
    }

    #[test]
    fn colonists_build_the_roof() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        house(&mut w, 5, 5, 9, 9);
        w.map.drop_item((2, 3, Z), Stack { kind: item("wood"), count: 20 });
        run(&mut w, 240.0);
        for y in 6..=8 {
            for x in 6..=8 {
                let i = w.map.idx((x, y, Z + 1));
                assert!(w.map.roof[i], "roof over ({}, {})", x, y);
                assert_eq!(w.map.feature[i], Feature::None);
                assert!(!w.map.floor[i] && !w.map.standable((x, y, Z + 1)), "a roof isn't a floor");
            }
        }
        assert!(w.map.roofed((7, 7, Z)), "now it's a room");
        assert_eq!(w.map.count_items(item("wood")), 20 - 9);
    }

    #[test]
    fn roofs_need_walls_nearby() {
        let mut w = test_world(20, 20);
        house(&mut w, 3, 3, 13, 11);
        let marked = roof_blueprints(&w, Z + 1);
        // The middle of the hall is more than three cells from any wall or
        // the door at (3, 7).
        for x in 7..=9 {
            assert!(!marked.contains(&(x, 7)), "{:?}", marked);
        }
        assert!(marked.contains(&(5, 7)) && marked.contains(&(10, 7)) && marked.contains(&(4, 4)));
        assert_eq!(marked.len(), 9 * 7 - 3);
        assert!(!w.map.roofed((5, 5, Z)), "with holes in its roof it's no room");
        // The player is told where.
        let mut gaps: Vec<Pos> = w.map.roof_gaps.iter().map(|&i| w.map.pos(i)).collect();
        gaps.sort();
        assert_eq!(gaps, (7..=9).map(|x| (x, 7, Z + 1)).collect::<Vec<_>>());
        // A wall in the middle holds it all up.
        let pillar = w.map.idx((8, 6, Z));
        w.map.set_feature(pillar, Feature::built("wall"));
        w.map.recompute_rooms();
        w.mark_roofs();
        assert!(w.map.roof_gaps.is_empty());
    }

    #[test]
    fn opening_a_room_takes_its_roof_blueprints_away() {
        let mut w = test_world(16, 16);
        house(&mut w, 5, 5, 9, 9);
        let corner = w.map.idx((6, 6, Z + 1));
        w.map.feature[corner] = Feature::blueprint(defs().roof().unwrap(), 1, crate::map::Dir::PlusX);
        let gap = w.map.idx((9, 7, Z));
        w.map.set_feature(gap, Feature::None);
        w.map.recompute_rooms();
        w.mark_roofs();
        assert!(roof_blueprints(&w, Z + 1).is_empty());
        assert_eq!(w.map.count_items(item("wood")), 1, "what was delivered is dropped");
        assert!(w.map.items[w.map.idx((6, 6, Z))].is_some(), "into the room below");
    }

    #[test]
    fn players_cant_order_roofs() {
        let mut w = test_world(16, 16);
        house(&mut w, 5, 5, 9, 9);
        let roof = defs().roof().unwrap();
        let i = w.map.idx((7, 7, Z + 1));
        w.map.feature[i] = Feature::None;
        assert!(!w.apply_order((7, 7, Z + 1), Order::Build(roof)));
        assert!(!w.apply_order((12, 12, Z), Order::Build(roof)));
        assert!(!w.apply_order((6, 6, Z + 1), Order::Cancel), "nor cancel them");
    }

    #[test]
    fn floors_replace_roofs() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (7, 7, Z));
        house(&mut w, 5, 5, 9, 9);
        for y in 6..=8 {
            for x in 6..=8 {
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_feature(i, Feature::None);
                w.map.set_roof(i, true);
            }
        }
        let floor = BuildId::named("floor");
        assert!(w.apply_order((7, 7, Z + 1), Order::Build(floor)), "a floor can go over a roof");
        w.map.drop_item((7, 7, Z), Stack { kind: item("wood"), count: 10 });
        run(&mut w, 60.0);
        let i = w.map.idx((7, 7, Z + 1));
        assert!(w.map.floor[i] && !w.map.roof[i], "the floor took the roof's place");
        assert!(w.map.roofed((7, 7, Z)), "still a room");
        assert!(roof_blueprints(&w, Z + 2).is_empty(), "nothing over a half-floored attic");
    }

    #[test]
    fn deconstructed_roofs_come_back() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (7, 7, Z));
        house(&mut w, 5, 5, 9, 9);
        let i = w.map.idx((7, 7, Z + 1));
        w.map.set_feature(i, Feature::None);
        w.map.set_roof(i, true);
        assert!(w.apply_order((7, 7, Z + 1), Order::Desig(Desig::Deconstruct)));
        let pi = w.pawn_index(id).unwrap();
        w.pawns[pi].priorities = crate::pawn::PerWork::filled(crate::pawn::WorkPriority::Never);
        w.pawns[pi].priorities[WorkType::Construction] = crate::pawn::WorkPriority::First;
        run(&mut w, 10.0);
        assert!(!w.map.roof[i], "taken down");
        assert!(w.map.roof_blueprint(i), "and marked again, as the room is still closed");
    }

    #[test]
    fn roofs_survive_saving() {
        let mut w = test_world(16, 16);
        house(&mut w, 5, 5, 9, 9);
        let i = w.map.idx((7, 7, Z + 1));
        w.map.set_feature(i, Feature::None);
        w.map.set_roof(i, true);
        let loaded = World::load_json(&w.save_json().unwrap()).unwrap();
        assert!(loaded.map.roof[i]);
        assert_eq!(loaded.map.roof, w.map.roof);
        assert_eq!(roof_blueprints(&loaded, Z + 1).len(), 8);
    }

    #[test]
    fn deconstruction_is_builders_work() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.map.idx((8, 8, Z));
        w.map.set_feature(i, Feature::built("wall"));
        assert!(!w.apply_order((9, 9, Z), Order::Desig(Desig::Deconstruct)), "nothing to take down");
        assert!(w.apply_order((8, 8, Z), Order::Desig(Desig::Deconstruct)));
        // Not a miner's job.
        let p = w.pawn_index(id).unwrap();
        w.pawns[p].priorities = crate::pawn::PerWork::filled(crate::pawn::WorkPriority::Never);
        w.pawns[p].priorities[WorkType::Mining] = crate::pawn::WorkPriority::First;
        run(&mut w, 20.0);
        assert_eq!(w.map.feature[i], Feature::built("wall"), "miners leave it");
        w.pawns[p].priorities[WorkType::Construction] = crate::pawn::WorkPriority::First;
        run(&mut w, 20.0);
        assert_eq!(w.map.feature[i], Feature::None, "builders take it down");
        assert_eq!(w.map.desig[i], None);
        for (item, count) in BuildId::named("wall").def().drops() {
            assert_eq!(w.map.count_items(item), count);
        }
    }

    #[test]
    fn blueprints_can_be_replaced_directly() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let (wall, door) = (BuildId::named("wall"), BuildId::named("door"));
        let at = (10, 10, Z);
        let i = w.map.idx(at);
        assert!(w.apply_order(at, Order::Build(wall)));
        w.map.feature[i] = Feature::blueprint(wall, 3, crate::map::Dir::PlusX);
        assert!(!w.apply_order(at, Order::Build(wall)), "the same thing again changes nothing");
        assert!(w.apply_order(at, Order::Build(door)), "a door replaces the wall blueprint");
        assert_eq!(w.map.feature[i], Feature::blueprint(door, 0, crate::map::Dir::PlusX));
        assert_eq!(w.map.count_items(wall.def().cost[0].item), 3, "what was delivered is left for the haulers");
        w.map.drop_item((4, 4, Z), Stack { kind: door.def().cost[0].item, count: door.def().total_cost() });
        run(&mut w, 160.0);
        assert_eq!(w.map.feature[i], Feature::built("door"));
    }

    #[test]
    fn corners_are_built_from_diagonally_beside() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        // A corner closed in by walls on two sides and shelves on the others:
        // only the cell diagonally in front of it is free to stand on.
        for (at, id) in [((4, 5, Z), "wall"), ((5, 4, Z), "wall"), ((6, 5, Z), "shelf"), ((5, 6, Z), "shelf")] {
            let i = w.map.idx(at);
            w.map.set_feature(i, Feature::built(id));
        }
        let chair = BuildId::named("chair");
        assert!(w.apply_order((5, 5, Z), Order::Build(chair)));
        w.map.drop_item((2, 4, Z), Stack { kind: chair.def().cost[0].item, count: chair.def().total_cost() });
        run(&mut w, 60.0);
        assert!(matches!(w.map.feature[w.map.idx((5, 5, Z))], Feature::Built { kind, .. } if kind == chair), "built from a diagonal");
    }

    #[test]
    fn nobody_builds_from_straight_above_or_below() {
        let m = &test_world(8, 8).map;
        let at = (3, 3, Z);
        let spots: Vec<Pos> = m.build_around(at, false).collect();
        assert!(spots.contains(&(4, 4, Z)) && spots.contains(&(2, 3, Z + 1)), "diagonals, and a level up or down beside");
        assert!(!spots.contains(&(3, 3, Z + 1)) && !spots.contains(&(3, 3, Z - 1)), "not straight above or below");
        // Floors and roofs are built from underneath.
        assert!(m.build_around((3, 3, Z + 1), true).any(|c| c == (3, 3, Z)));
    }

    #[test]
    fn walls_stack_onto_walls() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let wall = BuildId::named("wall");
        let i = w.map.idx((10, 10, Z));
        w.map.set_feature(i, Feature::built("wall"));
        assert!(!w.apply_order((12, 10, Z + 1), Order::Build(wall)), "not on thin air");
        assert!(!w.apply_order((10, 10, Z + 1), Order::Build(BuildId::named("bed"))), "only walls go on walls");
        assert!(w.apply_order((10, 10, Z + 1), Order::Build(wall)));
        // Built from the ground beside it, one level down.
        w.map.drop_item((4, 4, Z), Stack { kind: item("stone"), count: 4 });
        run(&mut w, 40.0);
        assert_eq!(w.map.feature[w.map.idx((10, 10, Z + 1))], Feature::built("wall"));
        // And again on top of that (building it needs somewhere to stand up there).
        assert!(w.apply_order((10, 10, Z + 2), Order::Build(wall)));
    }

    #[test]
    fn windows_are_built_over_standing_walls() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        house(&mut w, 5, 5, 9, 9);
        for y in 6..=8 {
            for x in 6..=8 {
                let a = w.map.idx((x, y, Z + 1));
                w.map.feature[a] = Feature::None;
                w.map.set_roof(a, true);
            }
        }
        w.map.recompute_rooms();
        assert!(w.map.roofed((7, 7, Z)));
        let wall = (7, 5, Z);
        let i = w.map.idx(wall);
        let window = BuildId::named("window");
        assert!(!w.apply_order(wall, Order::Build(BuildId::named("wall"))), "not a wall over itself");
        assert!(!w.apply_order(wall, Order::Build(BuildId::named("bed"))), "only walls, doors and windows");
        assert!(w.apply_order(wall, Order::Build(window)));
        assert!(!w.apply_order(wall, Order::Build(window)), "already planned");
        // Until it's built the wall stands: still in the way, still closing the room.
        assert!(!w.map.passable(wall) && w.map.feature[i].blocks_room());
        w.map.recompute_rooms();
        assert!(w.map.roofed((7, 7, Z)));
        // Cancelled, the wall is simply left.
        assert!(w.apply_order(wall, Order::Cancel));
        assert_eq!(w.map.feature[i], Feature::built("wall"));
        // Built: the window replaces it, and the wall's stone comes back.
        assert!(w.apply_order(wall, Order::Build(window)));
        for a in &window.def().cost {
            w.map.drop_item((3, 3, Z), Stack { kind: a.item, count: a.count });
        }
        let stone = w.map.count_items(item("stone"));
        run(&mut w, 120.0);
        assert_eq!(w.map.feature[i], Feature::built("window"));
        assert!(w.map.count_items(item("stone")) > stone, "the wall's stone is given back");
        assert!(w.map.roofed((7, 7, Z)), "still a room");
    }

    #[test]
    fn walls_stack_onto_doors_and_planned_walls() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let wall = BuildId::named("wall");
        let door = w.map.idx((10, 10, Z));
        w.map.set_feature(door, Feature::built("door"));
        assert!(w.apply_order((10, 10, Z + 1), Order::Build(wall)), "a wall over a door");
        // A wall three high, planned all at once: it goes up one level at a time.
        assert!(w.apply_order((12, 10, Z), Order::Build(wall)));
        assert!(w.apply_order((12, 10, Z + 1), Order::Build(wall)));
        assert!(!w.apply_order((13, 10, Z + 1), Order::Build(wall)), "not on thin air");
        assert!(w.stacked_on_plan((12, 10, Z + 1)));
        assert!(!w.stacked_on_plan((12, 10, Z)));
        w.map.drop_item((4, 4, Z), Stack { kind: item("stone"), count: 12 });
        run(&mut w, 80.0);
        for at in [(10, 10, Z + 1), (12, 10, Z), (12, 10, Z + 1)] {
            assert_eq!(w.map.feature[w.map.idx(at)], Feature::built("wall"), "{:?}", at);
        }
        // Cancelling a planned wall takes the ones planned on top of it along.
        assert!(w.apply_order((14, 10, Z), Order::Build(wall)));
        assert!(w.apply_order((14, 10, Z + 1), Order::Build(wall)));
        assert!(w.apply_order((14, 10, Z + 2), Order::Build(wall)));
        assert!(w.apply_order((14, 10, Z), Order::Cancel));
        for z in Z..Z + 3 {
            assert_eq!(w.map.feature[w.map.idx((14, 10, z))], Feature::None);
        }
    }

    #[test]
    fn stairs_lead_to_an_upper_floor() {
        let mut w = test_world(16, 4);
        colonist(&mut w, (1, 1, Z));
        let stairs = w.map.idx((4, 1, Z));
        w.map.feature[stairs] = Feature::built("stairs");
        for x in 4..12 {
            let i = w.map.idx((x, 1, Z + 1));
            w.map.floor[i] = true;
        }
        for x in 1..4 {
            let i = w.map.idx((x, 2, Z));
            w.map.zone[i] = Zone::Stockpile(crate::map::StockpileId::new(0));
        }
        let src = w.map.idx((11, 1, Z + 1));
        w.map.items[src] = Some(Stack { kind: item("stone"), count: 10 });
        run(&mut w, 30.0);
        assert!(w.map.items[src].is_none(), "fetched from upstairs");
        assert_eq!(w.map.count_items(item("stone")), 10);
    }

    /// Every buildable in the definitions can be placed, built from its
    /// materials, and deconstructed again for what it drops.
    #[test]
    fn every_buildable_can_be_built_and_removed() {
        for kind in BuildId::all() {
            let def = kind.def();
            let mut w = test_world(16, 16);
            colonist(&mut w, (2, 2, Z));
            if def.special == Special::Roof {
                continue; // rooms get them by themselves (`roofs_*` tests)
            }
            for c in &def.cost {
                w.map.drop_item((3, 3, Z), Stack { kind: c.item, count: c.count });
            }
            if def.on_wall {
                // Hung on a wall, then taken down again.
                let wall = (8, 8, Z);
                let i = w.map.idx(wall);
                w.map.set_feature(i, Feature::built("wall"));
                assert!(w.order_decor(wall, crate::map::Dir::PlusY, 2, kind), "{} can be hung", def.id);
                run(&mut w, 40.0 + 15.0 * def.cost.len() as f32 + 3.0 * def.work_time);
                assert!(w.map.decor(i).iter().all(|d| d.is_up()), "{} put up", def.id);
                for c in &def.cost {
                    assert_eq!(w.map.count_items(c.item), 0, "{} used its {}", def.id, c.item.def().id);
                }
                assert!(w.cancel_decor(wall, crate::map::Dir::PlusY, 2));
                run(&mut w, 30.0 + def.work_time);
                assert!(w.map.decor(i).is_empty(), "{} taken down", def.id);
                for (item, count) in def.drops() {
                    assert_eq!(w.map.count_items(item), count, "{} drops {}", def.id, item.def().id);
                }
                continue;
            }
            let at = if def.special == Special::Floor {
                // Floors go in the air, held up by rock nearby.
                let rock = w.map.idx((8, 8, Z));
                w.map.set_block(rock, Block::Rock);
                (9, 8, Z + 1)
            } else {
                (8, 8, Z)
            };
            assert!(w.apply_order(at, Order::Build(kind)), "{} can be placed", def.id);
            // A trip per material, then the work.
            run(&mut w, 40.0 + 15.0 * def.cost.len() as f32 + 3.0 * def.work_time);
            let i = w.map.idx(at);
            match def.special {
                Special::Floor => assert!(w.map.floor[i], "{} built", def.id),
                _ => assert!(matches!(w.map.feature[i], Feature::Built { kind: k, .. } if k == kind), "{} built", def.id),
            }
            for c in &def.cost {
                assert_eq!(w.map.count_items(c.item), 0, "{} used its {}", def.id, c.item.def().id);
            }

            assert!(!w.apply_order(at, Order::Desig(Desig::Mine)), "{} isn't mined", def.id);
            assert!(w.apply_order(at, Order::Desig(Desig::Deconstruct)), "{} can be removed", def.id);
            run(&mut w, 30.0 + def.work_time);
            assert_eq!(w.map.feature[i], Feature::None, "{} removed", def.id);
            assert!(!w.map.floor[i], "{} removed", def.id);
            for (item, count) in def.drops() {
                assert_eq!(w.map.count_items(item), count, "{} drops {}", def.id, item.def().id);
            }
        }
    }

    /// Every plant can be harvested (designated or automatically) for its yield.
    #[test]
    fn every_plant_can_be_harvested() {
        for kind in PlantId::all() {
            let def = kind.def();
            let mut w = test_world(16, 16);
            colonist(&mut w, (2, 2, Z));
            let i = w.map.idx((8, 8, Z));
            w.map.set_feature(i, Feature::Plant { kind, growth: 1.0 });
            if let Some(d) = def.harvest.desig {
                assert!(w.apply_order((8, 8, Z), Order::Desig(d)), "{} can be designated", def.id);
            }
            run(&mut w, 30.0);
            assert_eq!(w.map.feature[i], Feature::None, "{} harvested", def.id);
            assert_eq!(w.map.count_items(def.harvest.yields.item), def.harvest.yields.count, "{} yields", def.id);
        }
    }

    #[test]
    fn crops_ripen_and_get_harvested() {
        let mut w = test_world(12, 12);
        colonist(&mut w, (2, 2, Z));
        w.apply_order((6, 6, Z), Order::Zone(Zone::Grow(defs().sown().unwrap())));
        let yields = &defs().sown().unwrap().def().harvest.yields;
        // Crops only grow in daylight, so this takes a few days.
        let harvested = (0..(6.0 * DAY_LEN) as usize).any(|_| {
            run(&mut w, 1.0);
            w.pawns.iter_mut().for_each(|p| p.hunger = 100.0);
            w.map.count_items(yields.item) >= yields.count
        });
        assert!(harvested, "a harvest came in");
    }

    #[test]
    fn things_that_happen_make_sounds() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let t = w.map.idx((8, 8, Z));
        w.map.feature[t] = Feature::plant("tree");
        w.apply_order((8, 8, Z), Order::Desig(Desig::Chop));
        let mut heard = Vec::new();
        for _ in 0..(20.0 / TICK) as usize {
            w.tick(TICK);
            heard.extend(w.take_sounds().into_iter().map(|e| e.name));
        }
        assert!(heard.contains(&"tree_fall"), "{:?}", heard);
        assert!(w.take_sounds().is_empty(), "taken sounds are gone");

        // Shots, and hits or glancing blows, in a battle the AI plays for both sides.
        let mut w = test_world(30, 30);
        colonist(&mut w, (5, 15, Z));
        w.start_battle(crate::battle::Parley { squads: vec![1], entry: (12, 15, Z), demand: Vec::new(), tier: 0 });
        w.battle.as_mut().unwrap().auto_colony = true;
        let mut heard = Vec::new();
        for _ in 0..(10.0 / TICK) as usize {
            w.tick(TICK);
            heard.extend(w.take_sounds().into_iter().map(|e| e.name));
        }
        assert!(heard.contains(&"rifle"), "{:?}", heard);
        assert!(heard.iter().any(|n| *n == "hit" || *n == "glance"), "{:?}", heard);
    }

    #[test]
    fn unheard_sounds_dont_pile_up() {
        let mut w = test_world(8, 8);
        for _ in 0..1000 {
            w.sound("thud", None);
        }
        assert_eq!(w.sounds.len(), MAX_SOUNDS);
    }

    #[test]
    fn the_first_colonists_land_with_different_weapons() {
        let w = World::new(7);
        let mut weapons: Vec<&str> = w.pawns.iter().filter(|p| p.is_colonist()).filter_map(|p| p.weapon).map(|k| k.def().id.as_str()).collect();
        weapons.sort();
        assert_eq!(weapons, vec!["knife", "pistol", "rifle"]);
    }

    #[test]
    fn save_and_load_round_trip() {
        let w = World::new(42);
        let json = w.save_json().unwrap();
        let loaded = World::load_json(&json).unwrap();
        assert_eq!(loaded.pawns.len(), w.pawns.len());
        assert_eq!(loaded.time(), w.time());
        assert_eq!(loaded.map.items, w.map.items);
        assert_eq!(loaded.map.room.len(), w.map.len());
    }
}
