//! Doctoring: rescuing downed colonists, tending wounds and diseases,
//! feeding bedridden patients, and patients resting in bed.

use crate::defs::{defs, Defs, ItemId};
use crate::map::{Pos, Stack};
use crate::pathing::{bfs, DistField, ANY_DISTANCE};
use crate::pawn::{Job, Memory, Pawn, PawnId, Skill, WorkType};
use crate::jobs::Fetch;
use crate::world::{Step, World, SLEEP_GAIN};

/// Seconds of work for one treatment at speed 1.0.
const TEND_TIME: f32 = 3.0;
/// Conditions treated per tending session.
const TEND_BATCH: usize = 3;

/// The first of `kinds` that isn't raw food, else the first of them.
fn cooked_first(kinds: impl Iterator<Item = ItemId>) -> Option<ItemId> {
    let kinds: Vec<ItemId> = kinds.collect();
    kinds.iter().copied().find(|k| !k.def().raw).or(kinds.first().copied())
}

impl World {
    pub(crate) fn in_bed(&self, p: &Pawn) -> bool {
        self.map.feature[self.map.idx(p.pos)].is_bed()
    }

    pub(crate) fn patient_reserved(&self, patient: PawnId) -> bool {
        self.res_patient.iter().any(|&(pt, _)| pt == patient)
    }

    /// Is anyone other than `id` able to doctor right now?
    pub(crate) fn other_doctor_available(&self, id: PawnId) -> bool {
        self.pawns.iter().any(|o| {
            o.id != id
                && o.is_colonist()
                && o.active()
                && o.break_timer <= 0.0
                && o.priorities[WorkType::Doctor].is_enabled()
        })
    }

    /// Nearest unreserved stack of any item that `is` (food, medicine).
    pub(crate) fn nearest_free_item(&self, from: Pos, is: fn(&Defs, ItemId) -> bool) -> Option<Pos> {
        self.nearest_stack(&bfs(&self.map, from), ANY_DISTANCE, |i, s| self.item_free(i) && is(defs(), s.kind))
    }

    /// Nearest reachable free food, cooked food first.
    pub(crate) fn nearest_food(&self, from: Pos) -> Option<Pos> {
        let df = bfs(&self.map, from);
        let food = |raw_too: bool| {
            self.nearest_stack(&df, ANY_DISTANCE, |i, s| {
                self.item_free(i) && defs().is_food(s.kind) && (raw_too || !s.kind.def().raw)
            })
        };
        food(false).or_else(|| food(true))
    }

    /// `p` eats one `kind`: nutrition, how it felt, and maybe food poisoning.
    pub(crate) fn eat(&mut self, p: &mut Pawn, kind: ItemId) {
        let d = kind.def();
        p.hunger = (p.hunger + d.nutrition).min(100.0);
        p.add_memory(if d.raw { Memory::AteRawFood } else { Memory::AteMeal });
        if self.rng.chance(d.poison) {
            p.health.add_food_poisoning();
            p.health.recompute();
            self.push_log(format!("{} got food poisoning.", p.name));
        }
    }

    /// Take one unit of whatever item in cell `i` passes `is`.
    pub(crate) fn take_one(&mut self, i: usize, is: fn(&Defs, ItemId) -> bool) -> Option<Stack> {
        // Cooked before raw, if it's food (a shelf may hold both).
        let kind = cooked_first(self.map.stacks(i).map(|s| s.kind).filter(|&k| is(defs(), k)))?;
        self.map.take_item(i, kind, 1)
    }

    pub(crate) fn self_tend_job(&self, p: &Pawn) -> Job {
        Job::Tend { patient: p.id, medicine: self.nearest_free_item(p.pos, Defs::is_medicine), picked: false }
    }

    /// Most urgent medical job for doctor `p`: rescue > tend > feed, then nearest.
    pub(crate) fn doctor_candidate(
        &self,
        p: &Pawn,
        df: &DistField,
        medicine: Option<Pos>,
        food: Option<Pos>,
    ) -> Option<(i32, Job)> {
        let mut best: Option<(i32, Job)> = None;
        for o in &self.pawns {
            if !o.is_colonist() || o.is_dead() || o.id == p.id || self.patient_reserved(o.id) {
                continue;
            }
            let Some((_, d)) = df.approach(&self.map, o.pos, true) else { continue };
            if let Some((score, job)) = self.doctor_job(o, d, medicine, food) {
                if best.map_or(true, |(b, _)| score < b) {
                    best = Some((score, job));
                }
            }
        }
        // Operations come after rescuing, tending and feeding.
        best.or_else(|| self.surgery_candidate(p, df))
    }

    /// What colonist `o` (`d` steps away) needs from a doctor, most urgent
    /// first: rescue > tend > feed. Scored so that medical work outranks
    /// other work of the same priority.
    pub(crate) fn doctor_job(&self, o: &Pawn, d: i32, medicine: Option<Pos>, food: Option<Pos>) -> Option<(i32, Job)> {
        let rescue_bed = if o.is_downed() && !self.in_bed(o) { self.find_patient_bed(o) } else { None };
        if let Some(bed) = rescue_bed {
            Some((d - 3000, Job::Rescue { patient: o.id, bed, carrying: false }))
        } else if o.health.needs_tending() && o.lying() {
            Some((d - 2000, Job::Tend { patient: o.id, medicine, picked: false }))
        } else if o.is_downed() && o.hunger < 30.0 {
            food.map(|f| (d - 1000, Job::Feed { patient: o.id, food: f, picked: false }))
        } else {
            None
        }
    }

    /// `medicine` is the potency of the medicine used (0 = none).
    fn tend_quality(&mut self, doctor: &Pawn, medicine: f32, self_tend: bool, in_bed: bool) -> f32 {
        let caps = &doctor.health.caps;
        let skill = doctor.skill(Skill::Medicine) as f32;
        let mut q = (0.25 + 0.04 * skill) * caps.manipulation.min(1.0) * (0.6 + 0.4 * caps.sight.min(1.0));
        q = if medicine > 0.0 { q + 0.25 * medicine } else { q * 0.6 };
        if in_bed {
            q += 0.08;
        }
        q += doctor.tend_bonus();
        if self_tend {
            q *= 0.7;
        }
        (q + self.rng.range_f32(-0.12, 0.12)).clamp(0.05, 1.0)
    }

    pub(crate) fn run_patient(&mut self, p: &mut Pawn, bed: Option<Pos>, dt: f32) {
        if !p.health.wants_bed_rest() {
            self.end_job(p);
            return;
        }
        if let Some(b) = bed {
            if !self.map.feature[self.map.idx(b)].is_bed() {
                p.job = Some(Job::Patient { bed: None });
                return;
            }
            match self.go_to(p, b, false, dt) {
                Step::Moving => return,
                Step::Unreachable => {
                    p.job = Some(Job::Patient { bed: None });
                    return;
                }
                Step::Arrived => {}
            }
        }
        p.resting = true;
        if p.rest < 40.0 {
            p.sleeping = true;
        }
        if p.sleeping {
            p.rest = (p.rest + 1.25 * SLEEP_GAIN * dt).min(100.0);
            if p.rest >= 100.0 {
                p.sleeping = false;
            }
        }
    }

    pub(crate) fn run_tend(&mut self, p: &mut Pawn, patient: PawnId, medicine: Option<Pos>, picked: bool, dt: f32) {
        let self_tend = patient == p.id;
        let target = if self_tend { None } else { self.pawn_index(patient) };
        let (ppos, ready, needs, in_bed) = match target {
            Some(pi) => {
                let o = &self.pawns[pi];
                (o.pos, o.lying(), !o.is_dead() && o.health.needs_tending(), self.in_bed(o))
            }
            None if self_tend => (p.pos, true, p.health.needs_tending(), self.in_bed(p)),
            None => {
                self.end_job(p);
                return;
            }
        };
        if !needs {
            self.end_job(p);
            return;
        }

        // Fetch medicine first, if there is some (without it if it's gone or out of reach).
        if let (Some(m), false) = (medicine, picked) {
            match self.fetch(p, m, dt, |w, i| w.take_one(i, Defs::is_medicine)) {
                Fetch::Walking => {}
                Fetch::Unreachable => p.job = Some(Job::Tend { patient, medicine: None, picked: false }),
                Fetch::Got | Fetch::Gone => p.job = Some(Job::Tend { patient, medicine, picked: true }),
            }
            return;
        }

        if !self_tend {
            if !self.walk_to(p, ppos, true, dt) {
                return;
            }
            if !ready {
                // Wait for the patient to lie down.
                return;
            }
        }

        if !self.work_on(p, Skill::Medicine, TEND_TIME, (1.0, 1.0), dt) {
            return;
        }

        let potency = p.carrying.map_or(0.0, |s| s.kind.def().medicine);
        let has_med = potency > 0.0;
        if has_med {
            p.carrying = None;
        }
        // A hospital or surgery room helps too.
        let room = if in_bed { self.room_effects(ppos).map_or(0.0, |(_, e)| e.tend) } else { 0.0 };
        let quality = (self.tend_quality(p, potency, self_tend, in_bed) + room).min(1.0);
        let who = match target {
            Some(pi) => {
                let o = &mut self.pawns[pi];
                o.health.tend(quality, TEND_BATCH);
                o.health.recompute();
                o.name.clone()
            }
            None => {
                p.health.tend(quality, TEND_BATCH);
                p.health.recompute();
                "themselves".to_string()
            }
        };
        let note = if has_med { "" } else { ", no medicine" };
        self.push_log(format!("{} tended {} ({:.0}% quality{}).", p.name, who, quality * 100.0, note));
        self.end_job(p);
    }

    pub(crate) fn run_rescue(&mut self, p: &mut Pawn, patient: PawnId, bed: Pos, carrying: bool, dt: f32) {
        let Some(pi) = self.pawn_index(patient) else {
            self.end_job(p);
            return;
        };
        let bed_ok = self.map.feature[self.map.idx(bed)].is_bed();
        if self.pawns[pi].is_dead() || !self.pawns[pi].is_downed() || !bed_ok {
            self.end_job(p);
            return;
        }
        if !carrying {
            let ppos = self.pawns[pi].pos;
            if self.walk_to(p, ppos, true, dt) {
                p.job = Some(Job::Rescue { patient, bed, carrying: true });
            }
            return;
        }
        // The patient comes along.
        let there = self.walk_to(p, bed, false, dt);
        self.carry_along(p, pi);
        if there {
            self.pawns[pi].bed = Some(bed);
            let name = self.pawns[pi].name.clone();
            self.push_log(format!("{} carried {} to bed.", p.name, name));
            self.end_job(p);
        }
    }

    pub(crate) fn run_feed(&mut self, p: &mut Pawn, patient: PawnId, food: Pos, picked: bool, dt: f32) {
        let Some(pi) = self.pawn_index(patient) else {
            self.end_job(p);
            return;
        };
        if self.pawns[pi].is_dead() || !self.pawns[pi].is_downed() {
            self.end_job(p);
            return;
        }
        if !picked {
            match self.fetch(p, food, dt, |w, i| w.take_one(i, Defs::is_food)) {
                Fetch::Walking => {}
                Fetch::Got => p.job = Some(Job::Feed { patient, food, picked: true }),
                Fetch::Unreachable | Fetch::Gone => self.end_job(p),
            }
            return;
        }
        let ppos = self.pawns[pi].pos;
        if !self.walk_to(p, ppos, true, dt) {
            return;
        }
        p.job_progress += dt / 2.0;
        if p.job_progress >= 1.0 {
            if let Some(s) = p.carrying.take() {
                let mut o = self.pawns[pi].clone();
                self.eat(&mut o, s.kind);
                self.pawns[pi] = o;
            }
            self.end_job(p);
        }
    }
}
