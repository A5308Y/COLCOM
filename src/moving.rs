//! Moving furniture: the player picks a piece of furniture and a free cell
//! for it (`World::order_move`). A blueprint of it goes up at the new cell
//! (needing no materials), a builder packs the furniture up where it stands
//! (`Task::Uninstall`), and then sets it down at the blueprint like any
//! construction. A workbench keeps its bills and a bed its owner.

use crate::pawn::PawnId;
use crate::defs::{BuildId, Special};
use crate::map::{Feature, Pos, Stack};
use crate::production::Bill;
use crate::world::{Order, World};
use serde::{Deserialize, Serialize};

/// Share of a buildable's work time it takes to pack it up or set it down.
pub const MOVE_SHARE: f32 = 0.5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Move {
    pub kind: BuildId,
    pub from: Pos,
    pub to: Pos,
    /// Packed up: gone from `from`, waiting to be set down at `to`.
    pub lifted: bool,
    /// A workbench's bills, kept while it's packed up.
    pub bills: Vec<Bill>,
    /// Who slept in it, if it's a bed.
    pub owner: Option<PawnId>,
    /// How it was turned where it stood.
    #[serde(default)]
    pub dir: u8,
}

impl World {
    /// The furniture at `p`, if it can be moved: anything built that isn't
    /// part of the building (walls, doors, floors, stairs) or a grave, and
    /// isn't already being moved.
    pub fn movable(&self, p: Pos) -> Option<BuildId> {
        let kind = self.map.feature[self.map.idx(p)].structure_id()?;
        let d = kind.def();
        let fixed = d.blocks_room || matches!(d.special, Special::Floor | Special::Roof | Special::Stairs | Special::Grave);
        (!fixed && self.move_from(p).is_none()).then_some(kind)
    }

    /// Whether the furniture at `from` could go to `to`.
    pub fn move_target_ok(&self, from: Pos, to: Pos) -> bool {
        let Some(kind) = self.movable(from) else { return false };
        to != from && self.map.feature[self.map.idx(to)] == Feature::None && self.order_applies(to, Order::Build(kind))
    }

    /// Have the furniture at `from` moved to `to`, facing the way it does now.
    /// Returns false if it can't go there.
    pub fn order_move(&mut self, from: Pos, to: Pos) -> bool {
        self.order_move_facing(from, to, self.facing_of(from))
    }

    /// How the structure at `p` is turned (0 if there's none).
    pub fn facing_of(&self, p: Pos) -> u8 {
        match self.map.feature[self.map.idx(p)] {
            Feature::Built { dir, .. } => dir,
            _ => 0,
        }
    }

    /// Have the furniture at `from` moved to `to`, turned to `dir` there.
    pub fn order_move_facing(&mut self, from: Pos, to: Pos, dir: u8) -> bool {
        if !self.move_target_ok(from, to) {
            return false;
        }
        let Some(kind) = self.movable(from) else { return false };
        let (fi, ti) = (self.map.idx(from), self.map.idx(to));
        // How it stands now, for if it goes back; what can't be turned keeps that.
        let was = self.facing_of(from);
        let dir = if kind.def().turnable() { dir % 4 } else { was };
        // Moving it instead of taking it down.
        self.map.desig[fi] = None;
        self.map.feature[ti] = Feature::blueprint(kind, 0, dir);
        self.moves.push(Move { kind, from, to, lifted: false, bills: Vec::new(), owner: None, dir: was });
        true
    }

    /// The move of the furniture standing (or that stood) at `p`.
    pub fn move_from(&self, p: Pos) -> Option<&Move> {
        self.moves.iter().find(|m| m.from == p)
    }

    /// The move whose furniture is to be set down at `p`.
    pub fn move_to(&self, p: Pos) -> Option<&Move> {
        self.moves.iter().find(|m| m.to == p)
    }

    /// Call off the move from or to `p`. Returns false if there is none.
    pub(crate) fn cancel_move(&mut self, p: Pos) -> bool {
        let Some(k) = self.moves.iter().position(|m| m.from == p || m.to == p) else { return false };
        let mv = self.moves.remove(k);
        let ti = self.map.idx(mv.to);
        if self.is_move_blueprint(&mv) {
            self.map.feature[ti] = Feature::None;
        }
        if mv.lifted {
            self.unpack(&mv);
        }
        true
    }

    /// Furniture packed up for a move that won't happen: back where it
    /// was if that's still free, or else its materials where it was.
    fn unpack(&mut self, mv: &Move) {
        let fi = self.map.idx(mv.from);
        if self.map.feature[fi] == Feature::None {
            self.set_down(mv, mv.from);
        } else {
            let at = self.map.landing(mv.from);
            for c in &mv.kind.def().cost {
                self.map.drop_item(at, Stack { kind: c.item, count: c.count });
            }
        }
    }

    /// Pack up the furniture at `from` (a builder finished `Task::Uninstall`).
    pub(crate) fn lift(&mut self, from: Pos) {
        let Some(k) = self.moves.iter().position(|m| m.from == from && !m.lifted) else { return };
        let fi = self.map.idx(from);
        if self.map.feature[fi].structure_id() != Some(self.moves[k].kind) {
            return;
        }
        let bills = self.bench_at(from).map(|b| b.bills.clone()).unwrap_or_default();
        let owner = if self.map.feature[fi].is_bed() { self.bed_owner(from) } else { None };
        let storage = self.moves[k].kind.def().storage.is_some();
        let mv = &mut self.moves[k];
        mv.lifted = true;
        mv.bills = bills;
        mv.owner = owner;
        self.map.set_feature(fi, Feature::None);
        self.sound("built", Some(from));
        if storage {
            self.sync_containers();
        }
    }

    /// The blueprint at `to` was built: if it was a move, finish it (bills
    /// and owner come along). Returns whether it was one.
    pub(crate) fn finish_move(&mut self, to: Pos) -> bool {
        let Some(k) = self.moves.iter().position(|m| m.to == to && m.lifted) else { return false };
        let mv = self.moves.remove(k);
        self.set_down(&mv, to);
        true
    }

    /// The blueprint for move `mv` is still where it's going.
    fn is_move_blueprint(&self, mv: &Move) -> bool {
        matches!(self.map.feature[self.map.idx(mv.to)], Feature::Blueprint { kind, delivered: 0, .. } if kind == mv.kind)
    }

    /// Put moved furniture down at `at`, with its bills and owner, turned
    /// as its blueprint is (or as it was, back where it came from).
    fn set_down(&mut self, mv: &Move, at: Pos) {
        let i = self.map.idx(at);
        let dir = match self.map.feature[i] {
            Feature::Blueprint { dir, .. } => dir,
            _ => mv.dir,
        };
        self.map.set_feature(i, Feature::Built { kind: mv.kind, dir });
        // Shove anything lying here out of the way.
        self.map.move_items(i, at);
        if !mv.bills.is_empty() {
            if let Some(b) = self.bench_mut(at) {
                b.bills = mv.bills.clone();
            }
            self.choose_recipes();
        }
        if let Some(id) = mv.owner {
            self.assign_bed(at, Some(id));
        }
        if mv.kind.def().storage.is_some() {
            self.sync_containers();
        }
    }

    /// Once a second: drop moves whose blueprint or furniture is gone
    /// (built over, deconstructed, burnt).
    pub(crate) fn update_moves(&mut self) {
        let broken: Vec<Pos> = self
            .moves
            .iter()
            .filter(|mv| {
                let blueprint = self.is_move_blueprint(mv);
                let there = mv.lifted || self.map.feature[self.map.idx(mv.from)].structure_id() == Some(mv.kind);
                !blueprint || !there
            })
            .map(|mv| mv.to)
            .collect();
        for to in broken {
            self.cancel_move(to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{build, quiet_world, run};
    use crate::map::FLAT_Z as Z;
    use crate::production::BillMode;
    use crate::defs::RecipeId;

    fn test_world() -> World {
        quiet_world(16, 16, 3)
    }



    fn feature(w: &World, at: Pos) -> Feature {
        w.map.feature[w.map.idx(at)]
    }

    #[test]
    fn a_builder_moves_furniture() {
        let mut w = test_world();
        let id = w.spawn_colonist((2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 100.0;
        w.pawns[i].rest = 100.0;
        let (from, to) = ((5, 5, Z), (10, 9, Z));
        build(&mut w, from, "bed");
        assert!(w.assign_bed(from, Some(id)));
        assert!(!w.order_move(from, from), "not onto itself");
        assert!(w.order_move(from, to));
        assert!(!w.order_move(from, (11, 9, Z)), "already on the move");
        assert!(matches!(feature(&w, to), Feature::Blueprint { .. }));
        run(&mut w, 60.0);
        assert!(w.moves.is_empty(), "done");
        assert_eq!(feature(&w, from), Feature::None);
        assert_eq!(feature(&w, to), Feature::built("bed"));
        assert_eq!(w.bed_owner(to), Some(id), "the bed is still theirs");
        assert_eq!(w.item_totals().iter().sum::<u32>(), 0, "no materials needed or left over");
    }

    #[test]
    fn furniture_is_turned_as_it_is_moved() {
        let mut w = test_world();
        w.spawn_colonist((2, 2, Z));
        let (from, to) = ((5, 5, Z), (10, 9, Z));
        let bed = BuildId::named("bed");
        let i = w.map.idx(from);
        w.map.set_feature(i, Feature::Built { kind: bed, dir: 1 });
        assert!(w.order_move_facing(from, to, 3));
        assert_eq!(w.moves[0].dir, 1, "how it stood, for if it goes back");
        run(&mut w, 60.0);
        assert_eq!(feature(&w, to), Feature::Built { kind: bed, dir: 3 });
        // Moved without turning, it keeps facing the same way.
        assert!(w.order_move(to, from));
        run(&mut w, 60.0);
        assert_eq!(feature(&w, from), Feature::Built { kind: bed, dir: 3 });
    }

    #[test]
    fn workbenches_keep_their_bills() {
        let mut w = test_world();
        w.spawn_colonist((2, 2, Z));
        let (from, to) = ((5, 5, Z), (10, 9, Z));
        build(&mut w, from, "stove");
        let cook = RecipeId::named("cook_meal");
        w.add_bill(from, cook, BillMode::Count(3));
        assert!(w.order_move(from, to));
        run(&mut w, 60.0);
        assert_eq!(feature(&w, to), Feature::built("stove"));
        let bills = &w.bench_at(to).expect("a bench there").bills;
        assert_eq!(bills.len(), 1);
        assert_eq!(bills[0].recipe, cook);
    }

    #[test]
    fn moved_furniture_keeps_facing_its_way() {
        let mut w = test_world();
        w.spawn_colonist((2, 2, Z));
        let (from, to) = ((5, 5, Z), (10, 9, Z));
        let i = w.map.idx(from);
        w.map.set_feature(i, Feature::Built { kind: BuildId::named("chair"), dir: 2 });
        assert!(w.order_move(from, to));
        assert!(matches!(feature(&w, to), Feature::Blueprint { dir: 2, .. }));
        run(&mut w, 60.0);
        assert_eq!(feature(&w, to), Feature::Built { kind: BuildId::named("chair"), dir: 2 });
    }

    #[test]
    fn walls_and_occupied_cells_cant_be_used() {
        let mut w = test_world();
        build(&mut w, (5, 5, Z), "wall");
        build(&mut w, (6, 6, Z), "table");
        build(&mut w, (7, 7, Z), "chair");
        assert!(w.movable((5, 5, Z)).is_none(), "walls stay");
        assert!(!w.order_move((6, 6, Z), (7, 7, Z)), "something is there");
        assert!(w.order_move((6, 6, Z), (8, 8, Z)));
    }

    #[test]
    fn cancelling_puts_things_back() {
        let mut w = test_world();
        let (from, to) = ((5, 5, Z), (10, 9, Z));
        build(&mut w, from, "table");
        assert!(w.order_move(from, to));
        w.lift(from);
        assert_eq!(feature(&w, from), Feature::None, "packed up");
        assert!(w.apply_order(to, Order::Cancel));
        assert!(w.moves.is_empty());
        assert_eq!(feature(&w, to), Feature::None, "blueprint gone");
        assert_eq!(feature(&w, from), Feature::built("table"), "set back down");

        // Before it's packed up, cancelling where it stands works too.
        assert!(w.order_move(from, to));
        assert!(w.apply_order(from, Order::Cancel));
        assert!(w.moves.is_empty());
        assert_eq!(feature(&w, to), Feature::None);
    }

    #[test]
    fn building_over_the_blueprint_calls_the_move_off() {
        let mut w = test_world();
        let (from, to) = ((5, 5, Z), (10, 9, Z));
        build(&mut w, from, "table");
        assert!(w.order_move(from, to));
        w.lift(from);
        assert!(w.apply_order(to, Order::Build(BuildId::named("chair"))));
        w.update_moves();
        assert!(w.moves.is_empty());
        assert_eq!(feature(&w, from), Feature::built("table"), "back where it was");
    }
}
