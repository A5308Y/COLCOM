//! Breadth-first distance fields over the walking graph, and line of sight.

use crate::map::{Map, Pos};
use std::collections::VecDeque;

/// No limit on how far `DistField::nearest_first` and friends look.
pub const ANY_DISTANCE: i32 = i32::MAX;

/// Distances from one start cell to every cell a pawn can walk to.
pub struct DistField {
    /// Steps from the start plus one; 0 = unreachable.
    steps: Vec<u32>,
    parent: Vec<u32>,
    /// Reached cells, nearest first.
    pub order: Vec<usize>,
    w: usize,
    h: usize,
    queue: VecDeque<Pos>,
}

// Fields can coexist (e.g. ordinary and climbing searches). Return their
// storage on drop, clearing only previously reached cells when reused.
#[derive(Default)]
struct Buffers {
    steps: Vec<u32>,
    parent: Vec<u32>,
    order: Vec<usize>,
    queue: VecDeque<Pos>,
}

thread_local! {
    static FIELDS: std::cell::RefCell<Vec<Buffers>> = const { std::cell::RefCell::new(Vec::new()) };
}

impl Buffers {
    fn take(len: usize) -> Self {
        let mut b = FIELDS.with_borrow_mut(|pool| pool.pop().unwrap_or_default());
        for &i in &b.order { b.steps[i] = 0; }
        b.steps.resize(len, 0);
        b.order.clear();
        b.queue.clear();
        b
    }
}

impl Drop for DistField {
    fn drop(&mut self) {
        FIELDS.with_borrow_mut(|pool| {
            if pool.len() < 4 {
                pool.push(Buffers {
                    steps: std::mem::take(&mut self.steps),
                    parent: std::mem::take(&mut self.parent),
                    order: std::mem::take(&mut self.order),
                    queue: std::mem::take(&mut self.queue),
                });
            }
        });
    }
}

/// BFS over walkable cells. The start cell is always included, even if it
/// can't be stood on (e.g. a pawn where a wall was just built).
///
/// Shelves (and other storage nobody walks onto) beside reached cells are
/// reached too, one step further, but nothing is reached through them.
pub fn bfs(map: &Map, start: Pos) -> DistField {
    search(map, start, |_| false, true, false)
}

/// BFS for a trapped pawn: also over furniture (`Map::climbable`) and up the
/// rock face of a pit (`Map::cling`), never through walls.
pub fn bfs_climbing(map: &Map, start: Pos) -> DistField {
    search(map, start, |_| false, true, true)
}

/// BFS that doesn't go through cells (by index) that are `blocked`, such as
/// enemies' cells in a battle; only cells that can be stood on.
pub fn bfs_avoiding(map: &Map, start: Pos, blocked: impl Fn(usize) -> bool) -> DistField {
    search(map, start, blocked, false, false)
}

/// How a cell was reached from its neighbour in a search.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// Walked onto: searched on from.
    Walk,
    /// Climbed onto by a trapped pawn: searched on from.
    Climb,
    /// Storage reached from beside it: a dead end, not searched on from.
    Shelf,
}

/// The cells one step from `p`, in the order every search takes them.
fn successors(map: &Map, p: Pos, shelves: bool, climb: bool, mut visit: impl FnMut(Pos, Step)) {
    for nb in map.walk_neighbors(p) {
        visit(nb, Step::Walk);
    }
    // Over furniture, for a pawn who'd otherwise be stuck.
    if climb {
        for (dx, dy) in crate::map::DIRS {
            let Some(nb) = map.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32) else { continue };
            if map.climbable(nb) {
                visit(nb, Step::Climb);
            }
        }
        // Up a pit's wall, hand over hand, and over its rim at the top.
        let up = map.above(p).filter(|&a| map.cling(a));
        let headroom = map.above(p).is_some_and(|a| map.open(a));
        let rims = crate::map::DIRS.into_iter().filter_map(|(dx, dy)| {
            let wall = map.at(p.0 as i32 + dx, p.1 as i32 + dy, p.2 as i32)?;
            let top = map.above(wall)?;
            (headroom && map.solid(wall) && map.standable(top)).then_some(top)
        });
        for nb in up.into_iter().chain(rims) {
            visit(nb, Step::Climb);
        }
    }
    // Reached from here, but a dead end: not queued.
    if shelves {
        for nb in map.reach_neighbors(p) {
            visit(nb, Step::Shelf);
        }
    }
}

fn search(map: &Map, start: Pos, blocked: impl Fn(usize) -> bool, shelves: bool, climb: bool) -> DistField {
    search_until(map, start, blocked, shelves, climb, &[])
}

fn search_until(map: &Map, start: Pos, blocked: impl Fn(usize) -> bool, shelves: bool, climb: bool, goals: &[Pos]) -> DistField {
    let Buffers { mut steps, mut parent, mut order, mut queue } = Buffers::take(map.len());
    parent.resize(map.len(), 0);
    let si = map.idx(start);
    steps[si] = 1;
    order.push(si);
    queue.push_back(start);
    let mut goal_step = goals.contains(&start).then_some(1);
    while let Some(p) = queue.pop_front() {
        let pi = map.idx(p);
        let d = steps[pi];
        if goal_step.is_some_and(|best| d >= best) { break; }
        successors(map, p, shelves, climb, |nb, how| {
            let ni = map.idx(nb);
            if steps[ni] == 0 && (how != Step::Walk || !blocked(ni)) {
                steps[ni] = d + 1;
                parent[ni] = pi as u32;
                order.push(ni);
                if goals.contains(&nb) { goal_step = Some(d + 1); }
                if how != Step::Shelf {
                    queue.push_back(nb);
                }
            }
        });
    }
    DistField { steps, parent, order, w: map.w, h: map.h, queue }
}

/// A shortest path to the nearest goal; ties follow the goals' order, as
/// in `DistField::approach`. Finish its BFS layer to preserve that ordering.
pub fn path_to_any(map: &Map, start: Pos, goals: &[Pos], climb: bool) -> Option<Vec<Pos>> {
    if goals.is_empty() { return None; }
    let df = search_until(map, start, |_| false, true, climb, goals);
    let goal = goals.iter().filter_map(|&p| df.dist(p).map(|d| (p, d))).min_by_key(|&(_, d)| d)?.0;
    Some(df.path_to(goal))
}

/// Cells from which an edge can be walked to. Traverse the walking graph
/// backwards: stairs and unusual starting cells need not have symmetric edges.
pub struct EdgeReach {
    field: DistField,
}

impl EdgeReach {
    pub fn contains(&self, map: &Map, p: Pos) -> bool {
        self.field.dist(p).is_some() || (!map.standable(p) && map.walk_neighbors(p).any(|nb| self.field.dist(nb).is_some()))
    }
}

pub fn edge_reach(map: &Map) -> EdgeReach {
    let Buffers { mut steps, parent, mut order, mut queue } = Buffers::take(map.len());
    for z in 0..map.d {
        let boundary = (0..map.w).flat_map(|x| [(x, 0, z), (x, map.h - 1, z)])
            .chain((0..map.h).flat_map(|y| [(0, y, z), (map.w - 1, y, z)]));
        for p in boundary {
            let i = map.idx(p);
            if steps[i] == 0 && map.standable(p) {
                steps[i] = 1;
                order.push(i);
                queue.push_back(p);
            }
        }
    }
    while let Some(p) = queue.pop_front() {
        for nb in map.walk_predecessors(p) {
            let i = map.idx(nb);
            if steps[i] == 0 {
                steps[i] = 1;
                order.push(i);
                queue.push_back(nb);
            }
        }
    }
    EdgeReach { field: DistField { steps, parent, order, w: map.w, h: map.h, queue } }
}

/// Scratch space for `nearest`, kept between searches: cells are marked
/// with the search's number, so nothing has to be cleared or allocated.
struct Scratch {
    seen: Vec<u32>,
    search: u32,
    queue: VecDeque<(Pos, u32)>,
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> =
        const { std::cell::RefCell::new(Scratch { seen: Vec::new(), search: 0, queue: VecDeque::new() }) };
}

/// The cell `bfs(map, start).nearest(max, test)` would find, but stopping
/// there instead of searching the whole map first.
pub fn nearest(map: &Map, start: Pos, max: i32, mut test: impl FnMut(usize) -> bool) -> Option<usize> {
    SCRATCH.with_borrow_mut(|s| {
        let n = map.len();
        if s.seen.len() != n || s.search == u32::MAX {
            s.seen = vec![0; n];
            s.search = 0;
        }
        s.search += 1;
        let mark = s.search;
        s.queue.clear();
        let si = map.idx(start);
        s.seen[si] = mark;
        if max >= 0 && test(si) {
            return Some(si);
        }
        s.queue.push_back((start, 0));
        while let Some((p, d)) = s.queue.pop_front() {
            if d as i64 + 1 > max as i64 {
                return None;
            }
            let mut found = None;
            let Scratch { seen, queue, .. } = s;
            successors(map, p, true, false, |nb, how| {
                let ni = map.idx(nb);
                if found.is_some() || seen[ni] == mark {
                    return;
                }
                seen[ni] = mark;
                if test(ni) {
                    found = Some(ni);
                } else if how != Step::Shelf {
                    queue.push_back((nb, d + 1));
                }
            });
            if found.is_some() {
                return found;
            }
        }
        None
    })
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

    /// Reached cells (by index), nearest first, with how far they are, up to
    /// `max` steps (`ANY_DISTANCE` for all).
    pub fn nearest_first(&self, max: i32) -> impl Iterator<Item = (usize, i32)> + '_ {
        self.order.iter().map(|&i| (i, self.dist_i(i).unwrap_or(0))).take_while(move |&(_, d)| d <= max)
    }

    /// The nearest reached cell (by index) within `max` steps that passes `test`.
    pub fn nearest(&self, max: i32, mut test: impl FnMut(usize) -> bool) -> Option<usize> {
        self.nearest_first(max).map(|(i, _)| i).find(|&i| test(i))
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
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((nb, d));
                    }
                }
            }
        }
        best
    }

    /// The nearest place to build at `target` from (`Map::build_around`),
    /// and how far it is. Never on shelves.
    pub fn approach_build(&self, map: &Map, target: Pos) -> Option<(Pos, i32)> {
        map.build_around(target, map.overhead_work(target))
            .filter(|&nb| !map.storage_furniture(map.idx(nb)))
            .filter_map(|nb| self.dist(nb).map(|d| (nb, d)))
            .min_by_key(|&(_, d)| d)
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

    #[test]
    fn nearest_looks_outwards_and_stops_at_the_limit() {
        let m = Map::flat(10, 3);
        let df = bfs(&m, (1, 1, Z));
        let dists: Vec<i32> = df.nearest_first(3).map(|(_, d)| d).collect();
        assert!(dists.windows(2).all(|w| w[0] <= w[1]), "nearest first");
        assert_eq!(dists.last(), Some(&3), "up to the limit");
        let at = |x: usize| m.idx((x, 1, Z));
        assert_eq!(df.nearest(ANY_DISTANCE, |i| i == at(7) || i == at(4)), Some(at(4)), "the nearer of two");
        assert_eq!(df.nearest(2, |i| i == at(4)), None, "too far");
    }

    #[test]
    fn early_search_matches_full_fields_and_reuses_buffers() {
        for (w, h) in [(12, 8), (3, 3), (15, 9)] {
            let mut m = Map::flat(w, h);
            let wall = m.idx((w / 2, h / 2, Z));
            m.feature[wall] = Feature::built("wall");
            let shelf = m.idx((1, 1, Z));
            m.feature[shelf] = Feature::built("shelf");
            let start = (0, 0, Z);
            let df = bfs(&m, start);
            for limit in [-1, 0, 1, 5, ANY_DISTANCE] {
                for target in 0..m.len() {
                    assert_eq!(nearest(&m, start, limit, |i| i == target), df.nearest(limit, |i| i == target));
                }
            }
            for goal in [(w - 1, h - 1, Z), (1, 1, Z), (w / 2, h / 2, Z)] {
                let goals: Vec<_> = std::iter::once(goal).chain(m.around(goal))
                    .filter(|&c| !m.storage_furniture(m.idx(c))).collect();
                for climb in [false, true] {
                    let full = if climb { bfs_climbing(&m, start) } else { bfs(&m, start) };
                    let expected = full.approach(&m, goal, true).map(|(at, _)| full.path_to(at));
                    assert_eq!(path_to_any(&m, start, &goals, climb), expected);
                }
            }
            // A fresh full field must not inherit marks from an early exit.
            let again = bfs(&m, start);
            assert_eq!(again.order, df.order);
            assert_eq!(again.steps, df.steps);
        }
    }

    #[test]
    fn edge_field_matches_forward_search_on_slopes_stairs_and_blocked_starts() {
        let mut maps = vec![Map::flat(7, 7), Map::generate(12, 10, 10, &mut crate::rng::Rng::new(42))];
        let m = &mut maps[0];
        for x in 2..5 {
            for y in 2..5 {
                if x == 2 || x == 4 || y == 2 || y == 4 {
                    let i = m.idx((x, y, Z));
                    m.feature[i] = Feature::built("wall");
                }
            }
        }
        let stair = m.idx((1, 1, Z));
        m.feature[stair] = Feature::built("stairs");
        let ladder = m.idx((5, 5, Z));
        m.feature[ladder] = Feature::built("ladder");
        let above = m.idx((5, 5, Z + 1));
        m.floor[above] = true;
        for m in maps {
            let edges = edge_reach(&m);
            for i in 0..m.len() {
                let p = m.pos(i);
                let expected = bfs(&m, p).order.iter().any(|&j| {
                    let q = m.pos(j);
                    (q.0 == 0 || q.1 == 0 || q.0 + 1 == m.w || q.1 + 1 == m.h) && m.standable(q)
                });
                assert_eq!(edges.contains(&m, p), expected, "start {p:?}");
            }
        }
    }

}
