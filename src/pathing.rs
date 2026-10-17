//! Breadth-first distance fields over the walking graph, and line of sight.

use crate::map::{Map, Pos};
use std::collections::VecDeque;

/// Distances from one start cell to every cell a pawn can walk to.
pub struct DistField {
    /// Steps from the start plus one; 0 = unreachable. (Zero-initialised
    /// arrays are nearly free to allocate, which matters at 100k+ cells.)
    steps: Vec<u32>,
    parent: Vec<u32>,
    /// Reached cells, nearest first.
    pub order: Vec<usize>,
    w: usize,
    h: usize,
}

/// BFS over walkable cells. The start cell is always included, even if it
/// can't be stood on (e.g. a pawn where a wall was just built).
///
/// Shelves (and other storage nobody walks onto) beside reached cells are
/// reached too, one step further, but nothing is reached through them.
pub fn bfs(map: &Map, start: Pos) -> DistField {
    search(map, start, |_| false, true, false)
}

/// BFS for a trapped pawn: also over furniture (`Map::climbable`), never through walls.
pub fn bfs_climbing(map: &Map, start: Pos) -> DistField {
    search(map, start, |_| false, true, true)
}

/// BFS that doesn't go through cells (by index) that are `blocked`, such as
/// enemies' cells in a battle; only cells that can be stood on.
pub fn bfs_avoiding(map: &Map, start: Pos, blocked: impl Fn(usize) -> bool) -> DistField {
    search(map, start, blocked, false, false)
}

fn search(map: &Map, start: Pos, blocked: impl Fn(usize) -> bool, shelves: bool, climb: bool) -> DistField {
    let n = map.len();
    let mut steps = vec![0u32; n];
    let mut parent = vec![0u32; n];
    let mut order = Vec::new();
    let si = map.idx(start);
    steps[si] = 1;
    order.push(si);
    let mut q = VecDeque::new();
    q.push_back(start);
    while let Some(p) = q.pop_front() {
        let pi = map.idx(p);
        let d = steps[pi];
        for nb in map.walk_neighbors(p) {
            let ni = map.idx(nb);
            if steps[ni] == 0 && !blocked(ni) {
                steps[ni] = d + 1;
                parent[ni] = pi as u32;
                order.push(ni);
                q.push_back(nb);
            }
        }
        // Over furniture, for a pawn who'd otherwise be stuck.
        if climb {
            for (dx, dy) in crate::map::DIRS {
                let Some(nb) = map.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32) else { continue };
                let ni = map.idx(nb);
                if steps[ni] == 0 && map.climbable(nb) {
                    steps[ni] = d + 1;
                    parent[ni] = pi as u32;
                    order.push(ni);
                    q.push_back(nb);
                }
            }
        }
        // Reached from here, but a dead end: not queued.
        for nb in map.reach_neighbors(p).filter(|_| shelves) {
            let ni = map.idx(nb);
            if steps[ni] == 0 {
                steps[ni] = d + 1;
                parent[ni] = pi as u32;
                order.push(ni);
            }
        }
    }
    DistField { steps, parent, order, w: map.w, h: map.h }
}

impl DistField {
    fn idx(&self, p: Pos) -> usize {
        (p.2 * self.h + p.1) * self.w + p.0
    }

    fn pos(&self, i: usize) -> Pos {
        let layer = self.w * self.h;
        (i % self.w, (i % layer) / self.w, i / layer)
    }

    pub fn dist(&self, p: Pos) -> Option<i32> {
        self.dist_i(self.idx(p))
    }

    /// Distance to the cell with index `i`.
    pub fn dist_i(&self, i: usize) -> Option<i32> {
        let s = self.steps[i];
        (s > 0).then(|| s as i32 - 1)
    }

    /// Closest cell from which `target` can be reached: the target itself, or
    /// (if `adjacent`) a neighbouring cell up to one level above or below.
    pub fn approach(&self, map: &Map, target: Pos, adjacent: bool) -> Option<(Pos, i32)> {
        // Shelves are reached, but not stood on (unless that's where we already are).
        let stand = |p: Pos| !map.storage_furniture(map.idx(p)) || self.dist(p) == Some(0);
        let adjacent = adjacent || !stand(target);
        let mut best = self.dist(target).filter(|_| !adjacent || stand(target)).map(|d| (target, d));
        if adjacent {
            for nb in map.around(target).filter(|&nb| stand(nb)) {
                if let Some(d) = self.dist(nb) {
                    if best.map_or(true, |(_, bd)| d < bd) {
                        best = Some((nb, d));
                    }
                }
            }
        }
        best
    }

    /// Path to a reachable `goal`, reversed so that `pop()` yields the next step.
    /// The start cell is not included.
    pub fn path_to(&self, goal: Pos) -> Vec<Pos> {
        let mut path = Vec::new();
        let mut cur = self.idx(goal);
        while self.steps[cur] > 1 {
            path.push(self.pos(cur));
            cur = self.parent[cur] as usize;
        }
        path
    }
}

/// Straight line between cell centres (at chest height), blocked by solid
/// ground, walls and doors. The endpoints themselves don't count.
pub fn line_of_sight(map: &Map, a: Pos, b: Pos) -> bool {
    let from = [a.0 as f32 + 0.5, a.1 as f32 + 0.5, a.2 as f32 + 0.6];
    let to = [b.0 as f32 + 0.5, b.1 as f32 + 0.5, b.2 as f32 + 0.6];
    let span = (0..3).map(|k| (to[k] - from[k]).abs()).fold(0.0, f32::max);
    let steps = (span * 3.0).ceil() as usize;
    for s in 1..steps {
        let t = s as f32 / steps as f32;
        let c: Vec<f32> = (0..3).map(|k| from[k] + (to[k] - from[k]) * t).collect();
        let p = (c[0].floor() as usize, c[1].floor() as usize, c[2].floor() as usize);
        if p == a || p == b {
            continue;
        }
        let i = map.idx(p);
        if map.block[i].solid() || map.feature[i].blocks_sight() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Block, Feature, FLAT_Z};

    const Z: usize = FLAT_Z;

    #[test]
    fn path_goes_around_walls() {
        let mut m = Map::flat(10, 5);
        for y in 0..4 {
            let i = m.idx((5, y, Z));
            m.feature[i] = Feature::built("wall");
        }
        let df = bfs(&m, (0, 0, Z));
        let d = df.dist((9, 0, Z)).expect("reachable through the gap at y=4");
        assert_eq!(d, 9 + 8);
        let path = df.path_to((9, 0, Z));
        assert_eq!(path.len() as i32, d);
        assert_eq!(path[0], (9, 0, Z));
        assert!(path.iter().all(|&p| m.passable(p)));
        assert_eq!(df.order[0], m.idx((0, 0, Z)), "order starts at the start");
    }

    #[test]
    fn walled_off_cells_are_unreachable() {
        let mut m = Map::flat(10, 5);
        for y in 0..5 {
            let i = m.idx((5, y, Z));
            m.feature[i] = Feature::built("wall");
        }
        let df = bfs(&m, (0, 0, Z));
        assert!(df.dist((9, 0, Z)).is_none());
        assert_eq!(df.approach(&m, (5, 2, Z), true).map(|(p, _)| p), Some((4, 2, Z)));
    }

    #[test]
    fn paths_climb_gentle_slopes() {
        // A staircase of natural ground: each column one level higher.
        let mut m = Map::empty(6, 1, 8);
        for x in 0..6 {
            for z in 0..=x {
                let i = m.idx((x, 0, z));
                m.block[i] = Block::Soil;
            }
        }
        let df = bfs(&m, (0, 0, 1));
        assert_eq!(df.dist((5, 0, 6)), Some(5));
    }

    #[test]
    fn hills_and_walls_block_line_of_sight() {
        let mut m = Map::flat(10, 10);
        assert!(line_of_sight(&m, (0, 0, Z), (9, 9, Z)));
        let i = m.idx((5, 5, Z));
        m.feature[i] = Feature::built("wall");
        assert!(!line_of_sight(&m, (0, 0, Z), (9, 9, Z)));

        let mut m = Map::flat(10, 3);
        let i = m.idx((5, 1, Z));
        m.block[i] = Block::Rock;
        assert!(!line_of_sight(&m, (0, 1, Z), (9, 1, Z)), "a mound blocks the view");
        assert!(line_of_sight(&m, (0, 1, Z + 1), (9, 1, Z + 1)), "but not from above");
    }
}
