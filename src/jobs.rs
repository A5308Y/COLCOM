//! Finding work for colonists and carrying jobs out step by step.

use crate::defs::{defs, Defs, ItemId, PlantId, Special};
use crate::map::{Block, Desig, Feature, Map, Pos, Stack, Zone, STACK_MAX};
use crate::pathing::{bfs, DistField};
use crate::pawn::{Job, Memory, Pawn, Skill, Task, WorkType};
use crate::world::{Step, World};

/// Base seconds to mine out one cell.
const MINE_TIME: f32 = 5.0;

/// Nearest (distance, cell) per item kind.
type Nearest = Vec<Option<(i32, Pos)>>;

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

/// An empty grow-zone cell on soil, ready for planting.
fn plantable(m: &Map, i: usize) -> bool {
    let p = m.pos(i);
    m.zone[i] == Zone::Grow
        && m.feature[i] == Feature::None
        && m.items[i].is_none()
        && m.standable(p)
        && m.below(p).is_some_and(|b| m.block[m.idx(b)] == Block::Soil)
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
        Task::Mine | Task::Construct => true,
    }
}

impl World {
    /// Highest-priority, then nearest, available job for `p`.
    pub(crate) fn find_work(&self, p: &Pawn) -> Option<Job> {
        let df = bfs(&self.map, p.pos);
        let m = &self.map;
        let candidates = reachable_and_around(m, &df);

        // Nearest free stack of each item kind (for deliveries) and nearest
        // stockpile cell that can take each kind (for hauling).
        let kinds = defs().items.len();
        let mut nearest_item: Nearest = vec![None; kinds];
        let mut nearest_cell: Nearest = vec![None; kinds];
        for &i in &df.order {
            let pos = m.pos(i);
            let Some(d) = df.dist(pos) else { continue };
            if let Some(s) = m.items[i] {
                let k = s.kind.0 as usize;
                if self.res_item[i] == 0 && nearest_item[k].map_or(true, |(bd, _)| d < bd) {
                    nearest_item[k] = Some((d, pos));
                }
            }
            if m.zone[i] == Zone::Stockpile && self.res_cell[i] == 0 {
                for kind in ItemId::all() {
                    let k = kind.0 as usize;
                    if m.can_accept(i, kind) && nearest_cell[k].map_or(true, |(bd, _)| d < bd) {
                        nearest_cell[k] = Some((d, pos));
                    }
                }
            }
        }

        let nearest_of = |is: fn(&Defs, ItemId) -> bool| {
            ItemId::all()
                .filter(|&k| is(defs(), k))
                .filter_map(|k| nearest_item[k.0 as usize])
                .min_by_key(|(d, _)| *d)
                .map(|(_, pos)| pos)
        };
        // What grow zones get planted with, and by whom.
        let sow = defs().sown().and_then(|k| k.def().sow.as_ref().map(|s| (k, s.work)));

        // Nearest job of each enabled work type (one pass per type), then the
        // highest priority wins, nearest first among equal priorities.
        let mut per_type: Vec<(u8, i32, Job)> = Vec::new();
        for wt in WorkType::ALL {
            let prio = p.priorities[wt as usize];
            if prio == 0 {
                continue;
            }
            let found = if wt == WorkType::Doctor {
                self.doctor_candidate(p, &df, nearest_of(Defs::is_medicine), nearest_of(Defs::is_food))
            } else {
                candidates
                    .iter()
                    .filter_map(|&i| self.work_candidate(wt, i, &df, &nearest_item, &nearest_cell, sow))
                    .min_by_key(|(d, _)| *d)
            };
            if let Some((d, job)) = found {
                per_type.push((prio, d, job));
            }
        }
        per_type.into_iter().min_by_key(|(prio, d, _)| (*prio, *d)).map(|(_, _, job)| job)
    }

    #[allow(clippy::too_many_arguments)]
    fn work_candidate(
        &self,
        wt: WorkType,
        i: usize,
        df: &DistField,
        nearest_item: &Nearest,
        nearest_cell: &Nearest,
        sow: Option<(PlantId, WorkType)>,
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
                let s = m.items[i]?;
                if self.res_item[i] == 0 && m.zone[i] != Zone::Stockpile {
                    let d = df.dist(pos)?;
                    let (_, to) = nearest_cell[s.kind.0 as usize]?;
                    return Some((d, Job::Haul { from: pos, to, to_blueprint: false, picked: false }));
                }
                return None;
            }
            _ => {}
        }
        if self.res_target[i] != 0 {
            return None;
        }
        if wt == WorkType::Mining && m.desig[i] == Some(Desig::Mine) && m.mineable(pos) {
            return work(Task::Mine);
        }
        match m.feature[i] {
            Feature::Plant { kind, .. } => {
                if kind.def().harvest.work == wt && harvestable(m, i).is_some() {
                    return work(Task::Harvest(kind));
                }
            }
            Feature::Blueprint { kind, delivered } => {
                let def = kind.def();
                if def.work != wt {
                    return None;
                }
                if delivered >= def.cost.count {
                    return work(Task::Construct);
                }
                let (_, d) = df.approach(m, pos, true)?;
                let (di, from) = nearest_item[def.cost.item.0 as usize]?;
                return Some((d + di, Job::Haul { from, to: pos, to_blueprint: true, picked: false }));
            }
            Feature::None => {
                if let Some((crop, work_type)) = sow {
                    if work_type == wt && plantable(m, i) {
                        return work(Task::Sow(crop));
                    }
                }
            }
            Feature::Built { .. } => {}
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
            Job::Leave { to } => {
                if !matches!(self.go_to(p, to, false, dt), Step::Moving) {
                    p.left = true;
                }
            }
            Job::Attack { target } => self.run_attack(p, target, dt),
            Job::Work { target, task } => self.run_work(p, target, task, dt),
            Job::Haul { from, to, to_blueprint, picked } => self.run_haul(p, from, to, to_blueprint, picked, dt),
            Job::Eat { from, picked } => self.run_eat(p, from, picked, dt),
            Job::Sleep { bed } => self.run_sleep(p, bed, dt),
            Job::Patient { bed } => self.run_patient(p, bed, dt),
            Job::Tend { patient, medicine, picked } => self.run_tend(p, patient, medicine, picked, dt),
            Job::Rescue { patient, bed, carrying } => self.run_rescue(p, patient, bed, carrying, dt),
            Job::Feed { patient, food, picked } => self.run_feed(p, patient, food, picked, dt),
        }
    }

    fn run_work(&mut self, p: &mut Pawn, target: Pos, task: Task, dt: f32) {
        if !self.work_valid(target, task) {
            self.end_job(p);
            return;
        }
        match self.go_to(p, target, from_beside(task), dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let Some((skill, time)) = self.task_skill_time(target, task) else {
                    self.end_job(p);
                    return;
                };
                p.job_progress += dt * p.work_speed(skill) / time;
                if p.learn(skill, dt) {
                    let msg = format!("{} reached {} level {}.", p.name, skill.name(), p.skill(skill));
                    self.push_log(msg);
                }
                if p.job_progress >= 1.0 {
                    self.complete_work(target, task);
                    self.end_job(p);
                }
            }
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
            Task::Sow(_) => plantable(m, i),
            Task::Construct => {
                matches!(f, Feature::Blueprint { kind, delivered } if delivered >= kind.def().cost.count)
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
            Task::Construct => match self.map.feature[self.map.idx(target)] {
                Feature::Blueprint { kind, .. } => Some((kind.def().skill, kind.def().work_time)),
                _ => None,
            },
        }
    }

    fn complete_work(&mut self, target: Pos, task: Task) {
        let i = self.map.idx(target);
        let f = self.map.feature[i];
        self.map.desig[i] = None;
        let drop = |m: &mut Map, at: Pos, (kind, count): (ItemId, u32)| {
            if count > 0 {
                m.drop_item(at, Stack { kind, count });
            }
        };
        match task {
            Task::Harvest(kind) => {
                self.map.set_feature(i, Feature::None);
                let y = &kind.def().harvest.yields;
                drop(&mut self.map, target, (y.item, y.count));
            }
            Task::Sow(kind) => self.map.set_feature(i, Feature::Plant { kind, growth: 0.0 }),
            Task::Mine => {
                let dug = self.map.block[i];
                if dug.solid() {
                    // Dug out: rock or soil becomes open space (a tunnel if there's more above).
                    self.map.set_block(i, Block::Air);
                    if dug == Block::Rock {
                        let r = &defs().rock_drops;
                        drop(&mut self.map, target, (r.item, r.count));
                    }
                } else if let Some(def) = f.structure() {
                    self.map.set_feature(i, Feature::None);
                    drop(&mut self.map, target, def.drops());
                } else if self.map.floor[i] {
                    self.map.set_floor(i, false);
                    let at = self.map.landing(target);
                    if let Some(floor) = defs().floor() {
                        drop(&mut self.map, at, floor.def().drops());
                    }
                }
            }
            Task::Construct => {
                if let Feature::Blueprint { kind, .. } = f {
                    match kind.def().special {
                        Special::Floor => {
                            self.map.set_feature(i, Feature::None);
                            self.map.set_floor(i, true);
                        }
                        Special::Stairs => {
                            let dir = self.map.stairs_facing(target);
                            self.map.set_feature(i, Feature::Built { kind, dir });
                        }
                        _ => self.map.set_feature(i, Feature::Built { kind, dir: 0 }),
                    }
                    // Shove anything lying here out of the way.
                    if let Some(s) = self.map.items[i].take() {
                        self.map.drop_item(target, s);
                    }
                }
            }
        }
    }

    /// Materials a blueprint still needs, if it is one.
    fn blueprint_need(&self, at: Pos) -> Option<Stack> {
        match self.map.feature[self.map.idx(at)] {
            Feature::Blueprint { kind, delivered } => {
                let cost = &kind.def().cost;
                (delivered < cost.count).then(|| Stack { kind: cost.item, count: cost.count - delivered })
            }
            _ => None,
        }
    }

    fn run_haul(&mut self, p: &mut Pawn, from: Pos, to: Pos, to_blueprint: bool, picked: bool, dt: f32) {
        if !picked {
            match self.go_to(p, from, false, dt) {
                Step::Moving => {}
                Step::Unreachable => self.end_job(p),
                Step::Arrived => {
                    let fi = self.map.idx(from);
                    let Some(kind) = self.map.items[fi].map(|s| s.kind) else {
                        self.end_job(p);
                        return;
                    };
                    let want = if to_blueprint {
                        match self.blueprint_need(to) {
                            Some(need) if need.kind == kind => need.count,
                            _ => {
                                self.end_job(p);
                                return;
                            }
                        }
                    } else {
                        STACK_MAX
                    };
                    match self.map.take_item(fi, kind, want) {
                        Some(stack) => {
                            p.carrying = Some(stack);
                            self.res_item[fi] = 0;
                            p.job = Some(Job::Haul { from, to, to_blueprint, picked: true });
                        }
                        None => self.end_job(p),
                    }
                }
            }
            return;
        }

        let Some(carry) = p.carrying else {
            self.end_job(p);
            return;
        };
        let ti = self.map.idx(to);
        let still_wanted = if to_blueprint {
            self.blueprint_need(to).map_or(false, |n| n.kind == carry.kind)
        } else {
            self.map.zone[ti] == Zone::Stockpile && self.map.can_accept(ti, carry.kind)
        };
        if !still_wanted {
            self.end_job(p);
            return;
        }
        match self.go_to(p, to, to_blueprint, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let used = if to_blueprint {
                    let need = self.blueprint_need(to).map_or(0, |n| n.count);
                    let used = need.min(carry.count);
                    if let Feature::Blueprint { kind, delivered } = self.map.feature[ti] {
                        self.map.feature[ti] = Feature::Blueprint { kind, delivered: delivered + used };
                    }
                    used
                } else {
                    let have = self.map.items[ti].map_or(0, |s| s.count);
                    let put = (STACK_MAX - have).min(carry.count);
                    self.map.items[ti] = Some(Stack { kind: carry.kind, count: have + put });
                    put
                };
                p.carrying = (carry.count > used).then(|| Stack { kind: carry.kind, count: carry.count - used });
                // Drops any leftovers where we stand.
                self.end_job(p);
            }
        }
    }

    fn run_eat(&mut self, p: &mut Pawn, from: Pos, picked: bool, dt: f32) {
        if picked {
            p.job_progress += dt / 2.0;
            if p.job_progress >= 1.0 {
                let nutrition = p.carrying.take().map_or(0.0, |s| s.kind.def().nutrition);
                p.hunger = (p.hunger + nutrition).min(100.0);
                self.end_job(p);
            }
            return;
        }
        match self.go_to(p, from, false, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let i = self.map.idx(from);
                match self.take_one(i, Defs::is_food) {
                    Some(s) => {
                        // Held while eating; dropped again if interrupted.
                        p.carrying = Some(s);
                        self.res_item[i] = 0;
                        p.job = Some(Job::Eat { from, picked: true });
                        p.job_progress = 0.0;
                    }
                    None => self.end_job(p),
                }
            }
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
        let rate = if bed.is_some() { 1.2 } else { 0.8 };
        p.rest = (p.rest + rate * dt).min(100.0);
        if p.rest >= 100.0 {
            let memory = match bed {
                Some(b) if self.map.roofed(b) => Memory::SleptInBedroom,
                Some(_) => Memory::SleptOutside,
                None => Memory::SleptOnGround,
            };
            p.add_memory(memory);
            self.end_job(p);
        }
    }

    /// Start eating the nearest reachable food. Returns false if there is none.
    pub(crate) fn try_eat(&mut self, p: &mut Pawn) -> bool {
        match self.nearest_free_item(p.pos, Defs::is_food) {
            Some(from) => {
                self.end_job(p);
                self.start_job(p, Job::Eat { from, picked: false });
                true
            }
            None => false,
        }
    }

    /// The pawn's own bed, or the nearest unclaimed one.
    pub(crate) fn find_bed(&self, p: &Pawn) -> Option<Pos> {
        if let Some(b) = p.bed {
            if self.map.feature[self.map.idx(b)].is_bed() {
                return Some(b);
            }
        }
        // Reachable cells come nearest first, so the first free bed wins.
        let df = bfs(&self.map, p.pos);
        df.order.iter().map(|&i| self.map.pos(i)).find(|&pos| {
            self.map.feature[self.map.idx(pos)].is_bed()
                && !self.pawns.iter().any(|o| o.id != p.id && !o.dead && o.bed == Some(pos))
        })
    }

    pub(crate) fn start_wander(&mut self, p: &mut Pawn, radius: i32) {
        let df = bfs(&self.map, p.pos);
        let options: Vec<usize> =
            df.order.iter().copied().filter(|&i| df.dist_i(i).is_some_and(|d| d > 0 && d <= radius)).collect();
        if options.is_empty() {
            return;
        }
        let to = self.map.pos(options[self.rng.below(options.len())]);
        self.start_job(p, Job::Wander { to });
    }
}
