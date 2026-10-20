//! Injuries and deaths in battle (see `battle.rs` for the battles themselves).

use crate::defs::ItemId;
use crate::health::DamageKind;
use crate::map::{Pos, Stack};
use crate::pawn::{Life, Memory, Pawn};
use crate::world::World;

impl World {
    pub fn damage(&mut self, ti: usize, amount: f32, kind: DamageKind, by: &str) {
        let mut t = self.pawns[ti].clone();
        if t.is_dead() {
            return;
        }
        let armor: Vec<f32> = crate::health::Part::ALL.iter().map(|&p| t.armor_at(p)).collect();
        let hit = t.health.apply_damage(amount, kind, |p| armor[p as usize], &mut self.rng);
        t.health.recompute();
        // Blows that don't get through the armour glance off.
        self.sound(if hit.is_some() { "hit" } else { "glance" }, Some(t.pos));
        let wound = hit.map_or(String::new(), |h| {
            format!(" ({} to the {})", kind.name().to_lowercase(), h.part.name().to_lowercase())
        });
        if t.health.death_cause().is_some() {
            self.kill(&mut t, &format!("was killed by {}{}", by, wound));
        } else {
            if let Some(h) = hit.filter(|h| h.destroyed) {
                self.push_log(format!("{}'s {} was destroyed by {}.", t.name, h.part.name().to_lowercase(), by));
            }
            if t.life == Life::Up {
                if let Some(reason) = t.health.downed_reason() {
                    self.end_job(&mut t);
                    t.life = Life::Downed { time: 0.0 };
                    self.push_log(format!("{} was downed by {}{}: {}.", t.name, by, wound, reason));
                    self.sound("thud", Some(t.pos));
                }
            }
        }
        self.pawns[ti] = t;
    }

    /// A shot or blast at pawn `ti`: their shield, if it has energy left,
    /// takes what it can (and a blow too big for it breaks through with the rest).
    pub fn shot_damage(&mut self, ti: usize, amount: f32, kind: DamageKind, by: &str) {
        let left = self.pawns[ti].shield_energy();
        if left <= 0.0 {
            self.damage(ti, amount, kind, by);
            return;
        }
        let taken = amount.min(left);
        let t = &mut self.pawns[ti];
        t.shield_drain += taken;
        let (pos, broke, name) = (t.pos, t.shield_energy() <= 0.0, t.name.clone());
        if broke {
            self.push_log(format!("{}'s shield broke.", name));
        }
        self.sound("glance", Some(pos));
        if amount - taken >= 1.0 {
            self.damage(ti, amount - taken, kind, by);
        }
    }

    pub(crate) fn kill(&mut self, p: &mut Pawn, cause: &str) {
        if p.is_dead() {
            return; 
        }
        self.end_job(p);
        // Their gear stays behind for others to use.
        let stashed: Vec<ItemId> = p.stash.drain(..).filter_map(|(_, prev)| prev).collect();
        for item in p.weapon.take().into_iter().chain(p.sidearm.take()).chain(p.apparel.drain(..)).chain(stashed) {
            self.map.drop_item(p.pos, Stack { kind: item, count: 1 });
        }
        if let Some(s) = p.utility.take() {
            self.map.drop_item(p.pos, s);
        }
        // Raiders carry odds and ends worth salvaging.
        if p.faction == crate::pawn::Faction::Raider {
            for kind in ItemId::all() {
                if let Some((lo, hi)) = kind.def().raider_loot {
                    let count = lo + self.rng.below((hi - lo + 1) as usize) as u32;
                    self.map.drop_item(p.pos, Stack { kind, count });
                }
            }
        }
        p.life = Life::Dead { buried: false };
        self.sound("death", Some(p.pos));
        self.push_log(format!("{} {}.", p.name, cause));
        if p.is_colonist() {
            for o in self.pawns.iter_mut() {
                if o.is_colonist() && o.id != p.id && !o.is_dead() {
                    o.add_memory(Memory::ColonistDied);
                }
            }
        }
    }

    /// The nearest map-edge cell `from` can walk to (where raiders flee).
    pub(crate) fn nearest_edge(&self, from: Pos) -> Pos {
        let (w, h) = (self.map.w, self.map.h);
        self.nearest_cell(from, crate::pathing::ANY_DISTANCE, |i| {
            let p = self.map.pos(i);
            p.0 == 0 || p.1 == 0 || p.0 == w - 1 || p.1 == h - 1
        })
        .unwrap_or(from)
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::map::FLAT_Z as Z;
    use crate::pawn::{Job, Memory};
    use crate::testing::{colonist, item, quiet_world};

    #[test]
    fn death_drops_inventory_frees_reservations_and_only_happens_once() {
        let mut w = quiet_world(8, 8, 1);
        let id = colonist(&mut w, (2, 2, Z));
        let survivor = colonist(&mut w, (6, 6, Z));
        let (from, to) = ((2, 2, Z), (4, 4, Z));
        w.with_pawn(id, |w, p| {
            p.weapon = Some(item("rifle"));
            p.sidearm = Some(item("knife"));
            p.apparel = vec![item("clothes")];
            p.utility = Some(Stack { kind: item("personal_shield"), count: 1 });
            p.carrying = Some(Stack { kind: item("wood"), count: 3 });
            w.start_job(p, Job::Haul { from, to, deliver: false, picked: true });
            w.res_patient.push((survivor, id));
            w.kill(p, "test injury");
        });
        let p = w.pawn(id).unwrap();
        assert_eq!(p.life, Life::Dead { buried: false });
        assert!(p.job.is_none() && p.carrying.is_none() && p.weapon.is_none() && p.sidearm.is_none());
        assert!(p.apparel.is_empty() && p.utility.is_none());
        assert_eq!(w.res_item[w.map.idx(from)], None);
        assert_eq!(w.res_cell[w.map.idx(to)], None);
        assert!(!w.patient_reserved(survivor));
        assert!(w.pawn(survivor).unwrap().memories.iter().any(|m| m.kind == Memory::ColonistDied));
        for gear in ["rifle", "knife", "clothes", "personal_shield"] {
            assert_eq!(w.map.count_items(item(gear)), 1);
        }
        assert_eq!(w.map.count_items(item("wood")), 3);
        let before = w.save_json().unwrap();
        w.with_pawn(id, |w, p| w.kill(p, "second injury"));
        let index = w.pawn_index(id).unwrap();
        w.damage(index, 100.0, DamageKind::Gunshot, "after death");
        assert_eq!(w.save_json().unwrap(), before, "dead pawns neither drop twice nor consume RNG on damage");
    }

    #[test]
    fn a_shield_absorbs_shots_and_only_overflow_reaches_the_body() {
        let mut w = quiet_world(8, 8, 2);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        let p = &mut w.pawns[i];
        p.apparel.clear();
        p.take_utility(Stack { kind: item("personal_shield"), count: 1 });
        let energy = p.shield_energy();
        let health = serde_json::to_string(&p.health).unwrap();
        w.shot_damage(i, energy - 5.0, DamageKind::Gunshot, "test");
        assert_eq!(w.pawns[i].shield_energy(), 5.0);
        assert_eq!(serde_json::to_string(&w.pawns[i].health).unwrap(), health);
        // Match the overflow against the same pawn and RNG with its shield removed.
        let mut unshielded = w.clone();
        unshielded.pawns[i].utility = None;
        w.shot_damage(i, 15.0, DamageKind::Gunshot, "test");
        unshielded.damage(i, 10.0, DamageKind::Gunshot, "test");
        assert_eq!(w.pawns[i].shield_energy(), 0.0);
        assert!(!w.pawns[i].health.hediffs.is_empty());
        assert_eq!(serde_json::to_string(&w.pawns[i].health).unwrap(), serde_json::to_string(&unshielded.pawns[i].health).unwrap());
    }
}
