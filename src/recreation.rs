//! Recreation and daily schedules. Colonists have a need for fun
//! (`Pawn::joy`, 0..100) that drops while they're awake. They have fun at
//! things built for it (`BuildDef::fun`: a chess table, a bookshelf, a
//! dartboard on a wall), or by strolling outdoors in daylight; a themed
//! Recreation room makes it come faster (`RoomEffects::fun`).
//!
//! Each colonist's day is 24 hours, each marked (`Pawn::schedule`):
//! - `Sleep`: they go to bed even if only a little tired;
//! - `Fun`: they have fun until they're full, but don't work;
//! - `Work`: they don't stop for fun (unless there's no work);
//! - `Anything`: they work, and have fun when they're bored.
//!
//! Hunger, exhaustion and health come first whatever the hour.

use crate::defs::Special;
use crate::map::{Feature, Pos};
use crate::pawn::{Job, Pawn, PawnId, Task};
use crate::world::World;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

/// What an hour of the day is for (see the module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Block {
    #[default]
    Anything,
    Work,
    Fun,
    Sleep,
}

impl Block {
    pub const ALL: [Block; 4] = [Block::Anything, Block::Work, Block::Fun, Block::Sleep];

    pub fn name(self) -> &'static str {
        match self {
            Block::Anything => "Anything",
            Block::Work => "Work",
            Block::Fun => "Fun",
            Block::Sleep => "Sleep",
        }
    }
}

/// The day a new colonist starts with: sleep from 22:00 to 06:00, fun from
/// 18:00, anything the rest of the day.
pub fn default_schedule() -> [Block; 24] {
    std::array::from_fn(|h| match h {
        0..=5 | 22..=23 => Block::Sleep,
        18..=21 => Block::Fun,
        _ => Block::Anything,
    })
}

/// Fun lost per second awake: a full need lasts about a day and a half.
pub const JOY_DRAIN: f32 = 100.0 / (1.5 * DAY_LEN);
/// Fun gained per second at something with `fun` 1 (a stroll is 0.7).
pub const JOY_GAIN: f32 = 1.0;
/// Below this they go for some fun in `Anything` hours; from this they're full.
pub const JOY_BORED: f32 = 30.0;
pub const JOY_FULL: f32 = 98.0;
/// How much fun a stroll is.
const STROLL: f32 = 0.7;
/// Sitting on a chair for a while: a little fun.
const SIT: f32 = 0.3;
/// Seconds it takes to stand up from a chair before walking off.
pub const GET_UP: f32 = 1.2;

impl World {
    /// Set what hour `hour` of colonist `id`'s day is for.
    pub fn set_schedule(&mut self, id: PawnId, hour: usize, block: Block) {
        if let Some(i) = self.pawn_index(id) {
            if hour < 24 {
                self.pawns[i].schedule[hour] = block;
            }
        }
    }

    /// What the current hour is for, for colonist `p`.
    pub fn block_now(&self, p: &Pawn) -> Block {
        p.schedule[(self.hour() as usize).min(23)]
    }

    /// The most fun thing `p` can get to: something built for it (in a
    /// recreation room, better), else a stroll outdoors in daylight.
    pub(crate) fn find_fun(&mut self, p: &Pawn) -> Option<Job> {
        let df = crate::pathing::bfs(&self.map, p.pos);
        let m = &self.map;
        let mut best: Option<(f32, Job)> = None;
        let mut consider = |score: f32, job: Job| {
            if best.as_ref().is_none_or(|b| score > b.0) {
                best = Some((score, job));
            }
        };
        for i in 0..m.len() {
            if self.res_target[i].is_some() {
                continue;
            }
            let at = m.pos(i);
            // Furniture, used from anywhere beside it (from a free chair there if there is one).
            if let Feature::Built { kind, .. } = m.feature[i] {
                let fun = kind.def().fun;
                if fun > 0.0 {
                    let chair = m
                        .build_around(at, m.overhead_work(at))
                        .filter(|&c| m.feature[m.idx(c)].is_special(Special::Seat) && self.seat_free(c, p.id))
                        .filter_map(|c| df.dist(c).map(|d| (c, d)))
                        .min_by_key(|&(_, d)| d);
                    if let Some((stand, d)) = chair.or_else(|| df.approach_build(m, at)) {
                        consider(fun * self.fun_bonus(at) / (1.0 + d as f32 / 20.0), Job::Relax { at, stand });
                    }
                }
            }
            // On walls (a dartboard): from in front of it.
            for d in m.decor(i).iter().filter(|d| d.is_up() && d.kind.def().fun > 0.0) {
                if let Some(stand) = m.decor_front(at, d.face, d.slot) {
                    if let Some(dist) = df.dist(stand) {
                        consider(d.kind.def().fun * self.fun_bonus(stand) / (1.0 + dist as f32 / 20.0), Job::Relax { at, stand });
                    }
                }
            }
        }
        match best {
            Some((_, job)) => Some(job),
            // Nothing built for fun: a stroll, by day.
            None if !self.is_night() => self.stroll_spot(p.pos).map(|to| Job::Stroll { to }),
            None => None,
        }
    }

    /// Sitting on a chair (arrived, not passing through): eating at a table,
    /// crafting at a bench with a chair at its front, playing at a chess
    /// table, or just sitting.
    pub fn seated(&self, p: &Pawn) -> bool {
        let m = &self.map;
        let settled = p.path.is_empty() && (p.x - p.pos.0 as f32).abs() < 0.05 && (p.y - p.pos.1 as f32).abs() < 0.05;
        if !settled || !m.feature[m.idx(p.pos)].is_special(Special::Seat) {
            return false;
        }
        match p.job {
            Some(Job::Eat { picked: true, seat: Some(s), .. }) => s == p.pos,
            Some(Job::Sit { at, .. }) => at == p.pos,
            Some(Job::Relax { stand, .. }) => stand == p.pos,
            Some(Job::Work { target, task: Task::Craft }) => m.front_cell(target) == Some(p.pos),
            _ => false,
        }
    }

    /// Nobody else is on the chair at `s` or on their way to it.
    pub(crate) fn seat_free(&self, s: Pos, me: PawnId) -> bool {
        !self.pawns.iter().any(|o| {
            o.id != me
                && !o.is_dead()
                && (o.pos == s
                    || o.path_goal == Some(s)
                    || matches!(o.job, Some(Job::Eat { seat: Some(t), .. }) | Some(Job::Sit { at: t, .. }) | Some(Job::Relax { stand: t, .. }) if t == s))
        })
    }

    /// A free chair within `max` steps.
    pub(crate) fn free_chair(&self, p: &Pawn, max: i32) -> Option<Pos> {
        self.nearest_cell(p.pos, max, |i| {
            self.map.feature[i].is_special(Special::Seat) && self.seat_free(self.map.pos(i), p.id)
        })
    }

    /// Sitting on the chair at `at` until game second `until`.
    pub(crate) fn run_sit(&mut self, p: &mut Pawn, at: Pos, until: f32, dt: f32) {
        if !self.map.feature[self.map.idx(at)].is_special(Special::Seat) || self.time() >= until {
            self.end_job(p);
            return;
        }
        if !self.walk_to(p, at, false, dt) {
            return;
        }
        p.joy = (p.joy + JOY_GAIN * SIT * dt).min(100.0);
    }

    /// How much faster fun comes at `p`: a recreation room's bonus.
    fn fun_bonus(&self, p: Pos) -> f32 {
        self.room_effects(p).map_or(1.0, |(_, e)| e.fun)
    }

    /// Somewhere outdoors not too far off to stroll to.
    fn stroll_spot(&mut self, from: Pos) -> Option<Pos> {
        let df = crate::pathing::bfs(&self.map, from);
        let spots: Vec<Pos> = df
            .nearest_first(12)
            .map(|(i, _)| self.map.pos(i))
            .filter(|&c| !self.map.roofed(c) && !self.map.covered(c) && c != from)
            .collect();
        (!spots.is_empty()).then(|| spots[self.rng.below(spots.len())])
    }

    /// Having fun at `at`, from `stand`.
    pub(crate) fn run_relax(&mut self, p: &mut Pawn, at: Pos, stand: Pos, dt: f32) {
        let fun = self.fun_at(at);
        if fun <= 0.0 || self.fun_over(p) {
            self.end_job(p);
            return;
        }
        if !self.walk_to(p, stand, false, dt) {
            return;
        }
        p.joy = (p.joy + JOY_GAIN * fun * self.fun_bonus(stand) * dt).min(100.0);
    }

    /// How much fun there is at `at`: what hangs on its wall (a dartboard),
    /// or what's built there (a chess table).
    fn fun_at(&self, at: Pos) -> f32 {
        let i = self.map.idx(at);
        let on_wall = self.map.decor(i).iter().filter(|d| d.is_up()).map(|d| d.kind.def().fun).fold(0.0, f32::max);
        let built = self.map.feature[i].structure().map_or(0.0, |d| d.fun);
        on_wall.max(built)
    }

    /// Strolling outdoors: fun while walking in daylight, on to somewhere else on arrival.
    pub(crate) fn run_stroll(&mut self, p: &mut Pawn, to: Pos, dt: f32) {
        if self.fun_over(p) || self.is_night() {
            self.end_job(p);
            return;
        }
        if !self.map.roofed(p.pos) {
            p.joy = (p.joy + JOY_GAIN * STROLL * dt).min(100.0);
        }
        match self.go_to(p, to, false, dt) {
            crate::world::Step::Moving => {}
            crate::world::Step::Unreachable => self.end_job(p),
            crate::world::Step::Arrived => match self.stroll_spot(p.pos) {
                Some(next) => p.job = Some(Job::Stroll { to: next }),
                None => self.end_job(p),
            },
        }
    }

    /// Fun is over: they're full, or it's time for work or sleep.
    fn fun_over(&self, p: &Pawn) -> bool {
        p.joy >= JOY_FULL || matches!(self.block_now(p), Block::Work | Block::Sleep)
    }

    /// Something fun to do now, by the hour and how bored they are; None to work (or idle).
    pub(crate) fn fun_now(&mut self, p: &Pawn) -> Option<Job> {
        let wanted = match self.block_now(p) {
            Block::Fun => p.joy < JOY_FULL,
            Block::Anything => p.joy < JOY_BORED,
            Block::Work | Block::Sleep => false,
        };
        if wanted {
            self.find_fun(p)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Desig, FLAT_Z as Z};
    use crate::testing::{build, colonist, quiet_world, run};

    fn world() -> World {
        quiet_world(20, 20, 2)
    }

    /// A colonist with `joy`, hours all `block`.
    fn person(w: &mut World, at: Pos, joy: f32, block: Block) -> PawnId {
        let id = colonist(w, at);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].joy = joy;
        w.pawns[i].schedule = [block; 24];
        id
    }

    fn job(w: &World, id: PawnId) -> Option<Job> {
        w.pawn(id).unwrap().job
    }

    #[test]
    fn the_default_day_sleeps_at_night_and_has_fun_in_the_evening() {
        let day = default_schedule();
        assert_eq!((day[23], day[3], day[10], day[19]), (Block::Sleep, Block::Sleep, Block::Anything, Block::Fun));
    }

    #[test]
    fn bored_they_play_chess_until_entertained() {
        let mut w = world();
        build(&mut w, (8, 8, Z), "chess_table");
        let id = person(&mut w, (2, 2, Z), 20.0, Block::Anything);
        run(&mut w, 2.0);
        assert!(matches!(job(&w, id), Some(Job::Relax { .. })), "{:?}", job(&w, id));
        run(&mut w, 90.0);
        let p = w.pawn(id).unwrap();
        assert!(p.joy > 85.0, "{}", p.joy);
        assert!(p.thoughts().iter().any(|t| t.0 == "Entertained"));
        assert!(!matches!(p.job, Some(Job::Relax { .. })), "and then stop");
    }

    #[test]
    fn with_a_chair_at_the_chess_table_they_sit_down_to_play() {
        let mut w = world();
        build(&mut w, (8, 8, Z), "chess_table");
        build(&mut w, (9, 7, Z), "chair");
        let id = person(&mut w, (2, 2, Z), 20.0, Block::Anything);
        run(&mut w, 2.0);
        assert!(matches!(job(&w, id), Some(Job::Relax { stand: (9, 7, _), .. })), "{:?}", job(&w, id));
        run(&mut w, 15.0);
        assert!(w.seated(w.pawn(id).unwrap()));
    }

    #[test]
    fn crafting_with_a_chair_at_the_bench_is_sitting() {
        let mut w = world();
        build(&mut w, (8, 8, Z), "crafting_bench");
        let front = w.map.front_cell((8, 8, Z)).unwrap();
        build(&mut w, front, "chair");
        let id = person(&mut w, front, 100.0, Block::Anything);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].job = Some(Job::Work { target: (8, 8, Z), task: Task::Craft });
        assert!(w.seated(w.pawn(id).unwrap()));
        w.pawns[i].job = Some(Job::Work { target: (8, 8, Z), task: Task::Construct });
        assert!(!w.seated(w.pawn(id).unwrap()), "building isn't sitting");
    }

    #[test]
    fn idle_colonists_sit_on_free_chairs_and_get_up_before_walking_off() {
        let mut w = world();
        build(&mut w, (6, 6, Z), "chair");
        let id = person(&mut w, (4, 4, Z), 100.0, Block::Anything);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].priorities = [crate::pawn::WorkPriority::Never; crate::pawn::WorkType::ALL.len()];
        let mut sat = false;
        for _ in 0..400 {
            run(&mut w, 1.0);
            if w.seated(w.pawn(id).unwrap()) {
                sat = true;
                break;
            }
        }
        assert!(sat, "sat down at some point");
        // Taken: nobody else sits there.
        let other = person(&mut w, (2, 2, Z), 100.0, Block::Anything);
        assert_eq!(w.free_chair(w.pawn(other).unwrap(), 12), None);
        // Done sitting: they stand up before going anywhere.
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].job = Some(Job::Sit { at: (6, 6, Z), until: w.time() });
        run(&mut w, 0.2);
        let i = w.pawn_index(id).unwrap();
        assert!(w.pawns[i].getting_up > 0.0);
        w.pawns[i].job = Some(Job::Goto { to: (2, 6, Z) });
        run(&mut w, 0.6);
        assert_eq!(w.pawn(id).unwrap().pos, (6, 6, Z), "still getting up");
        run(&mut w, 3.0);
        assert_ne!(w.pawn(id).unwrap().pos, (6, 6, Z), "then off");
    }

    #[test]
    fn work_hours_mean_work_and_fun_hours_fun() {
        let mut w = world();
        build(&mut w, (8, 8, Z), "chess_table");
        let tree = w.map.idx((4, 4, Z));
        w.map.feature[tree] = Feature::plant("tree");
        w.map.desig[tree] = Some(Desig::Chop);
        let worker = person(&mut w, (2, 2, Z), 10.0, Block::Work);
        let player = person(&mut w, (2, 4, Z), 90.0, Block::Fun);
        run(&mut w, 2.0);
        assert!(matches!(job(&w, worker), Some(Job::Work { .. })), "bored, but it's work time: {:?}", job(&w, worker));
        assert!(matches!(job(&w, player), Some(Job::Relax { .. })), "fun time, though there's work: {:?}", job(&w, player));
    }

    #[test]
    fn in_their_sleep_hours_they_go_to_bed_a_little_tired() {
        let mut w = world();
        let id = person(&mut w, (2, 2, Z), 100.0, Block::Sleep);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].rest = 85.0;
        run(&mut w, 2.0);
        assert!(matches!(job(&w, id), Some(Job::Sleep { .. })));
        // The same tiredness in hours for anything: they carry on.
        let other = person(&mut w, (4, 2, Z), 100.0, Block::Anything);
        let i = w.pawn_index(other).unwrap();
        w.pawns[i].rest = 85.0;
        run(&mut w, 2.0);
        assert!(!matches!(job(&w, other), Some(Job::Sleep { .. })));
    }

    #[test]
    fn with_nothing_built_for_fun_they_stroll_by_day() {
        let mut w = world();
        let id = person(&mut w, (5, 5, Z), 20.0, Block::Fun);
        run(&mut w, 2.0);
        assert!(matches!(job(&w, id), Some(Job::Stroll { .. })));
        run(&mut w, 30.0);
        assert!(w.pawn(id).unwrap().joy > 30.0);
    }

    #[test]
    fn darts_are_thrown_from_in_front_of_the_board() {
        let mut w = world();
        for x in 6..=8 {
            build(&mut w, (x, 8, Z), "wall");
        }
        let wall = w.map.idx((7, 8, Z));
        w.map.decor.insert(wall as u32, vec![crate::decor::Decor { kind: crate::defs::BuildId::named("dartboard"), face: 2, slot: 2, state: crate::decor::DecorState::Up }]);
        let id = person(&mut w, (2, 2, Z), 20.0, Block::Fun);
        run(&mut w, 2.0);
        assert!(matches!(job(&w, id), Some(Job::Relax { stand: (7, 9, _), .. })), "{:?}", job(&w, id));
        run(&mut w, 40.0);
        assert_eq!(w.pawn(id).unwrap().pos, (7, 9, Z));
        assert!(w.pawn(id).unwrap().joy > 40.0);
    }

    #[test]
    fn fun_drains_while_awake_and_boredom_hurts() {
        let mut w = world();
        let id = person(&mut w, (2, 2, Z), 26.0, Block::Work);
        run(&mut w, 20.0);
        let p = w.pawn(id).unwrap();
        assert!(p.joy < 26.0 - 2.0, "{}", p.joy);
        assert!(p.thoughts().iter().any(|t| t.0 == "Bored"));
    }

    #[test]
    fn a_recreation_room_makes_fun_come_faster() {
        let mut w = world();
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
        build(&mut w, (3, 3, Z), "chess_table");
        build(&mut w, (5, 3, Z), "bookshelf");
        w.map.recompute_rooms();
        w.update_room_stats();
        let (k, e) = w.room_effects((4, 4, Z)).expect("a themed room");
        assert_eq!(k.def().id, "recreation");
        assert!(e.fun > 1.0);
    }
}
