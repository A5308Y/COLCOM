//! Graves for the colony's dead: dug into soil by builders (a buildable
//! with `special: Grave`), then haulers carry a dead colonist to an empty
//! one and bury them (`Job::Bury`). A buried colonist stays in the pawn list
//! (`Life::Dead { buried: true }`, at the grave's cell) so the grave knows whose it is.
//! While anyone lies unburied, the living feel it.

use crate::defs::Special;
use crate::map::Pos;
use crate::pathing::DistField;
use crate::pawn::{Job, Life, Pawn, PawnId};
use crate::world::World;

impl World {
    /// Who is buried in the grave at `grave`, if anyone.
    pub fn grave_occupant(&self, grave: Pos) -> Option<PawnId> {
        self.pawns.iter().find(|p| p.is_buried() && p.pos == grave).map(|p| p.id)
    }

    fn is_grave(&self, p: Pos) -> bool {
        self.map.feature[self.map.idx(p)].is_special(Special::Grave)
    }

    /// Finished graves nobody lies in yet.
    pub fn empty_graves(&self) -> Vec<Pos> {
        (0..self.map.len())
            .filter(|&i| self.map.feature[i].is_special(Special::Grave))
            .map(|i| self.map.pos(i))
            .filter(|&g| self.grave_occupant(g).is_none())
            .collect()
    }

    /// A dead colonist lying unburied (not already being carried).
    pub(crate) fn unburied(&self, p: &Pawn) -> bool {
        p.is_colonist() && p.is_dead() && !p.is_buried() && !p.has_left()
    }

    /// The nearest dead colonist to bury, with the nearest free empty grave.
    pub(crate) fn bury_candidate(&self, df: &DistField) -> Option<(i32, Job)> {
        let corpse = self
            .pawns
            .iter()
            .filter(|o| self.unburied(o) && !self.patient_reserved(o.id))
            .filter_map(|o| Some((df.approach(&self.map, o.pos, true)?.1, o.id)))
            .min_by_key(|c| c.0)?;
        let grave = self
            .empty_graves()
            .into_iter()
            .filter(|&g| self.res_target[self.map.idx(g)].is_none())
            .filter_map(|g| Some((df.approach(&self.map, g, false)?.1, g)))
            .min_by_key(|g| g.0)?;
        Some((corpse.0, Job::Bury { corpse: corpse.1, grave: grave.1, carrying: false }))
    }

    pub(crate) fn run_bury(&mut self, p: &mut Pawn, corpse: PawnId, grave: Pos, carrying: bool, dt: f32) {
        let Some(ci) = self.pawn_index(corpse) else {
            self.end_job(p);
            return;
        };
        let still_open = self.is_grave(grave) && self.grave_occupant(grave).is_none();
        if !self.unburied(&self.pawns[ci]) || !still_open {
            self.end_job(p);
            return;
        }
        if !carrying {
            let at = self.pawns[ci].pos;
            if self.walk_to(p, at, true, dt) {
                p.job = Some(Job::Bury { corpse, grave, carrying: true });
            }
            return;
        }
        // The body comes along.
        let there = self.walk_to(p, grave, false, dt);
        self.carry_along(p, ci);
        if there {
            let o = &mut self.pawns[ci];
            // Only the dead are carried here (see `bury_candidate`).
            o.life = Life::Dead { buried: true };
            o.pos = grave;
            o.x = grave.0 as f32;
            o.y = grave.1 as f32;
            o.z = grave.2 as f32;
            let name = o.name.clone();
            self.sound("dig", Some(grave));
            self.push_log(format!("{} buried {}.", p.name, name));
            self.end_job(p);
        }
    }

    /// Once a second: whether a dead colonist lies unburied, for everyone's mood.
    pub(crate) fn update_graves(&mut self) {
        let unburied = self.pawns.iter().any(|p| self.unburied(p));
        for p in self.pawns.iter_mut() {
            p.unburied_dead = unburied;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{colonist, quiet_world, run};
    use crate::defs::{BuildId, ItemId};
    use crate::map::{Block, Desig, Stack, FLAT_Z as Z};
    use crate::world::Order;

    fn world() -> World {
        quiet_world(20, 20, 5)
    }



    fn die(w: &mut World, id: PawnId) {
        let i = w.pawn_index(id).unwrap();
        let mut p = w.pawns[i].clone();
        w.kill(&mut p, "died");
        w.pawns[i] = p;
    }

    #[test]
    fn the_dead_get_buried() {
        let mut w = world();
        let digger = colonist(&mut w, (2, 2, Z));
        let dead = colonist(&mut w, (14, 14, Z));
        die(&mut w, dead);
        run(&mut w, 1.5);
        assert!(w.pawn(digger).unwrap().thoughts().iter().any(|t| t.0 == "A colonist lies unburied"));
        assert!(w.alerts().iter().any(|a| a.0.contains("dig a grave")), "{:?}", w.alerts());

        let grave = (5, 10, Z);
        let kind = BuildId::named("grave");
        assert!(w.apply_order(grave, Order::Build(kind)));
        w.map.drop_item((3, 3, Z), Stack { kind: ItemId::named("wood"), count: 2 });
        run(&mut w, 120.0);
        let p = w.pawn(dead).unwrap();
        assert!(p.is_buried(), "buried");
        assert_eq!(p.pos, grave);
        assert_eq!(w.grave_occupant(grave), Some(dead));
        assert!(w.empty_graves().is_empty());
        assert!(!w.pawn(digger).unwrap().thoughts().iter().any(|t| t.0 == "A colonist lies unburied"));
        assert!(!w.alerts().iter().any(|a| a.0.contains("unburied")));
        assert!(!w.apply_order(grave, Order::Desig(Desig::Deconstruct)), "a filled grave stays");
        // Saved and loaded, they're still there.
        let loaded = World::load_json(&w.save_json().unwrap()).unwrap();
        assert_eq!(loaded.grave_occupant(grave), Some(dead));
    }

    #[test]
    fn graves_go_in_soil() {
        let mut w = world();
        let kind = BuildId::named("grave");
        let rock = w.map.idx((6, 6, Z - 1));
        w.map.set_block(rock, Block::Rock);
        assert!(!w.apply_order((6, 6, Z), Order::Build(kind)), "not into rock");
        let floor = w.map.idx((8, 8, Z + 1));
        w.map.set_floor(floor, true);
        assert!(!w.apply_order((8, 8, Z + 1), Order::Build(kind)), "not into a floor");
        assert!(w.apply_order((10, 10, Z), Order::Build(kind)));
    }
}
