//! Finding work for colonists and carrying jobs out step by step.

use crate::map::{Block, Desig, Feature, ItemKind, Map, Pos, Stack, Zone, STACK_MAX};
use crate::pathing::{bfs, DistField};
use crate::pawn::{Job, Memory, Pawn, Task, WorkType};
use crate::world::{Step, World};

type Nearest = [Option<(i32, Pos)>; ItemKind::ALL.len()];

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

impl World {
    /// Highest-priority, then nearest, available job for `p`.
    pub(crate) fn find_work(&self, p: &Pawn) -> Option<Job> {
        let df = bfs(&self.map, p.pos);
        let m = &self.map;
        let candidates = reachable_and_around(m, &df);

        // Nearest free stack of each item kind (for deliveries) and nearest
        // stockpile cell that can take each kind (for hauling).
        let mut nearest_item: Nearest = [None; ItemKind::ALL.len()];
        let mut nearest_cell: Nearest = [None; ItemKind::ALL.len()];
        for &i in &df.order {
            let pos = m.pos(i);
            let Some(d) = df.dist(pos) else { continue };
            if let Some(s) = m.items[i] {
                let k = s.kind as usize;
                if self.res_item[i] == 0 && nearest_item[k].map_or(true, |(bd, _)| d < bd) {
                    nearest_item[k] = Some((d, pos));
                }
            }
            if m.zone[i] == Zone::Stockpile && self.res_cell[i] == 0 {
                for kind in ItemKind::ALL {
                    let k = kind as usize;
                    if m.can_accept(i, kind) && nearest_cell[k].map_or(true, |(bd, _)| d < bd) {
                        nearest_cell[k] = Some((d, pos));
                    }
                }
            }
        }

        for prio in 1..=4u8 {
            let mut best: Option<(i32, Job)> = None;
            for wt in WorkType::ALL {
                if p.priorities[wt as usize] != prio {
                    continue;
                }
                if wt == WorkType::Doctor {
                    let medicine = nearest_item[ItemKind::Medicine as usize].map(|(_, pos)| pos);
                    let food = nearest_item[ItemKind::Food as usize].map(|(_, pos)| pos);
                    if let Some((d, job)) = self.doctor_candidate(p, &df, medicine, food) {
                        if best.map_or(true, |(bd, _)| d < bd) {
                            best = Some((d, job));
                        }
                    }
                    continue;
                }
                for &i in &candidates {
                    if let Some((d, job)) = self.work_candidate(wt, i, &df, &nearest_item, &nearest_cell) {
                        if best.map_or(true, |(bd, _)| d < bd) {
                            best = Some((d, job));
                        }
                    }
                }
            }
            if let Some((_, job)) = best {
                return Some(job);
            }
        }
        None
    }

    fn work_candidate(
        &self,
        wt: WorkType,
        i: usize,
        df: &DistField,
        nearest_item: &Nearest,
        nearest_cell: &Nearest,
    ) -> Option<(i32, Job)> {
        let m = &self.map;
        let pos = m.pos(i);
        let free = self.res_target[i] == 0;
        let f = m.feature[i];
        let work = |task: Task, adjacent: bool| {
            df.approach(m, pos, adjacent).map(|(_, d)| (d, Job::Work { target: pos, task }))
        };
        match wt {
            WorkType::PlantCutting => {
                if free && m.desig[i] == Some(Desig::Chop) && f == Feature::Tree {
                    return work(Task::Chop, true);
                }
            }
            WorkType::Mining => {
                if free && m.desig[i] == Some(Desig::Mine) && m.mineable(m.pos(i)) {
                    return work(Task::Mine, true);
                }
            }
            WorkType::Growing => {
                if !free {
                    return None;
                }
                if m.desig[i] == Some(Desig::Forage) && f == Feature::Bush {
                    return work(Task::Forage, true);
                }
                if let Feature::Crop { growth } = f {
                    if growth >= 1.0 {
                        return work(Task::HarvestCrop, false);
                    }
                }
                if plantable(m, i) {
                    return work(Task::Plant, false);
                }
            }
            WorkType::Construction => {
                if !free {
                    return None;
                }
                if let Feature::Blueprint { kind, delivered } = f {
                    let cost = kind.cost();
                    if delivered >= cost.count {
                        return work(Task::Construct, true);
                    }
                    let (_, d) = df.approach(m, pos, true)?;
                    let (di, from) = nearest_item[cost.kind as usize]?;
                    return Some((d + di, Job::Haul { from, to: pos, to_blueprint: true, picked: false }));
                }
            }
            // Handled per patient in `doctor_candidate`.
            WorkType::Doctor => {}
            WorkType::Hauling => {
                let s = m.items[i]?;
                if self.res_item[i] == 0 && m.zone[i] != Zone::Stockpile {
                    let d = df.dist(pos)?;
                    let (_, to) = nearest_cell[s.kind as usize]?;
                    return Some((d, Job::Haul { from: pos, to, to_blueprint: false, picked: false }));
                }
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
        let adjacent = !matches!(task, Task::Plant | Task::HarvestCrop);
        match self.go_to(p, target, adjacent, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let skill = task.skill();
                p.job_progress += dt * p.work_speed(skill) / self.work_time(target, task);
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
            Task::Chop => d == Some(Desig::Chop) && f == Feature::Tree,
            Task::Mine => d == Some(Desig::Mine) && m.mineable(target),
            Task::Forage => d == Some(Desig::Forage) && f == Feature::Bush,
            Task::Plant => plantable(m, i),
            Task::HarvestCrop => matches!(f, Feature::Crop { growth } if growth >= 1.0),
            Task::Construct => {
                matches!(f, Feature::Blueprint { kind, delivered } if delivered >= kind.cost().count)
            }
        }
    }

    fn work_time(&self, target: Pos, task: Task) -> f32 {
        match (task, self.map.feature[self.map.idx(target)]) {
            (Task::Construct, Feature::Blueprint { kind, .. }) => kind.work_time(),
            _ => task.work_time(),
        }
    }

    fn complete_work(&mut self, target: Pos, task: Task) {
        let i = self.map.idx(target);
        let f = self.map.feature[i];
        self.map.desig[i] = None;
        let drop = |kind: ItemKind, count: u32| Stack { kind, count };
        match task {
            Task::Chop => {
                self.map.set_feature(i, Feature::None);
                self.map.drop_item(target, drop(ItemKind::Wood, 10));
            }
            Task::Mine => {
                let refund = match f.built_from() {
                    Some(kind) => {
                        let c = kind.cost();
                        drop(c.kind, c.count / 2)
                    }
                    None => drop(ItemKind::Stone, 6),
                };
                let dug = self.map.block[i];
                if dug.solid() {
                    // Dug out: rock or soil becomes open space (a tunnel if there's more above).
                    self.map.set_block(i, Block::Air);
                    if dug == Block::Soil {
                        return;
                    }
                } else if f.built_from().is_some() {
                    self.map.set_feature(i, Feature::None);
                } else if self.map.floor[i] {
                    self.map.set_floor(i, false);
                    self.map.drop_item(self.map.landing(target), drop(ItemKind::Wood, 1));
                    return;
                }
                if refund.count > 0 {
                    self.map.drop_item(target, refund);
                }
            }
            Task::Forage => {
                self.map.set_feature(i, Feature::None);
                self.map.drop_item(target, drop(ItemKind::Food, 4));
            }
            Task::Plant => self.map.set_feature(i, Feature::Crop { growth: 0.0 }),
            Task::HarvestCrop => {
                self.map.set_feature(i, Feature::None);
                self.map.drop_item(target, drop(ItemKind::Food, 6));
            }
            Task::Construct => {
                if let Feature::Blueprint { kind, .. } = f {
                    match kind.built() {
                        Some(Feature::Stairs { .. }) => {
                            let dir = self.map.stairs_facing(target);
                            self.map.set_feature(i, Feature::Stairs { dir });
                        }
                        Some(built) => self.map.set_feature(i, built),
                        None => {
                            self.map.set_feature(i, Feature::None);
                            self.map.set_floor(i, true);
                        }
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
                let cost = kind.cost();
                (delivered < cost.count).then(|| Stack { kind: cost.kind, count: cost.count - delivered })
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
                p.hunger = (p.hunger + 45.0).min(100.0);
                self.end_job(p);
            }
            return;
        }
        match self.go_to(p, from, false, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let i = self.map.idx(from);
                match self.map.take_item(i, ItemKind::Food, 1) {
                    Some(_) => {
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
            if self.map.feature[self.map.idx(b)] != Feature::Bed {
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
        match self.nearest_free_item(p.pos, ItemKind::Food) {
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
            if self.map.feature[self.map.idx(b)] == Feature::Bed {
                return Some(b);
            }
        }
        // Reachable cells come nearest first, so the first free bed wins.
        let df = bfs(&self.map, p.pos);
        df.order.iter().map(|&i| self.map.pos(i)).find(|&pos| {
            self.map.feature[self.map.idx(pos)] == Feature::Bed
                && !self.pawns.iter().any(|o| o.id != p.id && !o.dead && o.bed == Some(pos))
        })
    }

    pub(crate) fn start_wander(&mut self, p: &mut Pawn, radius: i32) {
        let df = bfs(&self.map, p.pos);
        let options: Vec<usize> = (0..df.cost.len()).filter(|&i| df.cost[i] > 0 && df.cost[i] <= radius).collect();
        if options.is_empty() {
            return;
        }
        let to = self.map.pos(options[self.rng.below(options.len())]);
        self.start_job(p, Job::Wander { to });
    }
}
