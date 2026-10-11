//! The whole simulation state and its fixed-step update.

use crate::health::TickCtx;
use crate::map::{Block, BuildKind, Desig, Feature, ItemKind, Map, Pos, Stack, Zone};
use crate::pawn::{Faction, Job, Memory, Pawn, WorkType};
use crate::pathing::bfs;
use crate::rng::Rng;
use crate::story::Storyteller;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

pub const MAP_W: usize = 100;
pub const MAP_H: usize = 70;
pub const MAP_D: usize = 16;
/// Daylight seconds for a crop to ripen.
const CROP_GROW_TIME: f32 = 180.0;

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

/// Something the player paints onto tiles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    Desig(Desig),
    Zone(Zone),
    Build(BuildKind),
    Cancel,
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
    /// Per-tile reservations by pawn id (0 = free): work targets, item stacks
    /// being picked up, and stockpile cells being delivered to.
    #[serde(with = "crate::save::rle")]
    pub res_target: Vec<u32>,
    #[serde(with = "crate::save::rle")]
    pub res_item: Vec<u32>,
    #[serde(with = "crate::save::rle")]
    pub res_cell: Vec<u32>,
    /// (patient, doctor) pairs, so two doctors don't treat the same patient.
    #[serde(default)]
    pub res_patient: Vec<(u32, u32)>,
    pub time: f32,
    pub rng: Rng,
    pub next_id: u32,
    pub log: Vec<String>,
    pub story: Storyteller,
    pub effects: Vec<Effect>,
    pub colonist_names_used: usize,
    pub regrow_timer: f32,
}

impl World {
    pub fn new(seed: u64) -> World {
        let mut rng = Rng::new(seed);
        let map = Map::generate(MAP_W, MAP_H, MAP_D, &mut rng);
        let mut w = World::with_map(map, rng);
        let (cx, cy) = (MAP_W / 2, MAP_H / 2);
        // The landing zone is flat, so one level works for all of it.
        let z = w.map.surface(cx, cy).unwrap_or(1);
        for k in 0..3 {
            w.spawn_colonist((cx - 1 + k, cy, z));
        }
        for y in cy + 2..cy + 5 {
            for x in cx - 4..cx + 2 {
                let i = w.map.idx((x, y, z));
                w.map.zone[i] = Zone::Stockpile;
            }
        }
        w.map.drop_item((cx - 4, cy + 2, z), Stack { kind: ItemKind::Wood, count: 80 });
        w.map.drop_item((cx + 1, cy + 4, z), Stack { kind: ItemKind::Food, count: 40 });
        w.map.drop_item((cx - 1, cy + 4, z), Stack { kind: ItemKind::Medicine, count: 8 });
        w.push_log("Three colonists crash-landed. Press H for help.".into());
        w
    }

    /// An empty world around an existing map (also used by tests).
    pub fn with_map(map: Map, rng: Rng) -> World {
        let n = map.len();
        World {
            map,
            pawns: Vec::new(),
            res_target: vec![0; n],
            res_item: vec![0; n],
            res_cell: vec![0; n],
            res_patient: Vec::new(),
            time: DAY_LEN * 0.25,
            rng,
            next_id: 1,
            log: Vec::new(),
            story: Storyteller::new(),
            effects: Vec::new(),
            colonist_names_used: 0,
            regrow_timer: 4.0,
        }
    }

    // ---------- queries ----------

    pub fn day(&self) -> u32 {
        (self.time / DAY_LEN) as u32 + 1
    }

    pub fn hour(&self) -> f32 {
        (self.time % DAY_LEN) / DAY_LEN * 24.0
    }

    pub fn is_night(&self) -> bool {
        let h = self.hour();
        !(6.0..21.0).contains(&h)
    }

    /// 0 at midnight, 1 at noon.
    pub fn daylight(&self) -> f32 {
        let t = (self.time % DAY_LEN) / DAY_LEN;
        0.5 - 0.5 * (t * std::f32::consts::TAU).cos()
    }

    pub fn pawn_index(&self, id: u32) -> Option<usize> {
        self.pawns.iter().position(|p| p.id == id)
    }

    pub fn pawn(&self, id: u32) -> Option<&Pawn> {
        self.pawns.iter().find(|p| p.id == id)
    }

    pub fn colonists_alive(&self) -> usize {
        self.pawns.iter().filter(|p| p.is_colonist() && !p.dead).count()
    }

    pub fn colony_lost(&self) -> bool {
        self.colonists_alive() == 0
    }

    /// Warnings for the player, `true` = urgent.
    pub fn alerts(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        let raiders = self.pawns.iter().filter(|p| p.faction == Faction::Raider && p.active()).count();
        if raiders > 0 {
            out.push((format!("Raid: {} raiders", raiders), true));
        }
        let colonists: Vec<&Pawn> = self.pawns.iter().filter(|p| p.is_colonist() && !p.dead).collect();
        let mut needs_tending = false;
        for p in &colonists {
            let h = &p.health;
            if let Some(secs) = h.time_to_bleed_out() {
                let hours = secs / DAY_LEN * 24.0;
                out.push((format!("{} bleeding out (~{:.0}h)", p.name, hours.max(1.0)), true));
            } else if p.downed && !self.in_bed(p) {
                out.push((format!("{} needs rescue", p.name), true));
            } else if p.downed {
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
        let medicine = self.map.count_items(ItemKind::Medicine);
        if needs_tending && medicine == 0 {
            out.push(("No medicine".into(), true));
        } else if medicine < 3 {
            out.push((format!("Low medicine ({} left)", medicine), false));
        }
        if !colonists.iter().any(|p| p.active() && p.priorities[WorkType::Doctor as usize] > 0) {
            out.push(("No doctor available".into(), needs_tending));
        }
        let food = self.map.count_items(ItemKind::Food);
        if (food as usize) < colonists.len() * 4 {
            out.push((format!("Low food ({} left)", food), false));
        }
        let beds = self.map.feature.iter().filter(|f| **f == Feature::Bed).count();
        if beds < colonists.len() {
            out.push((format!("Need beds ({}/{})", beds, colonists.len()), false));
        }
        if !self.map.zone.iter().any(|z| *z == Zone::Stockpile) {
            out.push(("No stockpile zone".into(), false));
        }
        let idle = colonists
            .iter()
            .filter(|p| p.active() && !p.drafted && matches!(p.job, None | Some(Job::Wander { .. })))
            .count();
        if idle > 0 {
            out.push((format!("{} idle colonist(s)", idle), false));
        }
        out
    }

    pub fn push_log(&mut self, msg: String) {
        self.log.push(msg);
        if self.log.len() > 50 {
            self.log.remove(0);
        }
    }

    // ---------- spawning ----------

    pub fn spawn_colonist(&mut self, pos: Pos) -> u32 {
        let name = COLONIST_NAMES
            .get(self.colonist_names_used)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Colonist {}", self.colonist_names_used + 1));
        self.colonist_names_used += 1;
        let id = self.next_id;
        self.next_id += 1;
        let p = Pawn::new_colonist(id, name, pos, &mut self.rng);
        self.pawns.push(p);
        id
    }

    pub fn spawn_raider(&mut self, name: String, pos: Pos) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let p = Pawn::new_raider(id, name, pos, &mut self.rng);
        self.pawns.push(p);
        id
    }

    // ---------- simulation ----------

    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        if self.map.rooms_dirty {
            self.map.recompute_rooms();
        }
        if self.map.support_dirty {
            self.settle_unsupported();
        }
        self.grow_plants(dt);
        self.storyteller_tick();
        let fade = dt / (1.5 * DAY_LEN);
        for b in self.map.blood.iter_mut() {
            *b = (*b - fade).max(0.0);
        }

        // Each pawn is updated on a copy so it can freely read and modify the
        // rest of the world (including other pawns) while it thinks.
        for i in 0..self.pawns.len() {
            let mut p = self.pawns[i].clone();
            self.update_pawn(&mut p, dt);
            self.pawns[i] = p;
        }
        self.pawns.retain(|p| p.is_colonist() || !(p.dead || p.left));

        for e in self.effects.iter_mut() {
            e.ttl -= dt;
        }
        self.effects.retain(|e| e.ttl > 0.0);
    }

    fn grow_plants(&mut self, dt: f32) {
        if !self.is_night() {
            let g = dt / CROP_GROW_TIME;
            for f in self.map.feature.iter_mut() {
                if let Feature::Crop { growth } = f {
                    *growth = (*growth + g).min(1.0);
                }
            }
        }
        self.regrow_timer -= dt;
        if self.regrow_timer <= 0.0 {
            self.regrow_timer = 4.0;
            let (x, y) = (self.rng.below(self.map.w), self.rng.below(self.map.h));
            let m = &self.map;
            let Some(z) = m.surface(x, y) else { return };
            let i = m.idx((x, y, z));
            let free = m.feature[i] == Feature::None
                && m.block[m.idx((x, y, z - 1))] == Block::Soil
                && m.zone[i] == Zone::None
                && m.items[i].is_none()
                && !m.covered((x, y, z))
                && !self.pawns.iter().any(|p| m.idx(p.pos) == i);
            if free {
                let f = if self.rng.chance(0.6) { Feature::Tree } else { Feature::Bush };
                self.map.set_feature(i, f);
            }
        }
    }

    /// Items drop to the next support; plants that lost their ground die.
    fn settle_unsupported(&mut self) {
        self.map.support_dirty = false;
        for i in 0..self.map.len() {
            let p = self.map.pos(i);
            if p.2 == 0 || self.map.block[i] != Block::Air || self.map.supported(p) {
                continue;
            }
            if matches!(self.map.feature[i], Feature::Tree | Feature::Bush | Feature::Crop { .. }) {
                self.map.set_feature(i, Feature::None);
            }
            if let Some(s) = self.map.items[i].take() {
                let land = self.map.landing(p);
                self.map.drop_item(land, s);
            }
        }
    }

    /// A pawn left standing on nothing drops to the next support. Falls of
    /// two levels or more hurt.
    fn fall_if_unsupported(&mut self, p: &mut Pawn) {
        let i = self.map.idx(p.pos);
        if p.pos.2 == 0 || self.map.block[i] != Block::Air || self.map.supported(p.pos) {
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
        if levels >= 2 {
            for _ in 0..levels - 1 {
                p.health.apply_damage(7.0, crate::health::DamageKind::Bruise, &mut self.rng);
            }
            p.health.recompute();
            self.push_log(format!("{} fell {} levels.", p.name, levels));
        }
    }

    fn update_pawn(&mut self, p: &mut Pawn, dt: f32) {
        if p.dead || p.left {
            return;
        }
        self.fall_if_unsupported(p);
        p.cooldown = (p.cooldown - dt).max(0.0);
        self.update_health(p, dt);
        if p.dead {
            return;
        }
        if p.is_colonist() {
            self.update_needs(p, dt);
        }
        if p.downed {
            p.downed_time += dt;
            // Nobody treats raiders; left lying long enough, they die.
            if p.faction == Faction::Raider && p.downed_time > 120.0 {
                self.kill(p, "succumbed to their wounds");
            }
            return;
        }
        match p.faction {
            Faction::Colony => self.colonist_think(p, dt),
            Faction::Raider => self.raider_think(p, dt),
        }
        self.run_job(p, dt);
    }

    fn update_health(&mut self, p: &mut Pawn, dt: f32) {
        let lying = p.lying();
        let ctx = TickCtx { in_bed: lying && self.in_bed(p), resting: lying, fed: !p.is_colonist() || p.hunger > 0.0 };
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
        match (p.downed, p.health.downed_reason()) {
            (false, Some(reason)) => {
                self.end_job(p);
                p.downed = true;
                p.drafted = false;
                p.downed_time = 0.0;
                self.push_log(format!("{} collapsed ({}).", p.name, reason));
            }
            (true, None) => {
                p.downed = false;
                self.push_log(format!("{} got back up.", p.name));
            }
            _ => {}
        }
    }

    fn update_needs(&mut self, p: &mut Pawn, dt: f32) {
        p.hunger = (p.hunger - 0.4 * dt).max(0.0);
        if !p.sleeping {
            p.rest = (p.rest - 0.33 * dt).max(0.0);
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
        } else if p.mood < 20.0 && !p.drafted && !p.downed && self.rng.chance(dt * 0.01) {
            self.end_job(p);
            p.break_timer = 40.0;
            self.push_log(format!("{} is having a mental break and wanders off in a daze.", p.name));
        }
    }

    fn colonist_think(&mut self, p: &mut Pawn, dt: f32) {
        if p.drafted {
            if p.job.is_none() {
                self.auto_fire(p);
            }
            return;
        }
        if p.break_timer > 0.0 {
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

        // Urgent needs interrupt whatever they're doing.
        let on_need = matches!(p.job, Some(Job::Eat { .. }) | Some(Job::Sleep { .. }));
        if !on_need {
            if p.hunger < 20.0 && self.try_eat(p) {
                return;
            }
            if p.rest < 8.0 && !matches!(p.job, Some(Job::Patient { .. })) {
                self.end_job(p);
                let bed = self.find_bed(p);
                self.start_job(p, Job::Sleep { bed });
                return;
            }
        }
        let tending_self = matches!(p.job, Some(Job::Tend { patient, .. }) if patient == p.id);
        let resting = matches!(p.job, Some(Job::Patient { .. }));
        if !on_need && !tending_self && p.health.wants_bed_rest() {
            // With no one else to treat them, doctors patch themselves up first.
            if p.health.needs_tending()
                && p.priorities[WorkType::Doctor as usize] > 0
                && !self.other_doctor_available(p.id)
            {
                let job = self.self_tend_job(p);
                self.end_job(p);
                self.start_job(p, job);
                return;
            }
            if !resting {
                self.end_job(p);
                let bed = self.find_bed(p);
                self.start_job(p, Job::Patient { bed });
                return;
            }
        }
        if p.job.is_some() {
            return;
        }
        if p.hunger < 40.0 && self.try_eat(p) {
            return;
        }
        if p.rest < 30.0 || (self.is_night() && p.rest < 75.0) {
            let bed = self.find_bed(p);
            self.start_job(p, Job::Sleep { bed });
            return;
        }
        if let Some(job) = self.find_work(p) {
            self.start_job(p, job);
            return;
        }
        if self.rng.chance(0.25) {
            self.start_wander(p, 5);
        }
    }

    // ---------- jobs, reservations, movement ----------

    pub(crate) fn release(&mut self, id: u32) {
        for v in [&mut self.res_target, &mut self.res_item, &mut self.res_cell] {
            for r in v.iter_mut() {
                if *r == id {
                    *r = 0;
                }
            }
        }
        self.res_patient.retain(|&(_, doctor)| doctor != id);
    }

    pub(crate) fn start_job(&mut self, p: &mut Pawn, job: Job) {
        let id = p.id;
        match job {
            Job::Work { target, .. } => {
                let i = self.map.idx(target);
                self.res_target[i] = id;
            }
            Job::Haul { from, to, to_blueprint, .. } => {
                let (fi, ti) = (self.map.idx(from), self.map.idx(to));
                self.res_item[fi] = id;
                if to_blueprint {
                    self.res_target[ti] = id;
                } else {
                    self.res_cell[ti] = id;
                }
            }
            Job::Eat { from, .. } => {
                let i = self.map.idx(from);
                self.res_item[i] = id;
            }
            Job::Sleep { bed: Some(b) } | Job::Patient { bed: Some(b) } => p.bed = Some(b),
            Job::Tend { patient, medicine, .. } => {
                if let Some(m) = medicine {
                    let i = self.map.idx(m);
                    self.res_item[i] = id;
                }
                if patient != id {
                    self.res_patient.push((patient, id));
                }
            }
            Job::Rescue { patient, bed, .. } => {
                self.res_patient.push((patient, id));
                if let Some(pi) = self.pawn_index(patient) {
                    self.pawns[pi].bed = Some(bed);
                }
            }
            Job::Feed { patient, food, .. } => {
                let i = self.map.idx(food);
                self.res_item[i] = id;
                self.res_patient.push((patient, id));
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
        if p.path_goal != Some(goal) {
            p.path.clear();
            p.path_goal = Some(goal);
            let at_goal = p.pos == goal || (adjacent && crate::map::adjacent(p.pos, goal));
            if !at_goal {
                let df = bfs(&self.map, p.pos);
                match df.approach(&self.map, goal, adjacent) {
                    Some((stand, _)) => p.path = df.path_to(stand),
                    None => {
                        p.path_goal = None;
                        return Step::Unreachable;
                    }
                }
            }
        }
        if let Some(&next) = p.path.last() {
            if !self.map.passable(next) {
                // Something was built or grew in the way; plan again next tick.
                p.path.clear();
                p.path_goal = None;
                return Step::Moving;
            }
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
            Order::Desig(Desig::Chop) => f == Feature::Tree,
            Order::Desig(Desig::Mine) => m.mineable(p),
            Order::Desig(Desig::Forage) => f == Feature::Bush,
            Order::Zone(Zone::None) => m.zone[i] != Zone::None,
            Order::Zone(_) => m.standable(p) && matches!(f, Feature::None | Feature::Crop { .. }),
            Order::Build(BuildKind::Floor) => m.floor_buildable(p),
            Order::Build(BuildKind::Stairs) => {
                m.standable(p) && f == Feature::None && m.above(p).is_some_and(|a| m.block[m.idx(a)] == Block::Air)
            }
            Order::Build(_) => m.standable(p) && f == Feature::None,
            Order::Cancel => m.desig[i].is_some() || matches!(f, Feature::Blueprint { .. }),
        }
    }

    /// Apply a player order to one cell. Returns false if it doesn't apply there.
    pub fn apply_order(&mut self, p: Pos, order: Order) -> bool {
        if !self.order_applies(p, order) {
            return false;
        }
        let i = self.map.idx(p);
        match order {
            Order::Desig(d) => self.map.desig[i] = Some(d),
            Order::Zone(z) => self.map.zone[i] = z,
            Order::Build(kind) => self.map.feature[i] = Feature::Blueprint { kind, delivered: 0 },
            Order::Cancel => {
                self.map.desig[i] = None;
                if let Feature::Blueprint { kind, delivered } = self.map.feature[i] {
                    self.map.feature[i] = Feature::None;
                    if delivered > 0 {
                        self.map.drop_item(p, Stack { kind: kind.cost().kind, count: delivered });
                    }
                }
            }
        }
        true
    }

    /// Run `f` on a copy of pawn `id` with full access to the world.
    fn with_pawn(&mut self, id: u32, f: impl FnOnce(&mut World, &mut Pawn)) {
        if let Some(i) = self.pawn_index(id) {
            let mut p = self.pawns[i].clone();
            f(self, &mut p);
            self.pawns[i] = p;
        }
    }

    pub fn set_drafted(&mut self, id: u32, drafted: bool) {
        self.with_pawn(id, |w, p| {
            if p.is_colonist() && p.active() && p.drafted != drafted {
                w.end_job(p);
                p.drafted = drafted;
                p.break_timer = 0.0;
            }
        });
    }

    pub fn order_move(&mut self, id: u32, to: Pos) {
        self.with_pawn(id, |w, p| {
            if p.drafted && p.active() && w.map.passable(to) {
                w.end_job(p);
                w.start_job(p, Job::Goto { to });
            }
        });
    }

    pub fn order_attack(&mut self, id: u32, target: u32) {
        self.with_pawn(id, |w, p| {
            if p.drafted && p.active() {
                w.end_job(p);
                w.start_job(p, Job::Attack { target });
            }
        });
    }

    pub fn set_priority(&mut self, id: u32, wt: WorkType, prio: u8) {
        if let Some(i) = self.pawn_index(id) {
            self.pawns[i].priorities[wt as usize] = prio.min(4);
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
        w.map.recompute_rooms();
        let n = w.map.len();
        if w.map.blood.len() != n {
            w.map.blood = vec![0.0; n];
        }
        for p in w.pawns.iter_mut() {
            p.health.recompute();
        }
        Ok(w)
    }
}

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
    use crate::map::Feature;
    use crate::pawn::Weapon;
    use crate::TICK;

    const Z: usize = crate::map::FLAT_Z;

    fn test_world(w: usize, h: usize) -> World {
        let mut world = World::with_map(Map::flat(w, h), Rng::new(7));
        world.story.next_raid = f32::MAX;
        world.story.next_event = f32::MAX;
        world.regrow_timer = f32::MAX;
        world
    }

    fn colonist(world: &mut World, pos: Pos) -> u32 {
        let id = world.spawn_colonist(pos);
        let i = world.pawn_index(id).unwrap();
        world.pawns[i].hunger = 100.0;
        world.pawns[i].rest = 100.0;
        id
    }

    fn run(world: &mut World, secs: f32) {
        for _ in 0..(secs / TICK) as usize {
            world.tick(TICK);
        }
    }

    #[test]
    fn hauling_moves_items_into_the_stockpile() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        for x in 2..5 {
            let i = w.map.idx((x, 10, Z));
            w.map.zone[i] = Zone::Stockpile;
        }
        let src = w.map.idx((12, 12, Z));
        w.map.items[src] = Some(Stack { kind: ItemKind::Wood, count: 20 });
        run(&mut w, 30.0);
        assert!(w.map.items[src].is_none(), "wood should have been picked up");
        let stored: u32 = (2..5).filter_map(|x| w.map.items[w.map.idx((x, 10, Z))]).map(|s| s.count).sum();
        assert_eq!(stored, 20);
    }

    #[test]
    fn construction_delivers_materials_and_builds() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let stone = w.map.idx((3, 8, Z));
        w.map.items[stone] = Some(Stack { kind: ItemKind::Stone, count: 4 });
        w.apply_order((10, 10, Z), Order::Build(BuildKind::Wall));
        run(&mut w, 40.0);
        assert_eq!(w.map.feature[w.map.idx((10, 10, Z))], Feature::Wall);
        assert_eq!(w.map.count_items(ItemKind::Stone), 0);
    }

    #[test]
    fn chopping_drops_wood() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let t = w.map.idx((8, 8, Z));
        w.map.feature[t] = Feature::Tree;
        w.apply_order((8, 8, Z), Order::Desig(Desig::Chop));
        run(&mut w, 20.0);
        assert_eq!(w.map.feature[t], Feature::None);
        assert!(w.map.count_items(ItemKind::Wood) > 0);
    }

    #[test]
    fn hungry_colonists_eat_from_the_ground() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        let f = w.map.idx((6, 6, Z));
        w.map.items[f] = Some(Stack { kind: ItemKind::Food, count: 3 });
        run(&mut w, 10.0);
        assert!(w.pawn(id).unwrap().hunger > 40.0);
        assert_eq!(w.map.count_items(ItemKind::Food), 2);
    }

    #[test]
    fn grow_zone_gets_planted() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let i = w.map.idx((6, 6, Z));
        w.apply_order((6, 6, Z), Order::Zone(Zone::Grow));
        run(&mut w, 10.0);
        assert!(matches!(w.map.feature[i], Feature::Crop { .. }));
    }

    #[test]
    fn work_priorities_decide_what_comes_first() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        w.set_priority(id, WorkType::PlantCutting, 4);
        w.set_priority(id, WorkType::Mining, 1);
        // The tree is closer, but mining has the higher priority.
        let tree = w.map.idx((3, 2, Z));
        w.map.feature[tree] = Feature::Tree;
        w.apply_order((3, 2, Z), Order::Desig(Desig::Chop));
        let rock = w.map.idx((15, 15, Z));
        w.map.block[rock] = Block::Rock;
        w.apply_order((15, 15, Z), Order::Desig(Desig::Mine));
        run(&mut w, 1.5);
        let job = w.pawn(id).unwrap().job;
        assert!(matches!(job, Some(Job::Work { task: crate::pawn::Task::Mine, .. })), "{:?}", job);
    }

    #[test]
    fn drafted_colonist_shoots_an_approaching_raider() {
        let mut w = test_world(30, 30);
        let id = colonist(&mut w, (5, 15, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].skills[crate::pawn::Skill::Shooting as usize] = 20;
        w.set_drafted(id, true);
        let rid = w.spawn_raider("Grim".into(), (14, 15, Z));
        let ri = w.pawn_index(rid).unwrap();
        w.pawns[ri].weapon = Weapon::Club;
        run(&mut w, 10.0);
        let hurt_or_gone = w.pawn(rid).map_or(true, |r| !r.health.hediffs.is_empty() || r.dead);
        assert!(hurt_or_gone);
    }

    fn wound(w: &mut World, id: u32, part: crate::health::Part, amount: f32, kind: crate::health::DamageKind) {
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
        w.set_priority(patient, WorkType::Doctor, 0);
        let med = w.map.idx((5, 5, Z));
        w.map.items[med] = Some(Stack { kind: ItemKind::Medicine, count: 2 });
        wound(&mut w, patient, Part::LeftLeg, 10.0, DamageKind::Gunshot);
        run(&mut w, 25.0);
        let p = w.pawn(patient).unwrap();
        assert!(!p.health.needs_tending(), "patient should have been tended");
        assert_eq!(p.health.bleed_rate(), 0.0);
        assert_eq!(w.map.count_items(ItemKind::Medicine), 1);
        assert!(w.pawn(doctor).unwrap().job.map_or(true, |j| !matches!(j, Job::Tend { .. })));
    }

    #[test]
    fn downed_colonists_are_carried_to_bed() {
        use crate::health::{DamageKind, Part};
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        let patient = colonist(&mut w, (12, 12, Z));
        w.set_priority(patient, WorkType::Doctor, 0);
        let bed = w.map.idx((4, 15, Z));
        w.map.feature[bed] = Feature::Bed;
        // Both legs gone: conscious, but unable to walk.
        wound(&mut w, patient, Part::LeftLeg, 100.0, DamageKind::Bruise);
        wound(&mut w, patient, Part::RightLeg, 100.0, DamageKind::Bruise);
        run(&mut w, 30.0);
        let p = w.pawn(patient).unwrap();
        assert!(p.downed);
        assert_eq!(p.pos, (4, 15, Z), "patient should be in the bed");
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
            w.map.zone[i] = Zone::Stockpile;
        }
        let src = w.map.idx((15, 15, Z + 1));
        w.map.items[src] = Some(Stack { kind: ItemKind::Wood, count: 20 });
        run(&mut w, 40.0);
        assert!(w.map.items[src].is_none(), "wood on the hill should have been fetched");
        assert_eq!(w.map.count_items(ItemKind::Wood), 20);
    }

    #[test]
    fn cliffs_keep_colonists_out() {
        let mut w = test_world(20, 20);
        raise(&mut w, 10, 0, 19, 19, 2);
        colonist(&mut w, (2, 2, Z));
        for x in 2..5 {
            let i = w.map.idx((x, 10, Z));
            w.map.zone[i] = Zone::Stockpile;
        }
        let src = w.map.idx((15, 15, Z + 2));
        w.map.items[src] = Some(Stack { kind: ItemKind::Wood, count: 20 });
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
        assert!(w.map.count_items(ItemKind::Stone) >= 18);
    }

    #[test]
    fn a_generated_colony_runs_for_half_a_day() {
        let mut w = World::new(5);
        for p in &w.pawns {
            assert!(w.map.standable(p.pos), "{} starts on solid ground", p.name);
        }
        run(&mut w, DAY_LEN / 2.0);
        assert_eq!(w.colonists_alive(), 3);
        for p in w.pawns.iter().filter(|p| !p.dead) {
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
        w.set_priority(bystander, WorkType::Mining, 0);
        // Keep the bystander standing still on the spot being dug out.
        w.set_drafted(bystander, true);
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
        w.set_drafted(id, true);
        let i = w.map.idx((4, 4, 5));
        w.map.set_floor(i, false);
        run(&mut w, 0.5);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.pos, (4, 4, 1));
        assert!(!p.health.hediffs.is_empty(), "a four-level fall should bruise");
    }

    #[test]
    fn colonists_roof_a_room_from_inside() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (7, 7, Z));
        for y in 5..=9 {
            for x in 5..=9 {
                if x == 5 || x == 9 || y == 5 || y == 9 {
                    let i = w.map.idx((x, y, Z));
                    w.map.set_feature(i, if (x, y) == (5, 7) { Feature::Door } else { Feature::Wall });
                }
            }
        }
        w.map.recompute_rooms();
        assert!(!w.map.roofed((7, 7, Z)));
        let mut placed = 0;
        for y in 5..=9 {
            for x in 5..=9 {
                placed += w.apply_order((x, y, Z + 1), Order::Build(BuildKind::Floor)) as usize;
            }
        }
        assert_eq!(placed, 25, "every cell over the room has wall support nearby");
        w.map.drop_item((7, 7, Z), Stack { kind: ItemKind::Wood, count: 50 });
        run(&mut w, 120.0);
        let built = (5..=9).flat_map(|y| (5..=9).map(move |x| (x, y))).filter(|&(x, y)| w.map.floor[w.map.idx((x, y, Z + 1))]).count();
        assert_eq!(built, 25, "all floors built");
        assert!(w.map.roofed((7, 7, Z)), "now it's a room");
    }

    #[test]
    fn stairs_lead_to_an_upper_floor() {
        let mut w = test_world(16, 4);
        colonist(&mut w, (1, 1, Z));
        let stairs = w.map.idx((4, 1, Z));
        w.map.feature[stairs] = Feature::Stairs { dir: 0 };
        for x in 4..12 {
            let i = w.map.idx((x, 1, Z + 1));
            w.map.floor[i] = true;
        }
        for x in 1..4 {
            let i = w.map.idx((x, 2, Z));
            w.map.zone[i] = Zone::Stockpile;
        }
        let src = w.map.idx((11, 1, Z + 1));
        w.map.items[src] = Some(Stack { kind: ItemKind::Stone, count: 10 });
        run(&mut w, 30.0);
        assert!(w.map.items[src].is_none(), "fetched from upstairs");
        assert_eq!(w.map.count_items(ItemKind::Stone), 10);
    }

    #[test]
    fn save_and_load_round_trip() {
        let w = World::new(42);
        let json = w.save_json().unwrap();
        let loaded = World::load_json(&json).unwrap();
        assert_eq!(loaded.pawns.len(), w.pawns.len());
        assert_eq!(loaded.time, w.time);
        assert_eq!(loaded.map.items, w.map.items);
        assert_eq!(loaded.map.room.len(), w.map.len());
    }
}
