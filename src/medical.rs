//! Doctoring: rescuing downed colonists, tending wounds and diseases,
//! feeding bedridden patients, and patients resting in bed.

use crate::map::{Feature, ItemKind, Pos, Stack};
use crate::pathing::{bfs, DistField};
use crate::pawn::{Job, Pawn, Skill, WorkType};
use crate::world::{Step, World};

/// Seconds of work for one treatment at speed 1.0.
const TEND_TIME: f32 = 3.0;
/// Conditions treated per tending session.
const TEND_BATCH: usize = 3;

impl World {
    pub(crate) fn in_bed(&self, p: &Pawn) -> bool {
        self.map.feature[self.map.idx(p.pos)] == Feature::Bed
    }

    pub(crate) fn patient_reserved(&self, patient: u32) -> bool {
        self.res_patient.iter().any(|&(pt, _)| pt == patient)
    }

    /// Is anyone other than `id` able to doctor right now?
    pub(crate) fn other_doctor_available(&self, id: u32) -> bool {
        self.pawns.iter().any(|o| {
            o.id != id
                && o.is_colonist()
                && o.active()
                && !o.drafted
                && o.break_timer <= 0.0
                && o.priorities[WorkType::Doctor as usize] > 0
        })
    }

    pub(crate) fn nearest_free_item(&self, from: Pos, kind: ItemKind) -> Option<Pos> {
        let df = bfs(&self.map, from);
        let mut best: Option<(i32, Pos)> = None;
        for i in 0..self.map.items.len() {
            let Some(s) = self.map.items[i] else { continue };
            if s.kind != kind || self.res_item[i] != 0 {
                continue;
            }
            let pos = self.map.pos(i);
            if let Some(d) = df.dist(pos) {
                if best.map_or(true, |(bd, _)| d < bd) {
                    best = Some((d, pos));
                }
            }
        }
        best.map(|(_, p)| p)
    }

    pub(crate) fn self_tend_job(&self, p: &Pawn) -> Job {
        Job::Tend { patient: p.id, medicine: self.nearest_free_item(p.pos, ItemKind::Medicine), picked: false }
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
            if !o.is_colonist() || o.dead || o.id == p.id || self.patient_reserved(o.id) {
                continue;
            }
            let Some((_, d)) = df.approach(&self.map, o.pos, true) else { continue };
            let rescue_bed = if o.downed && !self.in_bed(o) { self.find_bed(o) } else { None };
            // Medical work outranks other work of the same priority.
            let candidate = if let Some(bed) = rescue_bed {
                Some((d - 3000, Job::Rescue { patient: o.id, bed, carrying: false }))
            } else if o.health.needs_tending() && o.lying() {
                Some((d - 2000, Job::Tend { patient: o.id, medicine, picked: false }))
            } else if o.downed && o.hunger < 30.0 {
                food.map(|f| (d - 1000, Job::Feed { patient: o.id, food: f, picked: false }))
            } else {
                None
            };
            if let Some((score, job)) = candidate {
                if best.map_or(true, |(b, _)| score < b) {
                    best = Some((score, job));
                }
            }
        }
        best
    }

    fn tend_quality(&mut self, doctor: &Pawn, medicine: bool, self_tend: bool, in_bed: bool) -> f32 {
        let caps = &doctor.health.caps;
        let skill = doctor.skill(Skill::Medicine) as f32;
        let mut q = (0.25 + 0.04 * skill) * caps.manipulation.min(1.0) * (0.6 + 0.4 * caps.sight.min(1.0));
        q = if medicine { q + 0.25 } else { q * 0.6 };
        if in_bed {
            q += 0.08;
        }
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
            if self.map.feature[self.map.idx(b)] != Feature::Bed {
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
            p.rest = (p.rest + dt).min(100.0);
            if p.rest >= 100.0 {
                p.sleeping = false;
            }
        }
    }

    pub(crate) fn run_tend(&mut self, p: &mut Pawn, patient: u32, medicine: Option<Pos>, picked: bool, dt: f32) {
        let self_tend = patient == p.id;
        let target = if self_tend { None } else { self.pawn_index(patient) };
        let (ppos, ready, needs, in_bed) = match target {
            Some(pi) => {
                let o = &self.pawns[pi];
                (o.pos, o.lying(), !o.dead && o.health.needs_tending(), self.in_bed(o))
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

        // Fetch medicine first, if there is some.
        if let (Some(m), false) = (medicine, picked) {
            match self.go_to(p, m, false, dt) {
                Step::Moving => {}
                Step::Unreachable => p.job = Some(Job::Tend { patient, medicine: None, picked: false }),
                Step::Arrived => {
                    let i = self.map.idx(m);
                    p.carrying = self.map.take_item(i, ItemKind::Medicine, 1);
                    self.res_item[i] = 0;
                    p.job = Some(Job::Tend { patient, medicine, picked: true });
                }
            }
            return;
        }

        if !self_tend {
            match self.go_to(p, ppos, true, dt) {
                Step::Moving => return,
                Step::Unreachable => {
                    self.end_job(p);
                    return;
                }
                Step::Arrived => {}
            }
            if !ready {
                // Wait for the patient to lie down.
                return;
            }
        }

        p.job_progress += dt * p.work_speed(Skill::Medicine) / TEND_TIME;
        if p.learn(Skill::Medicine, dt) {
            let msg = format!("{} reached Medicine level {}.", p.name, p.skill(Skill::Medicine));
            self.push_log(msg);
        }
        if p.job_progress < 1.0 {
            return;
        }

        let has_med = matches!(p.carrying, Some(Stack { kind: ItemKind::Medicine, .. }));
        if has_med {
            p.carrying = None;
        }
        let quality = self.tend_quality(p, has_med, self_tend, in_bed);
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

    pub(crate) fn run_rescue(&mut self, p: &mut Pawn, patient: u32, bed: Pos, carrying: bool, dt: f32) {
        let Some(pi) = self.pawn_index(patient) else {
            self.end_job(p);
            return;
        };
        let bed_ok = self.map.feature[self.map.idx(bed)] == Feature::Bed;
        if self.pawns[pi].dead || !self.pawns[pi].downed || !bed_ok {
            self.end_job(p);
            return;
        }
        if !carrying {
            let ppos = self.pawns[pi].pos;
            match self.go_to(p, ppos, true, dt) {
                Step::Moving => {}
                Step::Unreachable => self.end_job(p),
                Step::Arrived => p.job = Some(Job::Rescue { patient, bed, carrying: true }),
            }
            return;
        }
        let step = self.go_to(p, bed, false, dt);
        {
            // The patient comes along.
            let o = &mut self.pawns[pi];
            o.x = p.x;
            o.y = p.y;
            o.pos = p.pos;
            if matches!(step, Step::Arrived) {
                o.bed = Some(bed);
            }
        }
        match step {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let name = self.pawns[pi].name.clone();
                self.push_log(format!("{} carried {} to bed.", p.name, name));
                self.end_job(p);
            }
        }
    }

    pub(crate) fn run_feed(&mut self, p: &mut Pawn, patient: u32, food: Pos, picked: bool, dt: f32) {
        let Some(pi) = self.pawn_index(patient) else {
            self.end_job(p);
            return;
        };
        if self.pawns[pi].dead || !self.pawns[pi].downed {
            self.end_job(p);
            return;
        }
        if !picked {
            match self.go_to(p, food, false, dt) {
                Step::Moving => {}
                Step::Unreachable => self.end_job(p),
                Step::Arrived => {
                    let i = self.map.idx(food);
                    match self.map.take_item(i, ItemKind::Food, 1) {
                        Some(s) => {
                            p.carrying = Some(s);
                            self.res_item[i] = 0;
                            p.job = Some(Job::Feed { patient, food, picked: true });
                        }
                        None => self.end_job(p),
                    }
                }
            }
            return;
        }
        let ppos = self.pawns[pi].pos;
        match self.go_to(p, ppos, true, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                p.job_progress += dt / 2.0;
                if p.job_progress >= 1.0 {
                    p.carrying = None;
                    let o = &mut self.pawns[pi];
                    o.hunger = (o.hunger + 45.0).min(100.0);
                    self.end_job(p);
                }
            }
        }
    }
}
