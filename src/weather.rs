//! Weather: clear skies, fog, or rain (snow while it's freezing). Each spell
//! lasts a while, then the next is drawn by how likely each is in the season.
//! Rain and snow make it a little colder outdoors and slow walking under the
//! open sky (snow more); fog only hides the distance (drawn by the front end).

use crate::seasons::Season;
use crate::world::World;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Weather {
    #[default]
    Clear,
    Fog,
    /// Rain, or snow while it's freezing (see `World::falling`).
    Rain,
}

/// What comes down while it rains.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Precipitation {
    Rain,
    Snow,
}

/// The weather now, and the game second it changes.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct WeatherSpell {
    pub now: Weather,
    pub until: f32,
}

/// Rain turns to snow below this outdoor temperature (°C).
const SNOW_BELOW: f32 = 0.5;
/// How much colder rain and snow make it outdoors (°C).
const WET_CHILL: f32 = 2.0;
/// Walking speed under the open sky in rain and in snow.
const RAIN_PACE: f32 = 0.9;
const SNOW_PACE: f32 = 0.75;

impl Season {
    /// Chances of fog and rain when the weather changes (the rest is clear).
    fn weather_odds(self) -> (f32, f32) {
        match self {
            Season::Spring => (0.15, 0.35),
            Season::Summer => (0.05, 0.2),
            Season::Autumn => (0.25, 0.35),
            Season::Winter => (0.15, 0.35),
        }
    }
}

impl World {
    pub fn weather(&self) -> Weather {
        self.weather.now
    }

    /// What's coming down, if anything.
    pub fn falling(&self) -> Option<Precipitation> {
        match self.weather.now {
            Weather::Rain if crate::temperature::outdoor_at(self.time()) < SNOW_BELOW => Some(Precipitation::Snow),
            Weather::Rain => Some(Precipitation::Rain),
            Weather::Clear | Weather::Fog => None,
        }
    }

    /// The weather in a word: "Clear", "Fog", "Rain" or "Snow".
    pub fn weather_name(&self) -> &'static str {
        match (self.weather.now, self.falling()) {
            (Weather::Clear, _) => "Clear",
            (Weather::Fog, _) => "Fog",
            (Weather::Rain, Some(Precipitation::Snow)) => "Snow",
            (Weather::Rain, _) => "Rain",
        }
    }

    /// How much the weather lowers the outdoor temperature (°C).
    pub(crate) fn weather_chill(&self) -> f32 {
        match self.weather.now {
            Weather::Rain => WET_CHILL,
            Weather::Clear | Weather::Fog => 0.0,
        }
    }

    /// Share of their usual speed people walk at under the open sky.
    pub fn outdoor_pace(&self) -> f32 {
        match self.falling() {
            Some(Precipitation::Rain) => RAIN_PACE,
            Some(Precipitation::Snow) => SNOW_PACE,
            None => 1.0,
        }
    }

    /// Once a second: when the spell is over, draw the next one.
    pub(crate) fn update_weather(&mut self) {
        if self.time() < self.weather.until {
            return;
        }
        let (fog, rain) = self.season().weather_odds();
        let roll = self.rng.range_f32(0.0, 1.0);
        let next = if roll < fog {
            Weather::Fog
        } else if roll < fog + rain {
            Weather::Rain
        } else {
            Weather::Clear
        };
        // Clear spells last longer.
        let days = match next {
            Weather::Clear => self.rng.range_f32(0.5, 2.0),
            Weather::Fog => self.rng.range_f32(0.15, 0.4),
            Weather::Rain => self.rng.range_f32(0.25, 1.0),
        };
        let before = self.weather.now;
        self.weather = WeatherSpell { now: next, until: self.time() + days * DAY_LEN };
        if next != before {
            let text = match (next, self.falling()) {
                (Weather::Clear, _) => "The sky clears.",
                (Weather::Fog, _) => "Fog rolls in.",
                (Weather::Rain, Some(Precipitation::Snow)) => "It starts to snow.",
                (Weather::Rain, _) => "It starts to rain.",
            };
            self.push_log(text.into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLAT_Z as Z;
    use crate::testing::{colonist, quiet_world};
    use crate::TICK;

    #[test]
    fn the_weather_changes_with_the_seasons() {
        let mut w = quiet_world(10, 10, 9);
        w.weather.until = 0.0;
        let mut seen = std::collections::BTreeSet::new();
        // A year, sampled each hour.
        for _ in 0..(4 * crate::seasons::SEASON_DAYS * 24) {
            w.advance_clock(DAY_LEN / 24.0);
            w.update_weather();
            seen.insert(w.weather_name());
            assert!(w.weather.until > w.time());
        }
        for name in ["Clear", "Fog", "Rain", "Snow"] {
            assert!(seen.contains(name), "no {} in a year: {:?}", name, seen);
        }
    }

    #[test]
    fn rain_chills_and_slows_walking_outdoors_but_not_under_a_roof() {
        let walk = |rain: bool, roofed: bool| {
            let mut w = quiet_world(30, 10, 2);
            if rain {
                w.weather = WeatherSpell { now: Weather::Rain, until: f32::MAX };
            }
            if roofed {
                for x in 0..30 {
                    for y in 0..10 {
                        let i = w.map.idx((x, y, Z + 1));
                        w.map.set_roof(i, true);
                    }
                }
            }
            let id = colonist(&mut w, (2, 5, Z));
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].think_timer = f32::MAX;
            let mut p = w.pawns[i].clone();
            w.start_job(&mut p, crate::pawn::Job::Goto { to: (25, 5, Z) });
            w.pawns[i] = p;
            let mut ticks: i32 = 0;
            while w.pawn(id).unwrap().pos != (25, 5, Z) && ticks < 10_000 {
                w.tick(TICK);
                ticks += 1;
            }
            (ticks, w.outdoor_temperature())
        };
        let (dry, dry_temp) = walk(false, false);
        let (wet, wet_temp) = walk(true, false);
        let (covered, _) = walk(true, true);
        assert!(wet > dry, "{} ticks in the rain, {} dry", wet, dry);
        assert!((covered - dry).abs() <= 1, "under a roof: {} vs {}", covered, dry);
        assert!(wet_temp < dry_temp);
    }
}
