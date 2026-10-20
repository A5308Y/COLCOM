//! Things hung on walls: paintings, posters, a calendar, wall lamps, wall
//! shelves and hanging plants. Each face of a wall has `SLOTS` places side by
//! side. The player plans one in a slot (`World::order_decor`); haulers or
//! builders bring what it costs to the wall, and a builder standing in front of
//! it puts it up (`Task::Decorate`). Taking one down works the same way.
//!
//! Walls are drawn thin, through the middle of their cell (`WALL_T`), joined
//! to the walls, doors, windows and rock beside them. A face's slots run
//! where the wall's body is: a wall ending in a cell has fewer, and beside a
//! wall running off the face (a corner, a T) the slots there face the cell
//! diagonally in front.
//!
//! What hangs in a slot counts for the room it faces: beauty, light. A wall
//! shelf holds a stack in its wall's cell, which is storage as long as it has one.

use crate::defs::{BuildId, ItemId};
use crate::map::{Feature, Map, Pos, Stack, DIRS};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// Places side by side on each face of a wall.
pub const SLOTS: u8 = 5;
/// How thick walls are drawn, as a share of their cell (they run through the middle).
pub const WALL_T: f32 = 0.2;
/// Where a wall's core starts and ends across its cell.
pub const WALL_A: f32 = 0.5 - WALL_T / 2.0;
pub const WALL_B: f32 = 0.5 + WALL_T / 2.0;

/// Something on a wall, or planned there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decor {
    pub kind: BuildId,
    /// The way the face looks out (index into `DIRS`).
    pub face: u8,
    /// Which place on the face, 0..`SLOTS`, counting towards +x (on faces
    /// looking along y) or +y (on faces looking along x).
    pub slot: u8,
    pub state: DecorState,
}

/// Where a piece of wall decor is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecorState {
    /// Planned; `delivered` counts the materials brought so far (in the order of its cost).
    Planned { delivered: u32 },
    Up,
    /// Up, and to be taken down.
    ComingDown,
}

impl Decor {
    /// On the wall (even if it's to come down).
    pub fn is_up(&self) -> bool {
        matches!(self.state, DecorState::Up | DecorState::ComingDown)
    }

    /// Waiting for a builder: everything is there to put it up, or it's to come down.
    pub fn ready(&self) -> bool {
        match self.state {
            DecorState::Planned { delivered } => self.kind.def().missing(delivered).is_none(),
            DecorState::Up => false,
            DecorState::ComingDown => true,
        }
    }

    /// Planned, with materials still to bring: what's missing next.
    pub fn still_needs(&self) -> Option<(ItemId, u32)> {
        match self.state {
            DecorState::Planned { delivered } => self.kind.def().missing(delivered),
            DecorState::Up | DecorState::ComingDown => None,
        }
    }
}

/// The two directions (indices into `DIRS`) a face's slots count from and towards.
pub fn slot_axis(face: u8) -> (usize, usize) {
    if face < 2 {
        (3, 2)
    } else {
        (1, 0)
    }
}

/// Where slot `slot` spans along its face, as shares of the cell.
pub fn slot_span(slot: u8) -> (f32, f32) {
    (slot as f32 / SLOTS as f32, (slot + 1) as f32 / SLOTS as f32)
}

impl Map {
    /// Which sides (by `DIRS` index) the wall at `p` joins up with: walls,
    /// doors, windows, and the natural ground or rock it's built against.
    pub fn wall_links(&self, p: Pos) -> [bool; 4] {
        DIRS.map(|(dx, dy)| {
            self.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32).is_some_and(|q| {
                let i = self.idx(q);
                self.block[i].solid() || self.feature[i].blocks_room()
            })
        })
    }

    /// The open cell slot `slot` on face `face` of the wall at `p` looks
    /// into, if something can hang there: the wall reaches all along the slot
    /// and doesn't run off that way in front of it.
    pub fn decor_front(&self, p: Pos, face: u8, slot: u8) -> Option<Pos> {
        if face > 3 || slot >= SLOTS || !self.feature[self.idx(p)].is_block() {
            return None;
        }
        let links = self.wall_links(p);
        let (minus, plus) = slot_axis(face);
        let (lo, hi) = (if links[minus] { 0.0 } else { WALL_A }, if links[plus] { 1.0 } else { WALL_B });
        let (s0, s1) = slot_span(slot);
        const E: f32 = 1e-4;
        if s0 < lo - E || s1 > hi + E {
            return None;
        }
        let (fx, fy) = DIRS[face as usize];
        let (x, y, z) = (p.0 as i32 + fx, p.1 as i32 + fy, p.2 as i32);
        let front = if links[face as usize] {
            // A wall runs off this way: only beside it, looking at the cell diagonally in front.
            let side = if s1 <= WALL_A + E {
                minus
            } else if s0 >= WALL_B - E {
                plus
            } else {
                return None;
            };
            let (sx, sy) = DIRS[side];
            self.at(x + sx, y + sy, z)
        } else {
            self.at(x, y, z)
        };
        front.filter(|&q| self.open(q))
    }

    /// What hangs on (or is planned for) the wall in cell `i`.
    pub fn decor(&self, i: usize) -> &[Decor] {
        self.decor.get(&(i as u32)).map_or(&[], |v| v.as_slice())
    }

    /// Wall shelves up on the wall in cell `i` (each holds one stack there).
    pub fn wall_shelves(&self, i: usize) -> usize {
        self.decor(i).iter().filter(|d| d.is_up() && d.kind.def().storage.is_some()).count()
    }
}

impl World {
    /// Whether `kind` could be hung in slot `slot` on face `face` of the wall at `p`.
    pub fn decor_ok(&self, p: Pos, face: u8, slot: u8, kind: BuildId) -> bool {
        let m = &self.map;
        let i = m.idx(p);
        kind.def().on_wall
            && matches!(m.feature[i], Feature::Built { .. })
            && m.decor_front(p, face, slot).is_some()
            && !m.decor(i).iter().any(|d| d.face == face && d.slot == slot)
    }

    /// Plan `kind` for slot `slot` on face `face` of the wall at `p`.
    pub fn order_decor(&mut self, p: Pos, face: u8, slot: u8, kind: BuildId) -> bool {
        if !self.decor_ok(p, face, slot, kind) {
            return false;
        }
        let i = self.map.idx(p) as u32;
        let list = self.map.decor.entry(i).or_default();
        list.push(Decor { kind, face, slot, state: DecorState::Planned { delivered: 0 } });
        list.sort_by_key(|d| (d.face, d.slot));
        true
    }

    /// Call off what's planned for a slot, or have what hangs there taken
    /// down (or call that off). Returns false if the slot is empty.
    pub fn cancel_decor(&mut self, p: Pos, face: u8, slot: u8) -> bool {
        let i = self.map.idx(p);
        let Some(k) = self.map.decor(i).iter().position(|d| d.face == face && d.slot == slot) else { return false };
        let list = self.map.decor.get_mut(&(i as u32)).unwrap();
        match list[k].state {
            DecorState::Up => list[k].state = DecorState::ComingDown,
            DecorState::ComingDown => list[k].state = DecorState::Up,
            DecorState::Planned { .. } => {
                let d = list.remove(k);
                self.drop_decor(p, d);
            }
        }
        true
    }

    /// Where whatever comes off the wall at `p` (face `d.face`) lands.
    fn decor_landing(&self, p: Pos, d: &Decor) -> Pos {
        let m = &self.map;
        let at = m.decor_front(p, d.face, d.slot).or_else(|| {
            let (dx, dy) = DIRS[d.face as usize % 4];
            m.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32).filter(|&q| m.open(q))
        });
        m.landing(at.unwrap_or(p))
    }

    /// `d` is gone from the wall at `p` (already out of its list): what was
    /// delivered for it, or what it gives back once up, goes on the floor.
    fn drop_decor(&mut self, p: Pos, d: Decor) {
        let at = self.decor_landing(p, &d);
        let def = d.kind.def();
        let back = match d.state {
            DecorState::Planned { delivered } => def.delivered_split(delivered),
            DecorState::Up | DecorState::ComingDown => def.drops(),
        };
        for (item, count) in back {
            self.map.drop_item(at, Stack { kind: item, count });
        }
        if d.is_up() && def.storage.is_some() {
            self.release_shelved(p);
            self.sync_containers();
        }
    }

    /// The wall at `p` lost a shelf: with none left, what they held goes on
    /// the floor in front (with fewer, `sync_containers` puts down what no longer fits).
    fn release_shelved(&mut self, p: Pos) {
        let i = self.map.idx(p);
        if self.map.wall_shelves(i) > 0 || !self.map.has_items(i) {
            return;
        }
        let m = &self.map;
        let front = (0..4).find_map(|f| {
            let (dx, dy) = DIRS[f];
            m.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32).filter(|&q| m.open(q))
        });
        let at = m.landing(front.unwrap_or(p));
        for s in self.map.take_all(i) {
            self.map.drop_item(at, s);
        }
    }

    /// What the next thing planned for the wall at `p` still needs.
    pub(crate) fn decor_need(&self, p: Pos) -> Option<Stack> {
        let (item, count) = self.map.decor(self.map.idx(p)).iter().find_map(Decor::still_needs)?;
        Some(Stack { kind: item, count })
    }

    /// Hand `carry` over for the next thing planned for the wall at `p`; returns how many were used.
    pub(crate) fn stock_decor(&mut self, p: Pos, carry: Stack) -> u32 {
        let Some(need) = self.decor_need(p).filter(|n| n.kind == carry.kind) else { return 0 };
        let i = self.map.idx(p) as u32;
        let Some(d) = self.map.decor.get_mut(&i).and_then(|l| l.iter_mut().find(|d| d.still_needs().is_some())) else {
            return 0;
        };
        let DecorState::Planned { delivered } = &mut d.state else { return 0 };
        let used = need.count.min(carry.count);
        *delivered += used;
        used
    }

    /// The next thing on the wall at `p` a builder can see to (put up or take
    /// down), and where they'd stand for it.
    pub(crate) fn decor_job(&self, p: Pos) -> Option<(Decor, Pos)> {
        let m = &self.map;
        m.decor(m.idx(p)).iter().filter(|d| d.ready()).find_map(|d| {
            let front = m.decor_front(p, d.face, d.slot).filter(|&f| m.standable(f))?;
            Some((*d, front))
        })
    }

    /// A builder finished with the wall at `p`: the next thing is up, or down.
    pub(crate) fn finish_decor(&mut self, p: Pos) {
        let Some((d, _)) = self.decor_job(p) else { return };
        let i = self.map.idx(p) as u32;
        let list = self.map.decor.get_mut(&i).unwrap();
        let k = list.iter().position(|x| *x == d).unwrap();
        match d.state {
            DecorState::ComingDown => {
                let gone = list.remove(k);
                self.drop_decor(p, gone);
            }
            DecorState::Planned { .. } => {
                list[k].state = DecorState::Up;
                if d.kind.def().storage.is_some() {
                    self.sync_containers();
                }
            }
            // Not ready, so `decor_job` never returns it.
            DecorState::Up => {}
        }
        if self.map.decor.get(&i).is_some_and(|l| l.is_empty()) {
            self.map.decor.remove(&i);
        }
    }

    /// Once a second: what hangs where there's no wall any more, or no room
    /// in front of it, comes down.
    pub(crate) fn update_decor(&mut self) {
        let cells: Vec<u32> = self.map.decor.keys().copied().collect();
        for i in cells {
            let p = self.map.pos(i as usize);
            let wall = self.map.feature[i as usize].is_block();
            let list = self.map.decor.remove(&i).unwrap_or_default();
            let (keep, lost): (Vec<Decor>, Vec<Decor>) =
                list.into_iter().partition(|d| wall && self.map.decor_front(p, d.face, d.slot).is_some());
            if !keep.is_empty() {
                self.map.decor.insert(i, keep);
            }
            for d in lost {
                self.drop_decor(p, d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLAT_Z as Z;
    use crate::testing::{build, colonist, item, quiet_world, run};

    fn painting() -> BuildId {
        BuildId::named("painting")
    }

    /// A straight wall along x from (3, 5) to (7, 5).
    fn walled() -> World {
        let mut w = quiet_world(12, 12, 4);
        for x in 3..=7 {
            build(&mut w, (x, 5, Z), "wall");
        }
        w
    }

    #[test]
    fn slots_run_where_the_wall_is() {
        let w = walled();
        let m = &w.map;
        // The middle of the wall: all five on both faces, looking straight out.
        for slot in 0..SLOTS {
            assert_eq!(m.decor_front((5, 5, Z), 2, slot), Some((5, 6, Z)));
            assert_eq!(m.decor_front((5, 5, Z), 3, slot), Some((5, 4, Z)));
        }
        // Not on the ends of a face along the wall (it joins the next wall there).
        assert_eq!(m.decor_front((5, 5, Z), 0, 2), None);
        // The wall's last cell only reaches halfway across: the three slots on that half.
        let end: Vec<u8> = (0..SLOTS).filter(|&s| m.decor_front((7, 5, Z), 2, s).is_some()).collect();
        assert_eq!(end, [0, 1, 2]);
        // Its end face has just the middle slot.
        let tip: Vec<u8> = (0..SLOTS).filter(|&s| m.decor_front((7, 5, Z), 0, s).is_some()).collect();
        assert_eq!(tip, [2]);
        assert_eq!(m.decor_front((5, 4, Z), 2, 0), None, "only on walls");
    }

    #[test]
    fn in_a_corner_the_slots_look_across() {
        let mut w = walled();
        // A wall running off +y at x = 5: the +y face there is beside it.
        build(&mut w, (5, 6, Z), "wall");
        let m = &w.map;
        assert_eq!(m.decor_front((5, 5, Z), 2, 2), None, "the other wall is in the way");
        assert_eq!(m.decor_front((5, 5, Z), 2, 0), Some((4, 6, Z)));
        assert_eq!(m.decor_front((5, 5, Z), 2, 4), Some((6, 6, Z)));
    }

    #[test]
    fn planned_decor_is_brought_and_put_up() {
        let mut w = walled();
        colonist(&mut w, (2, 2, Z));
        let wall = (5, 5, Z);
        assert!(!w.order_decor(wall, 2, 1, BuildId::named("bed")), "only things for walls");
        assert!(w.order_decor(wall, 2, 1, painting()));
        assert!(!w.order_decor(wall, 2, 1, BuildId::named("poster")), "the slot is taken");
        assert!(w.order_decor(wall, 3, 1, BuildId::named("poster")), "the other side is free");
        for a in painting().def().cost.iter().chain(&BuildId::named("poster").def().cost) {
            w.map.drop_item((2, 8, Z), Stack { kind: a.item, count: a.count });
        }
        run(&mut w, 120.0);
        let i = w.map.idx(wall);
        assert!(w.map.decor(i).iter().all(|d| d.is_up()), "{:?}", w.map.decor(i));
        assert_eq!(w.map.decor(i).len(), 2);
        assert_eq!(w.item_totals().iter().sum::<u32>(), 0, "all used");
        // Taken down, it gives back half.
        assert!(w.cancel_decor(wall, 2, 1));
        run(&mut w, 60.0);
        assert_eq!(w.map.decor(i).len(), 1);
        assert_eq!(w.map.count_items(item("cloth")), 1);
    }

    #[test]
    fn cancelled_plans_give_back_what_was_brought() {
        let mut w = walled();
        let wall = (5, 5, Z);
        assert!(w.order_decor(wall, 2, 0, painting()));
        let need = w.decor_need(wall).unwrap();
        assert_eq!(w.stock_decor(wall, need), need.count);
        assert!(w.cancel_decor(wall, 2, 0));
        assert!(w.map.decor(w.map.idx(wall)).is_empty());
        assert_eq!(w.map.count_items(need.kind), need.count);
    }

    #[test]
    fn decor_comes_down_with_its_wall() {
        let mut w = walled();
        let wall = (5, 5, Z);
        let i = w.map.idx(wall);
        w.map.decor.insert(i as u32, vec![Decor { kind: painting(), face: 2, slot: 2, state: DecorState::Up }]);
        w.update_decor();
        assert_eq!(w.map.decor(i).len(), 1, "still on its wall");
        w.map.set_feature(i, Feature::None);
        w.update_decor();
        assert!(w.map.decor(i).is_empty());
        assert!(w.item_totals().iter().sum::<u32>() > 0, "what it gives back is on the floor");
    }

    #[test]
    fn decor_counts_for_the_room_it_faces() {
        let mut w = quiet_world(12, 12, 4);
        // A roofed 3×3 room inside walls at 2..=6, door on the west.
        for y in 2..=6 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 2 || y == 6 {
                    build(&mut w, (x, y, Z), if (x, y) == (2, 4) { "door" } else { "wall" });
                }
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_roof(i, true);
            }
        }
        w.map.recompute_rooms();
        w.update_room_stats();
        let before = w.room_at((4, 4, Z)).unwrap().beauty;
        assert!(!w.room_at((4, 4, Z)).unwrap().lit);
        let up = |kind: &str, face, slot| Decor { kind: BuildId::named(kind), face, slot, state: DecorState::Up };
        // On the north wall's inside (+y face) and the east wall's outside (+x face).
        let north = w.map.idx((4, 2, Z)) as u32;
        let east = w.map.idx((6, 4, Z)) as u32;
        w.map.decor.insert(north, vec![up("painting", 2, 1), up("wall_lamp", 2, 3)]);
        w.map.decor.insert(east, vec![up("painting", 0, 2)]);
        w.update_room_stats();
        let room = w.room_at((4, 4, Z)).unwrap();
        assert_eq!(room.beauty, before + 4.0 + 0.5, "the one outside doesn't count");
        assert!(room.lit, "the wall lamp lights it");
    }

    #[test]
    fn decor_is_saved() {
        let mut w = walled();
        assert!(w.order_decor((5, 5, Z), 3, 4, painting()));
        let loaded = World::load_json(&w.save_json().unwrap()).unwrap();
        assert_eq!(loaded.map.decor, w.map.decor);
    }

    #[test]
    fn wall_shelves_store_a_stack_each() {
        let mut w = walled();
        let wall = (5, 5, Z);
        let i = w.map.idx(wall);
        let shelf = BuildId::named("wall_shelf");
        let up = |face, slot| Decor { kind: shelf, face, slot, state: DecorState::Up };
        w.map.decor.insert(i as u32, vec![up(2, 0), up(3, 4)]);
        w.sync_containers();
        assert_eq!(w.map.slots(i), 2);
        assert!(w.map.storage_furniture(i) && w.container_at(i), "storage, reached from beside");
        // Haulers stock it from the ground.
        colonist(&mut w, (2, 2, Z));
        w.map.drop_item((2, 8, Z), Stack { kind: item("wood"), count: 30 });
        run(&mut w, 60.0);
        assert_eq!(w.map.count_in(i, item("wood")), 30);
        // Both shelves taken down: what they held goes on the floor.
        assert!(w.cancel_decor(wall, 2, 0) && w.cancel_decor(wall, 3, 4));
        run(&mut w, 60.0);
        assert!(w.map.decor(i).is_empty());
        assert!(!w.map.has_items(i) && !w.container_at(i));
        assert_eq!(w.map.count_items(item("wood")), 30);
        assert_eq!(w.map.count_items(item("planks")), 2, "and a plank back from each shelf");
    }
}
