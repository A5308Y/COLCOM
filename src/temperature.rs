//! Temperature. Outdoors it follows the year (spring starting mild, summer
//! warm, autumn cooling, winter freezing from its start and thawing towards
//! spring) and the day (coldest before dawn, warmest in the afternoon). Rooms keep their warmth: each follows the outdoor
//! temperature slowly (bigger rooms slower, every door to it faster), plus the
//! heat of what burns in it (`BuildDef::heat`: torches, stoves, forges).
//!
//! Colonists feel the temperature where they are. Below `COLD` they're cold,
//! below `FREEZING` freezing and they get hypothermia (`Health::hypothermia`),
//! which slows them, then downs and in the end kills them; warm again, it
//! wears off. Above `HOT` they're hot.

use crate::map::Pos;
use crate::pawn::Pawn;
use crate::pathing::ANY_DISTANCE;
use crate::seasons::SEASON_LEN;
use crate::world::World;
use crate::DAY_LEN;

/// Average temperature over the year (°C), and how far summer and winter
/// are from it; the day's swing either side of the day's average.
const YEAR_MEAN: f32 = 11.0;
const YEAR_SWING: f32 = 16.0;
const DAY_SWING: f32 = 4.0;
/// Where in the year (0 = the first day of spring, 1 = a whole year) it's
/// warmest: around the start of summer, so it's coldest around the start of
/// winter and spring begins mild.
const WARMEST: f32 = 0.275;
/// Felt temperatures (°C) below which colonists are cold, freezing (and get
/// hypothermia), and above which they're hot.
pub const COLD: f32 = 10.0;
pub const FREEZING: f32 = 0.0;
pub const HOT: f32 = 30.0;
/// Hypothermia from which freezing colonists stop what they're doing to get
/// warm somewhere (`Job::WarmUp`), and how warm a place must feel for that:
/// warm enough for hypothermia to wear off quickly.
pub const WARM_UP_AT: f32 = 0.1;
const WARM_SPOT: f32 = FREEZING + 5.0;
/// How long (game seconds) a small room takes to follow the outdoors most of the way.
const ROOM_LAG: f32 = 0.2 * DAY_LEN;

/// How much clothes' warmth counts at `t` °C: fully at `COLD` and below,
/// fading to nothing at 20°C, so warm clothes don't make summer hotter.
pub fn chill(t: f32) -> f32 {
    ((20.0 - t) / (20.0 - COLD)).clamp(0.0, 1.0)
}

/// What someone wearing clothes `warmth` warm feels at `t` °C.
pub fn felt(t: f32, warmth: f32) -> f32 {
    t + warmth * chill(t)
}

/// Outdoor temperature at game time `t`: the warmest around the start of
/// summer, the coldest around the start of winter, and in the day at 15:00 and 03:00.
pub fn outdoor_at(t: f32) -> f32 {
    use std::f32::consts::TAU;
    let year = (t / (4.0 * SEASON_LEN)).fract();
    let season = YEAR_MEAN + YEAR_SWING * (TAU * (year - WARMEST)).cos();
    let hour = (t / DAY_LEN).fract() * 24.0;
    season + DAY_SWING * (TAU * (hour - 15.0) / 24.0).cos()
}

impl World {
    pub fn outdoor_temperature(&self) -> f32 {
        outdoor_at(self.time())
    }

    /// The temperature at `p`: its room's, or the outdoor one.
    pub fn temperature_at(&self, p: crate::map::Pos) -> f32 {
        let r = self.map.room.get(self.map.idx(p)).copied().unwrap_or(0) as usize;
        match self.derived.room_stats.get(r).filter(|_| r != 0) {
            Some(st) => st.temp,
            None => self.outdoor_temperature(),
        }
    }

    /// Freezing where they are, with hypothermia setting in.
    pub(crate) fn needs_warmth(&self, p: &Pawn) -> bool {
        p.health.hypothermia >= WARM_UP_AT && p.felt < FREEZING
    }

    /// The nearest place they can walk to that feels warm enough to recover.
    pub(crate) fn warm_spot(&self, p: &Pawn) -> Option<Pos> {
        let warmth = p.warmth();
        let outdoor = self.outdoor_temperature();
        // Outdoors it's as cold everywhere: only rooms can be warmer.
        if felt(outdoor, warmth) >= WARM_SPOT {
            return None;
        }
        self.nearest_cell(p.pos, ANY_DISTANCE, |i| {
            let c = self.map.pos(i);
            self.map.room[i] != 0 && self.map.standable(c) && felt(self.temperature_at(c), warmth) >= WARM_SPOT
        })
    }

    /// Warming up at `to` until the hypothermia is gone (looking again if it turns cold there).
    pub(crate) fn run_warm_up(&mut self, p: &mut Pawn, to: Pos, dt: f32) {
        if p.health.hypothermia <= 0.0 {
            self.end_job(p);
            return;
        }
        if !self.walk_to(p, to, false, dt) {
            return;
        }
        if felt(self.temperature_at(to), p.warmth()) < WARM_SPOT {
            self.end_job(p);
        }
    }

    /// Once a second, after the rooms were recounted: each room's temperature
    /// moves towards the outdoor one plus the heat of what burns in it. Rooms
    /// that changed shape start from the average of their cells' last temperatures.
    pub(crate) fn update_temperatures(&mut self, dt: f32) {
        let outdoor = self.outdoor_temperature();
        let n = self.map.len();
        if self.derived.cell_temp.len() != n {
            self.derived.cell_temp = vec![f32::NAN; n];
        }
        let rooms = self.derived.room_stats.len();
        // Recounting an unchanged room must preserve its temperature exactly.
        // Summing its repeated f32 value in f32 can round the mean on each
        // recount (including load); keep the accumulation and division in f64.
        let mut sum = vec![0.0f64; rooms];
        let mut known = vec![0usize; rooms];
        for i in 0..n {
            let r = self.map.room.get(i).copied().unwrap_or(0) as usize;
            if r != 0 && r < rooms && !self.derived.cell_temp[i].is_nan() {
                sum[r] += self.derived.cell_temp[i] as f64;
                known[r] += 1;
            }
        }
        for r in 1..rooms {
            let st = &mut self.derived.room_stats[r];
            if st.size == 0 {
                continue;
            }
            let size = st.size as f32;
            let now = if known[r] > 0 { (sum[r] / known[r] as f64) as f32 } else { outdoor };
            let target = outdoor + st.heat * 3.0 / size.sqrt();
            // Bigger rooms change slower; doors let the outside in.
            let lag = ROOM_LAG * (size.sqrt() / 3.0).max(1.0) / (1.0 + 0.5 * st.doors as f32);
            st.temp = now + (target - now) * (1.0 - (-dt / lag).exp());
        }
        for i in 0..n {
            let r = self.map.room.get(i).copied().unwrap_or(0) as usize;
            self.derived.cell_temp[i] = if r != 0 && r < rooms { self.derived.room_stats[r].temp } else { f32::NAN };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Feature, FLAT_Z as Z};
    use crate::seasons::Season;
    use crate::testing::{build, colonist, quiet_world, run};

    #[test]
    fn recounting_rooms_preserves_saved_temperature_bits() {
        let mut w = quiet_world(12, 12, 1);
        room(&mut w);
        w.map.recompute_rooms();
        w.update_room_stats();
        let i = w.map.idx((4, 4, Z));
        let r = w.map.room[i];
        assert_ne!(r, 0);
        for temperature in [6.916_224_5f32, -6.916_224_5, 21.5] {
            for (i, &room) in w.map.room.iter().enumerate() {
                if room == r { w.derived.cell_temp[i] = temperature; }
            }
            for _ in 0..10 {
                w.update_room_stats();
                assert_eq!(w.temperature_at((4, 4, Z)).to_bits(), temperature.to_bits());
                assert!(w.map.room.iter().enumerate().filter(|&(_, &room)| room == r)
                    .all(|(i, _)| w.derived.cell_temp[i].to_bits() == temperature.to_bits()));
            }
        }
    }

    #[test]
    fn summer_is_warm_winter_freezing_and_nights_cold() {
        let mid = |season: f32| season * SEASON_LEN + 2.0 * DAY_LEN;
        let at = |t: f32, hour: f32| outdoor_at(t + hour / 24.0 * DAY_LEN);
        assert!(at(mid(1.0), 15.0) > 25.0, "a summer afternoon");
        assert!(at(3.0 * SEASON_LEN, 3.0) < -6.0, "a night as winter starts");
        assert!(at(mid(3.0), 3.0) < 0.0, "still freezing nights in midwinter");
        assert!(at(0.0, 6.0) > 3.0, "a new colony's first morning isn't freezing");
        assert!(at(mid(1.0), 15.0) - at(mid(1.0), 3.0) > 6.0, "the day's swing");
        assert_eq!(Season::at(mid(3.0)), Season::Winter);
    }

    /// A roofed 3×3 room inside walls at 2..=6, door on the west.
    fn room(w: &mut World) {
        for y in 2..=6 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 2 || y == 6 {
                    build(w, (x, y, Z), if (x, y) == (2, 4) { "door" } else { "wall" });
                }
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_roof(i, true);
            }
        }
        w.map.recompute_rooms();
    }

    #[test]
    fn rooms_keep_warm_and_fires_heat_them() {
        let mut w = quiet_world(12, 12, 1);
        room(&mut w);
        // A winter afternoon: the room starts at the outdoor temperature...
        w.set_time(3.0 * SEASON_LEN + 0.6 * DAY_LEN);
        run(&mut w, 1.0);
        let start = w.temperature_at((4, 4, Z));
        // ...and through the night stays warmer than outside.
        run(&mut w, 0.4 * DAY_LEN);
        let (inside, outside) = (w.temperature_at((4, 4, Z)), w.outdoor_temperature());
        assert!(inside > outside + 2.0 && inside < start, "{} inside, {} out, from {}", inside, outside, start);
        assert_eq!(w.temperature_at((9, 9, Z)), outside, "outdoors is outdoors");
        // A stove in it warms it well above the outdoors.
        build(&mut w, (4, 4, Z), "stove");
        run(&mut w, DAY_LEN);
        assert!(w.temperature_at((4, 5, Z)) > w.outdoor_temperature() + 10.0);
    }

    #[test]
    fn clothes_warm_in_the_cold_but_not_in_the_heat() {
        assert_eq!(felt(-5.0, 14.0), 9.0, "all of it in the cold");
        assert_eq!(felt(25.0, 14.0), 25.0, "none in the heat");
        assert!(felt(15.0, 10.0) > 15.0 && felt(15.0, 10.0) < 25.0, "some in between");
    }

    #[test]
    fn dressed_for_winter_they_work_outside_without_freezing() {
        let mut w = quiet_world(12, 12, 1);
        let warm = colonist(&mut w, (8, 8, Z));
        let bare = colonist(&mut w, (9, 9, Z));
        for p in w.pawns.iter_mut() {
            p.priorities = crate::pawn::PerWork::filled(crate::pawn::WorkPriority::Never);
            p.apparel = vec![crate::testing::item("clothes")];
            if p.id == warm {
                p.apparel.extend([crate::testing::item("parka"), crate::testing::item("wool_hat")]);
            }
        }
        // Winter's first day, outdoors all of it, from midnight.
        w.set_time(3.0 * SEASON_LEN);
        for _ in 0..8 {
            run(&mut w, 60.0);
            for p in w.pawns.iter_mut() {
                p.hunger = 100.0;
                p.rest = 100.0;
            }
        }
        let (pw, pb) = (w.pawn(warm).unwrap(), w.pawn(bare).unwrap());
        assert!(pw.felt > FREEZING && pw.health.hypothermia == 0.0, "{} felt", pw.felt);
        assert!(pb.health.hypothermia > 0.0);
    }

    #[test]
    fn freezing_colonists_warm_up_indoors_then_carry_on() {
        let mut w = quiet_world(14, 12, 1);
        room(&mut w);
        build(&mut w, (4, 4, Z), "fireplace");
        build(&mut w, (5, 3, Z), "stove");
        // A winter day: the fires have had a day to warm the room.
        w.set_time(3.0 * SEASON_LEN + 0.3 * DAY_LEN);
        run(&mut w, DAY_LEN);
        assert!(w.temperature_at((5, 5, Z)) > WARM_SPOT && w.outdoor_temperature() < FREEZING, "{} in, {} out", w.temperature_at((5, 5, Z)), w.outdoor_temperature());
        let id = colonist(&mut w, (11, 9, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].priorities = crate::pawn::PerWork::filled(crate::pawn::WorkPriority::Never);
        w.pawns[i].health.hypothermia = 0.3;
        run(&mut w, 2.0);
        assert!(matches!(w.pawn(id).unwrap().job, Some(crate::pawn::Job::WarmUp { .. })), "{:?}", w.pawn(id).unwrap().job);
        // Once warm they're done with it (and free to go out again).
        let mut warmed = false;
        for _ in 0..14 * 30 {
            run(&mut w, 1.0);
            let i = w.pawn_index(id).unwrap();
            (w.pawns[i].hunger, w.pawns[i].rest) = (100.0, 100.0);
            let p = &w.pawns[i];
            if p.health.hypothermia == 0.0 && !matches!(p.job, Some(crate::pawn::Job::WarmUp { .. })) {
                warmed = true;
                break;
            }
        }
        assert!(warmed, "warmed up: {}", w.pawn(id).unwrap().health.hypothermia);
    }

    #[test]
    fn with_nowhere_warm_they_carry_on() {
        let mut w = quiet_world(12, 12, 1);
        w.set_time(3.0 * SEASON_LEN);
        let id = colonist(&mut w, (8, 8, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].health.hypothermia = 0.3;
        run(&mut w, 2.0);
        let p = w.pawn(id).unwrap();
        assert!(w.needs_warmth(p) && w.warm_spot(p).is_none());
        assert!(!matches!(p.job, Some(crate::pawn::Job::WarmUp { .. })));
    }

    #[test]
    fn a_fireplace_keeps_a_room_above_freezing_through_winter() {
        let mut w = quiet_world(12, 12, 1);
        room(&mut w);
        build(&mut w, (4, 4, Z), "fireplace");
        // From an evening at the start of winter through its coldest night.
        w.set_time(3.0 * SEASON_LEN - 0.2 * DAY_LEN);
        run(&mut w, 0.4 * DAY_LEN);
        assert!(w.outdoor_temperature() < -6.0);
        assert!(w.temperature_at((5, 5, Z)) > FREEZING, "{}", w.temperature_at((5, 5, Z)));
    }

    #[test]
    fn out_in_the_winter_cold_colonists_freeze_and_warm_up_again() {
        let mut w = quiet_world(12, 12, 1);
        let id = colonist(&mut w, (9, 9, Z));
        w.set_time(3.0 * SEASON_LEN + 0.5 * DAY_LEN);
        for p in w.pawns.iter_mut() {
            p.priorities = crate::pawn::PerWork::filled(crate::pawn::WorkPriority::Never);
        }
        let feed = |w: &mut World| {
            for p in w.pawns.iter_mut() {
                p.hunger = 100.0;
                p.rest = 100.0;
            }
        };
        for _ in 0..8 {
            run(&mut w, 60.0);
            feed(&mut w);
        }
        let p = w.pawn(id).unwrap();
        assert!(p.health.hypothermia > 0.05, "{}", p.health.hypothermia);
        assert!(p.felt < FREEZING, "the cold is still affecting them");
        let mut comfortable = p.clone();
        comfortable.felt = 20.0;
        assert!(p.mood_target() < comfortable.mood_target(), "feeling freezing hurts mood");
        let before = p.health.hypothermia;
        // Indoors by a stove and a smelter (the doorway walled up, so they stay in), it wears off.
        room(&mut w);
        build(&mut w, (2, 4, Z), "wall");
        build(&mut w, (4, 4, Z), "stove");
        build(&mut w, (3, 3, Z), "smelter");
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].pos = (5, 5, Z);
        (w.pawns[i].x, w.pawns[i].y) = (5.0, 5.0);
        for _ in 0..6 {
            run(&mut w, 60.0);
            feed(&mut w);
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].pos = (5, 5, Z);
            (w.pawns[i].x, w.pawns[i].y) = (5.0, 5.0);
            w.pawns[i].job = None;
        }
        assert!(w.pawn(id).unwrap().health.hypothermia < before);
        assert!(!matches!(w.map.feature[w.map.idx((4, 4, Z))], Feature::None));
    }
}
