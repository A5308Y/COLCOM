//! Room quality: how big, beautiful, clean and lit each room is. Colonists
//! feel it when they sleep or eat in a room.
//!
//! Themed rooms: a room that has what one of the room kinds in the
//! definitions needs (`defs().rooms`) is that kind of room: a bedroom, a
//! kitchen, a workshop. `RoomStats::kind` says which, and `progress` how far
//! a room is from each kind.

use crate::defs::{defs, BuildId, RoomDef};
use crate::map::Pos;
use crate::world::World;
use std::collections::HashSet;

/// Names of the quality tiers, worst first.
pub const TIERS: [&str; 5] = ["Awful", "Dull", "Decent", "Nice", "Impressive"];

#[derive(Clone, Debug, Default)]
pub struct RoomStats {
    pub size: usize,
    /// Sum of the beauty of everything in it and of its walls and windows.
    pub beauty: f32,
    /// Blood, and items lying around outside stockpiles and shelves.
    pub dirt: f32,
    /// Has a lamp, torch or window.
    pub lit: bool,
    /// How many of each buildable are in it, or in its walls (doors, windows).
    pub counts: Vec<u32>,
    /// The themed room it is (index into `defs().rooms`), if any.
    pub kind: Option<usize>,
    /// Middle of its floor (cell coordinates) and its level, for labels.
    pub centre: (f32, f32),
    pub level: usize,
}

/// How far a room is from being one kind of themed room.
pub struct Progress {
    /// Per need of the kind: how many the room has, and whether that's enough.
    pub needs: Vec<(u32, bool)>,
    /// Beds and workbenches in it that the kind doesn't allow.
    pub unwanted: Vec<BuildId>,
}

impl Progress {
    pub fn done(&self) -> bool {
        self.unwanted.is_empty() && self.needs.iter().all(|n| n.1)
    }

    /// Needs met, out of all needs (unwanted things count as one more unmet).
    pub fn score(&self) -> (usize, usize) {
        let met = self.needs.iter().filter(|n| n.1).count();
        (met, self.needs.len() + (!self.unwanted.is_empty()) as usize)
    }
}

impl RoomStats {
    pub fn space_points(&self) -> f32 {
        match self.size {
            0..=5 => -2.0,
            6..=15 => 0.0,
            16..=35 => 1.0,
            _ => 2.0,
        }
    }

    /// Beauty counts for less the bigger the room gets.
    pub fn beauty_points(&self) -> f32 {
        (self.beauty / (self.size.max(1) as f32).sqrt() * 2.0).clamp(-3.0, 5.0)
    }

    pub fn dirt_points(&self) -> f32 {
        -(self.dirt / self.size.max(1) as f32 * 8.0).min(3.0)
    }

    pub fn light_points(&self) -> f32 {
        if self.lit {
            0.0
        } else {
            -1.0
        }
    }

    /// How many of `kind` it has.
    pub fn count(&self, kind: BuildId) -> u32 {
        self.counts.get(kind.index()).copied().unwrap_or(0)
    }

    /// How it measures up to themed room `room`.
    pub fn progress(&self, room: &RoomDef) -> Progress {
        let needs = room
            .needs
            .iter()
            .map(|n| {
                let have: u32 = n.things.iter().map(|t| self.count(t.id())).sum();
                (have, n.met(have))
            })
            .collect();
        let unwanted = BuildId::all().filter(|&k| self.count(k) > 0 && !room.allows(k)).collect();
        Progress { needs, unwanted }
    }

    /// The name of its themed kind, if it has one.
    pub fn kind_name(&self) -> Option<&'static str> {
        self.kind.map(|k| defs().rooms[k].name.as_str())
    }

    pub fn score(&self) -> f32 {
        self.space_points() + self.beauty_points() + self.dirt_points() + self.light_points()
    }

    /// Index into `TIERS`.
    pub fn tier(&self) -> u8 {
        match self.score() {
            s if s < -1.0 => 0,
            s if s < 1.0 => 1,
            s if s < 3.0 => 2,
            s if s < 5.0 => 3,
            _ => 4,
        }
    }
}

impl World {
    /// Quality of the room `p` is in, if it's in one.
    pub fn room_at(&self, p: Pos) -> Option<&RoomStats> {
        let r = *self.map.room.get(self.map.idx(p))? as usize;
        if r == 0 {
            return None;
        }
        self.room_stats.get(r)
    }

    /// Recount every room (one pass over the map).
    pub(crate) fn update_room_stats(&mut self) {
        let m = &self.map;
        let builds = defs().builds.len();
        let blank = RoomStats { counts: vec![0; builds], ..RoomStats::default() };
        let mut stats = vec![blank; m.room_sizes.len()];
        // Walls and windows bordering a room count once each for it.
        let mut borders: HashSet<(usize, usize)> = HashSet::new();
        for i in 0..m.len() {
            let r = m.room.get(i).copied().unwrap_or(0) as usize;
            if r == 0 || r >= stats.len() {
                continue;
            }
            let st = &mut stats[r];
            st.size += 1;
            let p = m.pos(i);
            st.centre.0 += p.0 as f32;
            st.centre.1 += p.1 as f32;
            st.level = p.2;
            let here = m.feature[i].structure();
            if let Some(d) = here {
                st.beauty += d.beauty;
                st.lit |= d.light.is_some();
            }
            if let Some(k) = m.feature[i].structure_id() {
                st.counts[k.index()] += 1;
            }
            st.dirt += m.blood[i] * 2.0;
            let stored = m.zone[i].stockpile().is_some() || here.is_some_and(|d| d.storage.is_some());
            if m.items[i].is_some() && !stored {
                st.dirt += 1.0;
            }
            for nb in m.neighbors(m.pos(i)) {
                let ni = m.idx(nb);
                if let Some(d) = m.feature[ni].structure().filter(|d| d.blocks_room) {
                    if borders.insert((r, ni)) {
                        st.beauty += d.beauty;
                        st.lit |= d.daylight || d.light.is_some();
                        if let Some(k) = m.feature[ni].structure_id() {
                            st.counts[k.index()] += 1;
                        }
                    }
                }
            }
        }
        for st in stats.iter_mut().filter(|s| s.size > 0) {
            st.centre = (st.centre.0 / st.size as f32, st.centre.1 / st.size as f32);
            st.kind = defs().rooms.iter().position(|room| st.progress(room).done());
        }
        self.room_stats = stats;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Feature, Map, FLAT_Z as Z};
    use crate::rng::Rng;

    /// A roofed 3×3 room (walls around 5..=9) with a door.
    fn room_world() -> World {
        let mut m = Map::flat(16, 16);
        for y in 5..=9 {
            for x in 5..=9 {
                if x == 5 || x == 9 || y == 5 || y == 9 {
                    let i = m.idx((x, y, Z));
                    m.set_feature(i, Feature::built(if (x, y) == (5, 7) { "door" } else { "wall" }));
                }
                let i = m.idx((x, y, Z + 1));
                m.set_floor(i, true);
            }
        }
        m.recompute_rooms();
        let mut w = World::with_map(m, Rng::new(1));
        w.update_room_stats();
        w
    }

    fn place(w: &mut World, at: Pos, id: &str) {
        let i = w.map.idx(at);
        w.map.set_feature(i, Feature::built(id));
        w.update_room_stats();
    }

    #[test]
    fn furniture_and_light_make_a_room_better() {
        let mut w = room_world();
        let bare = w.room_at((7, 7, Z)).unwrap().clone();
        assert_eq!(bare.size, 9);
        assert!(!bare.lit);
        place(&mut w, (6, 6, Z), "bed");
        place(&mut w, (8, 6, Z), "lamp");
        place(&mut w, (8, 8, Z), "table");
        place(&mut w, (8, 5, Z), "window");
        let nice = w.room_at((7, 7, Z)).unwrap();
        assert!(nice.lit);
        assert!(nice.score() > bare.score() + 2.0, "{} vs {}", nice.score(), bare.score());
        assert!(nice.tier() > bare.tier());
        assert!(w.room_at((2, 2, Z)).is_none(), "outdoors");
    }

    #[test]
    fn rooms_with_what_they_need_get_a_theme() {
        let mut w = room_world();
        assert_eq!(w.room_at((7, 7, Z)).unwrap().kind_name(), None, "an empty room");
        place(&mut w, (6, 6, Z), "bed");
        place(&mut w, (8, 6, Z), "dresser");
        let st = w.room_at((7, 7, Z)).unwrap();
        let bedroom = &defs().rooms[0];
        let p = st.progress(bedroom);
        assert_eq!(p.score(), (3, 4), "bed, door (in the wall) and dresser; no window yet");
        assert!(!p.needs[1].1);
        place(&mut w, (7, 5, Z), "window");
        assert_eq!(w.room_at((7, 7, Z)).unwrap().kind_name(), Some("Bedroom"));
        // A second bed: no longer a bedroom (barracks need more).
        place(&mut w, (6, 8, Z), "bed");
        assert_eq!(w.room_at((7, 7, Z)).unwrap().kind_name(), None);
        let p = w.room_at((7, 7, Z)).unwrap().progress(bedroom);
        assert_eq!(p.needs[0], (2, false), "exactly one bed");
    }

    #[test]
    fn workbenches_a_room_doesnt_ask_for_spoil_it() {
        let mut w = room_world();
        place(&mut w, (6, 6, Z), "sawbench");
        place(&mut w, (8, 8, Z), "table");
        assert_eq!(w.room_at((7, 7, Z)).unwrap().kind_name(), Some("Workshop"));
        place(&mut w, (6, 8, Z), "stonecutter");
        assert_eq!(w.room_at((7, 7, Z)).unwrap().kind_name(), Some("Workshop"), "any number of these");
        place(&mut w, (8, 6, Z), "stove");
        let st = w.room_at((7, 7, Z)).unwrap();
        assert_eq!(st.kind_name(), None, "a stove isn't workshop work");
        let workshop = defs().rooms.iter().find(|r| r.id == "workshop").unwrap();
        assert_eq!(st.progress(workshop).unwanted, vec![BuildId::named("stove")]);
    }

    #[test]
    fn every_room_kind_can_be_made() {
        for (k, room) in defs().rooms.iter().enumerate() {
            let mut w = room_world();
            // Things in the walls go in the walls, the rest inside (the room is 3×3: (6..=8, 6..=8)).
            let mut wall = [(6, 5), (7, 5), (8, 5), (6, 9), (7, 9), (8, 9), (9, 6), (9, 8)].into_iter();
            let mut inside = (6..=8).flat_map(|y| (6..=8).map(move |x| (x, y)));
            for need in &room.needs {
                let kind = need.things[0].id();
                for _ in 0..need.min {
                    let (x, y) = if kind.def().blocks_room { wall.next() } else { inside.next() }.expect("room enough");
                    place(&mut w, (x, y, Z), &kind.def().id);
                }
            }
            assert_eq!(w.room_at((7, 7, Z)).and_then(|r| r.kind), Some(k), "{}", room.id);
        }
    }

    #[test]
    fn blood_and_clutter_make_a_room_worse() {
        let mut w = room_world();
        let clean = w.room_at((7, 7, Z)).unwrap().score();
        for x in 6..9 {
            let i = w.map.idx((x, 7, Z));
            w.map.blood[i] = 1.0;
        }
        w.update_room_stats();
        assert!(w.room_at((7, 7, Z)).unwrap().score() < clean - 1.0);
    }

    #[test]
    fn sleeping_in_a_bedroom_remembers_its_quality() {
        let mut w = room_world();
        w.story.next_raid = f32::MAX;
        w.story.next_event = f32::MAX;
        place(&mut w, (7, 7, Z), "bed");
        let id = w.spawn_colonist((6, 6, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 100.0;
        w.pawns[i].rest = 5.0;
        let tier = w.room_at((7, 7, Z)).unwrap().tier();
        let remembered = (0..(200.0 / crate::TICK) as usize).any(|_| {
            w.tick(crate::TICK);
            w.pawn(id).unwrap().memories.iter().any(|m| m.kind == crate::pawn::Memory::Bedroom(tier))
        });
        assert!(remembered);
    }
}
