//! Hospitals and surgery. Beds in rooms for the sick (a hospital, a surgery
//! room: `RoomEffects::medical`) and operating tables (`BuildDef::medical`)
//! are care beds: patients look for them first, and nobody claims them as
//! their own bed. An operating table isn't slept in at all.
//!
//! Prosthetics: the player orders an operation for a colonist who has lost
//! a limb (`World::order_operation`: which part, which prosthetic). Once
//! such a prosthetic is lying around, an operating table is free and
//! another colonist can doctor, the patient lies down on the table
//! (`Job::Operation`). A doctor fetches the prosthetic and fits it
//! (`Job::Surgery`); it may go wrong, ruining the prosthetic and wounding
//! the patient. Better doctors and surgery rooms make that rarer. Either
//! way the order is done.

use crate::defs::{Defs, ItemId};
use crate::health::{DamageKind, Part};
use crate::map::Pos;
use crate::pathing::{bfs, DistField, ANY_DISTANCE};
use crate::pawn::{Job, Pawn, Skill};
use crate::jobs::Fetch;
use crate::world::{Step, World};
use serde::{Deserialize, Serialize};

/// An operation ordered for a colonist: fit a prosthetic of kind `item` to lost `part`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub part: Part,
    pub item: ItemId,
}

/// Seconds of work for an operation at speed 1.0.
const SURGERY_TIME: f32 = 10.0;
/// Seconds a patient waits on the table for a doctor before giving up.
const OPERATION_WAIT: f32 = 90.0;
/// Damage done by an operation that goes wrong.
const BOTCHED_WOUND: f32 = 8.0;

impl World {
    /// An operating table (or other bed only for the sick) at `b`.
    pub fn medical_bed(&self, b: Pos) -> bool {
        self.map.feature[self.map.idx(b)].structure().is_some_and(|d| d.medical)
    }

    /// A bed for patients first: an operating table, or a bed in a hospital.
    pub fn care_bed(&self, b: Pos) -> bool {
        self.map.feature[self.map.idx(b)].is_bed()
            && (self.medical_bed(b) || self.room_effects(b).is_some_and(|(_, e)| e.medical()))
    }

    /// Someone other than `me` owns the bed at `b`, lies in it, or is on their way to it.
    fn bed_taken(&self, b: Pos, me: u32) -> bool {
        self.pawns.iter().any(|o| {
            o.id != me
                && !o.dead
                && (o.bed == Some(b)
                    || o.pos == b && o.lying()
                    || matches!(o.job, Some(Job::Patient { bed: Some(t) } | Job::Operation { table: t }) if t == b))
        })
    }

    /// Where patient `p` should lie: the nearest free hospital bed, else
    /// their own (or any free) bed, and an operating table only if there's
    /// nothing else (they're kept for operations).
    pub(crate) fn find_patient_bed(&self, p: &Pawn) -> Option<Pos> {
        let df = bfs(&self.map, p.pos);
        let free = |pos: Pos| self.care_bed(pos) && !self.bed_taken(pos, p.id);
        let nearest = |test: &dyn Fn(Pos) -> bool| df.nearest(ANY_DISTANCE, |i| test(self.map.pos(i))).map(|i| self.map.pos(i));
        nearest(&|b| free(b) && !self.medical_bed(b))
            .or_else(|| self.find_bed(p))
            .or_else(|| nearest(&free))
    }

    /// Prosthetics that could replace `part`.
    pub fn prosthetics_for(part: Part) -> impl Iterator<Item = ItemId> {
        ItemId::all().filter(move |k| k.def().prosthetic.as_ref().is_some_and(|d| d.fits.contains(&part)))
    }

    /// Order fitting a prosthetic of kind `item` to colonist `id`'s lost
    /// `part`. Returns false if it doesn't fit, the part isn't lost, or it's
    /// already ordered.
    pub fn order_operation(&mut self, id: u32, part: Part, item: ItemId) -> bool {
        let Some(i) = self.pawn_index(id) else { return false };
        let p = &mut self.pawns[i];
        let fits = Self::prosthetics_for(part).any(|k| k == item);
        if !p.is_colonist() || p.dead || !fits || !p.health.lost_parts().contains(&part) {
            return false;
        }
        if p.operations.iter().any(|o| o.part == part) {
            return false;
        }
        p.operations.push(Operation { part, item });
        true
    }

    /// Call off the operation on colonist `id`'s `part`. Returns false if none was ordered.
    pub fn cancel_operation(&mut self, id: u32, part: Part) -> bool {
        let Some(i) = self.pawn_index(id) else { return false };
        let ops = &mut self.pawns[i].operations;
        let before = ops.len();
        ops.retain(|o| o.part != part);
        ops.len() < before
    }

    /// The part `p` has an operation ordered for with a prosthetic of `kind`, if still lost.
    fn fits(p: &Pawn, kind: ItemId) -> Option<Part> {
        let lost = p.health.lost_parts();
        p.operations.iter().find(|o| o.item == kind && lost.contains(&o.part)).map(|o| o.part)
    }

    /// The nearest free prosthetic that fits `p`, by distance field `df`.
    pub(crate) fn nearest_prosthetic(&self, p: &Pawn, df: &DistField) -> Option<Pos> {
        self.nearest_stack(df, ANY_DISTANCE, |i, s| self.res_item[i] == 0 && Self::fits(p, s.kind).is_some())
    }

    /// A free operating table for colonist `p` to lie on for an ordered
    /// operation, if its prosthetic is around and there's a doctor to fit it.
    pub(crate) fn operation_table(&self, p: &Pawn) -> Option<Pos> {
        if !p.is_colonist() || p.operations.is_empty() || !self.other_doctor_available(p.id) {
            return None;
        }
        let df = bfs(&self.map, p.pos);
        self.nearest_prosthetic(p, &df)?;
        let table = df.nearest(ANY_DISTANCE, |i| self.medical_bed(self.map.pos(i)) && !self.bed_taken(self.map.pos(i), p.id));
        table.map(|i| self.map.pos(i))
    }

    /// The patient: lie on the table and wait for the doctor.
    pub(crate) fn run_operation(&mut self, p: &mut Pawn, table: Pos, dt: f32) {
        let waiting_for = self.pawns.iter().any(|o| matches!(o.job, Some(Job::Surgery { patient, .. }) if patient == p.id));
        let ordered = p.operations.iter().any(|o| Self::fits(p, o.item).is_some());
        if !self.medical_bed(table) || !ordered {
            self.end_job(p);
            return;
        }
        match self.go_to(p, table, false, dt) {
            Step::Moving => return,
            Step::Unreachable => {
                self.end_job(p);
                return;
            }
            Step::Arrived => {}
        }
        p.resting = true;
        // Give up if no doctor comes (but not while one is on the way).
        if !waiting_for {
            p.job_progress += dt / OPERATION_WAIT;
            if p.job_progress >= 1.0 {
                self.end_job(p);
            }
        }
    }

    /// For doctor `p`: someone lying on an operating table for a
    /// prosthetic, with one to fit (nearest first).
    pub(crate) fn surgery_candidate(&self, p: &Pawn, df: &DistField) -> Option<(i32, Job)> {
        self.pawns
            .iter()
            .filter(|o| o.id != p.id && !o.dead && !self.patient_reserved(o.id))
            .filter(|o| matches!(o.job, Some(Job::Operation { table }) if o.pos == table && o.lying()))
            .filter_map(|o| {
                let (_, d) = df.approach(&self.map, o.pos, true)?;
                let prosthetic = self.nearest_prosthetic(o, df)?;
                Some((d - 500, Job::Surgery { patient: o.id, prosthetic, picked: false }))
            })
            .min_by_key(|c| c.0)
    }

    /// The doctor: fetch the prosthetic, then fit it.
    pub(crate) fn run_surgery(&mut self, p: &mut Pawn, patient: u32, prosthetic: Pos, picked: bool, dt: f32) {
        let Some(pi) = self.pawn_index(patient) else {
            self.end_job(p);
            return;
        };
        let o = &self.pawns[pi];
        let table = match o.job {
            Some(Job::Operation { table }) if !o.dead => table,
            _ => {
                self.end_job(p);
                return;
            }
        };
        if !picked {
            match self.fetch(p, prosthetic, dt, |w, i| w.take_one(i, Defs::is_prosthetic)) {
                Fetch::Walking => {}
                Fetch::Got if p.carrying.is_some_and(|s| Self::fits(&self.pawns[pi], s.kind).is_some()) => {
                    p.job = Some(Job::Surgery { patient, prosthetic, picked: true });
                }
                // Not one that fits (it's put back down), gone, or out of reach.
                _ => self.end_job(p),
            }
            return;
        }
        if !self.walk_to(p, table, true, dt) {
            return;
        }
        if self.pawns[pi].pos != table {
            // Wait for the patient to lie down.
            return;
        }
        if !self.work_on(p, Skill::Medicine, SURGERY_TIME, (1.0, 1.0), dt) {
            return;
        }
        let Some(item) = p.carrying.take() else {
            self.end_job(p);
            return;
        };
        let chance = self.surgery_chance(p, table);
        let ok = self.rng.chance(chance);
        let name = item.kind.name().to_lowercase();
        let (rng, o) = (&mut self.rng, &mut self.pawns[pi]);
        let Some(part) = Self::fits(o, item.kind) else {
            // Nothing to fit it to any more: put it back down.
            p.carrying = Some(item);
            self.end_job(p);
            return;
        };
        // Done, one way or the other.
        o.operations.retain(|op| op.part != part);
        let msg = if ok {
            let efficiency = item.kind.def().prosthetic.as_ref().map_or(1.0, |d| d.efficiency);
            o.health.fit_prosthetic(part, efficiency);
            format!("{} fitted {} with a {}.", p.name, o.name, name)
        } else {
            let wound = part.def().parent.unwrap_or(Part::Torso);
            o.health.injure(wound, BOTCHED_WOUND, DamageKind::Cut, rng);
            o.health.recompute();
            format!("{} botched the operation on {}: the {} is ruined.", p.name, o.name, name)
        };
        // The patient gets up (or stays in bed for the new wound).
        o.job = None;
        o.resting = false;
        o.job_progress = 0.0;
        self.push_log(msg);
        self.end_job(p);
    }

    /// Chance that doctor `p` fits a prosthetic well at `table`.
    pub fn surgery_chance(&self, p: &Pawn, table: Pos) -> f32 {
        let caps = &p.health.caps;
        let skill = p.skill(Skill::Medicine) as f32;
        let hands = caps.manipulation.min(1.0) * (0.6 + 0.4 * caps.sight.min(1.0));
        let room = self.room_effects(table).map_or(0.0, |(_, e)| e.surgery);
        ((0.55 + 0.025 * skill) * hands + room).clamp(0.05, 0.98)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{build, content_colonist as colonist, quiet_world, run};
    use crate::map::{Stack, FLAT_Z as Z};
    use crate::pawn::WorkType;
    use crate::rng::Rng;

    fn test_world() -> World {
        quiet_world(20, 20, 9)
    }




    /// Patient `id` lost their left leg a while ago (the stump has healed).
    fn lose_leg(w: &mut World, id: u32) {
        let i = w.pawn_index(id).unwrap();
        let mut rng = Rng::new(1);
        let h = &mut w.pawns[i].health;
        while !h.is_missing(Part::LeftLeg) {
            h.injure(Part::LeftLeg, 50.0, DamageKind::Cut, &mut rng);
        }
        h.hediffs.retain(|d| d.kind == crate::health::HediffKind::Missing);
        h.recompute();
    }

    #[test]
    fn a_doctor_fits_a_prosthetic_leg() {
        let mut w = test_world();
        let doctor = colonist(&mut w, (3, 3, Z));
        let patient = colonist(&mut w, (5, 3, Z));
        // Only the doctor doctors, and is good at it.
        let (d, pt) = (w.pawn_index(doctor).unwrap(), w.pawn_index(patient).unwrap());
        w.pawns[d].skills[Skill::Medicine as usize] = 15;
        w.pawns[pt].priorities[WorkType::Doctor as usize] = 0;
        lose_leg(&mut w, patient);
        let walk = w.pawn(patient).unwrap().health.caps.moving;
        build(&mut w, (10, 10, Z), "operating_table");
        let leg = ItemId::named("prosthetic_leg");
        w.map.drop_item((8, 3, Z), Stack { kind: leg, count: 1 });
        // Nothing happens until the player orders it.
        run(&mut w, 10.0);
        assert!(w.pawn(patient).unwrap().health.is_missing(Part::LeftLeg));
        assert!(!matches!(w.pawn(patient).unwrap().job, Some(Job::Operation { .. })));
        assert!(!w.order_operation(patient, Part::LeftArm, leg), "not lost");
        assert!(!w.order_operation(patient, Part::LeftLeg, ItemId::named("prosthetic_arm")), "doesn't fit");
        assert!(w.order_operation(patient, Part::LeftLeg, leg));
        assert!(!w.order_operation(patient, Part::LeftLeg, leg), "already ordered");
        let mut done = false;
        for _ in 0..120 {
            run(&mut w, 1.0);
            if !w.pawn(patient).unwrap().health.is_missing(Part::LeftLeg) {
                done = true;
                break;
            }
            // A botched try ruins the prosthetic and ends the order: try again.
            if w.pawn(patient).unwrap().operations.is_empty() && w.pawns.iter().all(|p| p.carrying.is_none()) {
                if w.map.count_items(leg) == 0 {
                    w.map.drop_item((8, 3, Z), Stack { kind: leg, count: 1 });
                }
                w.order_operation(patient, Part::LeftLeg, leg);
            }
        }
        assert!(done, "fitted in time");
        let p = w.pawn(patient).unwrap();
        assert!(p.health.caps.moving > walk + 0.2, "walks better: {} vs {}", p.health.caps.moving, walk);
        assert!(p.health.efficiency(Part::LeftFoot) > 0.5, "the foot comes with it");
        assert!(w.log.iter().any(|l| l.contains("fitted")), "{:?}", w.log);
        assert!(p.operations.is_empty(), "the order is done");
    }

    #[test]
    fn operations_can_be_called_off() {
        let mut w = test_world();
        let id = colonist(&mut w, (3, 3, Z));
        lose_leg(&mut w, id);
        let leg = ItemId::named("prosthetic_leg");
        assert!(w.order_operation(id, Part::LeftLeg, leg));
        assert!(w.cancel_operation(id, Part::LeftLeg));
        assert!(!w.cancel_operation(id, Part::LeftLeg), "nothing left to call off");
        assert!(w.pawn(id).unwrap().operations.is_empty());
        assert_eq!(World::prosthetics_for(Part::LeftFoot).collect::<Vec<_>>(), vec![leg]);
    }

    #[test]
    fn nobody_sleeps_on_an_operating_table() {
        let mut w = test_world();
        let id = colonist(&mut w, (3, 3, Z));
        build(&mut w, (6, 6, Z), "operating_table");
        assert!(w.find_bed(w.pawn(id).unwrap()).is_none(), "not a bed to sleep in");
        assert_eq!(w.find_patient_bed(w.pawn(id).unwrap()), Some((6, 6, Z)), "patients may lie on it, if nothing else");
        build(&mut w, (9, 9, Z), "bed");
        assert_eq!(w.find_patient_bed(w.pawn(id).unwrap()), Some((9, 9, Z)), "an ordinary bed first");
    }

    /// A roofed room inside walls (5..=9) with a door, and `things` in it.
    fn room(w: &mut World, things: &[((usize, usize), &str)]) {
        for y in 5..=9 {
            for x in 5..=9 {
                if x == 5 || x == 9 || y == 5 || y == 9 {
                    build(w, (x, y, Z), if (x, y) == (5, 7) { "door" } else { "wall" });
                }
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_roof(i, true);
            }
        }
        for &((x, y), id) in things {
            build(w, (x, y, Z), id);
        }
        w.map.recompute_rooms();
        w.update_room_stats();
    }

    #[test]
    fn hospitals_keep_their_beds_for_patients_and_heal_faster() {
        let mut w = test_world();
        let id = colonist(&mut w, (2, 2, Z));
        build(&mut w, (2, 12, Z), "bed");
        let hospital = [((6, 6), "bed"), ((8, 6), "bed"), ((6, 8), "medical_cabinet"), ((8, 8), "lamp"), ((7, 5), "window"), ((7, 9), "window")];
        room(&mut w, &hospital);
        assert_eq!(w.room_at((7, 7, Z)).and_then(|r| r.kind_name()), Some("Hospital"));
        let p = w.pawn(id).unwrap();
        assert_eq!(w.find_bed(p), Some((2, 12, Z)), "nobody sleeps in a hospital bed");
        let bed = w.find_patient_bed(p).unwrap();
        assert!(w.room_at(bed).is_some_and(|r| r.kind_name() == Some("Hospital")), "patients go to hospital");

        // The same tended wound, lying a while in each bed.
        let heal_in = |w: &mut World, at: Pos| {
            let i = w.pawn_index(id).unwrap();
            let p = &mut w.pawns[i];
            p.pos = at;
            p.resting = true;
            p.health = crate::health::Health::new();
            p.health.injure(Part::LeftArm, 10.0, DamageKind::Bruise, &mut Rng::new(3));
            p.health.tend(0.5, 3);
            let mut p = w.pawns[i].clone();
            for _ in 0..30 {
                w.update_health(&mut p, 1.0);
            }
            p.health.part_hp(Part::LeftArm)
        };
        let ordinary = heal_in(&mut w, (2, 12, Z));
        let hospital = heal_in(&mut w, bed);
        assert!(hospital > ordinary, "{} vs {}", hospital, ordinary);
    }

    #[test]
    fn surgery_rooms_make_operations_safer() {
        let mut w = test_world();
        let id = colonist(&mut w, (3, 3, Z));
        // A roofed room with a door, lamp, cabinet and an operating table.
        for y in 5..=9 {
            for x in 5..=9 {
                if x == 5 || x == 9 || y == 5 || y == 9 {
                    build(&mut w, (x, y, Z), if (x, y) == (5, 7) { "door" } else { "wall" });
                }
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_roof(i, true);
            }
        }
        build(&mut w, (7, 7, Z), "operating_table");
        w.map.recompute_rooms();
        w.update_room_stats();
        let doctor = w.pawn(id).unwrap().clone();
        let plain = w.surgery_chance(&doctor, (7, 7, Z));
        build(&mut w, (6, 6, Z), "lamp");
        build(&mut w, (8, 6, Z), "medical_cabinet");
        w.update_room_stats();
        assert_eq!(w.room_at((7, 7, Z)).and_then(|r| r.kind_name()), Some("Surgery room"));
        assert!(w.surgery_chance(&doctor, (7, 7, Z)) > plain + 0.2);
        assert!(w.care_bed((7, 7, Z)));
    }
}
