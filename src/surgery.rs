//! Hospitals and surgery. Beds in rooms for the sick (a hospital, a surgery
//! room: `RoomEffects::medical`) and operating tables (`BuildDef::medical`)
//! are care beds: patients look for them first, and nobody claims them as
//! their own bed. An operating table isn't slept in at all.
//!
//! Cyberware: the player orders an operation for a colonist
//! (`World::order_operation`: which cyberware slot, which item), to replace
//! a lost limb, upgrade a healthy one, or add an implant; or to take
//! cyberware out again (`World::order_removal`). Once the item is lying
//! around (if one is needed), an operating table is free and another
//! colonist can doctor, the patient lies down on the table
//! (`Job::Operation`). A doctor fetches the item and installs it
//! (`Job::Surgery`); whatever it replaces is left at the table. It may go
//! wrong, ruining the item and wounding the patient. Better doctors and
//! surgery rooms make that rarer. Either way the order is done.

use crate::defs::{Defs, ItemId};
use crate::health::{CyberSlot, DamageKind, Part};
use crate::map::{Pos, Stack};
use crate::pathing::{bfs, DistField, ANY_DISTANCE};
use crate::pawn::{Job, Pawn, PawnId, Skill};
use crate::jobs::Fetch;
use crate::world::{Step, World};
use serde::{Deserialize, Serialize};

/// An operation ordered for a colonist: install cyberware of kind `item`
/// into `slot`, or (`None`) take out what's there.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub slot: CyberSlot,
    pub item: Option<ItemId>,
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
    fn bed_taken(&self, b: Pos, me: PawnId) -> bool {
        self.pawns.iter().any(|o| {
            o.id != me
                && !o.is_dead()
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

    /// Cyberware that goes into `slot`.
    pub fn cyberware_for(slot: CyberSlot) -> impl Iterator<Item = ItemId> {
        ItemId::all().filter(move |k| k.def().cyberware.as_ref().is_some_and(|d| d.fits.contains(&slot)))
    }

    /// Order installing cyberware of kind `item` into colonist `id`'s `slot`
    /// (in place of a natural part or what's there). Returns false if it
    /// doesn't fit, is already there, or something is already ordered there.
    pub fn order_operation(&mut self, id: PawnId, slot: CyberSlot, item: ItemId) -> bool {
        let Some(i) = self.pawn_index(id) else { return false };
        let p = &mut self.pawns[i];
        let fits = Self::cyberware_for(slot).any(|k| k == item);
        if !p.is_colonist() || p.is_dead() || !fits || p.surgeries.iter().any(|o| o.slot == slot) {
            return false;
        }
        let op = Operation { slot, item: Some(item) };
        if !Self::still_to_do(p, op) {
            return false;
        }
        p.surgeries.push(op);
        true
    }

    /// Order taking the cyberware out of colonist `id`'s `slot` (the item is
    /// left at the table). Not for organs and the spine, which can only be
    /// swapped.
    pub fn order_removal(&mut self, id: PawnId, slot: CyberSlot) -> bool {
        let Some(i) = self.pawn_index(id) else { return false };
        let p = &mut self.pawns[i];
        let op = Operation { slot, item: None };
        if !p.is_colonist() || p.is_dead() || !slot.removable() || p.surgeries.iter().any(|o| o.slot == slot) || !Self::still_to_do(p, op) {
            return false;
        }
        p.surgeries.push(op);
        true
    }

    /// Call off the operation on colonist `id`'s `slot`. Returns false if none was ordered.
    pub fn cancel_operation(&mut self, id: PawnId, slot: CyberSlot) -> bool {
        let Some(i) = self.pawn_index(id) else { return false };
        let ops = &mut self.pawns[i].surgeries;
        let before = ops.len();
        ops.retain(|o| o.slot != slot);
        ops.len() < before
    }

    /// Whether operation `op` would still change something on `p`.
    pub(crate) fn still_to_do(p: &Pawn, op: Operation) -> bool {
        let there = p.health.implant(op.slot);
        match op.item {
            Some(kind) => there.is_none_or(|(k, _)| k != Some(kind)),
            None => there.is_some(),
        }
    }

    /// The slot `p` has an operation ordered for with cyberware of `kind`.
    fn fits(p: &Pawn, kind: ItemId) -> Option<CyberSlot> {
        p.surgeries.iter().find(|o| o.item == Some(kind) && Self::still_to_do(p, **o)).map(|o| o.slot)
    }

    /// A removal ordered for `p`, still to do.
    fn removal(p: &Pawn) -> Option<CyberSlot> {
        p.surgeries.iter().find(|o| o.item.is_none() && Self::still_to_do(p, **o)).map(|o| o.slot)
    }

    /// The nearest free cyberware ordered for `p`, by distance field `df`.
    pub(crate) fn nearest_cyberware(&self, p: &Pawn, df: &DistField) -> Option<Pos> {
        self.nearest_stack(df, ANY_DISTANCE, |i, s| self.res_item[i].is_none() && Self::fits(p, s.kind).is_some())
    }

    /// The job for a doctor to operate on `o` (lying on the table): fetch
    /// the nearest cyberware for it, or take some out.
    pub(crate) fn surgery_job(&self, o: &Pawn, df: &DistField) -> Option<Job> {
        match self.nearest_cyberware(o, df) {
            Some(at) => Some(Job::Surgery { patient: o.id, prosthetic: Some(at), picked: false }),
            None => Self::removal(o).map(|_| Job::Surgery { patient: o.id, prosthetic: None, picked: true }),
        }
    }

    /// A free operating table for colonist `p` to lie on for an ordered
    /// operation, if what it needs is around and there's a doctor to do it.
    pub(crate) fn operation_table(&self, p: &Pawn) -> Option<Pos> {
        if !p.is_colonist() || p.surgeries.is_empty() || !self.other_doctor_available(p.id) {
            return None;
        }
        let df = bfs(&self.map, p.pos);
        self.surgery_job(p, &df)?;
        let table = df.nearest(ANY_DISTANCE, |i| self.medical_bed(self.map.pos(i)) && !self.bed_taken(self.map.pos(i), p.id));
        table.map(|i| self.map.pos(i))
    }

    /// The patient: lie on the table and wait for the doctor.
    pub(crate) fn run_operation(&mut self, p: &mut Pawn, table: Pos, dt: f32) {
        let waiting_for = self.pawns.iter().any(|o| matches!(o.job, Some(Job::Surgery { patient, .. }) if patient == p.id));
        let ordered = p.surgeries.iter().any(|&o| Self::still_to_do(p, o));
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

    /// For doctor `p`: someone lying on an operating table for an
    /// operation that can be done (nearest first).
    pub(crate) fn surgery_candidate(&self, p: &Pawn, df: &DistField) -> Option<(i32, Job)> {
        self.pawns
            .iter()
            .filter(|o| o.id != p.id && !o.is_dead() && !self.patient_reserved(o.id))
            .filter(|o| matches!(o.job, Some(Job::Operation { table }) if o.pos == table && o.lying()))
            .filter_map(|o| {
                let (_, d) = df.approach(&self.map, o.pos, true)?;
                Some((d - 500, self.surgery_job(o, df)?))
            })
            .min_by_key(|c| c.0)
    }

    /// The doctor: fetch the cyberware (if it's an install), then operate.
    pub(crate) fn run_surgery(&mut self, p: &mut Pawn, patient: PawnId, prosthetic: Option<Pos>, picked: bool, dt: f32) {
        let Some(pi) = self.pawn_index(patient) else {
            self.end_job(p);
            return;
        };
        let o = &self.pawns[pi];
        let table = match o.job {
            Some(Job::Operation { table }) if !o.is_dead() => table,
            _ => {
                self.end_job(p);
                return;
            }
        };
        if !picked {
            let Some(at) = prosthetic else {
                self.end_job(p);
                return;
            };
            match self.fetch(p, at, dt, |w, i| w.take_one(i, Defs::is_cyberware)) {
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
        // An install with the item in hand, or else a removal.
        let item = if prosthetic.is_some() { p.carrying.take() } else { None };
        let op = match item {
            Some(item) => Self::fits(&self.pawns[pi], item.kind).map(|slot| Operation { slot, item: Some(item.kind) }),
            None if prosthetic.is_none() => Self::removal(&self.pawns[pi]).map(|slot| Operation { slot, item: None }),
            None => None,
        };
        let Some(op) = op else {
            // Nothing to do any more: put it back down.
            p.carrying = item;
            self.end_job(p);
            return;
        };
        let chance = self.surgery_chance(p, table);
        let ok = self.rng.chance(chance);
        let (rng, o) = (&mut self.rng, &mut self.pawns[pi]);
        // Done, one way or the other.
        o.surgeries.retain(|s| s.slot != op.slot);
        let had = o.health.implant(op.slot).map(|(k, _)| k.or_else(|| legacy_prosthetic(op.slot)));
        let slot = op.slot.name().to_lowercase();
        let (msg, back) = match (op.item, ok) {
            (Some(kind), true) => {
                let efficiency = kind.def().cyberware.as_ref().map_or(1.0, |d| d.efficiency);
                o.health.install(op.slot, kind, efficiency);
                (format!("{} installed a {} in {} ({}).", p.name, kind.name().to_lowercase(), o.name, slot), had.flatten())
            }
            (None, true) => {
                o.health.uninstall(op.slot);
                let what = had.flatten().map_or("cyberware".to_string(), |k| k.name().to_lowercase());
                (format!("{} took the {} out of {} ({}).", p.name, what, o.name, slot), had.flatten())
            }
            (item, false) => {
                let wound = op.slot.parts().first().and_then(|p| p.def().parent).unwrap_or(Part::Torso);
                o.health.injure(wound, BOTCHED_WOUND, DamageKind::Cut, rng);
                o.health.recompute();
                let msg = match item {
                    Some(kind) => format!("{} botched the operation on {}: the {} is ruined.", p.name, o.name, kind.name().to_lowercase()),
                    None => format!("{} botched the operation on {}: the {} stays in.", p.name, o.name, slot),
                };
                (msg, None)
            }
        };
        // The patient gets up (or stays in bed for the new wound).
        o.job = None;
        o.resting = false;
        o.job_progress = 0.0;
        if let Some(kind) = back {
            self.map.drop_item(table, Stack { kind, count: 1 });
        }
        self.push_log(msg);
        self.end_job(p);
    }

    /// Chance that doctor `p` operates well at `table`.
    pub fn surgery_chance(&self, p: &Pawn, table: Pos) -> f32 {
        let caps = &p.health.caps;
        let skill = p.skill(Skill::Medicine) as f32;
        let hands = caps.manipulation.min(1.0) * (0.6 + 0.4 * caps.sight.min(1.0));
        let room = self.room_effects(table).map_or(0.0, |(_, e)| e.surgery);
        ((0.55 + 0.025 * skill) * hands + room).clamp(0.05, 0.98)
    }
}

/// What an old prosthetic (fitted before cyberware had items) in `slot` was.
fn legacy_prosthetic(slot: CyberSlot) -> Option<ItemId> {
    match slot {
        CyberSlot::LeftArm | CyberSlot::RightArm => ItemId::find("prosthetic_arm"),
        CyberSlot::LeftLeg | CyberSlot::RightLeg => ItemId::find("prosthetic_leg"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{build, content_colonist as colonist, quiet_world, run};
    use crate::map::{Stack, FLAT_Z as Z};
    use crate::pawn::WorkType;
    use crate::rng::Rng;
    use crate::pawn::Pawn;

    fn test_world() -> World {
        quiet_world(20, 20, 9)
    }




    /// Patient `id` lost their left leg a while ago (the stump has healed).
    fn lose_leg(w: &mut World, id: PawnId) {
        let i = w.pawn_index(id).unwrap();
        let mut rng = Rng::new(1);
        let h = &mut w.pawns[i].health;
        while !h.is_missing(Part::LeftLeg) {
            h.injure(Part::LeftLeg, 50.0, DamageKind::Cut, &mut rng);
        }
        h.hediffs.retain(|d| d.kind == crate::health::HediffKind::Missing);
        h.recompute();
    }

    /// A doctor (good at it) and a patient who doesn't doctor, an operating table.
    fn clinic(w: &mut World) -> (PawnId, PawnId) {
        let doctor = colonist(w, (3, 3, Z));
        let patient = colonist(w, (5, 3, Z));
        let (d, pt) = (w.pawn_index(doctor).unwrap(), w.pawn_index(patient).unwrap());
        w.pawns[d].skills[Skill::Medicine as usize] = 15;
        w.pawns[pt].priorities[WorkType::Doctor as usize] = 0;
        build(w, (10, 10, Z), "operating_table");
        (doctor, patient)
    }

    /// Order `item` (or a removal) into `slot` and wait until it's done,
    /// ordering again (with a fresh item) after a botched try.
    fn operate(w: &mut World, patient: PawnId, slot: CyberSlot, item: Option<ItemId>) {
        let order = |w: &mut World| match item {
            Some(k) => {
                if w.map.count_items(k) == 0 {
                    w.map.drop_item((8, 3, Z), Stack { kind: k, count: 1 });
                }
                w.order_operation(patient, slot, k)
            }
            None => w.order_removal(patient, slot),
        };
        assert!(order(w), "ordered");
        for _ in 0..240 {
            run(w, 1.0);
            let p = w.pawn(patient).unwrap();
            if !World::still_to_do(p, Operation { slot, item }) {
                return;
            }
            // A botched try ends the order: try again.
            if p.surgeries.is_empty() && w.pawns.iter().all(|p| p.carrying.is_none()) {
                order(w);
            }
        }
        panic!("the operation on {:?} wasn't done in time", slot);
    }

    #[test]
    fn a_doctor_fits_a_prosthetic_leg() {
        let mut w = test_world();
        let (_, patient) = clinic(&mut w);
        lose_leg(&mut w, patient);
        let walk = w.pawn(patient).unwrap().health.caps.moving;
        assert!(w.pawn(patient).unwrap().health.slot_lost(CyberSlot::LeftLeg));
        let leg = ItemId::named("prosthetic_leg");
        w.map.drop_item((8, 3, Z), Stack { kind: leg, count: 1 });
        // Nothing happens until the player orders it.
        run(&mut w, 10.0);
        assert!(w.pawn(patient).unwrap().health.is_missing(Part::LeftLeg));
        assert!(!matches!(w.pawn(patient).unwrap().job, Some(Job::Operation { .. })));
        assert!(!w.order_operation(patient, CyberSlot::LeftArm, leg), "doesn't fit an arm");
        assert!(!w.order_operation(patient, CyberSlot::LeftLeg, ItemId::named("prosthetic_arm")), "an arm doesn't fit");
        assert!(!w.order_removal(patient, CyberSlot::LeftLeg), "nothing to take out");
        assert!(w.order_operation(patient, CyberSlot::LeftLeg, leg));
        assert!(!w.order_operation(patient, CyberSlot::LeftLeg, leg), "already ordered");
        assert!(w.cancel_operation(patient, CyberSlot::LeftLeg));
        operate(&mut w, patient, CyberSlot::LeftLeg, Some(leg));
        let p = w.pawn(patient).unwrap();
        assert!(!p.health.is_missing(Part::LeftLeg));
        assert!(p.health.caps.moving > walk + 0.2, "walks better: {} vs {}", p.health.caps.moving, walk);
        assert!(p.health.efficiency(Part::LeftFoot) > 0.5, "the foot comes with it");
        assert_eq!(p.health.implant(CyberSlot::LeftLeg), Some((Some(leg), 0.75)));
        assert!(w.log.iter().any(|l| l.contains("installed")), "{:?}", w.log);
        assert!(p.surgeries.is_empty(), "the order is done");
    }

    #[test]
    fn bionics_replace_healthy_parts_and_give_back_what_they_replace() {
        let mut w = test_world();
        let (_, patient) = clinic(&mut w);
        let (arm, bionic) = (ItemId::named("prosthetic_arm"), ItemId::named("bionic_arm"));
        let natural = w.pawn(patient).unwrap().health.caps.manipulation;
        // Elective: a healthy arm swapped for a prosthetic, then for a bionic one.
        operate(&mut w, patient, CyberSlot::RightArm, Some(arm));
        assert!(w.pawn(patient).unwrap().health.caps.manipulation < natural, "a prosthetic is worse than flesh");
        assert_eq!(w.map.count_items(arm), 0, "the natural arm isn't an item");
        operate(&mut w, patient, CyberSlot::RightArm, Some(bionic));
        let p = w.pawn(patient).unwrap();
        assert!(p.health.caps.manipulation > natural, "a bionic is better: {}", p.health.caps.manipulation);
        assert_eq!(p.health.implants().collect::<Vec<_>>(), vec![(CyberSlot::RightArm, bionic)]);
        assert_eq!(w.map.count_items(arm), 1, "the prosthetic it replaced is left at the table");
        assert!(!w.order_operation(patient, CyberSlot::RightArm, bionic), "already in");
        // Taken out again: the arm is gone, and the bionic is back.
        operate(&mut w, patient, CyberSlot::RightArm, None);
        let p = w.pawn(patient).unwrap();
        assert!(p.health.is_missing(Part::RightHand) && p.health.slot_lost(CyberSlot::RightArm));
        assert_eq!(p.health.bleed_rate(), 0.0, "closed up by the doctor");
        assert_eq!(w.map.count_items(bionic), 1);
    }

    #[test]
    fn implants_add_armour_aim_and_sight_and_organs_can_only_be_swapped() {
        let mut w = test_world();
        let (_, patient) = clinic(&mut w);
        let before = w.pawn(patient).unwrap().clone();
        let heart = ItemId::named("synthetic_heart");
        assert!(!w.order_removal(patient, CyberSlot::Heart), "nothing in it");
        for (slot, kind) in [(CyberSlot::Dermal, "subdermal_plating"), (CyberSlot::Brain, "neural_coprocessor"), (CyberSlot::Eyes, "bionic_eyes")] {
            operate(&mut w, patient, slot, Some(ItemId::named(kind)));
        }
        operate(&mut w, patient, CyberSlot::Heart, Some(heart));
        let p = w.pawn(patient).unwrap();
        assert!((p.armor() - before.armor() - 0.15).abs() < 1e-5, "plating counts as armour");
        assert!((p.aim_bonus() - 0.08).abs() < 1e-5, "the coprocessor helps aim");
        assert!(p.health.caps.sight > 1.2, "bionic eyes see better: {}", p.health.caps.sight);
        assert!(!p.health.is_missing(Part::Brain), "the brain stays: an add-on");
        let base = |p: &Pawn| crate::battle::base_hit_chance(p, true, 5.0);
        assert!(base(p) > base(&before) + 0.1, "{} vs {}", base(p), base(&before));
        assert!(!w.order_removal(patient, CyberSlot::Heart), "a heart can't just come out");
        operate(&mut w, patient, CyberSlot::Dermal, None);
        assert!((w.pawn(patient).unwrap().armor() - before.armor()).abs() < 1e-5);
    }

    #[test]
    fn operations_can_be_called_off() {
        let mut w = test_world();
        let id = colonist(&mut w, (3, 3, Z));
        lose_leg(&mut w, id);
        let leg = ItemId::named("prosthetic_leg");
        assert!(w.order_operation(id, CyberSlot::LeftLeg, leg));
        assert!(w.cancel_operation(id, CyberSlot::LeftLeg));
        assert!(!w.cancel_operation(id, CyberSlot::LeftLeg), "nothing left to call off");
        assert!(w.pawn(id).unwrap().surgeries.is_empty());
        let legs: Vec<ItemId> = World::cyberware_for(CyberSlot::RightLeg).collect();
        assert_eq!(legs, vec![leg, ItemId::named("bionic_leg")]);
    }

    #[test]
    fn prosthetics_fitted_before_cyberware_slots_still_count() {
        // An old save's hediff: no item, on the lost arm (not the shoulder).
        let json = r#"{"kind":{"Prosthetic":{"efficiency":0.75}},"part":"LeftArm","severity":0.0,"peak":0.0,"tend":null,"tend_left":0.0,"infection_timer":-1.0,"age":0.0}"#;
        let old: crate::health::Hediff = serde_json::from_str(json).unwrap();
        let mut h = crate::health::Health::new();
        h.hediffs.push(old);
        h.recompute();
        assert_eq!(h.implant(CyberSlot::LeftArm), Some((None, 0.75)));
        assert_eq!(legacy_prosthetic(CyberSlot::LeftArm), Some(ItemId::named("prosthetic_arm")));
        assert_eq!(h.implants().count(), 0, "no item to list");
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
