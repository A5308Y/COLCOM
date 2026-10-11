//! Breadth-first distance fields and line of sight.

use crate::map::{Map, Pos};
use std::collections::VecDeque;

/// Distances from one start tile to every reachable tile.
pub struct DistField {
    /// Steps from the start, or -1 if unreachable.
    pub cost: Vec<i32>,
    parent: Vec<usize>,
    w: usize,
}

/// BFS over passable tiles. The start tile is always included, even if it is
/// itself impassable (e.g. a pawn standing where a wall was just built).
pub fn bfs(map: &Map, start: Pos) -> DistField {
    let n = map.w * map.h;
    let mut cost = vec![-1; n];
    let mut parent = vec![usize::MAX; n];
    cost[map.idx(start)] = 0;
    let mut q = VecDeque::new();
    q.push_back(start);
    while let Some(p) = q.pop_front() {
        let pi = map.idx(p);
        let d = cost[pi];
        for nb in map.neighbors(p) {
            let ni = map.idx(nb);
            if cost[ni] < 0 && map.passable(nb) {
                cost[ni] = d + 1;
                parent[ni] = pi;
                q.push_back(nb);
            }
        }
    }
    DistField { cost, parent, w: map.w }
}

impl DistField {
    pub fn dist(&self, p: Pos) -> Option<i32> {
        let d = self.cost[p.1 * self.w + p.0];
        if d >= 0 {
            Some(d)
        } else {
            None
        }
    }

    /// Closest tile from which `target` can be reached: the target itself, or
    /// (if `adjacent`) any of its 4 neighbours.
    pub fn approach(&self, map: &Map, target: Pos, adjacent: bool) -> Option<(Pos, i32)> {
        let mut best = self.dist(target).map(|d| (target, d));
        if adjacent {
            for nb in map.neighbors(target) {
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
    /// The start tile is not included.
    pub fn path_to(&self, goal: Pos) -> Vec<Pos> {
        let mut path = Vec::new();
        let mut cur = goal.1 * self.w + goal.0;
        while self.cost[cur] > 0 {
            path.push((cur % self.w, cur / self.w));
            cur = self.parent[cur];
        }
        path
    }
}

/// Bresenham line between tile centres; blocked by walls, doors and rock
/// (the endpoints themselves don't count).
pub fn line_of_sight(map: &Map, a: Pos, b: Pos) -> bool {
    let (mut x, mut y) = (a.0 as i32, a.1 as i32);
    let (x1, y1) = (b.0 as i32, b.1 as i32);
    let dx = (x1 - x).abs();
    let dy = -(y1 - y).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let sy = if y < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if (x, y) == (x1, y1) {
            return true;
        }
        if (x, y) != (a.0 as i32, a.1 as i32) && map.feature[map.idx((x as usize, y as usize))].blocks_sight() {
            return false;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Feature;

    #[test]
    fn path_goes_around_walls() {
        let mut m = Map::new(10, 5);
        for y in 0..4 {
            let i = m.idx((5, y));
            m.feature[i] = Feature::Wall;
        }
        let df = bfs(&m, (0, 0));
        let d = df.dist((9, 0)).expect("reachable through the gap at y=4");
        assert_eq!(d, 9 + 8);
        let path = df.path_to((9, 0));
        assert_eq!(path.len() as i32, d);
        assert_eq!(path[0], (9, 0));
        assert!(path.iter().all(|&p| m.passable(p)));
    }

    #[test]
    fn walled_off_tiles_are_unreachable() {
        let mut m = Map::new(10, 5);
        for y in 0..5 {
            let i = m.idx((5, y));
            m.feature[i] = Feature::Wall;
        }
        let df = bfs(&m, (0, 0));
        assert!(df.dist((9, 0)).is_none());
        assert_eq!(df.approach(&m, (5, 2), true).map(|(p, _)| p), Some((4, 2)));
    }

    #[test]
    fn walls_block_line_of_sight() {
        let mut m = Map::new(10, 10);
        assert!(line_of_sight(&m, (0, 0), (9, 9)));
        let i = m.idx((5, 5));
        m.feature[i] = Feature::Wall;
        assert!(!line_of_sight(&m, (0, 0), (9, 9)));
    }
}
