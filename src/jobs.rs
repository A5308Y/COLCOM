//! Finding work for colonists and carrying jobs out step by step.

use crate::defs::{defs, Defs, ItemId, PlantId, Special};
use crate::map::{Block, Desig, Feature, Map, Pos, Stack, STACK_MAX};
use crate::pathing::{bfs, DistField};
use crate::pawn::{Job, Memory, Pawn, Skill, Task, WorkType};
use crate::world::{Step, World};

/// Base seconds to mine out one cell.
const MINE_TIME: f32 = 5.0;
/// Deconstructing takes this share of the building's work time.
const DECONSTRUCT_SHARE: f32 = 0.5;

/// Nearest (distance, cell) per item kind.
type Nearest = Vec<Option<(i32, Pos)>>;
/// Per item kind, the nearest stockpile cell that takes it, per stockpile priority (1..=4).
type NearestStore = Vec<[Option<(i32, Pos)>; 5]>;

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
        Task::Mine | Task::Construct | Task::Deconstruct | Task::Craft => true,
    }
}

impl World {
    /// Highest-priority, then nearest, available job for `p`.
    pub(crate) fn find_work(&self, p: &Pawn) -> Option<Job> {
        let df = bfs(&self.map, p.pos);
        let m = &self.map;
        let candidates = reachable_and_around(m, &df);

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
            if let Some(s) = m.items[i] {
                let k = s.kind.index();
                if self.res_item[i] == 0 && nearest_item[k].map_or(true, |(bd, _)| d < bd) {
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

        let nearest_of = |is: &dyn Fn(ItemId) -> bool| {
            ItemId::all()
                .filter(|&k| is(k))
                .filter_map(|k| nearest_item[k.index()])
                .min_by_key(|(d, _)| *d)
                .map(|(_, pos)| pos)
        };

        // Better gear first: it's quick.
        if p.is_colonist() {
            if let Some(job) = self.equip_candidate(p, &nearest_item) {
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
                let medicine = nearest_of(&|k| defs().is_medicine(k));
                // Cooked food if there is any.
                let food = nearest_of(&|k| defs().is_food(k) && !k.def().raw).or(nearest_of(&|k| defs().is_food(k)));
                self.doctor_candidate(p, &df, medicine, food)
            } else {
                let cells = candidates
                    .iter()
                    .filter_map(|&i| self.work_candidate(wt, i, &df, &nearest_item, &nearest_cell))
                    .min_by_key(|(d, _)| *d);
                // Haulers also bury the dead.
                let burial = if wt == WorkType::Hauling { self.bury_candidate(&df) } else { None };
                cells.into_iter().chain(burial).min_by_key(|(d, _)| *d)
            };
            if let Some((d, job)) = found {
                per_type.push((prio, d, job));
            }
        }
        per_type.into_iter().min_by_key(|(prio, d, _)| (*prio, *d)).map(|(_, _, job)| job)
    }

    fn work_candidate(
        &self,
        wt: WorkType,
        i: usize,
        df: &DistField,
        nearest_item: &Nearest,
        nearest_cell: &NearestStore,
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
                if let Some(s) = m.items[i].filter(|_| self.res_item[i] == 0) {
                    // Into a stockpile that takes it, preferring higher priorities
                    // (and only to better ones if it's already stored).
                    let here = self.storage_priority(i, s.kind).unwrap_or(0) as usize;
                    let d = df.dist(pos)?;
                    let to = (here + 1..=4).rev().find_map(|prio| nearest_cell[s.kind.index()][prio]);
                    return to.map(|(_, to)| (d, Job::Haul { from: pos, to, deliver: false, picked: false }));
                }
                // Ingredients to a workbench.
                if self.res_target[i] != 0 || !matches!(m.feature[i], Feature::Built { .. }) {
                    return None;
                }
                let need = self.bench_at(pos)?.need()?;
                let (_, d) = df.approach(m, pos, true)?;
                let (di, from) = nearest_item[need.kind.index()]?;
                return Some((d + di, Job::Haul { from, to: pos, deliver: true, picked: false }));
            }
            _ => {}
        }
        if self.res_target[i] != 0 {
            return None;
        }
        if wt == WorkType::Mining && m.desig[i] == Some(Desig::Mine) && m.mineable(pos) {
            return work(Task::Mine);
        }
        if wt == WorkType::Construction && m.desig[i] == Some(Desig::Deconstruct) && m.deconstructable(pos) {
            return work(Task::Deconstruct);
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
                let (di, from) = nearest_item[def.cost.item.index()]?;
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
                if bench.ready() && bench.current.is_some_and(|r| r.def().work == wt) {
                    return work(Task::Craft);
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
                // A themed room speeds work and learning at its workbenches.
                let (work, learn) = match task {
                    Task::Craft => self.room_effects(target).map_or((1.0, 1.0), |(_, e)| (e.work, e.learn)),
                    _ => (1.0, 1.0),
                };
                p.job_progress += dt * work * p.work_speed(skill) / time;
                if p.learn(skill, dt * learn) {
                    let msg = format!("{} reached {} level {}.", p.name, skill.name(), p.skill(skill));
                    self.sound("level_up", None);
                    self.push_log(msg);
                }
                if p.job_progress >= 1.0 {
                    self.complete_work(target, task, &p.name);
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
            Task::Deconstruct => d == Some(Desig::Deconstruct) && m.deconstructable(target),
            Task::Sow(_) => plantable(m, i),
            Task::Construct => {
                matches!(f, Feature::Blueprint { kind, delivered } if delivered >= kind.def().cost.count)
            }
            Task::Craft => self.bench_at(target).is_some_and(|b| b.ready()),
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
                Feature::Blueprint { kind, .. } => Some((kind.def().skill, kind.def().work_time)),
                _ => None,
            },
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
        let drop = |m: &mut Map, at: Pos, (kind, count): (ItemId, u32)| {
            if count > 0 {
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
                drop(&mut self.map, target, (y.item, y.count));
            }
            Task::Sow(kind) => self.map.set_feature(i, Feature::Plant { kind, growth: 0.0 }),
            Task::Mine => {
                let dug = self.map.block[i];
                if dug.solid() {
                    // Dug out: rock or soil becomes open space (a tunnel if there's more above).
                    self.map.set_block(i, Block::Air);
                    self.sound(if dug == Block::Soil { "dig" } else { "rock_break" }, Some(target));
                    let yields = match dug {
                        Block::Rock => Some(&defs().rock_drops),
                        Block::Ore => Some(&defs().ore_drops),
                        _ => None,
                    };
                    if let Some(r) = yields {
                        drop(&mut self.map, target, (r.item, r.count));
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
            Task::Construct => {
                if let Feature::Blueprint { kind, .. } = f {
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
                            let dir = self.map.stairs_facing(target);
                            self.map.set_feature(i, Feature::Built { kind, dir });
                        }
                        _ => self.map.set_feature(i, Feature::Built { kind, dir: 0 }),
                    }
                    self.sound("built", Some(target));
                    // Shove anything lying here out of the way.
                    if let Some(s) = self.map.items[i].take() {
                        self.map.drop_item(target, s);
                    }
                    if kind.def().storage.is_some() {
                        self.sync_containers();
                    }
                }
            }
        }
    }

    /// Materials a blueprint or workbench still needs.
    fn delivery_need(&self, at: Pos) -> Option<Stack> {
        match self.map.feature[self.map.idx(at)] {
            Feature::Blueprint { kind, delivered } => {
                let cost = &kind.def().cost;
                (delivered < cost.count).then(|| Stack { kind: cost.item, count: cost.count - delivered })
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
                    let Some(kind) = self.map.items[fi].map(|s| s.kind) else {
                        self.end_job(p);
                        return;
                    };
                    let want = if deliver {
                        match self.delivery_need(to) {
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
                            p.job = Some(Job::Haul { from, to, deliver, picked: true });
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
        let still_wanted = if deliver {
            self.delivery_need(to).map_or(false, |n| n.kind == carry.kind)
        } else {
            self.storage_priority(ti, carry.kind).is_some() && self.map.can_accept(ti, carry.kind)
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
                        Feature::Blueprint { kind, delivered } => {
                            let used = self.delivery_need(to).map_or(0, |n| n.count).min(carry.count);
                            self.map.feature[ti] = Feature::Blueprint { kind, delivered: delivered + used };
                            used
                        }
                        _ => self.stock_bench(to, carry),
                    }
                } else {
                    let have = self.map.items[ti].map_or(0, |s| s.count);
                    let put = (self.map.capacity(ti) - have).min(carry.count);
                    self.map.items[ti] = Some(Stack { kind: carry.kind, count: have + put });
                    put
                };
                p.carrying = (carry.count > used).then(|| Stack { kind: carry.kind, count: carry.count - used });
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
                        p.job = Some(Job::Eat { from, picked: true, seat });
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
        let themed = bed.and_then(|b| self.room_effects(b));
        let rate = if bed.is_some() { 1.2 * themed.map_or(1.0, |(_, e)| e.rest) } else { 0.8 };
        p.rest = (p.rest + rate * dt).min(100.0);
        if p.rest >= 100.0 {
            let memory = match bed {
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
        let df = bfs(&self.map, near);
        df.order
            .iter()
            .map(|&i| self.map.pos(i))
            .take_while(|&s| df.dist(s).is_some_and(|d| d <= MAX_WALK))
            .find(|&s| self.is_seat(s) && !taken(s))
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
            if !self.pawn(id).is_some_and(|p| p.is_colonist() && !p.dead) {
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
            df.order.iter().copied().filter(|&i| df.dist_i(i).is_some_and(|d| d > 0 && d <= radius)).collect();
        if options.is_empty() {
            return;
        }
        let to = self.map.pos(options[self.rng.below(options.len())]);
        self.start_job(p, Job::Wander { to });
    }
}
