//! Seasons: a year is four seasons of `SEASON_DAYS` days, starting in spring.
//! Sown crops grow normally in spring and summer, slower in autumn and not at
//! all in winter, and the first frost of winter kills the ones not yet ripe;
//! wild plants don't grow back in winter. So the colony has to sow in time
//! and put food by for the winter.

use crate::map::Feature;
use crate::world::{World, DAWN, DUSK};
use crate::DAY_LEN;

/// Days in each season.
pub const SEASON_DAYS: u32 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub const ALL: [Season; 4] = [Season::Spring, Season::Summer, Season::Autumn, Season::Winter];

    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "Spring",
            Season::Summer => "Summer",
            Season::Autumn => "Autumn",
            Season::Winter => "Winter",
        }
    }

    /// How fast sown crops grow, as a share of their normal rate.
    pub fn crop_growth(self) -> f32 {
        match self {
            Season::Spring | Season::Summer => 1.0,
            Season::Autumn => 0.6,
            Season::Winter => 0.0,
        }
    }

    /// Whether wild plants grow back.
    pub fn regrowth(self) -> bool {
        self != Season::Winter
    }

    /// Which season game time `t` falls in.
    pub fn at(t: f32) -> Season {
        Season::ALL[((t / DAY_LEN) as u32 / SEASON_DAYS) as usize % 4]
    }
}

/// Game seconds in a season.
pub const SEASON_LEN: f32 = SEASON_DAYS as f32 * DAY_LEN;

impl World {
    pub fn season(&self) -> Season {
        Season::at(self.time)
    }

    /// Which year it is, from 1.
    pub fn year(&self) -> u32 {
        (self.day() - 1) / (4 * SEASON_DAYS) + 1
    }

    /// Which day of its season today is, from 1.
    pub fn day_of_season(&self) -> u32 {
        (self.day() - 1) % SEASON_DAYS + 1
    }

    /// Game seconds until the next season starts.
    pub fn season_left(&self) -> f32 {
        SEASON_LEN - self.time % SEASON_LEN
    }

    /// Time moved on from `before`: if a new season began, say so; winter's
    /// first frost kills the crops that aren't ripe.
    pub(crate) fn check_season(&mut self, before: f32) {
        let now = self.season();
        if Season::at(before) == now {
            return;
        }
        if now == Season::Winter {
            let killed = self.frost();
            let what = if killed > 0 { format!(" Frost killed {} crop(s) that weren't ripe.", killed) } else { String::new() };
            self.push_log(format!("Winter has come: nothing grows until spring.{}", what));
        } else {
            let note = match now {
                Season::Spring => "time to sow",
                Season::Summer => "crops grow well",
                _ => "crops grow slower; winter's frost kills what isn't ripe",
            };
            self.push_log(format!("{} of year {} has come: {}.", now.name(), self.year(), note));
        }
    }

    /// Kill the sown crops not yet ripe; how many there were.
    fn frost(&mut self) -> usize {
        let mut killed = 0;
        for i in 0..self.map.len() {
            if let Feature::Plant { kind, growth } = self.map.feature[i] {
                if kind.def().sow.is_some() && growth < 1.0 {
                    self.map.set_feature(i, Feature::None);
                    killed += 1;
                }
            }
        }
        killed
    }

    /// In autumn: sown crops that won't be ripe before the frost, at the rate
    /// they grow now (in daylight only).
    pub fn crops_frost_will_kill(&self) -> usize {
        if self.season() != Season::Autumn {
            return 0;
        }
        let light = self.season_left() * (DUSK - DAWN) / 24.0;
        let rate = Season::Autumn.crop_growth();
        self.map
            .feature
            .iter()
            .filter(|f| match f {
                Feature::Plant { kind, growth } => {
                    let d = kind.def();
                    d.sow.is_some() && *growth < 1.0 && (1.0 - growth) * d.grow_time / rate > light
                }
                _ => false,
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Zone, FLAT_Z as Z};
    use crate::testing::{quiet_world, run};

    #[test]
    fn the_year_goes_round() {
        let mut w = quiet_world(8, 8, 1);
        assert_eq!((w.season(), w.year(), w.day_of_season()), (Season::Spring, 1, 1));
        w.time = DAY_LEN * (SEASON_DAYS as f32 + 0.5);
        assert_eq!((w.season(), w.day_of_season()), (Season::Summer, 1));
        w.time = DAY_LEN * (3.0 * SEASON_DAYS as f32 + 4.5);
        assert_eq!((w.season(), w.day_of_season()), (Season::Winter, 5));
        w.time = DAY_LEN * (4.0 * SEASON_DAYS as f32 + 0.5);
        assert_eq!((w.season(), w.year()), (Season::Spring, 2));
    }

    #[test]
    fn crops_grow_by_the_season_and_frost_kills_the_unripe() {
        let mut w = quiet_world(8, 8, 1);
        let crop = crate::defs::defs().sown().unwrap();
        let (a, b) = (w.map.idx((2, 2, Z)), w.map.idx((4, 4, Z)));
        let grown = |w: &World, i: usize| match w.map.feature[i] {
            Feature::Plant { growth, .. } => growth,
            _ => -1.0,
        };
        // A day in summer, a day in autumn and a day in winter, from the same start.
        let mut day = |season: u32| {
            w.time = DAY_LEN * (season * SEASON_DAYS + 1) as f32 + 0.25 * DAY_LEN;
            w.map.set_feature(a, Feature::Plant { kind: crop, growth: 0.0 });
            w.map.zone[a] = Zone::Grow(crop);
            run(&mut w, DAY_LEN);
            grown(&w, a)
        };
        let (summer, autumn, winter) = (day(1), day(2), day(3));
        assert!(summer > 0.0 && (autumn - summer * 0.6).abs() < 0.02, "{} {}", summer, autumn);
        assert_eq!(winter, 0.0, "nothing grows in winter");
        // The last moment of autumn: one crop ripe, one not.
        w.time = SEASON_LEN * 3.0 - 1.0;
        w.map.set_feature(a, Feature::Plant { kind: crop, growth: 0.5 });
        w.map.set_feature(b, Feature::Plant { kind: crop, growth: 1.0 });
        assert!(w.crops_frost_will_kill() >= 1);
        run(&mut w, 2.0);
        assert_eq!(w.season(), Season::Winter);
        assert_eq!(w.map.feature[a], Feature::None, "killed by the frost");
        assert_eq!(grown(&w, b), 1.0, "a ripe one waits to be harvested");
        assert!(w.log.iter().any(|l| l.starts_with("Winter has come")));
    }

    #[test]
    fn nothing_grows_back_in_winter() {
        let mut w = quiet_world(30, 30, 3);
        w.time = SEASON_LEN * 3.0 + DAY_LEN * 0.5;
        for _ in 0..2000 {
            w.regrow_timer = 0.0;
            w.grow_plants(1.0);
        }
        assert!(!w.map.feature.iter().any(|f| matches!(f, Feature::Plant { .. })));
    }
}
