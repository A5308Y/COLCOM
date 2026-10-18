//! Finding work for colonists and carrying jobs out step by step.

use crate::defs::{defs, Defs, ItemId, PlantId, Special};
use crate::map::{Block, Desig, Feature, Map, Pos, Stack, Zone, STACK_MAX};
use crate::moving::MOVE_SHARE;
use crate::pathing::{bfs, DistField, ANY_DISTANCE};
use crate::pawn::{Job, Memory, Pawn, Skill, Task, WorkType};
use crate::world::{Step, World, SLEEP_GAIN};

/// Base seconds to mine out one cell.
const MINE_TIME: f32 = 5.0;
/// Haulers pick up more of the same item lying within this many steps, and
/// deliver to other blueprints within this many steps on the same trip.
const NEARBY_STEPS: i32 = 8;
/// Base seconds to fill one cell with soil.
const FILL_TIME: f32 = 3.0;
/// Deconstructing takes this share of the building's work time.
const DECONSTRUCT_SHARE: f32 = 0.5;

/// How walking to an item to pick it up went (see `World::fetch`).
pub(crate) enum Fetch {
    Walking,
    Got,
    Unreachable,
    /// There, but nothing (fitting) left to take.
    Gone,
}

/// Nearest (distance, cell) per item kind.
pub(crate) type Nearest = Vec<Option<(i32, Pos)>>;
/// Per item kind, the nearest stockpile cell that takes it, per stockpile priority (1..=4).
pub(crate) type NearestStore = Vec<[Option<(i32, Pos)>; 5]>;

/// Cells worth checking for work: everywhere the pawn can walk, plus the
/// cells next to those (trees, rock faces and walls are worked from beside).
fn reachable_and_around(m: &Map, df: &DistField) -> Vec<usize> {
    let mut seen = vec![false; m.len()];
    let mut out = Vec::with_capacity(df.order.len() * 2);
    for &i in &df.order {
        if !std::mem::replace(&mut seen[i], true) {
            out.push(i);
        }
        for nb in m.around(m.pos(i)) {
            let ni = m.idx(nb);
            if !std::mem::replace(&mut seen[ni], true) {
                out.push(ni);
            }
        }
    }
    out
}

/// An empty grow-zone cell on soil, ready for planting its zone's crop.
fn plantable(m: &Map, i: usize) -> bool {
    let p = m.pos(i);
    m.zone[i].crop().is_some()
        && m.feature[i] == Feature::None
        && m.items[i].is_none()
        && m.standable(p)
        && m.below(p).is_some_and(|b| m.block[m.idx(b)] == Block::Soil)
        && !m.covered(p)
}

/// The grown plant in cell `i`, if it may be harvested now: automatically,
/// or because it carries the designation its definition asks for.
fn harvestable(m: &Map, i: usize) -> Option<PlantId> {
    match m.feature[i] {
        Feature::Plant { kind, growth } if growth >= 1.0 => {
            let desig = kind.def().harvest.desig;
            (desig.is_none() || desig == m.desig[i]).then_some(kind)
        }
        _ => None,
    }
}

/// Work done from beside the target (it's in the way) rather than standing on it.
fn from_beside(task: Task) -> bool {
    match task {
        Task::Harvest(kind) => !kind.def().passable,
        Task::Sow(_) => false,
        Task::Mine | Task::Fill | Task::Construct | Task::Deconstruct | Task::Craft | Task::Uninstall => true,
    }
}

/// What a colonist can reach from where they stand: distances, and the
/// nearest free stack of each item kind and stockpile cell for it.
pub(crate) struct Scan {
    pub df: DistField,
    pub nearest_item: Nearest,
    pub nearest_cell: NearestStore,
}

impl Scan {
    /// The nearest free stack of any item that `is`.
    pub fn nearest_of(&self, is: impl Fn(ItemId) -> bool) -> Option<Pos> {
        ItemId::all()
            .filter(|&k| is(k))
            .filter_map(|k| self.nearest_item[k.index()])
            .min_by_key(|(d, _)| *d)
            .map(|(_, pos)| pos)
    }

    /// Medicine and food for doctoring (cooked food if there is any).
    pub fn medicine_and_food(&self) -> (Option<Pos>, Option<Pos>) {
        let medicine = self.nearest_of(|k| defs().is_medicine(k));
        let food = self.nearest_of(|k| defs().is_food(k) && !k.def().raw).or(self.nearest_of(|k| defs().is_food(k)));
        (medicine, food)
    }
}

impl World {
    /// Look around from `from` (see `Scan`).
    pub(crate) fn scan(&self, from: Pos) -> Scan {
        let df = bfs(&self.map, from);
        let m = &self.map;
        // Nearest free stack of each item kind (for deliveries) and nearest
        // stockpile cell of each priority that can take each kind (for hauling).
        let kinds = defs().items.len();
        let mut nearest_item: Nearest = vec![None; kinds];
        let mut nearest_cell: NearestStore = vec![[None; 5]; kinds];
        let accepts: Vec<Vec<bool>> =
            self.stockpiles.iter().map(|s| ItemId::all().map(|k| s.accepts(k)).collect()).collect();
        for &i in &df.order {
            let pos = m.pos(i);
            let Some(d) = df.dist(pos) else { continue };
            for s in m.stacks(i) {
                let k = s.kind.index();
                if self.item_free(i) && nearest_item[k].map_or(true, |(bd, _)| d < bd) {
                    nearest_item[k] = Some((d, pos));
                }
            }
            if let (Some(id), 0) = (m.zone[i].stockpile(), self.res_cell[i]) {
                let Some(store) = self.stockpiles.get(id as usize) else { continue };
                let prio = store.priority as usize;
                for kind in ItemId::all() {
                    let k = kind.index();
                    let slot = &mut nearest_cell[k][prio];
                    if accepts[id as usize][k] && slot.is_none_or(|(bd, _)| d < bd) && m.can_accept(i, kind) {
                        *slot = Some((d, pos));
                    }
                }
            }
        }
        Scan { df, nearest_item, nearest_cell }
    }

    /// Highest-priority, then nearest, available job for `p`.
    pub(crate) fn find_work(&self, p: &Pawn) -> Option<Job> {
        let scan = self.scan(p.pos);
        let (df, nearest_item, nearest_cell) = (&scan.df, &scan.nearest_item, &scan.nearest_cell);
        let mut candidates = reachable_and_around(&self.map, df);
        // Cells to fill deep in a shaft are worked from its rim, further away.
        let deep = self.map.desig.iter().enumerate().filter(|(_, d)| **d == Some(Desig::Fill)).map(|(i, _)| i);
        // (A cell listed twice is harmless: the nearest job wins.)
        candidates.extend(deep);

        // Better gear first: it's quick.
        if p.is_colonist() {
            if let Some(job) = self.equip_candidate(p, nearest_item) {
                return Some(job);
            }
        }

        // Nearest job of each enabled work type (one pass per type), then the
        // highest priority wins, nearest first among equal priorities.
        let mut per_type: Vec<(u8, i32, Job)> = Vec::new();
        for wt in WorkType::ALL {
            let prio = p.priorities[wt as usize];
            if prio == 0 {
                continue;
            }
            let found = if wt == WorkType::Doctor {
                let (medicine, food) = scan.medicine_and_food();
                self.doctor_candidate(p, df, medicine, food)
            } else {
                let cells = candidates
                    .iter()
                    .filter_map(|&i| self.work_candidate(wt, i, df, nearest_item, nearest_cell, false))
                    .min_by_key(|(d, _)| *d);
                // Haulers also bury the dead.
                let burial = if wt == WorkType::Hauling { self.bury_candidate(df) } else { None };
                cells.into_iter().chain(burial).min_by_key(|(d, _)| *d)
            };
            if let Some((d, job)) = found {
                per_type.push((prio, d, job));
            }
        }
        per_type.into_iter().min_by_key(|(prio, d, _)| (*prio, *d)).map(|(_, _, job)| job)
    }

    pub(crate) fn work_candidate(
        &self,
        wt: WorkType,
        i: usize,
        df: &DistField,
        nearest_item: &Nearest,
        nearest_cell: &NearestStore,
        // Ordered by the player: take it over even if someone else has it.
        forced: bool,
    ) -> Option<(i32, Job)> {
        let m = &self.map;
        let pos = m.pos(i);
        let work = |task: Task| {
            df.approach(m, pos, from_beside(task)).map(|(_, d)| (d, Job::Work { target: pos, task }))
        };
        match wt {
            // Handled per patient in `doctor_candidate`.
            WorkType::Doctor => return None,
            WorkType::Hauling => {
                if self.res_item[i] == 0 && m.has_items(i) {
                    let d = df.dist(pos)?;
                    // The first stack here with somewhere better to go (a shelf may hold several).
                    let to = m.stacks(i).find_map(|s| {
                        // Into a stockpile that takes it, preferring higher priorities
                        // (and only to better ones if it's already stored).
                        let here = self.storage_priority(i, s.kind).unwrap_or(0) as usize;
                        let to = (here + 1..=4).rev().find_map(|prio| nearest_cell[s.kind.index()][prio]).map(|(_, to)| to);
                        // Refused where it's stored, and no stockpile takes it: out onto open ground.
                        let refused = m.zone[i].stockpile().is_some() && here == 0;
                        to.or_else(|| refused.then(|| self.dump_spot(pos, s.kind, df)).flatten())
                    });
                    return to.map(|to| (d, Job::Haul { from: pos, to, deliver: false, picked: false }));
                }
                // Ingredients to a workbench.
                if self.res_target[i] != 0 && !forced || !matches!(m.feature[i], Feature::Built { .. }) {
                    return None;
                }
                let need = self.bench_at(pos)?.need()?;
                let (_, d) = df.approach(m, pos, true)?;
                let (di, from) = nearest_item[need.kind.index()]?;
                return Some((d + di, Job::Haul { from, to: pos, deliver: true, picked: false }));
            }
            _ => {}
        }
        if self.res_target[i] != 0 && !forced {
            return None;
        }
        if wt == WorkType::Mining && m.desig[i] == Some(Desig::Mine) && m.mineable(pos) {
            return work(Task::Mine);
        }
        // Bottom up: a cell waits for the one below to be filled. Worked from the shaft's rim.
        if wt == WorkType::Mining && m.desig[i] == Some(Desig::Fill) && m.fillable(pos, false) && !self.someone_at(pos) {
            let spot = m.fill_spot(pos);
            return df.approach(m, spot, true).map(|(_, d)| (d, Job::Work { target: pos, task: Task::Fill }));
        }
        if wt == WorkType::Construction && m.desig[i] == Some(Desig::Deconstruct) && m.deconstructable(pos) {
            return work(Task::Deconstruct);
        }
        if wt == WorkType::Construction && self.move_from(pos).is_some_and(|mv| !mv.lifted) {
            return work(Task::Uninstall);
        }
        match m.feature[i] {
            Feature::Plant { kind, .. } => {
                if kind.def().harvest.work == wt && harvestable(m, i).is_some() {
                    return work(Task::Harvest(kind));
                }
            }
            Feature::Blueprint { kind, delivered, .. } => {
                let def = kind.def();
                // Walls planned on walls go up one storey at a time.
                if def.work != wt || self.stacked_on_plan(pos) {
                    return None;
                }
                // Moved furniture needs no materials, only to be packed up first.
                if let Some(mv) = self.move_to(pos) {
                    return if mv.lifted { work(Task::Construct) } else { None };
                }
                // Materials come in the order of the cost list.
                let Some((material, _)) = def.missing(delivered) else {
                    return work(Task::Construct);
                };
                let (_, d) = df.approach(m, pos, true)?;
                let (di, from) = nearest_item[material.index()]?;
                return Some((d + di, Job::Haul { from, to: pos, deliver: true, picked: false }));
            }
            Feature::None => {
                let crop = m.zone[i].crop()?;
                if crop.def().sow.as_ref().is_some_and(|s| s.work == wt) && plantable(m, i) {
                    return work(Task::Sow(crop));
                }
            }
            Feature::Built { kind, .. } => {
                if kind.def().recipes.is_empty() {
                    return None;
                }
                let bench = self.bench_at(pos)?;
                if !bench.current.is_some_and(|r| r.def().work == wt) {
                    return None;
                }
                if bench.ready() {
                    return work(Task::Craft);
                }
                // Crafters fetch what the bill still lacks themselves (haulers help too).
                let need = bench.need()?;
                let (_, d) = df.approach(m, pos, true)?;
                let (di, from) = nearest_item[need.kind.index()]?;
                return Some((d + di, Job::Haul { from, to: pos, deliver: true, picked: false }));
            }
        }
        None
    }

    pub(crate) fn run_job(&mut self, p: &mut Pawn, dt: f32) {
        let Some(job) = p.job else { return };
        match job {
            Job::Wander { to } | Job::Goto { to } => {
                if !matches!(self.go_to(p, to, false, dt), Step::Moving) {
                    self.end_job(p);
                }
            }
            Job::Work { target, task } => self.run_work(p, target, task, dt),
            Job::Haul { from, to, deliver, picked } => self.run_haul(p, from, to, deliver, picked, dt),
            Job::Eat { from, picked, seat } => self.run_eat(p, from, picked, seat, dt),
            Job::Sleep { bed } => self.run_sleep(p, bed, dt),
            Job::Patient { bed } => self.run_patient(p, bed, dt),
            Job::Tend { patient, medicine, picked } => self.run_tend(p, patient, medicine, picked, dt),
            Job::Rescue { patient, bed, carrying } => self.run_rescue(p, patient, bed, carrying, dt),
            Job::Feed { patient, food, picked } => self.run_feed(p, patient, food, picked, dt),
            Job::Equip { from, chosen } => self.run_equip(p, from, chosen, dt),
            Job::Bury { corpse, grave, carrying } => self.run_bury(p, corpse, grave, carrying, dt),
            Job::Operation { table } => self.run_operation(p, table, dt),
            Job::Surgery { patient, prosthetic, picked } => self.run_surgery(p, patient, prosthetic, picked, dt),
        }
    }

    // ---------- steps jobs share ----------

    /// Walk towards `goal` (to beside it if `beside`); true once there. If it
    /// can't be reached the job is given up (and false returned).
    pub(crate) fn walk_to(&mut self, p: &mut Pawn, goal: Pos, beside: bool, dt: f32) -> bool {
        match self.go_to(p, goal, beside, dt) {
            Step::Moving => false,
            Step::Unreachable => {
                self.end_job(p);
                false
            }
            Step::Arrived => true,
        }
    }

    /// Walk to the item at `from` and pick one up with `take` (into
    /// `p.carrying`, its reservation freed). The job itself goes on: what
    /// happens next is up to the caller.
    pub(crate) fn fetch(&mut self, p: &mut Pawn, from: Pos, dt: f32, take: impl FnOnce(&mut World, usize) -> Option<Stack>) -> Fetch {
        match self.go_to(p, from, false, dt) {
            Step::Moving => Fetch::Walking,
            Step::Unreachable => Fetch::Unreachable,
            Step::Arrived => {
                let i = self.map.idx(from);
                let got = take(self, i);
                self.res_item[i] = 0;
                match got {
                    Some(s) => {
                        p.carrying = Some(s);
                        Fetch::Got
                    }
                    None => Fetch::Gone,
                }
            }
        }
    }

    /// Pawn `oi` (a patient, a body) carried along by `p`.
    pub(crate) fn carry_along(&mut self, p: &Pawn, oi: usize) {
        let o = &mut self.pawns[oi];
        (o.x, o.y, o.z, o.pos) = (p.x, p.y, p.z, p.pos);
    }

    /// A stroke of `skill` work taking `time` seconds at speed 1 (`speed` and
    /// `learn` scale it and the practice); true once done. New skill levels are announced.
    pub(crate) fn work_on(&mut self, p: &mut Pawn, skill: Skill, time: f32, (speed, learn): (f32, f32), dt: f32) -> bool {
        p.job_progress += dt * speed * p.work_speed(skill) / time;
        if p.learn(skill, dt * learn) {
            let msg = format!("{} reached {} level {}.", p.name, skill.name(), p.skill(skill));
            self.sound("level_up", None);
            self.push_log(msg);
        }
        p.job_progress >= 1.0
    }

    fn run_work(&mut self, p: &mut Pawn, target: Pos, task: Task, dt: f32) {
        if !self.work_valid(target, task) {
            self.end_job(p);
            return;
        }
        let goal = if task == Task::Fill { self.map.fill_spot(target) } else { target };
        if !self.walk_to(p, goal, from_beside(task), dt) {
            return;
        }
        let Some((skill, time)) = self.task_skill_time(target, task) else {
            self.end_job(p);
            return;
        };
        // A themed room speeds work and learning at its workbenches.
        let rates = match task {
            Task::Craft => self.room_effects(target).map_or((1.0, 1.0), |(_, e)| (e.work, e.learn)),
            _ => (1.0, 1.0),
        };
        if self.work_on(p, skill, time, rates, dt) {
            // Setting down their own bed: `p` is a copy, so it's given back here.
            let own_bed = task == Task::Construct && self.move_to(target).is_some_and(|mv| mv.owner == Some(p.id));
            self.complete_work(target, task, &p.name);
            if own_bed {
                p.bed = Some(target);
            }
            self.end_job(p);
        }
    }

    fn work_valid(&self, target: Pos, task: Task) -> bool {
        let m = &self.map;
        let i = m.idx(target);
        let f = m.feature[i];
        let d = m.desig[i];
        match task {
            Task::Harvest(kind) => harvestable(m, i) == Some(kind),
            Task::Mine => d == Some(Desig::Mine) && m.mineable(target),
            Task::Fill => d == Some(Desig::Fill) && m.fillable(target, false) && !self.someone_at(target),
            Task::Deconstruct => d == Some(Desig::Deconstruct) && m.deconstructable(target),
            Task::Sow(_) => plantable(m, i),
            Task::Construct => match f {
                Feature::Blueprint { kind, delivered, .. } => match self.move_to(target) {
                    Some(mv) => mv.lifted,
                    None => delivered >= kind.def().total_cost(),
                },
                _ => false,
            },
            Task::Craft => self.bench_at(target).is_some_and(|b| b.ready()),
            Task::Uninstall => {
                self.move_from(target).is_some_and(|mv| !mv.lifted && f.structure_id() == Some(mv.kind))
            }
        }
    }

    /// Skill used and base seconds of work for `task` at `target`.
    fn task_skill_time(&self, target: Pos, task: Task) -> Option<(Skill, f32)> {
        match task {
            Task::Harvest(kind) => {
                let h = &kind.def().harvest;
                Some((h.skill, h.time))
            }
            Task::Sow(kind) => kind.def().sow.as_ref().map(|s| (s.skill, s.time)),
            Task::Mine => Some((Skill::Mining, MINE_TIME)),
            Task::Fill => Some((Skill::Mining, FILL_TIME)),
            // Taking down is quicker than putting up.
            Task::Deconstruct => {
                let m = &self.map;
                let i = m.idx(target);
                let flat = if m.floor[i] { defs().floor() } else if m.roof[i] { defs().roof() } else { None };
                let def = m.feature[i].structure().or_else(|| flat.map(|f| f.def()))?;
                Some((Skill::Construction, (def.work_time * DECONSTRUCT_SHARE).max(1.0)))
            }
            Task::Craft => self.bench_at(target)?.current.map(|r| (r.def().skill, r.def().time)),
            Task::Construct => match self.map.feature[self.map.idx(target)] {
                Feature::Blueprint { kind, .. } => {
                    let share = if self.move_to(target).is_some() { MOVE_SHARE } else { 1.0 };
                    Some((kind.def().skill, (kind.def().work_time * share).max(1.0)))
                }
                _ => None,
            },
            Task::Uninstall => {
                let kind = self.move_from(target)?.kind;
                Some((Skill::Construction, (kind.def().work_time * MOVE_SHARE).max(1.0)))
            }
        }
    }

    fn complete_work(&mut self, target: Pos, task: Task, who: &str) {
        let i = self.map.idx(target);
        let f = self.map.feature[i];
        if task == Task::Craft {
            if let Some(r) = self.finish_recipe(target) {
                let made: Vec<String> =
                    r.def().products.iter().map(|a| format!("{} {}", a.count, a.item.name().to_lowercase())).collect();
                self.push_log(format!("{} made {}.", who, made.join(", ")));
            }
            return;
        }
        self.map.desig[i] = None;
        let drop = |m: &mut Map, at: Pos, drops: Vec<(ItemId, u32)>| {
            for (kind, count) in drops.into_iter().filter(|d| d.1 > 0) {
                m.drop_item(at, Stack { kind, count });
            }
        };
        match task {
            Task::Harvest(kind) => {
                self.map.set_feature(i, Feature::None);
                if let Some(s) = kind.def().harvest.done_sound.as_deref() {
                    self.sound(s, Some(target));
                }
                let y = &kind.def().harvest.yields;
                drop(&mut self.map, target, vec![(y.item, y.count)]);
            }
            Task::Sow(kind) => self.map.set_feature(i, Feature::Plant { kind, growth: 0.0 }),
            Task::Fill => {
                // Anything lying here is pushed up onto the new ground.
                self.map.set_block(i, Block::Soil);
                self.map.zone[i] = Zone::None;
                self.map.blood[i] = 0.0;
                self.sound("dig", Some(target));
                match self.map.above(target) {
                    Some(up) => self.map.move_items(i, up),
                    None => {
                        self.map.take_all(i);
                    }
                }
            }
            Task::Mine => {
                let dug = self.map.block[i];
                if dug.solid() {
                    // Dug out: rock or soil becomes open space (a tunnel if there's more above).
                    self.map.set_block(i, Block::Air);
                    self.sound(if dug == Block::Soil { "dig" } else { "rock_break" }, Some(target));
                    let yields = match dug {
                        Block::Rock => Some(&defs().rock_drops),
                        Block::Ore => Some(&defs().ore_drops),
                        Block::Copper => Some(&defs().copper_drops),
                        _ => None,
                    };
                    if let Some(r) = yields {
                        drop(&mut self.map, target, vec![(r.item, r.count)]);
                    }
                }
            }
            Task::Deconstruct => {
                if let Some(def) = f.structure() {
                    self.map.set_feature(i, Feature::None);
                    self.sound("built", Some(target));
                    drop(&mut self.map, target, def.drops());
                    if def.storage.is_some() {
                        self.sync_containers();
                    }
                } else if self.map.floor[i] {
                    self.map.set_floor(i, false);
                    self.sound("built", Some(target));
                    let at = self.map.landing(target);
                    if let Some(floor) = defs().floor() {
                        drop(&mut self.map, at, floor.def().drops());
                    }
                } else if self.map.roof[i] {
                    // Marked again if the room is still closed (`mark_roofs`).
                    self.map.set_roof(i, false);
                    self.sound("built", Some(target));
                    let at = self.map.landing(target);
                    if let Some(roof) = defs().roof() {
                        drop(&mut self.map, at, roof.def().drops());
                    }
                }
            }
            Task::Craft => {}
            Task::Uninstall => self.lift(target),
            Task::Construct => {
                if let Feature::Blueprint { .. } = f {
                    if self.finish_move(target) {
                        self.sound("built", Some(target));
                        return;
                    }
                }
                if let Feature::Blueprint { kind, dir, .. } = f {
                    match kind.def().special {
                        Special::Floor => {
                            self.map.set_feature(i, Feature::None);
                            self.map.set_roof(i, false);
                            self.map.set_floor(i, true);
                        }
                        Special::Roof => {
                            self.map.set_feature(i, Feature::None);
                            self.map.set_roof(i, true);
                        }
                        Special::Stairs => {
                            // The way the player turned them, if they can climb that way.
                            let dir = if self.map.stairs_fit(target, dir) { dir } else { self.map.stairs_facing(target) };
                            self.map.set_feature(i, Feature::Built { kind, dir });
                            // A roof over them or where they're walked onto would block the way up:
                            // take it down.
                            let over: Vec<Pos> = [Some(target), self.map.stairs_foot(target)]
                                .into_iter()
                                .flatten()
                                .filter_map(|c| self.map.above(c))
                                .collect();
                            for a in over {
                                let ai = self.map.idx(a);
                                if self.map.roof[ai] {
                                    self.map.set_roof(ai, false);
                                    if let Some(roof) = defs().roof() {
                                        drop(&mut self.map, target, roof.def().drops());
                                    }
                                }
                            }
                        }
                        _ => self.map.set_feature(i, Feature::Built { kind, dir }),
                    }
                    self.sound("built", Some(target));
                    // Shove anything lying here out of the way (onto the shelf, if it is one).
                    self.map.move_items(i, target);
                    if kind.def().storage.is_some() {
                        self.sync_containers();
                    }
                }
            }
        }
    }

    /// Materials a blueprint or workbench still needs.
    /// Open ground near `from` (within 10 cells, nearest ring first) outside
    /// any zone, with nothing built on it, that the hauler can reach (`df`)
    /// and a stack of `kind` can go on: where things a stockpile refuses are
    /// cleared out to when no other stockpile takes them.
    fn dump_spot(&self, from: Pos, kind: ItemId, df: &DistField) -> Option<Pos> {
        let m = &self.map;
        let ok = |p: Pos| {
            let i = m.idx(p);
            m.zone[i] == Zone::None
                && m.feature[i] == Feature::None
                && self.res_cell[i] == 0
                && m.stacks(i).all(|s| s.kind == kind)
                && m.can_accept(i, kind)
                && df.dist(p).is_some()
        };
        let (x, y, z) = (from.0 as i32, from.1 as i32, from.2 as i32);
        (1..=10).find_map(|r: i32| {
            (-r..=r)
                .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
                .filter(|&(dx, dy)| dx.abs().max(dy.abs()) == r)
                .flat_map(|(dx, dy)| (-1..=1).filter_map(move |dz| m.at(x + dx, y + dy, z + dz)))
                .find(|&p| ok(p))
        })
    }

    /// What to pick up in cell `fi` for a haul to `to`: what's already in
    /// hand, what the blueprint or bench needs, or else the first thing here
    /// that `to` takes and that belongs there more than here (or that's
    /// refused here, to be cleared out).
    fn pick_up_kind(&self, fi: usize, to: Pos, deliver: bool, carrying: Option<Stack>) -> Option<ItemId> {
        let m = &self.map;
        let ti = m.idx(to);
        if let Some(c) = carrying {
            return (m.count_in(fi, c.kind) > 0).then_some(c.kind);
        }
        if deliver {
            return self.delivery_need(to).map(|n| n.kind).filter(|&k| m.count_in(fi, k) > 0);
        }
        m.stacks(fi).map(|s| s.kind).find(|&k| {
            let here = self.storage_priority(fi, k);
            let there = self.storage_priority(ti, k);
            m.can_accept(ti, k) && (there > here.or(Some(0)) || here.is_none() && m.zone[fi].stockpile().is_some())
        })
    }

    /// How many of `kind` fit in stockpile cell `to` (at most a stack).
    fn room_for(&self, to: Pos, kind: ItemId) -> u32 {
        let i = self.map.idx(to);
        self.map.room_in(i, kind).min(STACK_MAX)
    }

    /// The nearest free stack of `kind` within `NEARBY_STEPS` of `from` that
    /// belongs in stockpile cell `to` more than where it is, if `room` is left.
    fn gather_next(&self, from: Pos, kind: ItemId, to: Pos, room: u32) -> Option<Pos> {
        if room == 0 {
            return None;
        }
        let ti = self.map.idx(to);
        let target = self.storage_priority(ti, kind).unwrap_or(0);
        self.nearest_stack(&bfs(&self.map, from), NEARBY_STEPS, |i, s| {
            i != ti && s.kind == kind && self.res_item[i] == 0 && self.storage_priority(i, kind).unwrap_or(0) < target
        })
    }

    /// Blueprints within `NEARBY_STEPS` of `from` that need `kind`, nearest
    /// first, with how many; only free ones (or `me`'s).
    fn nearby_blueprints(&self, from: Pos, kind: ItemId, me: u32) -> Vec<(Pos, u32)> {
        let df = bfs(&self.map, from);
        let m = &self.map;
        // Blueprints aren't walked onto, so look at the cells beside reachable ones too.
        let mut found: Vec<(i32, Pos, u32)> = Vec::new();
        for (i, d) in df.nearest_first(NEARBY_STEPS) {
            for c in std::iter::once(m.pos(i)).chain(m.neighbors(m.pos(i))) {
                let ci = m.idx(c);
                if !matches!(m.feature[ci], Feature::Blueprint { .. }) || found.iter().any(|f| f.1 == c) {
                    continue;
                }
                if self.res_target[ci] != 0 && self.res_target[ci] != me {
                    continue;
                }
                if let Some(need) = self.delivery_need(c).filter(|n| n.kind == kind) {
                    found.push((d, c, need.count));
                }
            }
        }
        found.sort_by_key(|f| f.0);
        found.into_iter().map(|(_, c, n)| (c, n)).collect()
    }

    /// How many to pick up for blueprint `to` (needing `need`): enough for it
    /// and the free ones nearby needing the same, up to a stack.
    fn delivery_load(&self, to: Pos, need: Stack, me: u32) -> u32 {
        if !matches!(self.map.feature[self.map.idx(to)], Feature::Blueprint { .. }) {
            return need.count;
        }
        let others: u32 = self.nearby_blueprints(to, need.kind, me).iter().filter(|b| b.0 != to).map(|b| b.1).sum();
        (need.count + others).min(STACK_MAX).max(need.count)
    }

    /// The nearest blueprint within reach of `from` still needing `kind`.
    fn next_delivery(&self, from: Pos, kind: ItemId, me: u32) -> Option<Pos> {
        self.nearby_blueprints(from, kind, me).first().map(|b| b.0)
    }

    /// Whether the stack in cell `i` can be taken from: nobody has claimed
    /// it, or there's more than one, so others can share it (whoever comes
    /// too late just looks again).
    pub(crate) fn item_free(&self, i: usize) -> bool {
        self.res_item[i] == 0 || self.map.stacks(i).map(|s| s.count).sum::<u32>() > 1
    }

    /// Someone (alive, not buried) is in cell `p`.
    fn someone_at(&self, p: Pos) -> bool {
        self.pawns.iter().any(|o| !o.dead && !o.left && o.pos == p)
    }

    pub(crate) fn delivery_need(&self, at: Pos) -> Option<Stack> {
        match self.map.feature[self.map.idx(at)] {
            Feature::Blueprint { .. } if self.move_to(at).is_some() => None,
            Feature::Blueprint { kind, delivered, .. } => {
                kind.def().missing(delivered).map(|(item, count)| Stack { kind: item, count })
            }
            Feature::Built { .. } => self.bench_at(at)?.need(),
            _ => None,
        }
    }

    fn run_haul(&mut self, p: &mut Pawn, from: Pos, to: Pos, deliver: bool, picked: bool, dt: f32) {
        if !picked {
            match self.go_to(p, from, false, dt) {
                Step::Moving => {}
                Step::Unreachable => self.end_job(p),
                Step::Arrived => {
                    let fi = self.map.idx(from);
                    self.res_item[fi] = 0;
                    let carried = p.carrying.map_or(0, |c| c.count);
                    let here = self.pick_up_kind(fi, to, deliver, p.carrying);
                    // Gone, or not what's already in hand: carry on with what we have.
                    let Some(kind) = here.filter(|&k| p.carrying.is_none_or(|c| c.kind == k)) else {
                        if carried > 0 {
                            p.job = Some(Job::Haul { from, to, deliver, picked: true });
                        } else {
                            self.end_job(p);
                        }
                        return;
                    };
                    // How much to end up with: for this blueprint and others nearby, or what fits where it goes.
                    let load = if deliver {
                        match self.delivery_need(to) {
                            Some(need) if need.kind == kind => self.delivery_load(to, need, p.id),
                            _ => {
                                self.end_job(p);
                                return;
                            }
                        }
                    } else {
                        self.room_for(to, kind)
                    };
                    let taken = self.map.take_item(fi, kind, load.saturating_sub(carried)).map_or(0, |s| s.count);
                    let total = carried + taken;
                    if total == 0 {
                        self.end_job(p);
                        return;
                    }
                    p.carrying = Some(Stack { kind, count: total });
                    // More of the same lying nearby, while there's room for it.
                    let more = if deliver { None } else { self.gather_next(p.pos, kind, to, load - total) };
                    p.job = Some(match more {
                        Some(next) => {
                            self.res_item[self.map.idx(next)] = p.id;
                            Job::Haul { from: next, to, deliver, picked: false }
                        }
                        None => Job::Haul { from, to, deliver, picked: true },
                    });
                }
            }
            return;
        }

        let Some(carry) = p.carrying else {
            self.end_job(p);
            return;
        };
        let ti = self.map.idx(to);
        let still_wanted = if deliver {
            self.delivery_need(to).map_or(false, |n| n.kind == carry.kind)
        } else {
            // A stockpile that takes it, or open ground it's being cleared out onto.
            let dump = self.map.zone[ti] == Zone::None && self.map.feature[ti] == Feature::None;
            (self.storage_priority(ti, carry.kind).is_some() || dump) && self.map.can_accept(ti, carry.kind)
        };
        if !still_wanted {
            self.end_job(p);
            return;
        }
        match self.go_to(p, to, deliver, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let used = if deliver {
                    match self.map.feature[ti] {
                        Feature::Blueprint { kind, delivered, dir } => {
                            let used = self.delivery_need(to).filter(|n| n.kind == carry.kind).map_or(0, |n| n.count).min(carry.count);
                            self.map.feature[ti] = Feature::Blueprint { kind, delivered: delivered + used, dir };
                            used
                        }
                        _ => self.stock_bench(to, carry),
                    }
                } else {
                    self.map.add_to_cell(ti, carry)
                };
                p.carrying = (carry.count > used).then(|| Stack { kind: carry.kind, count: carry.count - used });
                // The rest to the next blueprint nearby that needs it.
                if let Some(next) = p.carrying.filter(|_| deliver).and_then(|c| self.next_delivery(p.pos, c.kind, p.id)) {
                    if self.res_target[ti] == p.id {
                        self.res_target[ti] = 0;
                    }
                    self.res_target[self.map.idx(next)] = p.id;
                    p.job = Some(Job::Haul { from: to, to: next, deliver: true, picked: true });
                    return;
                }
                // Drops any leftovers where we stand.
                self.end_job(p);
            }
        }
    }

    fn run_eat(&mut self, p: &mut Pawn, from: Pos, picked: bool, seat: Option<Pos>, dt: f32) {
        if picked {
            // Carry the food to the table first.
            if let Some(s) = seat {
                let step = if self.is_seat(s) { self.go_to(p, s, false, dt) } else { Step::Unreachable };
                match step {
                    Step::Moving => return,
                    Step::Unreachable => {
                        p.job = Some(Job::Eat { from, picked, seat: None });
                        return;
                    }
                    Step::Arrived => {}
                }
            }
            p.job_progress += dt / 2.0;
            if p.job_progress >= 1.0 {
                if let Some(s) = p.carrying.take() {
                    self.eat(p, s.kind);
                    let memory = match seat {
                        Some(s) => self.room_at(s).map(|r| Memory::DiningRoom(r.tier())),
                        None => Some(Memory::AteWithoutTable),
                    };
                    if let Some(m) = memory.filter(|_| p.is_colonist()) {
                        p.add_memory(m);
                    }
                }
                self.end_job(p);
            }
            return;
        }
        // Held while eating; dropped again if interrupted.
        match self.fetch(p, from, dt, |w, i| w.take_one(i, Defs::is_food)) {
            Fetch::Walking => {}
            Fetch::Got => {
                p.job = Some(Job::Eat { from, picked: true, seat });
                p.job_progress = 0.0;
            }
            Fetch::Unreachable | Fetch::Gone => self.end_job(p),
        }
    }

    fn run_sleep(&mut self, p: &mut Pawn, bed: Option<Pos>, dt: f32) {
        if let Some(b) = bed {
            if !self.map.feature[self.map.idx(b)].is_bed() {
                p.job = Some(Job::Sleep { bed: None });
                return;
            }
            match self.go_to(p, b, false, dt) {
                Step::Moving => return,
                Step::Unreachable => {
                    p.job = Some(Job::Sleep { bed: None });
                    return;
                }
                Step::Arrived => {}
            }
        }
        p.sleeping = true;
        let themed = bed.and_then(|b| self.room_effects(b));
        // From the ground to half again in a proper bed (a sleeping spot is in between).
        let comfort = bed.and_then(|b| self.map.feature[self.map.idx(b)].structure()).map_or(0.0, |d| d.comfort);
        let rate = SLEEP_GAIN * (1.0 + 0.5 * comfort) * themed.map_or(1.0, |(_, e)| e.rest);
        p.rest = (p.rest + rate * dt).min(100.0);
        if p.rest >= 100.0 {
            let memory = match bed {
                Some(_) if comfort < 1.0 => Memory::SleptOnSpot,
                Some(b) if self.map.roofed(b) => Memory::Bedroom(self.room_at(b).map_or(1, |r| r.tier())),
                Some(_) => Memory::SleptOutside,
                None => Memory::SleptOnGround,
            };
            p.add_memory(memory);
            if let Some((k, _)) = themed.filter(|(_, e)| e.thought.is_some()) {
                p.add_memory(Memory::RoomThought(k as u8));
            }
            self.end_job(p);
        }
    }

    /// Start eating the nearest reachable food. Returns false if there is none.
    pub(crate) fn try_eat(&mut self, p: &mut Pawn) -> bool {
        match self.nearest_food(p.pos) {
            Some(from) => {
                self.end_job(p);
                let seat = self.find_seat(from, p.id);
                self.start_job(p, Job::Eat { from, picked: false, seat });
                true
            }
            None => false,
        }
    }

    /// A seat beside a table.
    fn is_seat(&self, p: Pos) -> bool {
        let m = &self.map;
        m.feature[m.idx(p)].is_special(Special::Seat)
            && m.neighbors(p).any(|nb| m.feature[m.idx(nb)].is_special(Special::Table))
    }

    /// The nearest free seat at a table, not too far from the food at `near`.
    fn find_seat(&self, near: Pos, me: u32) -> Option<Pos> {
        const MAX_WALK: i32 = 40;
        let taken = |s: Pos| {
            self.pawns.iter().any(|o| o.id != me && matches!(o.job, Some(Job::Eat { seat: Some(t), .. }) if t == s))
        };
        self.nearest_cell(near, MAX_WALK, |i| {
            let s = self.map.pos(i);
            self.is_seat(s) && !taken(s)
        })
    }

    /// The pawn's own bed, or the nearest unclaimed one (not an operating
    /// table, nor a hospital bed: those are kept for patients).
    pub(crate) fn find_bed(&self, p: &Pawn) -> Option<Pos> {
        if let Some(b) = p.bed {
            if self.map.feature[self.map.idx(b)].is_bed() && !self.medical_bed(b) {
                return Some(b);
            }
        }
        self.nearest_cell(p.pos, ANY_DISTANCE, |i| {
            let pos = self.map.pos(i);
            self.map.feature[i].is_bed() && !self.care_bed(pos) && !self.pawns.iter().any(|o| o.id != p.id && !o.dead && o.bed == Some(pos))
        })
    }

    /// Who sleeps in the bed at `bed`, if anyone has claimed it.
    pub fn bed_owner(&self, bed: Pos) -> Option<u32> {
        self.pawns.iter().find(|o| !o.dead && o.bed == Some(bed)).map(|o| o.id)
    }

    /// Give the bed at `bed` to colonist `owner` (whose old bed is freed), or
    /// to nobody (`None`: whoever needs it next). Whoever had it looks for
    /// another. Returns false if there's no bed there or no such colonist.
    pub fn assign_bed(&mut self, bed: Pos, owner: Option<u32>) -> bool {
        if !self.map.feature[self.map.idx(bed)].is_bed() {
            return false;
        }
        if let Some(id) = owner {
            if !self.pawn(id).is_some_and(|p| p.is_living_colonist()) {
                return false;
            }
        }
        for p in self.pawns.iter_mut() {
            if p.bed == Some(bed) {
                p.bed = None;
            }
            if Some(p.id) == owner {
                p.bed = Some(bed);
            }
        }
        true
    }

    pub(crate) fn start_wander(&mut self, p: &mut Pawn, radius: i32) {
        let df = bfs(&self.map, p.pos);
        let options: Vec<usize> =
            df.nearest_first(radius).filter(|&(_, d)| d > 0).map(|(i, _)| i).collect();
        if options.is_empty() {
            return;
        }
        let to = self.map.pos(options[self.rng.below(options.len())]);
        self.start_job(p, Job::Wander { to });
    }
}
