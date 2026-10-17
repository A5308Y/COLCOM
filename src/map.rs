//! The world as a 3D grid of cells: natural blocks (soil, rock, water), things
//! placed in open cells (plants, walls, furniture), items, designations,
//! zones, and the rooms derived from walls and rock.
//!
//! A pawn stands in an open cell that has support: solid ground below, a
//! built floor, or stairs below. It can walk to a horizontal neighbour on the
//! same level, step one level up or down between cells on natural ground when
//! there is headroom, or use stairs. Built walls and floors are never climbed.

use crate::defs::{BuildDef, BuildId, ItemId, PlantId, Special};
use crate::rng::Rng;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// (x, y, level). Level 0 is the bottom of the world.
pub type Pos = (usize, usize, usize);

/// Most of one item a pawn carries, or a cell holds without storage furniture.
pub const STACK_MAX: u32 = 50;
/// Enclosed regions larger than this count as outdoors.
const MAX_ROOM: usize = 400;
/// Standing level of the flat maps used by tests and `Map::flat`.
pub const FLAT_Z: usize = 1;
/// A floor needs something holding it up within this many steps on its level.
pub const SUPPORT_RANGE: i32 = 3;
/// Water fills low ground up to this level.
const WATER_LEVEL: usize = 4;
/// Standing level of plains and the landing zone.
const PLAINS: usize = 5;

pub fn manhattan(a: Pos, b: Pos) -> usize {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1) + a.2.abs_diff(b.2)
}

/// Close enough to work on `target` from `a`: one step away horizontally (up
/// to one level up or down), or directly above or below it.
pub fn adjacent(a: Pos, target: Pos) -> bool {
    let flat = a.0.abs_diff(target.0) + a.1.abs_diff(target.1);
    (flat == 1 && a.2.abs_diff(target.2) <= 1) || (flat == 0 && a.2.abs_diff(target.2) == 1)
}

/// The four horizontal directions; stairs and other turned structures store an index into this.
pub const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Block {
    Air,
    Soil,
    Rock,
    Water,
    /// Rock with iron in it (mining yields `ore_drops`).
    Ore,
}

impl Block {
    /// Solid ground: something to stand on, and impassable.
    pub fn solid(self) -> bool {
        matches!(self, Block::Soil | Block::Rock | Block::Ore)
    }

    pub fn name(self) -> &'static str {
        match self {
            Block::Air => "Open",
            Block::Soil => "Soil",
            Block::Rock => "Rock",
            Block::Water => "Water",
            Block::Ore => "Iron ore",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Stack {
    pub kind: ItemId,
    pub count: u32,
}

/// Whatever occupies an open cell.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum Feature {
    None,
    /// Trees, bushes, crops. `growth` runs from 0 to 1.
    Plant { kind: PlantId, growth: f32 },
    /// A finished structure. For stairs, the steps rise towards `DIRS[dir]`
    /// and the only way in or out at this level is from the foot (the
    /// opposite side).
    Built { kind: BuildId, dir: u8 },
    /// `dir`: how it's turned once built (as in `Built`).
    Blueprint {
        kind: BuildId,
        delivered: u32,
        #[serde(default)]
        dir: u8,
    },
}

impl Feature {
    /// A finished structure, by definition id (tests and fixed content).
    pub fn built(id: &str) -> Feature {
        Feature::Built { kind: BuildId::named(id), dir: 0 }
    }

    /// A fully grown plant, by definition id.
    pub fn plant(id: &str) -> Feature {
        Feature::Plant { kind: PlantId::named(id), growth: 1.0 }
    }

    /// Which buildable the finished structure here is, if any.
    pub fn structure_id(self) -> Option<BuildId> {
        match self {
            Feature::Built { kind, .. } => Some(kind),
            _ => None,
        }
    }

    /// The finished structure here, if any.
    pub fn structure(self) -> Option<&'static BuildDef> {
        match self {
            Feature::Built { kind, .. } => Some(kind.def()),
            _ => None,
        }
    }

    pub fn passable(self) -> bool {
        match self {
            Feature::None | Feature::Blueprint { .. } => true,
            Feature::Plant { kind, .. } => kind.def().passable,
            Feature::Built { kind, .. } => kind.def().passable,
        }
    }

    pub fn blocks_room(self) -> bool {
        self.structure().is_some_and(|d| d.blocks_room)
    }

    pub fn blocks_sight(self) -> bool {
        self.structure().is_some_and(|d| d.blocks_sight)
    }

    /// A structure drawn as a solid block (walls).
    pub fn is_block(self) -> bool {
        self.structure().is_some_and(|d| d.is_block())
    }

    pub fn is_special(self, s: Special) -> bool {
        self.structure().is_some_and(|d| d.special == s)
    }

    pub fn is_bed(self) -> bool {
        self.is_special(Special::Bed)
    }

    /// A sown plant (crops in grow zones).
    pub fn is_crop(self) -> bool {
        matches!(self, Feature::Plant { kind, .. } if kind.def().sow.is_some())
    }

    /// The designation a player uses to have this harvested, if it's a plant that needs one.
    pub fn harvest_desig(self) -> Option<Desig> {
        match self {
            Feature::Plant { kind, .. } => kind.def().harvest.desig,
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Feature::None => "Ground",
            Feature::Plant { kind, .. } => kind.name(),
            Feature::Built { kind, .. } => kind.name(),
            Feature::Blueprint { .. } => "Blueprint",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Desig {
    Chop,
    Mine,
    Forage,
    /// Take down a structure or floor, getting part of its cost back.
    Deconstruct,
    /// Fill an open cell back in with soil (a hole, a tunnel, or to raise the ground).
    Fill,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Zone {
    None,
    /// Part of a stockpile (an id into `World::stockpiles`, which holds its settings).
    Stockpile(u16),
    /// Planted with this crop.
    Grow(PlantId),
    /// Where civilians shelter during battles.
    Safe,
}

impl Zone {
    pub fn stockpile(self) -> Option<u16> {
        match self {
            Zone::Stockpile(id) => Some(id),
            _ => None,
        }
    }

    pub fn crop(self) -> Option<PlantId> {
        match self {
            Zone::Grow(kind) => Some(kind),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Map {
    pub w: usize,
    pub h: usize,
    /// Number of levels.
    pub d: usize,
    #[serde(with = "crate::save::rle")]
    pub block: Vec<Block>,
    #[serde(with = "crate::save::rle")]
    pub feature: Vec<Feature>,
    #[serde(with = "crate::save::rle")]
    pub items: Vec<Option<Stack>>,
    #[serde(with = "crate::save::rle")]
    pub desig: Vec<Option<Desig>>,
    #[serde(with = "crate::save::rle")]
    pub zone: Vec<Zone>,
    /// Built floor slabs (at the bottom of the cell).
    #[serde(default, with = "crate::save::rle")]
    pub floor: Vec<bool>,
    /// Blood stains, 0..1, fading over time.
    #[serde(default, with = "crate::save::rle")]
    pub blood: Vec<f32>,
    /// Built roofs, in the cell above a room (where a floor would go). They
    /// close the room but can't be stood on.
    #[serde(default, with = "crate::save::rle")]
    pub roof: Vec<bool>,
    /// Room id per cell; 0 means outdoors. Derived from walls and rock, so not saved.
    #[serde(skip)]
    pub room: Vec<u32>,
    /// Cell count per room id (index 0 unused).
    #[serde(skip)]
    pub room_sizes: Vec<usize>,
    #[serde(skip)]
    pub rooms_dirty: bool,
    /// Cells that should get a roof, found with the rooms: above enclosed
    /// areas not yet covered (see `recompute_rooms`).
    #[serde(skip)]
    pub roof_wanted: Vec<usize>,
    /// Cells where a room wants a roof but none would be held up (no wall
    /// within `SUPPORT_RANGE`), so the room can't be closed. Found with `roof_wanted`.
    #[serde(skip)]
    pub roof_gaps: Vec<usize>,
    /// Bumped whenever a roof is built or removed, or rooms are recomputed (the
    /// walls a roof reaches over may have changed), so the front end knows to redraw.
    #[serde(skip)]
    pub roof_changes: u64,
    /// Something that held things up changed; check for falling.
    #[serde(skip)]
    pub support_dirty: bool,
}

impl Map {
    /// All air.
    pub fn empty(w: usize, h: usize, d: usize) -> Map {
        let n = w * h * d;
        let mut m = Map {
            w,
            h,
            d,
            block: vec![Block::Air; n],
            feature: vec![Feature::None; n],
            items: vec![None; n],
            desig: vec![None; n],
            zone: vec![Zone::None; n],
            floor: vec![false; n],
            blood: vec![0.0; n],
            roof: vec![false; n],
            room: Vec::new(),
            room_sizes: Vec::new(),
            rooms_dirty: true,
            roof_wanted: Vec::new(),
            roof_gaps: Vec::new(),
            roof_changes: 0,
            support_dirty: false,
        };
        m.recompute_rooms();
        m
    }

    /// Flat soil with open ground at level `FLAT_Z` (used by tests).
    pub fn flat(w: usize, h: usize) -> Map {
        let mut m = Map::empty(w, h, FLAT_Z + 3);
        for y in 0..h {
            for x in 0..w {
                for z in 0..FLAT_Z {
                    let i = m.idx((x, y, z));
                    m.block[i] = Block::Soil;
                }
            }
        }
        m.recompute_rooms();
        m
    }

    pub fn len(&self) -> usize {
        self.w * self.h * self.d
    }

    pub fn idx(&self, p: Pos) -> usize {
        (p.2 * self.h + p.1) * self.w + p.0
    }

    pub fn pos(&self, i: usize) -> Pos {
        let layer = self.w * self.h;
        (i % self.w, (i % layer) / self.w, i / layer)
    }

    /// Cell at signed coordinates, if inside the map.
    pub fn at(&self, x: i32, y: i32, z: i32) -> Option<Pos> {
        (x >= 0 && y >= 0 && z >= 0 && (x as usize) < self.w && (y as usize) < self.h && (z as usize) < self.d)
            .then(|| (x as usize, y as usize, z as usize))
    }

    pub fn below(&self, p: Pos) -> Option<Pos> {
        (p.2 > 0).then(|| (p.0, p.1, p.2 - 1))
    }

    pub fn above(&self, p: Pos) -> Option<Pos> {
        (p.2 + 1 < self.d).then(|| (p.0, p.1, p.2 + 1))
    }

    pub fn solid(&self, p: Pos) -> bool {
        self.block[self.idx(p)].solid()
    }

    /// Open air that nothing blocks (room to walk through or stand in).
    pub fn open(&self, p: Pos) -> bool {
        let i = self.idx(p);
        self.block[i] == Block::Air && self.feature[i].passable()
    }

    /// Solid natural ground directly underneath.
    pub fn on_ground(&self, p: Pos) -> bool {
        self.below(p).is_some_and(|b| self.solid(b))
    }

    /// Something to stand on: ground, a built floor, or the top of stairs.
    pub fn supported(&self, p: Pos) -> bool {
        self.on_ground(p)
            || self.floor[self.idx(p)]
            || self.below(p).is_some_and(|b| self.stairs_dir(b).is_some())
    }

    /// Open and supported: a pawn can stand here.
    pub fn standable(&self, p: Pos) -> bool {
        self.open(p) && self.supported(p)
    }

    /// Alias kept for readability at call sites about movement.
    pub fn passable(&self, p: Pos) -> bool {
        self.standable(p)
    }

    /// Where a pawn standing at `p` can get to in one step.
    pub fn walk_neighbors(&self, p: Pos) -> impl Iterator<Item = Pos> + '_ {
        let flat = DIRS.into_iter().filter_map(move |(dx, dy)| self.step(p, dx, dy));
        // Stairs lead straight up from their cell, and down into it.
        let up = self.above(p).filter(|&a| self.stairs_dir(p).is_some() && self.standable(a));
        let down = self.below(p).filter(|&b| self.stairs_dir(b).is_some() && self.standable(b));
        flat.chain(up).chain(down)
    }

    /// Rising direction of stairs in cell `p`, if there are stairs.
    pub fn stairs_dir(&self, p: Pos) -> Option<(i32, i32)> {
        match self.feature[self.idx(p)] {
            Feature::Built { kind, dir } if kind.def().special == Special::Stairs => Some(DIRS[dir as usize % 4]),
            _ => None,
        }
    }

    /// Which way new stairs at `p` should rise: away from walkable ground at
    /// their foot, ideally towards somewhere to stand at the top.
    pub fn stairs_facing(&self, p: Pos) -> u8 {
        (0..4).max_by_key(|&d| (self.stairs_score(p, d), 4 - d)).unwrap_or(0)
    }

    /// Whether stairs at `p` rising towards `DIRS[dir]` can be walked at both ends.
    pub fn stairs_fit(&self, p: Pos, dir: u8) -> bool {
        self.stairs_score(p, dir) == 3
    }

    /// 2 if the foot can be stood on, plus 1 if the top can.
    fn stairs_score(&self, p: Pos, dir: u8) -> u8 {
        let (dx, dy) = DIRS[dir as usize % 4];
        let (x, y, z) = (p.0 as i32, p.1 as i32, p.2 as i32);
        let foot = self.at(x - dx, y - dy, z).is_some_and(|f| self.standable(f));
        let top = self.at(x + dx, y + dy, z + 1).is_some_and(|t| self.standable(t));
        2 * foot as u8 + top as u8
    }

    fn step(&self, p: Pos, dx: i32, dy: i32) -> Option<Pos> {
        // Stairs are only left through their foot (or by climbing).
        if let Some((sx, sy)) = self.stairs_dir(p) {
            if (dx, dy) != (-sx, -sy) {
                return None;
            }
        }
        let (x, y) = (p.0 as i32 + dx, p.1 as i32 + dy);
        let z = p.2 as i32;
        let same = self.at(x, y, z)?;
        // ...and only entered from their foot: no walking across them.
        if let Some(rise) = self.stairs_dir(same) {
            return (rise == (dx, dy) && self.standable(same)).then_some(same);
        }
        if self.standable(same) {
            return Some(same);
        }
        // Slopes only exist in natural ground: never climb onto floors or walls.
        if !self.on_ground(p) {
            return None;
        }
        // Up onto a one-level rise, if there's headroom above us.
        if let (Some(up), Some(head)) = (self.at(x, y, z + 1), self.above(p)) {
            if self.standable(up) && self.on_ground(up) && self.open(head) && self.stairs_dir(up).is_none() {
                return Some(up);
            }
        }
        // Down a one-level drop, through the open cell beside us.
        if let Some(down) = self.at(x, y, z - 1) {
            if self.standable(down) && self.on_ground(down) && self.open(same) && self.stairs_dir(down).is_none() {
                return Some(down);
            }
        }
        None
    }

    /// Same-level horizontal neighbours inside the map.
    pub fn neighbors(&self, p: Pos) -> impl Iterator<Item = Pos> {
        let (w, h) = (self.w as i32, self.h as i32);
        let (x, y, z) = (p.0 as i32, p.1 as i32, p.2);
        DIRS.into_iter().filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            (nx >= 0 && ny >= 0 && nx < w && ny < h).then(|| (nx as usize, ny as usize, z))
        })
    }

    /// Cells a pawn could work on `p` from: horizontal neighbours one level
    /// below, at, or above it, and the cells directly above (digging down)
    /// and below (building a ceiling overhead).
    pub fn around(&self, p: Pos) -> impl Iterator<Item = Pos> + '_ {
        DIRS.into_iter()
            .flat_map(move |(dx, dy)| {
                (-1..=1).filter_map(move |dz| self.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32 + dz))
            })
            .chain(self.above(p))
            .chain(self.below(p))
    }

    /// Something a Mine order can dig out: rock, ore or soil.
    pub fn mineable(&self, p: Pos) -> bool {
        self.block[self.idx(p)].solid()
    }

    /// Where to work from to fill `p`: the lowest open cell in its shaft (up
    /// to 6 above) with ground beside it to stand on, so deep holes are
    /// filled from the rim.
    pub fn fill_spot(&self, p: Pos) -> Pos {
        let mut c = p;
        for _ in 0..6 {
            if self.around(c).any(|n| n != p && self.standable(n)) {
                return c;
            }
            match self.above(c) {
                Some(a) if self.block[self.idx(a)] == Block::Air => c = a,
                _ => break,
            }
        }
        p
    }

    /// An open cell with nothing built in it, which could be filled with
    /// soil: now if it's on the ground, or (`pending`) once the cell below is.
    pub fn fillable(&self, p: Pos, pending: bool) -> bool {
        let i = self.idx(p);
        let empty = self.block[i] == Block::Air && self.feature[i] == Feature::None && !self.floor[i] && !self.roof[i];
        let below = self.below(p).map(|b| self.idx(b));
        let ground = below.is_some_and(|b| self.block[b].solid() && self.block[b] != Block::Water);
        let filling = below.is_some_and(|b| self.desig[b] == Some(Desig::Fill));
        empty && (ground || pending && filling)
    }

    /// Something a Deconstruct order can take down: a structure, a floor or a roof.
    pub fn deconstructable(&self, p: Pos) -> bool {
        let i = self.idx(p);
        self.feature[i].structure().is_some() || self.floor[i] || self.roof[i]
    }

    /// Where a floor could be built: open air with nothing in it (or a roof,
    /// which the floor replaces), not already on the ground, and held up by
    /// something nearby.
    pub fn floor_buildable(&self, p: Pos) -> bool {
        let i = self.idx(p);
        self.block[i] == Block::Air
            && self.feature[i] == Feature::None
            && !self.floor[i]
            && !self.on_ground(p)
            && self.floor_supported(p)
    }

    /// Highest cell in a column that can be stood on.
    pub fn surface(&self, x: usize, y: usize) -> Option<usize> {
        (1..self.d).rev().find(|&z| self.standable((x, y, z)))
    }

    /// Replace a cell's feature, flagging rooms for recomputation if walls changed.
    pub fn set_feature(&mut self, i: usize, f: Feature) {
        if self.feature[i].blocks_room() != f.blocks_room() {
            self.rooms_dirty = true;
        }
        self.feature[i] = f;
    }

    pub fn set_block(&mut self, i: usize, b: Block) {
        if self.block[i] != b {
            self.rooms_dirty = true;
            self.support_dirty = true;
        }
        self.block[i] = b;
    }

    pub fn set_floor(&mut self, i: usize, on: bool) {
        if self.floor[i] != on {
            self.rooms_dirty = true;
            self.support_dirty = true;
        }
        self.floor[i] = on;
    }

    pub fn set_roof(&mut self, i: usize, on: bool) {
        if self.roof[i] != on {
            self.rooms_dirty = true;
            self.roof_changes += 1;
        }
        self.roof[i] = on;
    }

    /// A roof blueprint (put there by `World::mark_roofs`).
    pub fn roof_blueprint(&self, i: usize) -> bool {
        matches!(self.feature[i], Feature::Blueprint { kind, .. } if kind.def().special == Special::Roof)
    }

    /// Whether a floor at `p` would be held up: solid ground or a structure
    /// that holds floors (walls, stairs) directly beneath some cell within
    /// `SUPPORT_RANGE` on its level.
    pub fn floor_supported(&self, p: Pos) -> bool {
        let (x, y, z) = (p.0 as i32, p.1 as i32, p.2 as i32);
        let r = SUPPORT_RANGE;
        (-r..=r).any(|dy| {
            (-r..=r).any(|dx| {
                dx.abs() + dy.abs() <= r
                    && self.at(x + dx, y + dy, z - 1).is_some_and(|q| {
                        let i = self.idx(q);
                        self.block[i].solid() || self.feature[i].structure().is_some_and(|d| d.holds_floors)
                    })
            })
        })
    }

    /// Where something unsupported at `p` comes to rest: the first supported
    /// cell straight down (or the bottom of the world).
    pub fn landing(&self, p: Pos) -> Pos {
        let mut q = p;
        while q.2 > 0 && !self.supported(q) {
            let next = (q.0, q.1, q.2 - 1);
            if self.block[self.idx(next)] != Block::Air {
                break;
            }
            q = next;
        }
        q
    }

    /// Whether something built into a wall line at `p` (a door, a window)
    /// runs along x: there's a wall, door or solid ground beside it in x.
    pub fn runs_along_x(&self, p: Pos) -> bool {
        [-1, 1].into_iter().any(|dx| {
            self.at(p.0 as i32 + dx, p.1 as i32, p.2 as i32).is_some_and(|q| {
                let i = self.idx(q);
                self.block[i].solid() || self.feature[i].blocks_room()
            })
        })
    }

    /// Inside an enclosed room with a roof.
    pub fn roofed(&self, p: Pos) -> bool {
        self.room.get(self.idx(p)).is_some_and(|&r| r != 0)
    }

    /// Something overhead: rock, soil, a wall, or a floor or roof on a level above.
    pub fn covered(&self, p: Pos) -> bool {
        (p.2 + 1..self.d).any(|z| {
            let i = self.idx((p.0, p.1, z));
            self.block[i].solid() || self.feature[i].blocks_room() || self.floor[i] || self.roof[i]
        })
    }

    /// How many of one item cell `i` holds: more on shelves.
    pub fn capacity(&self, i: usize) -> u32 {
        self.feature[i].structure().and_then(|d| d.storage.as_ref()).map_or(STACK_MAX, |s| s.stack_limit)
    }

    /// Whether `kind` can be put down in cell `i`.
    pub fn can_accept(&self, i: usize, kind: ItemId) -> bool {
        self.standable(self.pos(i))
            && !matches!(self.feature[i], Feature::Blueprint { .. })
            && self.items[i].map_or(true, |s| s.kind == kind && s.count < self.capacity(i))
    }

    /// Take up to `max` of `kind` from cell `i`.
    pub fn take_item(&mut self, i: usize, kind: ItemId, max: u32) -> Option<Stack> {
        let s = self.items[i]?;
        if s.kind != kind || max == 0 {
            return None;
        }
        let n = max.min(s.count);
        self.items[i] = if s.count > n { Some(Stack { kind, count: s.count - n }) } else { None };
        Some(Stack { kind, count: n })
    }

    /// Put a stack down at `start`, spilling onto the nearest free cells a pawn could walk to.
    pub fn drop_item(&mut self, start: Pos, mut stack: Stack) {
        let mut seen = vec![false; self.len()];
        let mut q = VecDeque::new();
        seen[self.idx(start)] = true;
        q.push_back(start);
        while let Some(p) = q.pop_front() {
            let i = self.idx(p);
            if self.can_accept(i, stack.kind) {
                let have = self.items[i].map_or(0, |s| s.count);
                let put = (self.capacity(i) - have).min(stack.count);
                self.items[i] = Some(Stack { kind: stack.kind, count: have + put });
                stack.count -= put;
                if stack.count == 0 {
                    return;
                }
            }
            let next: Vec<Pos> = self.walk_neighbors(p).collect();
            for nb in next {
                let ni = self.idx(nb);
                if !seen[ni] {
                    seen[ni] = true;
                    q.push_back(nb);
                }
            }
        }
    }

    pub fn count_items(&self, kind: ItemId) -> u32 {
        self.count_items_where(|k| k == kind)
    }

    /// Total of all items whose kind passes `f` (all food, say).
    pub fn count_items_where(&self, f: impl Fn(ItemId) -> bool) -> u32 {
        self.items.iter().flatten().filter(|s| f(s.kind)).map(|s| s.count).sum()
    }

    /// Flood-fill each level's open cells, bounded by walls, doors and solid
    /// ground. An enclosed region that doesn't touch the map edge and is
    /// covered everywhere (floors or roofs above, rock overhead) is a room.
    ///
    /// Also finds where roofs are wanted (`roof_wanted`): over the uncovered
    /// cells of enclosed regions that built walls or doors help enclose
    /// (natural hollows don't get roofs), where a roof would be held up.
    /// Regions with a roof or roof blueprint in them are the space under a
    /// lower room's roof, so they get none until floors replace it. Stairs
    /// get no roof (it would block the way up) and count as covered.
    pub fn recompute_rooms(&mut self) {
        let n = self.len();
        self.room = vec![0; n];
        self.room_sizes = vec![0];
        self.roof_wanted.clear();
        self.roof_gaps.clear();
        let mut wanted = vec![false; n];
        let room_open = |m: &Map, i: usize| m.block[i] == Block::Air && !m.feature[i].blocks_room();
        let stairs = |m: &Map, i: usize| m.feature[i].is_special(Special::Stairs);
        let mut seen = vec![false; n];
        for start in 0..n {
            if seen[start] || !room_open(self, start) {
                continue;
            }
            seen[start] = true;
            let mut region = vec![start];
            let mut stack = vec![start];
            let mut touches_edge = false;
            let mut built_walls = false;
            while let Some(i) = stack.pop() {
                let p = self.pos(i);
                if p.0 == 0 || p.1 == 0 || p.0 == self.w - 1 || p.1 == self.h - 1 {
                    touches_edge = true;
                }
                for nb in self.neighbors(p) {
                    let ni = self.idx(nb);
                    if room_open(self, ni) && !seen[ni] {
                        seen[ni] = true;
                        stack.push(ni);
                        region.push(ni);
                    } else if self.feature[ni].blocks_room() {
                        built_walls = true;
                    }
                }
            }
            if touches_edge || region.len() > MAX_ROOM {
                continue;
            }
            // Levels are filled bottom up, so roofs wanted over lower regions are known here.
            let attic = region.iter().any(|&i| self.roof[i] || self.roof_blueprint(i) || wanted[i]);
            if built_walls && !attic {
                for &i in &region {
                    let p = self.pos(i);
                    let Some(a) = self.above(p) else { continue };
                    let ai = self.idx(a);
                    let free = matches!(self.feature[ai], Feature::None) || self.roof_blueprint(ai);
                    let open = self.block[ai] == Block::Air && !self.floor[ai] && !self.roof[ai];
                    if !(free && open) || self.covered(p) || stairs(self, i) {
                        continue;
                    }
                    if self.floor_supported(a) {
                        wanted[ai] = true;
                        self.roof_wanted.push(ai);
                    } else {
                        self.roof_gaps.push(ai);
                    }
                }
            }
            if region.iter().all(|&i| self.covered(self.pos(i)) || stairs(self, i)) {
                let id = self.room_sizes.len() as u32;
                self.room_sizes.push(region.len());
                for &i in &region {
                    self.room[i] = id;
                }
            }
        }
        self.rooms_dirty = false;
        self.roof_changes += 1;
    }

    /// Hills from layered noise, rocky mountains, lakes in the low ground,
    /// wild plants, and a flat clearing in the middle.
    pub fn generate(w: usize, h: usize, d: usize, rng: &mut Rng) -> Map {
        let mut m = Map::empty(w, h, d);
        let salts: [u32; 4] = [rng.next_u64() as u32, rng.next_u64() as u32, rng.next_u64() as u32, rng.next_u64() as u32];
        let (cx, cy) = (w / 2, h / 2);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32, y as f32);
                let hills = 0.65 * noise(fx / 16.0, fy / 16.0, salts[0]) + 0.35 * noise(fx / 7.0, fy / 7.0, salts[1]);
                let mountain = noise(fx / 13.0, fy / 13.0, salts[2]);
                let mut height = PLAINS as f32 + 0.3 + (hills - 0.5) * 6.0;
                let rocky = mountain > 0.66;
                if rocky {
                    height += (mountain - 0.66) / 0.34 * 9.0;
                }
                let landing = x.abs_diff(cx) <= 8 && y.abs_diff(cy) <= 6;
                let stand = if landing { PLAINS } else { (height.round() as usize).clamp(2, d - 3) };
                for z in 0..stand {
                    let i = m.idx((x, y, z));
                    m.block[i] = if !rocky && z + 3 >= stand { Block::Soil } else { Block::Rock };
                }
                for z in stand..=WATER_LEVEL {
                    let i = m.idx((x, y, z));
                    m.block[i] = Block::Water;
                }
                // Iron ore veins in the rock, richer deeper down.
                for z in 0..stand {
                    let i = m.idx((x, y, z));
                    let depth = (stand - z).min(8) as f32;
                    let vein = noise(fx / 4.0 + z as f32 * 17.3, fy / 4.0 + z as f32 * 5.1, salts[3]);
                    if m.block[i] == Block::Rock && vein > 0.93 - 0.015 * depth {
                        m.block[i] = Block::Ore;
                    }
                }
            }
        }
        // Wild plants on open soil, each covering its share of it.
        let wild: Vec<(PlantId, f32)> =
            PlantId::all().filter_map(|k| k.def().wild.as_ref().map(|w| (k, w.density))).collect();
        for y in 0..h {
            for x in 0..w {
                let landing = x.abs_diff(cx) <= 8 && y.abs_diff(cy) <= 6;
                let Some(z) = m.surface(x, y) else { continue };
                if landing || m.block[m.idx((x, y, z - 1))] != Block::Soil {
                    continue;
                }
                let mut r = rng.f32();
                let i = m.idx((x, y, z));
                for &(kind, density) in &wild {
                    if r < density {
                        m.feature[i] = Feature::Plant { kind, growth: 1.0 };
                        break;
                    }
                    r -= density;
                }
            }
        }
        m.recompute_rooms();
        m
    }
}

fn hash(x: i32, y: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ salt.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise in [0, 1).
fn noise(x: f32, y: f32, salt: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (x0 as i32, y0 as i32);
    let a = hash(xi, yi, salt);
    let b = hash(xi + 1, yi, salt);
    let c = hash(xi, yi + 1, salt);
    let d = hash(xi + 1, yi + 1, salt);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

#[cfg(test)]
mod tests {
    use super::*;

    const Z: usize = FLAT_Z;

    fn wood() -> ItemId {
        ItemId::named("wood")
    }

    fn food() -> ItemId {
        ItemId::named("raw_food")
    }

    /// Walls around a rectangle, optionally with a floor on top as a roof.
    fn wall_box(m: &mut Map, x0: usize, y0: usize, x1: usize, y1: usize, roof: bool) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                if x == x0 || x == x1 || y == y0 || y == y1 {
                    let i = m.idx((x, y, Z));
                    m.set_feature(i, Feature::built("wall"));
                }
                if roof {
                    let i = m.idx((x, y, Z + 1));
                    m.set_floor(i, true);
                }
            }
        }
        m.recompute_rooms();
    }

    #[test]
    fn walls_alone_are_not_a_room() {
        let mut m = Map::flat(20, 20);
        wall_box(&mut m, 5, 5, 9, 9, false);
        assert!(!m.roofed((7, 7, Z)), "no roof yet");
    }

    #[test]
    fn enclosed_area_becomes_a_roofed_room() {
        let mut m = Map::flat(20, 20);
        wall_box(&mut m, 5, 5, 9, 9, true);
        assert!(m.roofed((7, 7, Z)));
        assert!(m.covered((7, 7, Z)));
        assert!(!m.roofed((2, 2, Z)));
        assert_eq!(m.room_sizes[m.room[m.idx((7, 7, Z))] as usize], 9);

        let gap = m.idx((5, 7, Z));
        m.set_feature(gap, Feature::None);
        assert!(m.rooms_dirty);
        m.recompute_rooms();
        assert!(!m.roofed((7, 7, Z)));
    }

    #[test]
    fn doors_close_rooms() {
        let mut m = Map::flat(20, 20);
        wall_box(&mut m, 5, 5, 9, 9, true);
        let i = m.idx((5, 7, Z));
        m.set_feature(i, Feature::built("door"));
        m.recompute_rooms();
        assert!(m.roofed((7, 7, Z)));
    }

    #[test]
    fn a_tunnel_under_rock_is_a_room() {
        let mut m = Map::flat(20, 20);
        // A rock hill two levels high, with a tunnel dug into it.
        for y in 3..10 {
            for x in 3..10 {
                for z in Z..Z + 2 {
                    let i = m.idx((x, y, z));
                    m.block[i] = Block::Rock;
                }
            }
        }
        for x in 5..8 {
            let i = m.idx((x, 6, Z));
            m.set_block(i, Block::Air);
        }
        m.recompute_rooms();
        assert!(m.roofed((6, 6, Z)), "the tunnel has rock above it");
        assert!(m.standable((6, 6, Z)));
    }

    #[test]
    fn pawns_step_up_one_level_but_not_two() {
        let mut m = Map::flat(10, 3);
        // x=4 is one level higher, x=6.. is three levels higher (a cliff).
        for y in 0..3 {
            for z in Z..Z + 1 {
                let i = m.idx((4, y, z));
                m.block[i] = Block::Soil;
                let i = m.idx((5, y, z));
                m.block[i] = Block::Soil;
            }
            for x in 6..10 {
                for z in Z..Z + 3 {
                    let i = m.idx((x, y, z));
                    m.block[i] = Block::Rock;
                }
            }
        }
        let up: Vec<Pos> = m.walk_neighbors((3, 1, Z)).collect();
        assert!(up.contains(&(4, 1, Z + 1)), "{:?}", up);
        let cliff: Vec<Pos> = m.walk_neighbors((5, 1, Z + 1)).collect();
        assert!(!cliff.iter().any(|p| p.0 == 6), "{:?}", cliff);
        let down: Vec<Pos> = m.walk_neighbors((4, 1, Z + 1)).collect();
        assert!(down.contains(&(3, 1, Z)), "{:?}", down);
    }

    #[test]
    fn walls_are_not_climbable() {
        let mut m = Map::flat(5, 3);
        let i = m.idx((2, 1, Z));
        m.set_feature(i, Feature::built("wall"));
        let next: Vec<Pos> = m.walk_neighbors((1, 1, Z)).collect();
        assert!(!next.iter().any(|p| p.0 == 2), "{:?}", next);
    }

    #[test]
    fn floors_can_be_stood_on_but_not_climbed_onto() {
        let mut m = Map::flat(10, 3);
        let i = m.idx((5, 1, Z + 1));
        m.set_floor(i, true);
        assert!(m.standable((5, 1, Z + 1)));
        let from_ground: Vec<Pos> = m.walk_neighbors((4, 1, Z)).collect();
        assert!(!from_ground.contains(&(5, 1, Z + 1)), "a platform needs stairs");
    }

    #[test]
    fn stairs_connect_levels() {
        let mut m = Map::flat(10, 3);
        let stairs = m.idx((3, 1, Z));
        m.set_feature(stairs, Feature::built("stairs")); // rising towards +x
        for x in 3..8 {
            let i = m.idx((x, 1, Z + 1));
            m.set_floor(i, true);
        }
        assert!(m.standable((3, 1, Z + 1)), "the top of the stairs is supported by them");
        let up: Vec<Pos> = m.walk_neighbors((3, 1, Z)).collect();
        assert!(up.contains(&(3, 1, Z + 1)), "{:?}", up);
        let down: Vec<Pos> = m.walk_neighbors((3, 1, Z + 1)).collect();
        assert!(down.contains(&(3, 1, Z)), "{:?}", down);
        let df = crate::pathing::bfs(&m, (0, 1, Z));
        assert_eq!(df.dist((7, 1, Z + 1)), Some(3 + 1 + 4), "walk, climb, walk along the floor");
    }

    #[test]
    fn stairs_cannot_be_walked_across() {
        let mut m = Map::flat(10, 3);
        let stairs = m.idx((3, 1, Z));
        m.set_feature(stairs, Feature::built("stairs"));
        let from_side: Vec<Pos> = m.walk_neighbors((3, 0, Z)).collect();
        assert!(!from_side.contains(&(3, 1, Z)), "only from the foot");
        let from_stairs: Vec<Pos> = m.walk_neighbors((3, 1, Z)).collect();
        assert_eq!(from_stairs, vec![(2, 1, Z), (3, 1, Z + 1)], "back out the foot, or climb");
        let df = crate::pathing::bfs(&m, (2, 1, Z));
        assert_eq!(df.dist((4, 1, Z)), Some(4), "around, not through");
    }

    #[test]
    fn stairs_face_away_from_the_ground_they_start_on() {
        let mut m = Map::flat(10, 3);
        // A platform to the east at the next level: stairs should rise towards it.
        for x in 5..10 {
            for y in 0..3 {
                let i = m.idx((x, y, Z + 1));
                m.floor[i] = true;
            }
        }
        let dir = m.stairs_facing((4, 1, Z));
        assert_eq!(DIRS[dir as usize], (1, 0));
        // Turned that way they fit; turned away from the platform they don't.
        assert!(m.stairs_fit((4, 1, Z), 0));
        assert!(!m.stairs_fit((4, 1, Z), 1));
    }

    #[test]
    fn floors_need_support_nearby() {
        let mut m = Map::flat(20, 3);
        // A wall at x=5 holds up floors up to SUPPORT_RANGE away.
        let w = m.idx((5, 1, Z));
        m.set_feature(w, Feature::built("wall"));
        assert!(m.floor_buildable((5 + SUPPORT_RANGE as usize, 1, Z + 1)));
        assert!(!m.floor_buildable((5 + SUPPORT_RANGE as usize + 1, 1, Z + 1)));
        assert!(!m.floor_buildable((5, 1, Z)), "not inside a wall");
        assert!(!m.floor_buildable((2, 1, Z)), "no floors directly on the ground");
    }

    #[test]
    fn things_land_on_the_next_support() {
        let mut m = Map::flat(5, 5);
        let top = m.d - 1;
        assert_eq!(m.landing((2, 2, top)), (2, 2, Z));
        let i = m.idx((2, 2, Z + 1));
        m.set_floor(i, true);
        assert_eq!(m.landing((2, 2, top)), (2, 2, Z + 1));
    }

    #[test]
    fn drop_item_spills_over_full_cells() {
        let mut m = Map::flat(10, 10);
        m.drop_item((5, 5, Z), Stack { kind: wood(), count: 120 });
        assert_eq!(m.count_items(wood()), 120);
        assert_eq!(m.items[m.idx((5, 5, Z))].unwrap().count, STACK_MAX);
    }

    #[test]
    fn take_item_respects_kind_and_amount() {
        let mut m = Map::flat(5, 5);
        let i = m.idx((1, 1, Z));
        m.items[i] = Some(Stack { kind: food(), count: 3 });
        assert!(m.take_item(i, wood(), 1).is_none());
        assert_eq!(m.take_item(i, food(), 2).unwrap().count, 2);
        assert_eq!(m.take_item(i, food(), 5).unwrap().count, 1);
        assert!(m.items[i].is_none());
    }

    #[test]
    fn generated_maps_have_a_flat_landing_zone() {
        let mut rng = Rng::new(3);
        let m = Map::generate(100, 70, 16, &mut rng);
        for y in 30..40 {
            for x in 44..56 {
                assert_eq!(m.surface(x, y), Some(PLAINS), "at {},{}", x, y);
            }
        }
        let water = m.block.iter().filter(|b| **b == Block::Water).count();
        let rock = m.block.iter().filter(|b| **b == Block::Rock).count();
        let ore = m.block.iter().filter(|b| **b == Block::Ore).count();
        assert!(water > 0 && rock > 0 && ore > 0);
        assert!(ore < rock / 10, "ore is rarer than rock ({} vs {})", ore, rock);
    }
}
