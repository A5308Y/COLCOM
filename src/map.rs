//! The tile grid: terrain, plants and structures, items on the ground,
//! designations, zones, and the rooms derived from walls.

use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub type Pos = (usize, usize);

pub const STACK_MAX: u32 = 50;
/// Enclosed regions larger than this count as outdoors.
const MAX_ROOM: usize = 400;

pub fn manhattan(a: Pos, b: Pos) -> usize {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Terrain {
    Grass,
    Water,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ItemKind {
    Wood,
    Stone,
    Food,
    Medicine,
}

impl ItemKind {
    pub const ALL: [ItemKind; 4] = [ItemKind::Wood, ItemKind::Stone, ItemKind::Food, ItemKind::Medicine];

    pub fn name(self) -> &'static str {
        match self {
            ItemKind::Wood => "Wood",
            ItemKind::Stone => "Stone",
            ItemKind::Food => "Food",
            ItemKind::Medicine => "Medicine",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Stack {
    pub kind: ItemKind,
    pub count: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BuildKind {
    Wall,
    Door,
    Bed,
}

impl BuildKind {
    pub fn cost(self) -> Stack {
        match self {
            BuildKind::Wall => Stack { kind: ItemKind::Stone, count: 4 },
            BuildKind::Door => Stack { kind: ItemKind::Wood, count: 4 },
            BuildKind::Bed => Stack { kind: ItemKind::Wood, count: 8 },
        }
    }

    pub fn work_time(self) -> f32 {
        match self {
            BuildKind::Wall => 3.0,
            BuildKind::Door => 3.0,
            BuildKind::Bed => 5.0,
        }
    }

    pub fn built(self) -> Feature {
        match self {
            BuildKind::Wall => Feature::Wall,
            BuildKind::Door => Feature::Door,
            BuildKind::Bed => Feature::Bed,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BuildKind::Wall => "Wall",
            BuildKind::Door => "Door",
            BuildKind::Bed => "Bed",
        }
    }
}

/// Whatever occupies a tile on top of its terrain.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Feature {
    None,
    Tree,
    Rock,
    Bush,
    Wall,
    Door,
    Bed,
    Crop { growth: f32 },
    Blueprint { kind: BuildKind, delivered: u32 },
}

impl Feature {
    pub fn passable(self) -> bool {
        matches!(
            self,
            Feature::None | Feature::Door | Feature::Bed | Feature::Crop { .. } | Feature::Blueprint { .. }
        )
    }

    pub fn blocks_room(self) -> bool {
        matches!(self, Feature::Wall | Feature::Door | Feature::Rock)
    }

    pub fn blocks_sight(self) -> bool {
        matches!(self, Feature::Wall | Feature::Door | Feature::Rock)
    }

    /// The build kind this feature was constructed from, if it is a player structure.
    pub fn built_from(self) -> Option<BuildKind> {
        match self {
            Feature::Wall => Some(BuildKind::Wall),
            Feature::Door => Some(BuildKind::Door),
            Feature::Bed => Some(BuildKind::Bed),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Feature::None => "Ground",
            Feature::Tree => "Tree",
            Feature::Rock => "Rock",
            Feature::Bush => "Berry bush",
            Feature::Wall => "Wall",
            Feature::Door => "Door",
            Feature::Bed => "Bed",
            Feature::Crop { .. } => "Crop",
            Feature::Blueprint { .. } => "Blueprint",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Desig {
    Chop,
    Mine,
    Forage,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Zone {
    None,
    Stockpile,
    Grow,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Map {
    pub w: usize,
    pub h: usize,
    pub terrain: Vec<Terrain>,
    pub feature: Vec<Feature>,
    pub items: Vec<Option<Stack>>,
    pub desig: Vec<Option<Desig>>,
    pub zone: Vec<Zone>,
    /// Blood stains, 0..1, fading over time.
    #[serde(default)]
    pub blood: Vec<f32>,
    /// Room id per tile; 0 means outdoors. Derived from structures, so not saved.
    #[serde(skip)]
    pub room: Vec<u32>,
    /// Tile count per room id (index 0 unused).
    #[serde(skip)]
    pub room_sizes: Vec<usize>,
    #[serde(skip)]
    pub rooms_dirty: bool,
}

impl Map {
    pub fn new(w: usize, h: usize) -> Map {
        let n = w * h;
        let mut m = Map {
            w,
            h,
            terrain: vec![Terrain::Grass; n],
            feature: vec![Feature::None; n],
            items: vec![None; n],
            desig: vec![None; n],
            zone: vec![Zone::None; n],
            blood: vec![0.0; n],
            room: Vec::new(),
            room_sizes: Vec::new(),
            rooms_dirty: true,
        };
        m.recompute_rooms();
        m
    }

    pub fn idx(&self, p: Pos) -> usize {
        p.1 * self.w + p.0
    }

    pub fn pos(&self, i: usize) -> Pos {
        (i % self.w, i / self.w)
    }

    pub fn neighbors(&self, p: Pos) -> impl Iterator<Item = Pos> {
        let (w, h) = (self.w as i32, self.h as i32);
        let (x, y) = (p.0 as i32, p.1 as i32);
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .filter_map(move |(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w && ny < h {
                    Some((nx as usize, ny as usize))
                } else {
                    None
                }
            })
    }

    pub fn passable(&self, p: Pos) -> bool {
        let i = self.idx(p);
        self.terrain[i] != Terrain::Water && self.feature[i].passable()
    }

    /// Replace a tile's feature, flagging rooms for recomputation if walls changed.
    pub fn set_feature(&mut self, i: usize, f: Feature) {
        if self.feature[i].blocks_room() != f.blocks_room() {
            self.rooms_dirty = true;
        }
        self.feature[i] = f;
    }

    pub fn roofed(&self, p: Pos) -> bool {
        self.room.get(self.idx(p)).map_or(false, |&r| r != 0)
    }

    /// Whether `kind` can be put down on tile `i`.
    pub fn can_accept(&self, i: usize, kind: ItemKind) -> bool {
        self.passable(self.pos(i))
            && !matches!(self.feature[i], Feature::Blueprint { .. })
            && self.items[i].map_or(true, |s| s.kind == kind && s.count < STACK_MAX)
    }

    /// Take up to `max` of `kind` from tile `i`.
    pub fn take_item(&mut self, i: usize, kind: ItemKind, max: u32) -> Option<Stack> {
        let s = self.items[i]?;
        if s.kind != kind || max == 0 {
            return None;
        }
        let n = max.min(s.count);
        self.items[i] = if s.count > n { Some(Stack { kind, count: s.count - n }) } else { None };
        Some(Stack { kind, count: n })
    }

    /// Put a stack down at `start`, spilling onto the nearest free tiles.
    pub fn drop_item(&mut self, start: Pos, mut stack: Stack) {
        let mut seen = vec![false; self.w * self.h];
        let mut q = VecDeque::new();
        seen[self.idx(start)] = true;
        q.push_back(start);
        while let Some(p) = q.pop_front() {
            let i = self.idx(p);
            if self.can_accept(i, stack.kind) {
                let have = self.items[i].map_or(0, |s| s.count);
                let put = (STACK_MAX - have).min(stack.count);
                self.items[i] = Some(Stack { kind: stack.kind, count: have + put });
                stack.count -= put;
                if stack.count == 0 {
                    return;
                }
            }
            for nb in self.neighbors(p) {
                let ni = self.idx(nb);
                if !seen[ni] && self.passable(nb) {
                    seen[ni] = true;
                    q.push_back(nb);
                }
            }
        }
    }

    pub fn count_items(&self, kind: ItemKind) -> u32 {
        self.items.iter().flatten().filter(|s| s.kind == kind).map(|s| s.count).sum()
    }

    /// Flood-fill the areas bounded by walls, doors and rock. Small enclosed
    /// areas that don't touch the map edge become roofed rooms.
    pub fn recompute_rooms(&mut self) {
        let n = self.w * self.h;
        self.room = vec![0; n];
        self.room_sizes = vec![0];
        let mut seen = vec![false; n];
        for start in 0..n {
            if seen[start] || self.feature[start].blocks_room() {
                continue;
            }
            seen[start] = true;
            let mut region = vec![start];
            let mut stack = vec![start];
            let mut touches_edge = false;
            while let Some(i) = stack.pop() {
                let p = self.pos(i);
                if p.0 == 0 || p.1 == 0 || p.0 == self.w - 1 || p.1 == self.h - 1 {
                    touches_edge = true;
                }
                for nb in self.neighbors(p) {
                    let ni = self.idx(nb);
                    if !seen[ni] && !self.feature[ni].blocks_room() {
                        seen[ni] = true;
                        stack.push(ni);
                        region.push(ni);
                    }
                }
            }
            if !touches_edge && region.len() <= MAX_ROOM {
                let id = self.room_sizes.len() as u32;
                self.room_sizes.push(region.len());
                for &i in &region {
                    self.room[i] = id;
                }
            }
        }
        self.rooms_dirty = false;
    }

    pub fn generate(w: usize, h: usize, rng: &mut Rng) -> Map {
        let mut m = Map::new(w, h);
        for _ in 0..12 {
            let r = 3 + rng.below(6);
            let (cx, cy) = (rng.below(w), rng.below(h));
            for i in blob(&m, rng, cx, cy, r) {
                m.feature[i] = Feature::Rock;
            }
        }
        for _ in 0..4 {
            let r = 2 + rng.below(4);
            let (cx, cy) = (rng.below(w), rng.below(h));
            for i in blob(&m, rng, cx, cy, r) {
                m.terrain[i] = Terrain::Water;
                m.feature[i] = Feature::None;
            }
        }
        for i in 0..w * h {
            if m.feature[i] == Feature::None && m.terrain[i] == Terrain::Grass {
                let r = rng.f32();
                if r < 0.11 {
                    m.feature[i] = Feature::Tree;
                } else if r < 0.135 {
                    m.feature[i] = Feature::Bush;
                }
            }
        }
        // Landing zone in the middle.
        let (cx, cy) = (w / 2, h / 2);
        for y in cy - 6..=cy + 6 {
            for x in cx - 8..=cx + 8 {
                let i = m.idx((x, y));
                m.terrain[i] = Terrain::Grass;
                m.feature[i] = Feature::None;
            }
        }
        m.recompute_rooms();
        m
    }
}

fn blob(m: &Map, rng: &mut Rng, cx: usize, cy: usize, r: usize) -> Vec<usize> {
    let (cx, cy, r) = (cx as i32, cy as i32, r as i32);
    let mut out = Vec::new();
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            if x < 0 || y < 0 || x >= m.w as i32 || y >= m.h as i32 {
                continue;
            }
            let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
            if d <= r as f32 - rng.f32() * 1.5 {
                out.push(m.idx((x as usize, y as usize)));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_box(m: &mut Map, x0: usize, y0: usize, x1: usize, y1: usize) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x == x0 || x == x1 || y == y0 || y == y1 {
                    let i = m.idx((x, y));
                    m.set_feature(i, Feature::Wall);
                }
            }
        }
        m.recompute_rooms();
    }

    #[test]
    fn enclosed_area_becomes_a_roofed_room() {
        let mut m = Map::new(20, 20);
        wall_box(&mut m, 5, 5, 9, 9);
        assert!(m.roofed((7, 7)));
        assert!(!m.roofed((2, 2)));
        assert_eq!(m.room_sizes[m.room[m.idx((7, 7))] as usize], 9);

        let gap = m.idx((5, 7));
        m.set_feature(gap, Feature::None);
        assert!(m.rooms_dirty);
        m.recompute_rooms();
        assert!(!m.roofed((7, 7)));
    }

    #[test]
    fn doors_close_rooms() {
        let mut m = Map::new(20, 20);
        wall_box(&mut m, 5, 5, 9, 9);
        let i = m.idx((5, 7));
        m.set_feature(i, Feature::Door);
        m.recompute_rooms();
        assert!(m.roofed((7, 7)));
    }

    #[test]
    fn drop_item_spills_over_full_tiles() {
        let mut m = Map::new(10, 10);
        m.drop_item((5, 5), Stack { kind: ItemKind::Wood, count: 120 });
        assert_eq!(m.count_items(ItemKind::Wood), 120);
        assert_eq!(m.items[m.idx((5, 5))].unwrap().count, STACK_MAX);
    }

    #[test]
    fn take_item_respects_kind_and_amount() {
        let mut m = Map::new(5, 5);
        let i = m.idx((1, 1));
        m.items[i] = Some(Stack { kind: ItemKind::Food, count: 3 });
        assert!(m.take_item(i, ItemKind::Wood, 1).is_none());
        assert_eq!(m.take_item(i, ItemKind::Food, 2).unwrap().count, 2);
        assert_eq!(m.take_item(i, ItemKind::Food, 5).unwrap().count, 1);
        assert!(m.items[i].is_none());
    }
}
