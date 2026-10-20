//! Quarries: zones painted over the ground that miners dig out level by
//! level, from the top down to a depth the player sets. A quarry covers its
//! columns on every level. Once a second (`World::update_quarries`) it marks
//! what to dig next with ordinary Mine designations, so miners, yields and
//! markers work as for any digging.
//!
//! The way down is a shaft in one column at the quarry's edge
//! (`Quarry::shaft`), on the side colonists come from, with a ladder up its
//! outer side: ladders (a kind of stairs) lead straight up into the cell
//! above, so stacked in a column they reach from the bottom up to the ground
//! outside (`Quarry::ground`, where depths count from). Each layer starts with
//! the shaft: its cell is dug and a ladder put there (3 wood) before the rest
//! of the layer is marked, so nobody is ever shut in down there.
//!
//! The whole quarry comes down one layer at a time, from its highest rock
//! (a hill top in the zone above the entrance too): nothing is dug below a
//! level the rest hasn't come down to, so every dug cell is at most a step
//! below its neighbours and open to the way out; no pit can shut anyone in.
//! Columns with water or anything built in them are left alone, a rim is
//! left beside water, and plants in the way are marked to be cut first.
//! Mine marks in its columns other than the layer being dug are taken off.
//! Cells nobody can get to (the top of a pillar too tall to reach) are left
//! standing: the column is dug from the highest cell someone can reach, so
//! they don't hold up the layers around them.

use crate::defs::defs;
use crate::map::{Block, Desig, Dir, Feature, Map, Pos, DIRS};
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
    /// How many levels below the ground at the entrance to dig.
    pub depth: u8,
    /// The level people stand on outside the entrance.
    #[serde(default)]
    pub ground: usize,
    /// The column the stairs go down in, and the way out of it to the ground
    /// outside (index into `DIRS`). None if there's no way in (all water round it).
    #[serde(default)]
    pub shaft: Option<(u16, u16)>,
    #[serde(default)]
    pub out: Dir,
}

impl Quarry {
    /// The level people stand on at the bottom once it's dug out (never level 0).
    pub fn bottom(&self) -> usize {
        self.ground - (self.depth as usize).min(self.ground.saturating_sub(1))
    }
}

/// What a quarry is doing (see `World::quarry_state`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum QuarryState {
    /// Digging level `level`: `left` cells to go there.
    Digging { level: usize, left: usize },
    /// Waiting for the stairs down to level `level` to be built.
    Stairs { level: usize },
    /// Dug out to the bottom.
    Done,
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

    /// Water, or something built other than stairs, in column (x, y) from level `from` up.
    fn column_taken(&self, x: usize, y: usize, from: usize) -> bool {
        (from..self.d).any(|z| {
            let i = self.idx((x, y, z));
            let built = match self.feature[i] {
                Feature::Built { kind, .. } | Feature::Blueprint { kind, .. } => kind.def().special != crate::defs::Special::Stairs,
                _ => false,
            };
            self.block[i] == Block::Water || self.floor[i] || self.roof[i] || built
        })
    }

    fn is_stairs(&self, p: Pos) -> bool {
        matches!(self.feature[self.idx(p)], Feature::Built { kind, .. } if kind.def().special == crate::defs::Special::Stairs)
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
        // Not laid out yet (new, reshaped, or from before stairs were used).
        for k in 0..self.quarries.len() {
            if self.quarries[k].ground == 0 {
                self.lay_out_quarry(k);
            }
        }
    }

    /// Choose where quarry `k`'s stairs go down: the edge column whose
    /// outside neighbour is nearest to the colonists (ground they can walk
    /// to), and the ground there. Done when its shape changes.
    pub(crate) fn lay_out_quarry(&mut self, k: usize) {
        let m = &self.map;
        let from = self.pawns.iter().find(|p| p.is_living_colonist()).map(|p| p.pos);
        let df = from.map(|f| crate::pathing::bfs(m, f));
        let q = &self.quarries[k];
        // Nearest the colonists, then the highest ground, then the first.
        type Key = (bool, i32, std::cmp::Reverse<usize>);
        let mut best: Option<(Key, (u16, u16), Dir, usize)> = None;
        for &c in &q.cols {
            let (x, y) = (c.0 as usize, c.1 as usize);
            if m.column_taken(x, y, 0) {
                continue;
            }
            for d in Dir::ALL {
                let (dx, dy) = d.offset();
                let Some(o) = m.at(x as i32 + dx, y as i32 + dy, 0) else { continue };
                if m.quarry_at(o.0, o.1).is_some() {
                    continue;
                }
                let Some(g) = m.ground_level(o.0, o.1) else { continue };
                let outside = (o.0, o.1, g);
                if !m.standable(outside) {
                    continue;
                }
                let dist = df.as_ref().and_then(|df| df.dist(outside));
                let key = (dist.is_none(), dist.unwrap_or(0), std::cmp::Reverse(g));
                if best.as_ref().is_none_or(|b| key < b.0) {
                    best = Some((key, c, d, g));
                }
            }
        }
        let ground_max = q.cols.iter().filter_map(|c| m.ground_level(c.0 as usize, c.1 as usize)).max().unwrap_or(1);
        let q = &mut self.quarries[k];
        match best {
            Some((_, c, out, g)) => {
                q.shaft = Some(c);
                q.out = out;
                q.ground = g;
            }
            None => {
                q.shaft = None;
                q.ground = ground_max;
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
        let k = match touching.split_first() {
            None => {
                self.quarries.push(Quarry { cols: vec![c], depth: DEFAULT_DEPTH, ground: 0, shaft: None, out: crate::map::Dir::PlusX });
                self.quarries.len() - 1
            }
            Some((&first, rest)) => {
                // Later quarries merge into the first (removed from the back so indices hold).
                for &k in rest.iter().rev() {
                    let other = self.quarries.remove(k);
                    let q = &mut self.quarries[first];
                    q.cols.extend(other.cols);
                    q.depth = q.depth.max(other.depth);
                }
                self.quarries[first].cols.push(c);
                first
            }
        };
        let q = &mut self.quarries[k];
        q.cols.sort_unstable();
        q.cols.dedup();
        // Laid out again for its new shape.
        q.ground = 0;
        self.index_quarries();
    }

    /// Take the column (x, y) out of its quarry, with the mining it marked there.
    pub(crate) fn remove_quarry_column(&mut self, x: usize, y: usize) -> bool {
        let Some(k) = self.map.quarry_at(x, y) else { return false };
        let c = (x as u16, y as u16);
        self.quarries[k].cols.retain(|&o| o != c);
        if self.quarries[k].cols.is_empty() {
            self.quarries.remove(k);
        } else {
            self.quarries[k].ground = 0;
        }
        for z in 0..self.map.d {
            let i = self.map.idx((x, y, z));
            if self.map.desig[i] == Some(Desig::Mine) {
                self.map.set_desig(i, None);
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

    /// The top cell still to dig in each column of quarry `k` (the shaft's
    /// only above the ground), leaving out columns it doesn't touch.
    fn quarry_work(&self, k: usize) -> Vec<Pos> {
        let m = &self.map;
        let Some(q) = self.quarries.get(k) else { return Vec::new() };
        let bottom = q.bottom();
        q.cols
            .iter()
            .filter_map(|&(x, y)| {
                let (x, y) = (x as usize, y as usize);
                let shaft = q.shaft == Some((x as u16, y as u16));
                let from = if shaft { q.ground } else { bottom };
                if m.column_taken(x, y, from) {
                    return None;
                }
                let top = (from..m.d).rev().find(|&z| {
                    let i = m.idx((x, y, z));
                    m.block[i].solid() && !self.quarry_out_of_reach.contains(&i)
                })?;
                // Beside water: left standing as a rim (from here down), so the lake stays put.
                let wet = DIRS.iter().any(|&(dx, dy)| {
                    m.at(x as i32 + dx, y as i32 + dy, top as i32).is_some_and(|q| m.block[m.idx(q)] == Block::Water)
                });
                (!wet && !self.quarry_out_of_reach.contains(&m.idx((x, y, top)))).then_some((x, y, top))
            })
            .collect()
    }

    /// What to do next in quarry `k`: cells to mark for digging, or the
    /// shaft cell to put stairs in; and how it's going.
    fn quarry_plan(&self, k: usize) -> (Vec<Pos>, Option<Pos>, QuarryState) {
        let m = &self.map;
        let q = &self.quarries[k];
        let work = self.quarry_work(k);
        // One layer at a time, the highest first, wherever it is.
        let top = work.iter().map(|p| p.2).max();
        let layer = |z: usize| -> Vec<Pos> { work.iter().copied().filter(|p| p.2 == z).collect() };
        let at_or_above = top.filter(|&z| z >= q.ground || q.shaft.is_none());
        if let Some(z) = at_or_above {
            // Down to the entrance's level no way down is needed.
            let dig = layer(z);
            let left = dig.len();
            return (dig, None, QuarryState::Digging { level: z, left });
        }
        let mut dig = Vec::new();
        let level = top;
        let Some(c) = q.shaft else {
            return (dig, None, QuarryState::Done);
        };
        // The highest level of the shaft without its stairs yet.
        let missing = (q.bottom()..q.ground).rev().find(|&z| !m.is_stairs((c.0 as usize, c.1 as usize, z)));
        let mut stairs = None;
        let mut state = match level {
            Some(level) => QuarryState::Digging { level, left: work.iter().filter(|p| p.2 == level).count() },
            None => QuarryState::Done,
        };
        if let Some(z) = missing {
            let cell = (c.0 as usize, c.1 as usize, z);
            let i = m.idx(cell);
            // Only once the layer above is done.
            if level.is_none_or(|l| l <= z) {
                if m.block[i].solid() {
                    dig.push(cell);
                    state = QuarryState::Digging { level: z, left: 1 };
                } else {
                    stairs = matches!(m.feature[i], Feature::None).then_some(cell);
                    state = QuarryState::Stairs { level: z };
                }
            }
        }
        // The rest of a layer, once the stairs down to it are in.
        if let Some(level) = level.filter(|&l| missing.is_none_or(|z| z < l)) {
            dig.extend(work.iter().copied().filter(|p| p.2 == level));
        }
        (dig, stairs, state)
    }

    /// How quarry `k` is getting on.
    pub fn quarry_state(&self, k: usize) -> QuarryState {
        self.quarry_plan(k).2
    }

    /// Once a second: mark what each quarry digs next, put up the stairs down,
    /// and mark plants in the way for cutting.
    pub(crate) fn update_quarries(&mut self) {
        let Some(stairs_kind) = defs().ladder().or(defs().stairs()) else { return };
        // Where the colonists (not shut in somewhere) can get to: what nobody
        // can reach is left standing, and the player told.
        let from = self
            .pawns
            .iter()
            .filter(|p| p.is_living_colonist())
            .min_by_key(|p| self.trapped.contains(&p.id))
            .map(|p| p.pos);
        let reach = from.filter(|_| !self.quarries.is_empty()).map(|f| crate::pathing::bfs(&self.map, f));
        self.quarry_unreachable = vec![0; self.quarries.len()];
        self.quarry_out_of_reach.clear();
        if let Some(df) = &reach {
            for k in 0..self.quarries.len() {
                // Down each column past what can't be reached, to what can.
                loop {
                    let out: Vec<Pos> = self.quarry_work(k).into_iter().filter(|&p| df.approach(&self.map, p, true).is_none()).collect();
                    if out.is_empty() {
                        break;
                    }
                    for p in out {
                        self.quarry_out_of_reach.insert(self.map.idx(p));
                        self.quarry_unreachable[k] += 1;
                    }
                }
            }
        }
        for k in 0..self.quarries.len() {
            let (dig, stairs, _) = self.quarry_plan(k);
            if let Some(p) = stairs {
                let i = self.map.idx(p);
                let feature = Feature::blueprint(stairs_kind, 0, self.quarries[k].out);
                if self.map.feature[i] != feature {
                    self.map.feature[i] = feature;
                    self.map.mark_changed();
                }
            }
            let m = &mut self.map;
            // Only the layer being dug is marked (marks from before, deeper down, come off).
            for &(x, y) in &self.quarries[k].cols {
                for z in 0..m.d {
                    let i = m.idx((x as usize, y as usize, z));
                    if m.desig[i] == Some(Desig::Mine) && !dig.contains(&(x as usize, y as usize, z)) {
                        m.set_desig(i, None);
                    }
                }
            }
            for p in dig {
                // A plant on top goes first.
                if let Some(a) = m.above(p) {
                    let ai = m.idx(a);
                    if let Feature::Plant { kind, .. } = m.feature[ai] {
                        if m.desig[ai].is_none() {
                            m.set_desig(ai, kind.def().harvest.desig);
                        }
                        continue;
                    }
                }
                let i = m.idx(p);
                if m.desig[i].is_none() {
                    m.set_desig(i, Some(Desig::Mine));
                }
            }
        }
    }

    /// Stairs built in a quarry's shaft keep facing out of it, whatever the
    /// ground around them looks like while it's being dug.
    pub(crate) fn quarry_stairs_dir(&self, p: Pos) -> Option<Dir> {
        let q = &self.quarries[self.map.quarry_at(p.0, p.1)?];
        (q.shaft == Some((p.0 as u16, p.1 as u16))).then_some(q.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Stack};
    use crate::rng::Rng;
    use crate::testing::{content_colonist as colonist, item, run};

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
                let z = w.map.ground_level(x, y).unwrap();
                assert!(w.apply_order((x, y, z), crate::world::Order::Quarry));
            }
        }
    }

    fn marked(w: &World) -> Vec<Pos> {
        (0..w.map.len()).filter(|&i| w.map.desig[i] == Some(Desig::Mine)).map(|i| w.map.pos(i)).collect()
    }

    #[test]
    fn painted_columns_make_one_quarry() {
        let mut w = hill(16, 16);
        paint(&mut w, 4, 4, 8, 7);
        assert_eq!(w.quarries.len(), 1);
        let q = &w.quarries[0];
        assert_eq!((q.cols.len(), q.ground, q.depth, q.bottom()), (20, 8, DEFAULT_DEPTH, 3));
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
    fn the_stairs_go_down_on_the_side_the_colonists_are() {
        let mut w = hill(20, 16);
        colonist(&mut w, (17, 6, 8));
        paint(&mut w, 4, 4, 8, 7);
        let q = &w.quarries[0];
        let shaft = q.shaft.unwrap();
        assert_eq!(shaft.0, 8, "on the east edge, facing them");
        assert_eq!(q.out, Dir::PlusX, "stairs out to the east");
    }

    #[test]
    fn each_layer_starts_with_the_shaft_and_its_stairs() {
        let mut w = hill(16, 16);
        paint(&mut w, 4, 4, 8, 7);
        let shaft = w.quarries[0].shaft.unwrap();
        let (sx, sy) = (shaft.0 as usize, shaft.1 as usize);
        // The first layer is dug: the shaft cell first, alone.
        w.update_quarries();
        assert_eq!(marked(&w), vec![(sx, sy, 7)]);
        let i = w.map.idx((sx, sy, 7));
        w.map.block[i] = Block::Air;
        w.map.desig[i] = None;
        // Dug: stairs go in, and the rest waits for them.
        w.update_quarries();
        assert!(matches!(w.map.feature[i], Feature::Blueprint { .. }));
        assert!(marked(&w).is_empty());
        assert_eq!(w.quarry_state(0), QuarryState::Stairs { level: 7 });
        w.map.feature[i] = Feature::Built { kind: defs().stairs().unwrap(), dir: w.quarries[0].out };
        w.update_quarries();
        let layer = marked(&w);
        assert_eq!(layer.len(), 19, "the rest of the top layer");
        assert!(layer.iter().all(|p| p.2 == 7));
    }

    #[test]
    fn miners_dig_it_out_and_climb_out_by_the_stairs() {
        let mut w = hill(14, 14);
        for k in 0..3 {
            colonist(&mut w, (1 + k, 1, 8));
        }
        w.map.drop_item((1, 3, 8), Stack { kind: item("wood"), count: 40 });
        // A tree in the way (painted over too) is cut first.
        let tree = w.map.idx((6, 6, 8));
        w.map.feature[tree] = Feature::plant("tree");
        paint(&mut w, 4, 4, 8, 7);
        w.set_quarry_depth(0, 3);
        w.update_quarries();
        // Fed and rested meanwhile, so only the work takes time.
        for _ in 0..50 {
            run(&mut w, 60.0);
            for p in w.pawns.iter_mut() {
                p.hunger = 100.0;
                p.rest = 100.0;
            }
        }
        assert_eq!(w.quarry_state(0), QuarryState::Done);
        assert!(!matches!(w.map.feature[tree], Feature::Plant { .. }), "the tree came down");
        let q = w.quarries[0].clone();
        let shaft = q.shaft.unwrap();
        for &(x, y) in &q.cols {
            let (x, y) = (x as usize, y as usize);
            if (x as u16, y as u16) == shaft {
                for z in q.bottom()..q.ground {
                    assert!(w.map.is_stairs((x, y, z)), "stairs at level {}", z);
                    assert_eq!(w.map.feature[w.map.idx((x, y, z))].structure_id(), defs().ladder(), "a ladder");
                }
            } else {
                assert!(w.map.standable((x, y, q.bottom())), "({}, {}) dug down", x, y);
                assert!(w.map.solid((x, y, q.bottom() - 1)), "({}, {}) not deeper", x, y);
            }
        }
        // From the bottom, out up the stairs.
        let corner = q.cols.iter().find(|&&c| c != shaft).unwrap();
        let df = crate::pathing::bfs(&w.map, (corner.0 as usize, corner.1 as usize, q.bottom()));
        assert!(df.dist((1, 1, 8)).is_some(), "a way back up");
        assert!(w.map.count_items(item("stone")) > 0, "stone came out");
        assert!(w.trapped.is_empty(), "nobody was shut in");
    }

    #[test]
    fn a_hill_top_in_the_zone_comes_down_first() {
        let mut w = hill(16, 16);
        for z in 8..11 {
            let i = w.map.idx((4, 4, z));
            w.map.block[i] = Block::Rock;
        }
        paint(&mut w, 4, 4, 8, 7);
        assert_eq!(w.quarries[0].ground, 8, "the ground outside, not the hill top");
        w.update_quarries();
        assert_eq!(marked(&w), vec![(4, 4, 10)], "only the top layer: nothing deeper meanwhile");
    }

    /// Rock standing above the entrance in part of the zone used to be dug
    /// on its own while the layers below went down elsewhere, leaving deep
    /// pits shut off from the ladder. Now everyone can always get out.
    #[test]
    fn an_outcrop_in_the_zone_never_shuts_anyone_in() {
        let mut w = hill(16, 14);
        for (x, y) in [(6, 5), (7, 5), (6, 6), (7, 6), (8, 8)] {
            for z in 8..11 {
                let i = w.map.idx((x, y, z));
                w.map.block[i] = Block::Rock;
            }
        }
        w.map.recompute_rooms();
        for k in 0..3 {
            colonist(&mut w, (1 + k, 1, 8));
        }
        w.map.drop_item((1, 3, 8), Stack { kind: item("wood"), count: 40 });
        paint(&mut w, 4, 4, 9, 9);
        w.set_quarry_depth(0, 3);
        for _ in 0..100 {
            run(&mut w, 30.0);
            for p in w.pawns.iter_mut() {
                p.hunger = 100.0;
                p.rest = 100.0;
            }
            w.update_trapped();
            assert!(w.trapped.is_empty(), "shut in at {:?}", w.pawns.iter().filter(|p| w.trapped.contains(&p.id)).map(|p| p.pos).collect::<Vec<_>>());
        }
        assert_eq!(w.quarry_state(0), QuarryState::Done);
    }

    #[test]
    fn someone_shut_in_a_pit_climbs_out_up_its_wall() {
        let mut w = hill(12, 12);
        // A pit three levels deep, one cell wide, with someone at the bottom.
        for z in 5..8 {
            let i = w.map.idx((6, 6, z));
            w.map.block[i] = Block::Air;
        }
        w.map.recompute_rooms();
        let id = colonist(&mut w, (6, 6, 5));
        for _ in 0..20 {
            run(&mut w, 3.0);
            let i = w.pawn_index(id).unwrap();
            (w.pawns[i].hunger, w.pawns[i].rest) = (100.0, 100.0);
        }
        let p = w.pawn(id).unwrap();
        assert_eq!(p.pos.2, 8, "out on the ground: {:?}", p.pos);
        assert!(p.health.hediffs.is_empty(), "without falling");
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
        // Past the shaft and its stairs.
        let shaft = w.quarries[0].shaft.unwrap();
        let s = w.map.idx((shaft.0 as usize, shaft.1 as usize, 7));
        w.map.block[s] = Block::Air;
        w.map.feature[s] = Feature::Built { kind: defs().stairs().unwrap(), dir: crate::map::Dir::PlusX };
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

    #[test]
    fn cells_nobody_can_reach_are_counted() {
        let mut w = hill(16, 16);
        colonist(&mut w, (1, 1, 8));
        paint(&mut w, 4, 4, 8, 7);
        w.update_quarries();
        assert_eq!(w.quarry_unreachable, vec![0], "the shaft cell can be dug");
        // Walled off from everyone: the colonist is shut in a box of rock.
        for (x, y) in [(0, 0), (1, 0), (2, 0), (0, 1), (2, 1), (0, 2), (1, 2), (2, 2)] {
            for z in 8..10 {
                let i = w.map.idx((x, y, z));
                w.map.block[i] = Block::Rock;
            }
        }
        w.update_quarries();
        assert!(w.quarry_unreachable[0] > 0);
    }

    #[test]
    fn the_map_bottom_stops_it() {
        let mut w = hill(12, 12);
        paint(&mut w, 3, 3, 5, 5);
        w.set_quarry_depth(0, MAX_DEPTH);
        assert_eq!(w.quarries[0].bottom(), 1, "never into level 0");
    }
}
