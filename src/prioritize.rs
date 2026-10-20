//! Prioritizing single jobs: with a colonist selected, the player
//! right-clicks a cell and picks from what that colonist could do there
//! (`World::job_options`): build this grave, tend this colonist, haul this
//! stack. `World::prioritize` has them drop what they're doing and do it,
//! whatever their work priorities say, taking it over from anyone who had
//! it. They stick with that cell (`Pawn::forced`) until there's nothing
//! more to do there: materials delivered, then the building built.

use crate::map::{Feature, Pos};
use crate::pawn::{Job, Pawn, PawnId, Task, WorkType};
use crate::world::World;

impl World {
    /// What colonist `id` could do at cell `at`, with a label for each.
    pub fn job_options(&self, id: PawnId, at: Pos) -> Vec<(String, Job)> {
        self.pawn(id).map_or(Vec::new(), |p| self.jobs_at(p, at))
    }

    pub(crate) fn jobs_at(&self, p: &Pawn, at: Pos) -> Vec<(String, Job)> {
        if !p.is_colonist() || !p.active() || p.break_timer > 0.0 || self.battle.is_some() {
            return Vec::new();
        }
        let m = &self.map;
        let i = m.idx(at);
        let scan = self.scan(p.pos);
        let mut jobs: Vec<Job> = Vec::new();

        // Gear lying here.
        if self.res_item[i].is_none_or(|holder| holder == p.id) && m.stacks(i).any(|s| s.kind.def().is_gear()) {
            jobs.push(Job::Equip { from: at, chosen: true, sidearm: false });
            let sidearm = self.first_gear(at).filter(|k| k.def().weapon.as_ref().is_some_and(|w| w.sidearm));
            if sidearm.is_some_and(|k| crate::squads::sidearm_fits_squad(p, k)) {
                jobs.push(Job::Equip { from: at, chosen: true, sidearm: true });
            }
        }
        // Work of every type, whether or not it's switched on for them.
        for wt in WorkType::ALL.into_iter().filter(|&wt| wt != WorkType::Doctor) {
            if let Some((_, job)) = self.work_candidate(wt, i, &scan.df, &scan.nearest_item, &scan.nearest_cell, true) {
                jobs.push(job);
            }
        }
        // Colonists here who need a doctor, or an operation.
        let (medicine, food) = scan.medicine_and_food();
        for o in self.pawns.iter().filter(|o| o.is_living_colonist() && o.pos == at) {
            if o.id == p.id {
                if o.health.needs_tending() {
                    jobs.push(self.self_tend_job(p));
                }
                continue;
            }
            let Some((_, d)) = scan.df.approach(m, o.pos, true) else { continue };
            if let Some((_, job)) = self.doctor_job(o, d, medicine, food) {
                jobs.push(job);
            }
            if matches!(o.job, Some(Job::Operation { table }) if o.pos == table && o.lying()) {
                if let Some(job) = self.surgery_job(o, &scan.df) {
                    jobs.push(job);
                }
            }
        }
        // Burials: a body lying here into the nearest empty grave, or the nearest body into a grave here.
        let reachable = |pos: Pos, beside: bool| scan.df.approach(m, pos, beside).map(|(_, d)| d);
        let graves: Vec<(i32, Pos)> =
            self.empty_graves().into_iter().filter_map(|g| Some((reachable(g, false)?, g))).collect();
        for o in self.pawns.iter().filter(|o| self.unburied(o) && reachable(o.pos, true).is_some()) {
            if o.pos == at {
                if let Some(&(_, grave)) = graves.iter().min_by_key(|g| g.0) {
                    jobs.push(Job::Bury { corpse: o.id, grave, carrying: false });
                }
            }
        }
        if graves.iter().any(|g| g.1 == at) {
            let nearest = self
                .pawns
                .iter()
                .filter(|o| self.unburied(o))
                .filter_map(|o| Some((reachable(o.pos, true)?, o.id)))
                .min_by_key(|c| c.0);
            if let Some((_, corpse)) = nearest {
                jobs.push(Job::Bury { corpse, grave: at, carrying: false });
            }
        }
        jobs.dedup();
        jobs.into_iter().map(|job| (self.job_label(job), job)).collect()
    }

    /// "Prioritize building Grave", "Prioritize tending Ada", "Equip Rifle".
    pub fn job_label(&self, job: Job) -> String {
        let at = |p: Pos| self.map.feature[self.map.idx(p)];
        let thing = |p: Pos| match at(p) {
            Feature::None => self.map.block[self.map.idx(p)].name().to_string(),
            Feature::Blueprint { kind, .. } => kind.name().to_string(),
            f => f.name().to_string(),
        };
        let who = |id: PawnId| self.pawn(id).map_or("someone".to_string(), |o| o.name.clone());
        let item = |p: Pos| self.map.stacks(self.map.idx(p)).next().map_or("it".to_string(), |s| s.kind.name().to_lowercase());
        let gear = |p: Pos| {
            let first = self.map.stacks(self.map.idx(p)).find(|s| s.kind.def().is_gear());
            first.map_or("it".to_string(), |s| s.kind.name().to_lowercase())
        };
        // What a delivery brings: what's needed there (a shelf may hold other things too).
        let needed = |from: Pos, to: Pos| self.delivery_need(to).map_or_else(|| item(from), |n| n.kind.name().to_lowercase());
        match job {
            Job::Equip { from, sidearm: true, .. } => format!("Carry {} as sidearm", gear(from)),
            Job::Equip { from, .. } => format!("Equip {}", gear(from)),
            Job::Work { task: Task::Sow(kind), .. } => format!("Prioritize planting {}", kind.def().name.to_lowercase()),
            Job::Work { target, task: Task::Craft } => match self.bench_at(target).and_then(|b| b.current) {
                Some(r) => format!("Prioritize {} at {}", r.def().name.to_lowercase(), thing(target)),
                None => format!("Prioritize crafting at {}", thing(target)),
            },
            Job::Work { target, task } => format!("Prioritize {} {}", task.verb(), thing(target)),
            // Ingredients for a bill: the bill is what's being prioritized.
            Job::Haul { from, to, deliver: true, .. } if matches!(at(to), Feature::Built { .. }) => {
                match self.bench_at(to).and_then(|b| b.current) {
                    Some(r) => format!("Prioritize {} at {} (bringing {} first)", r.def().name.to_lowercase(), thing(to), needed(from, to)),
                    None => format!("Prioritize delivering {} to {}", item(from), thing(to)),
                }
            }
            Job::Haul { from, to, deliver: true, .. } => format!("Prioritize delivering {} to {}", needed(from, to), thing(to)),
            Job::Haul { from, .. } => format!("Prioritize hauling {}", item(from)),
            Job::Tend { patient, .. } => format!("Prioritize tending {}", who(patient)),
            Job::Rescue { patient, .. } => format!("Prioritize rescuing {}", who(patient)),
            Job::Feed { patient, .. } => format!("Prioritize feeding {}", who(patient)),
            Job::Surgery { patient, .. } => format!("Prioritize operating on {}", who(patient)),
            Job::Bury { corpse, .. } => format!("Prioritize burying {}", who(corpse)),
            other => format!("Prioritize {}", other.describe()),
        }
    }

    /// Have colonist `id` do `job` (one of `job_options(id, at)`) now,
    /// taking it over from whoever had it. Returns false if it isn't on offer.
    pub fn prioritize(&mut self, id: PawnId, at: Pos, job: Job) -> bool {
        let Some(p) = self.pawn(id) else { return false };
        if !self.jobs_at(p, at).iter().any(|(_, j)| *j == job) {
            return false;
        }
        self.take_over(id, job);
        let keep = !matches!(job, Job::Equip { .. });
        self.with_pawn(id, |w, p| {
            w.end_job(p);
            w.start_job(p, job);
            p.forced = keep.then_some(at);
        });
        true
    }

    /// Anyone but `id` holding what `job` needs (its target, or its patient)
    /// lets go of it.
    pub(crate) fn take_over(&mut self, id: PawnId, job: Job) {
        let cell = match job {
            Job::Work { target, .. } => Some(target),
            Job::Haul { to, deliver: true, .. } => Some(to),
            Job::Bury { grave, .. } => Some(grave),
            _ => None,
        };
        let patient = match job {
            Job::Tend { patient, .. }
            | Job::Rescue { patient, .. }
            | Job::Feed { patient, .. }
            | Job::Surgery { patient, .. } => Some(patient),
            Job::Bury { corpse, .. } => Some(corpse),
            _ => None,
        };
        let mut holders: Vec<PawnId> = Vec::new();
        if let Some(c) = cell {
            holders.extend(self.res_target[self.map.idx(c)]);
        }
        if let Some(pt) = patient {
            holders.extend(self.res_patient.iter().filter(|r| r.0 == pt).map(|r| r.1));
        }
        holders.retain(|&h| h != id);
        holders.dedup();
        for h in holders {
            self.with_pawn(h, |w, o| w.end_job(o));
        }
    }

    /// For a colonist who was given a cell to work on: the next thing to do
    /// there, if any (otherwise they're done with it).
    pub(crate) fn forced_job(&mut self, p: &mut Pawn) -> Option<Job> {
        let at = p.forced?;
        let next = self.jobs_at(p, at).into_iter().map(|(_, j)| j).find(|j| !matches!(j, Job::Equip { .. }));
        if next.is_none() {
            p.forced = None;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{content_colonist as colonist, quiet_world, run};
    use crate::defs::{BuildId, ItemId};
    use crate::map::{Stack, FLAT_Z as Z};
    use crate::rng::Rng;

    fn test_world() -> World {
        quiet_world(24, 24, 4)
    }



    #[test]
    fn a_prioritized_grave_gets_dug_first() {
        let mut w = test_world();
        let id = colonist(&mut w, (2, 2, Z));
        // Construction off: left alone, they'd never dig it.
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].priorities[WorkType::Construction as usize] = 0;
        let grave = BuildId::named("grave");
        let (near, far) = ((4, 4, Z), (16, 16, Z));
        for at in [near, far] {
            let k = w.map.idx(at);
            w.map.feature[k] = Feature::blueprint(grave, grave.def().total_cost(), 0);
        }
        let options = w.job_options(id, far);
        let (label, job) = options.iter().find(|(_, j)| matches!(j, Job::Work { .. })).expect("dig it").clone();
        assert_eq!(label, "Prioritize building Grave");
        assert!(w.prioritize(id, far, job));
        assert_eq!(w.pawn(id).unwrap().forced, Some(far));
        assert!(w.pawn(id).unwrap().status().contains("prioritized"));
        // A long walk (about 20 s) and about 10 s of digging.
        run(&mut w, 60.0);
        assert!(w.map.feature[w.map.idx(far)].is_special(crate::defs::Special::Grave), "the far one is dug");
        assert!(matches!(w.map.feature[w.map.idx(near)], Feature::Blueprint { .. }), "the near one isn't");
        assert_eq!(w.pawn(id).unwrap().forced, None, "done with it");
    }

    #[test]
    fn they_deliver_then_build() {
        let mut w = test_world();
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].priorities = [0; WorkType::ALL.len()];
        let at = (12, 12, Z);
        let k = w.map.idx(at);
        let table = BuildId::named("table");
        w.map.feature[k] = Feature::blueprint(table, 0, 0);
        w.map.drop_item((3, 3, Z), Stack { kind: table.def().cost[0].item, count: 20 });
        let (label, job) = w.job_options(id, at).into_iter().next().expect("deliver");
        assert!(label.starts_with("Prioritize delivering"), "{}", label);
        assert!(w.prioritize(id, at, job));
        run(&mut w, 60.0);
        assert_eq!(w.map.feature[k], Feature::built("table"), "delivered, then built");
    }

    #[test]
    fn a_doctor_can_be_sent_to_one_patient() {
        let mut w = test_world();
        let doctor = colonist(&mut w, (2, 2, Z));
        let (a, b) = (colonist(&mut w, (5, 5, Z)), colonist(&mut w, (15, 15, Z)));
        let mut rng = Rng::new(2);
        for id in [a, b] {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].health.injure(crate::health::Part::LeftArm, 6.0, crate::health::DamageKind::Cut, &mut rng);
            w.pawns[i].resting = true;
            w.pawns[i].job = Some(Job::Patient { bed: None });
        }
        let at = w.pawn(b).unwrap().pos;
        let (label, job) = w.job_options(doctor, at).into_iter().find(|(_, j)| matches!(j, Job::Tend { .. })).expect("tend");
        assert_eq!(label, format!("Prioritize tending {}", w.pawn(b).unwrap().name));
        assert!(w.prioritize(doctor, at, job));
        assert!(matches!(w.pawn(doctor).unwrap().job, Some(Job::Tend { patient, .. }) if patient == b));
        assert!(!w.prioritize(doctor, at, Job::Tend { patient: a, medicine: None, picked: false }), "not on offer there");
    }

    #[test]
    fn gear_can_still_be_equipped() {
        let mut w = test_world();
        let id = colonist(&mut w, (2, 2, Z));
        w.map.drop_item((6, 6, Z), Stack { kind: ItemId::named("rifle"), count: 1 });
        let options = w.job_options(id, (6, 6, Z));
        assert_eq!(options[0].0, "Equip rifle");
        assert!(w.prioritize(id, (6, 6, Z), options[0].1));
        assert_eq!(w.pawn(id).unwrap().forced, None, "a one-off");
    }
}
