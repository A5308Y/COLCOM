//! Quarries: zones painted over the ground that miners dig out level by
//! level, from the top down to a depth the player sets. A quarry covers its
//! columns on every level. Once a second (`World::update_quarries`) it marks
//! the next layer for mining with ordinary Mine designations, so miners,
//! yields and markers work as for any digging.
//!
//! The way down is cut into the rock: a path of columns along the zone's
//! edge (`Quarry::steps`) is dug one level less deep at each cell, so it
//! leaves single natural steps, which colonists walk up and down anyway. It
//! starts at the quarry's highest edge, runs round the outside and spirals
//! inwards if it must. A quarry can go as many levels down as it has room for steps.
//!
//! Columns with water, or with anything built in them, are left alone; plants
//! in the way are marked to be cut first.

use crate::map::{Block, Desig, Feature, Map, Pos, DIRS};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// How many levels down a new quarry digs.
pub const DEFAULT_DEPTH: u8 = 5;
/// Deepest a quarry can be set to.
pub const MAX_DEPTH: u8 = 12;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quarry {
    /// The columns it covers (x, y), sorted.
    pub cols: Vec<(u16, u16)>,
    /// The level people stand on at its highest point when it was laid out:
    /// depths count from here.
    pub top: usize,
    /// How many levels down to dig.
    pub depth: u8,
}

impl Quarry {
    fn has(&self, c: (u16, u16)) -> bool {
        self.cols.binary_search(&c).is_ok()
    }

    /// The path of steps down, from the entrance: column k is dug down to
    /// stand at `top - k`. Follows the edge (columns nearest the outside
    /// first), going straight on where it can, else turning right.
    pub fn steps(&self, m: &Map) -> Vec<(u16, u16)> {
        let n = self.cols.len();
        if n == 0 {
            return Vec::new();
        }
        let neighbours = |c: (u16, u16)| {
            DIRS.iter().filter_map(move |&(dx, dy)| {
                let (x, y) = (c.0 as i32 + dx, c.1 as i32 + dy);
                (x >= 0 && y >= 0).then_some((x as u16, y as u16))
            })
        };
        // How far each column is from the outside (the edge is 0).
        let mut ring = vec![u32::MAX; n];
        let mut queue = std::collections::VecDeque::new();
        for (k, &c) in self.cols.iter().enumerate() {
            if neighbours(c).count() < 4 || neighbours(c).any(|q| !self.has(q)) {
                ring[k] = 0;
                queue.push_back(k);
            }
        }
        while let Some(k) = queue.pop_front() {
            for q in neighbours(self.cols[k]) {
                if let Ok(j) = self.cols.binary_search(&q) {
                    if ring[j] == u32::MAX {
                        ring[j] = ring[k] + 1;
                        queue.push_back(j);
                    }
                }
            }
        }
        let index = |c: (u16, u16)| self.cols.binary_search(&c).ok();
        // In at the highest point of the edge (then the first in order).
        let surface = |c: (u16, u16)| m.ground_level(c.0 as usize, c.1 as usize).unwrap_or(0);
        let entry = (0..n).filter(|&k| ring[k] == 0).max_by_key(|&k| (surface(self.cols[k]), std::cmp::Reverse(k))).unwrap_or(0);
        let mut path = vec![self.cols[entry]];
        let mut seen = vec![false; n];
        seen[entry] = true;
        let mut heading: Option<(i32, i32)> = None;
        loop {
            let c = *path.last().unwrap();
            // Straight on, right, left: the outermost free column.
            let turns: Vec<(i32, i32)> = match heading {
                Some((dx, dy)) => vec![(dx, dy), (-dy, dx), (dy, -dx)],
                None => DIRS.to_vec(),
            };
            let next = turns
                .iter()
                .enumerate()
                .filter_map(|(order, &(dx, dy))| {
                    let (x, y) = (c.0 as i32 + dx, c.1 as i32 + dy);
                    let j = (x >= 0 && y >= 0).then(|| index((x as u16, y as u16))).flatten()?;
                    (!seen[j]).then_some((ring[j], order, j, (dx, dy)))
                })
                .min();
            // Hemmed in going on: any free neighbour at all.
            let next = next.or_else(|| {
                DIRS.iter()
                    .enumerate()
                    .filter_map(|(order, &(dx, dy))| {
                        let (x, y) = (c.0 as i32 + dx, c.1 as i32 + dy);
                        let j = (x >= 0 && y >= 0).then(|| index((x as u16, y as u16))).flatten()?;
                        (!seen[j]).then_some((ring[j], order, j, (dx, dy)))
                    })
                    .min()
            });
            let Some((_, _, j, d)) = next else { break };
            seen[j] = true;
            heading = Some(d);
            path.push(self.cols[j]);
        }
        path
    }

    /// How many levels down it really goes: no deeper than it has room for
    /// steps, nor into the bottom level of the map.
    pub fn reach(&self, m: &Map) -> u8 {
        let steps = self.steps(m).len();
        (self.depth as usize).min(steps).min(self.top.saturating_sub(1)) as u8
    }

    /// The level each column is dug down to (where people stand once it's
    /// done): steps as deep as their place on the path, the rest the bottom.
    pub fn floors(&self, m: &Map) -> Vec<((u16, u16), usize)> {
        let steps = self.steps(m);
        let reach = self.reach(m) as usize;
        let bottom = self.top - reach;
        self.cols
            .iter()
            .map(|&c| {
                let floor = match steps.iter().position(|&s| s == c) {
                    Some(k) if k < reach => self.top - k,
                    _ => bottom,
                };
                (c, floor)
            })
            .collect()
    }
}

impl Map {
    /// The open level just above the highest solid ground in column (x, y),
    /// whatever stands there (a tree).
    pub fn ground_level(&self, x: usize, y: usize) -> Option<usize> {
        (1..self.d).rev().find(|&z| self.block[self.idx((x, y, z))] == Block::Air && self.block[self.idx((x, y, z - 1))].solid())
    }

    /// The quarry (index into `World::quarries`) column (x, y) is in.
    pub fn quarry_at(&self, x: usize, y: usize) -> Option<usize> {
        match self.quarry_cols.get(y * self.w + x) {
            Some(&q) if q > 0 => Some(q as usize - 1),
            _ => None,
        }
    }
}

impl World {
    /// Look up which column belongs to which quarry again (after loading, or changes).
    pub(crate) fn index_quarries(&mut self) {
        let m = &mut self.map;
        m.quarry_cols = vec![0; m.w * m.h];
        for (k, q) in self.quarries.iter().enumerate() {
            for &(x, y) in &q.cols {
                m.quarry_cols[y as usize * m.w + x as usize] = k as u16 + 1;
            }
        }
    }

    /// Whether the column of `p` can join a quarry: natural ground (trees
    /// and all), not in one yet.
    pub(crate) fn quarry_applies(&self, p: Pos) -> bool {
        let m = &self.map;
        m.ground_level(p.0, p.1).is_some() && m.quarry_at(p.0, p.1).is_none()
    }

    /// Add the column of `p` to the quarry beside it (joining quarries it
    /// touches), or start a new one.
    pub(crate) fn add_quarry_column(&mut self, p: Pos) {
        let c = (p.0 as u16, p.1 as u16);
        let mut touching: Vec<usize> = DIRS
            .iter()
            .filter_map(|&(dx, dy)| {
                let q = self.map.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32)?;
                self.map.quarry_at(q.0, q.1)
            })
            .collect();
        touching.sort_unstable();
        touching.dedup();
        let level = self.map.ground_level(p.0, p.1).unwrap_or(p.2);
        match touching.split_first() {
            None => self.quarries.push(Quarry { cols: vec![c], top: level, depth: DEFAULT_DEPTH }),
            Some((&first, rest)) => {
                // Later quarries merge into the first (removed from the back so indices hold).
                for &k in rest.iter().rev() {
                    let other = self.quarries.remove(k);
                    let q = &mut self.quarries[first];
                    q.cols.extend(other.cols);
                    q.top = q.top.max(other.top);
                    q.depth = q.depth.max(other.depth);
                }
                let q = &mut self.quarries[first];
                q.cols.push(c);
                q.top = q.top.max(level);
            }
        }
        for q in &mut self.quarries {
            q.cols.sort_unstable();
            q.cols.dedup();
        }
        self.index_quarries();
    }

    /// Take the column (x, y) out of its quarry, with the mining it marked there.
    pub(crate) fn remove_quarry_column(&mut self, x: usize, y: usize) -> bool {
        let Some(k) = self.map.quarry_at(x, y) else { return false };
        let c = (x as u16, y as u16);
        self.quarries[k].cols.retain(|&o| o != c);
        if self.quarries[k].cols.is_empty() {
            self.quarries.remove(k);
        }
        for z in 0..self.map.d {
            let i = self.map.idx((x, y, z));
            if self.map.desig[i] == Some(Desig::Mine) {
                self.map.desig[i] = None;
            }
        }
        self.index_quarries();
        true
    }

    /// Set how many levels down quarry `k` goes.
    pub fn set_quarry_depth(&mut self, k: usize, depth: u8) {
        if let Some(q) = self.quarries.get_mut(k) {
            q.depth = depth.clamp(1, MAX_DEPTH);
        }
    }

    /// What's left to dig in quarry `k`: the level being dug and how many
    /// cells there are still to dig on it (None once it's done).
    pub fn quarry_progress(&self, k: usize) -> Option<(usize, usize)> {
        let level = self.quarry_work(k).into_iter().map(|p| p.2).max()?;
        let left = self.quarry_work(k).into_iter().filter(|p| p.2 == level).count();
        Some((level, left))
    }

    /// The top cell of each column of quarry `k` that's still to be dug
    /// (leaving out columns it doesn't touch: water, buildings).
    fn quarry_work(&self, k: usize) -> Vec<Pos> {
        let m = &self.map;
        let Some(q) = self.quarries.get(k) else { return Vec::new() };
        q.floors(m)
            .into_iter()
            .filter_map(|((x, y), floor)| {
                let (x, y) = (x as usize, y as usize);
                let column = |z: usize| m.idx((x, y, z));
                // Water, or something built: not here.
                let left_alone = (floor..m.d).any(|z| {
                    let i = column(z);
                    m.block[i] == Block::Water || m.floor[i] || m.roof[i] || matches!(m.feature[i], Feature::Built { .. } | Feature::Blueprint { .. })
                });
                if left_alone || floor == 0 {
                    return None;
                }
                let top = (floor..m.d).rev().find(|&z| m.block[column(z)].solid())?;
                Some((x, y, top))
            })
            .collect()
    }

    /// Once a second: mark the next layer of each quarry for mining (top down),
    /// and plants in the way for cutting.
    pub(crate) fn update_quarries(&mut self) {
        for k in 0..self.quarries.len() {
            let work = self.quarry_work(k);
            let Some(level) = work.iter().map(|p| p.2).max() else { continue };
            for p in work.into_iter().filter(|p| p.2 == level) {
                let m = &mut self.map;
                // Beside water: left standing as a rim, so the lake stays put.
                let wet = DIRS.iter().any(|&(dx, dy)| {
                    m.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32).is_some_and(|q| m.block[m.idx(q)] == Block::Water)
                });
                if wet {
                    continue;
                }
                // A plant on top goes first.
                if let Some(a) = m.above(p) {
                    let ai = m.idx(a);
                    if let Feature::Plant { kind, .. } = m.feature[ai] {
                        if m.desig[ai].is_none() {
                            m.desig[ai] = kind.def().harvest.desig;
                        }
                        continue;
                    }
                }
                let i = m.idx(p);
                if m.desig[i].is_none() {
                    m.desig[i] = Some(Desig::Mine);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Map;
    use crate::rng::Rng;
    use crate::testing::{content_colonist as colonist, run};

    /// Solid rock up to level 7 (people stand on 8), soil on top.
    fn hill(w: usize, h: usize) -> World {
        let mut m = Map::empty(w, h, 12);
        for y in 0..h {
            for x in 0..w {
                for z in 0..8 {
                    let i = m.idx((x, y, z));
                    m.block[i] = if z >= 6 { Block::Soil } else { Block::Rock };
                }
            }
        }
        m.recompute_rooms();
        let mut w = World::with_map(m, Rng::new(3));
        w.story.next_raid = f32::MAX;
        w.story.next_event = f32::MAX;
        w.regrow_timer = f32::MAX;
        w
    }

    fn paint(w: &mut World, x0: usize, y0: usize, x1: usize, y1: usize) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                assert!(w.apply_order((x, y, 8), crate::world::Order::Quarry));
            }
        }
    }

    #[test]
    fn painted_columns_make_one_quarry() {
        let mut w = hill(16, 16);
        paint(&mut w, 4, 4, 8, 7);
        assert_eq!(w.quarries.len(), 1);
        let q = &w.quarries[0];
        assert_eq!((q.cols.len(), q.top, q.depth), (20, 8, DEFAULT_DEPTH));
        assert!(!w.apply_order((5, 5, 8), crate::world::Order::Quarry), "already in it");
        // A second one apart, then a column joining them up: one quarry again.
        paint(&mut w, 11, 4, 12, 5);
        assert_eq!(w.quarries.len(), 2);
        paint(&mut w, 9, 4, 10, 4);
        assert_eq!(w.quarries.len(), 1);
        // Cleared again column by column.
        assert!(w.apply_order((4, 4, 8), crate::world::Order::Zone(crate::map::Zone::None)));
        assert_eq!(w.map.quarry_at(4, 4), None);
        assert!(w.map.quarry_at(5, 4).is_some());
    }

    #[test]
    fn the_steps_run_round_the_edge_one_level_at_a_time() {
        let mut w = hill(16, 16);
        paint(&mut w, 4, 4, 8, 7);
        let q = &w.quarries[0];
        let steps = q.steps(&w.map);
        assert_eq!(steps.len(), q.cols.len(), "every column on the path");
        for pair in steps.windows(2) {
            let d = (pair[0].0 as i32 - pair[1].0 as i32).abs() + (pair[0].1 as i32 - pair[1].1 as i32).abs();
            assert_eq!(d, 1, "each step beside the last");
        }
        // The first 14 go round the outside (a 5×4 has 14 edge columns).
        let edge = |c: &(u16, u16)| c.0 == 4 || c.0 == 8 || c.1 == 4 || c.1 == 7;
        assert!(steps[..14].iter().all(edge));
        let floors = q.floors(&w.map);
        let floor = |c: (u16, u16)| floors.iter().find(|f| f.0 == c).unwrap().1;
        for (k, &s) in steps.iter().take(DEFAULT_DEPTH as usize).enumerate() {
            assert_eq!(floor(s), 8 - k, "step {}", k);
        }
        assert_eq!(floor(steps[10]), 8 - DEFAULT_DEPTH as usize, "the rest dug to the bottom");
    }

    #[test]
    fn a_narrow_quarry_only_goes_as_deep_as_its_steps() {
        let mut w = hill(16, 16);
        paint(&mut w, 4, 4, 6, 4);
        w.set_quarry_depth(0, 9);
        assert_eq!(w.quarries[0].reach(&w.map), 3);
    }

    #[test]
    fn miners_dig_it_out_from_the_top_and_can_walk_out() {
        let mut w = hill(14, 14);
        for k in 0..3 {
            colonist(&mut w, (1 + k, 1, 8));
        }
        // A tree in the way (painted over too) is cut first.
        let tree = w.map.idx((6, 6, 8));
        w.map.feature[tree] = Feature::plant("tree");
        paint(&mut w, 4, 4, 8, 7);
        w.set_quarry_depth(0, 3);
        // Only the top layer is marked to begin with.
        w.update_quarries();
        let marked: Vec<Pos> = (0..w.map.len()).filter(|&i| w.map.desig[i] == Some(Desig::Mine)).map(|i| w.map.pos(i)).collect();
        assert!(!marked.is_empty() && marked.iter().all(|p| p.2 == 7), "{:?}", marked);
        assert_eq!(w.map.desig[tree], Some(Desig::Chop));
        // Fed and rested meanwhile, so only the digging takes time.
        for _ in 0..40 {
            run(&mut w, 60.0);
            for p in w.pawns.iter_mut() {
                p.hunger = 100.0;
                p.rest = 100.0;
            }
        }
        assert_eq!(w.quarry_progress(0), None, "dug out");
        let q = w.quarries[0].clone();
        for (c, floor) in q.floors(&w.map) {
            assert!(w.map.standable((c.0 as usize, c.1 as usize, floor)), "{:?} dug down to {}", c, floor);
            assert!(w.map.solid((c.0 as usize, c.1 as usize, floor - 1)), "{:?} not deeper", c);
        }
        // From the bottom, out along the steps.
        let bottom = q.floors(&w.map).into_iter().map(|(c, f)| (c.0 as usize, c.1 as usize, f)).min_by_key(|p| p.2).unwrap();
        let df = crate::pathing::bfs(&w.map, bottom);
        assert!(df.dist((1, 1, 8)).is_some(), "a way back up");
        assert!(w.map.count_items(crate::defs::ItemId::named("stone")) > 0, "stone came out");
    }

    #[test]
    fn water_and_buildings_are_left_alone() {
        let mut w = hill(14, 14);
        paint(&mut w, 4, 4, 7, 7);
        // A pool sunk into the top layer: its column stays, and so does a rim beside it.
        let wet = w.map.idx((5, 5, 7));
        w.map.block[wet] = Block::Water;
        let built = w.map.idx((6, 6, 8));
        w.map.feature[built] = Feature::built("wall");
        w.update_quarries();
        for (x, y) in [(5, 5), (6, 6), (5, 4), (4, 5)] {
            let i = w.map.idx((x, y, 7));
            assert_eq!(w.map.desig[i], None, "({}, {}) isn't dug", x, y);
        }
        assert_eq!(w.map.desig[w.map.idx((7, 7, 7))], Some(Desig::Mine));
    }

    #[test]
    fn quarries_are_saved() {
        let mut w = hill(12, 12);
        paint(&mut w, 3, 3, 5, 5);
        let loaded = World::load_json(&w.save_json().unwrap()).unwrap();
        assert_eq!(loaded.quarries, w.quarries);
        assert_eq!(loaded.map.quarry_at(4, 4), Some(0));
    }
}
