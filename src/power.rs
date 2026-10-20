//! Power: one grid for the whole colony (no cables). PV systems make power in
//! daylight (none at night), home batteries store what's left over, and
//! electric heaters and coolers use it (`Power::draw`) while they run. Each
//! is a thermostat for its room: a heater runs below `HEAT_BELOW`, a cooler
//! above `COOL_ABOVE`, and only in a room. Once a second (`update_power`)
//! the panels' power goes to the devices that want it, the batteries make up
//! the rest or take what's spare; when there isn't enough, the devices that
//! can't be fed stay off.

use crate::map::Feature;
use crate::world::World;
use crate::DAY_LEN;
use std::collections::BTreeSet;

/// Room temperatures (°C) below which heaters run, and above which coolers do.
pub const HEAT_BELOW: f32 = 18.0;
pub const COOL_ABOVE: f32 = 24.0;

/// How the grid did in the last second.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PowerState {
    /// Watts the panels make now, and the most they could.
    pub made: f32,
    pub peak: f32,
    /// Watts the running devices use.
    pub used: f32,
    /// Battery charge and room (watt-hours).
    pub stored: f32,
    pub capacity: f32,
    /// Devices that want to run but get no power.
    pub unpowered: usize,
}

/// What a heater or cooler is doing (see `World::device_state`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeviceState {
    Running,
    /// Its room needs nothing (or it isn't in a room).
    Idle,
    /// Wants to run, but there's no power for it.
    NoPower,
}

impl World {
    /// Whether the heater or cooler in cell `i` wants to run: its room is too
    /// cold for a heater, too warm for a cooler.
    fn device_wants(&self, i: usize) -> bool {
        let Some(d) = self.map.feature[i].structure() else { return false };
        let r = self.map.room.get(i).copied().unwrap_or(0) as usize;
        // Heaters and coolers stand in their room; one in a wall counts for the room beside.
        let r = if r == 0 {
            self.map.neighbors(self.map.pos(i)).map(|n| self.map.room[self.map.idx(n)] as usize).find(|&r| r != 0).unwrap_or(0)
        } else {
            r
        };
        let Some(room) = self.room_stats.get(r).filter(|_| r != 0) else { return false };
        if d.heat > 0.0 {
            room.temp < HEAT_BELOW
        } else {
            room.temp > COOL_ABOVE
        }
    }

    /// Once a second: share the panels' power and the batteries' charge out
    /// to the devices that want to run.
    pub(crate) fn update_power(&mut self, dt: f32) {
        let m = &self.map;
        let sun = if self.is_night() { 0.0 } else { self.daylight() };
        let (mut peak, mut made, mut capacity) = (0.0, 0.0, 0.0);
        let mut wanting: Vec<(usize, f32)> = Vec::new();
        for i in 0..m.len() {
            let Feature::Built { kind, .. } = m.feature[i] else { continue };
            let Some(p) = kind.def().power else { continue };
            peak += p.output;
            if !(kind.def().open_sky && m.covered(m.pos(i))) {
                made += p.output * sun;
            }
            capacity += p.storage;
            if p.draw > 0.0 && self.device_wants(i) {
                wanting.push((i, p.draw));
            }
        }
        let hours = dt / DAY_LEN * 24.0;
        let mut stored = self.stored_wh.min(capacity);
        let mut spare = made;
        let mut running = BTreeSet::new();
        let mut unpowered = 0;
        for (i, draw) in wanting {
            // From the panels first, then from the batteries.
            let from_battery = (draw - spare).max(0.0);
            if from_battery * hours <= stored {
                spare = (spare - draw).max(0.0);
                stored -= from_battery * hours;
                running.insert(i as u32);
            } else {
                unpowered += 1;
            }
        }
        stored = (stored + spare * hours).min(capacity);
        let used: f32 = running.iter().filter_map(|&i| self.map.feature[i as usize].structure()?.power).map(|p| p.draw).sum();
        self.stored_wh = stored;
        self.running = running;
        self.power = PowerState { made, peak, used, stored, capacity, unpowered };
    }

    /// What the heater or cooler at cell `i` is doing (None if it isn't one).
    pub fn device_state(&self, i: usize) -> Option<DeviceState> {
        self.map.feature[i].structure()?.power.filter(|p| p.draw > 0.0)?;
        Some(if self.running.contains(&(i as u32)) {
            DeviceState::Running
        } else if self.device_wants(i) {
            DeviceState::NoPower
        } else {
            DeviceState::Idle
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLAT_Z as Z;
    use crate::seasons::SEASON_LEN;
    use crate::testing::{build, quiet_world, run};

    /// A roofed 3×3 room inside walls at 2..=6, door on the west; panels outside.
    fn colony(panels: usize, battery: bool) -> World {
        let mut w = quiet_world(16, 12, 1);
        for y in 2..=6 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 2 || y == 6 {
                    build(&mut w, (x, y, Z), if (x, y) == (2, 4) { "door" } else { "wall" });
                }
                let i = w.map.idx((x, y, Z + 1));
                w.map.set_roof(i, true);
            }
        }
        for k in 0..panels {
            build(&mut w, (10 + k, 9, Z), "pv_system");
        }
        if battery {
            build(&mut w, (5, 5, Z), "home_battery");
        }
        build(&mut w, (4, 4, Z), "electric_heater");
        w.map.recompute_rooms();
        w
    }

    #[test]
    fn heaters_run_on_the_sun_and_the_battery_through_the_night() {
        // A winter morning with two panels and a battery.
        let mut w = colony(2, true);
        w.set_time(3.0 * SEASON_LEN + 0.3 * DAY_LEN);
        let heater = w.map.idx((4, 4, Z));
        run(&mut w, 0.3 * DAY_LEN);
        assert_eq!(w.device_state(heater), Some(DeviceState::Running), "{:?}", w.power);
        assert!(w.power.made > 0.0 && w.stored_wh > 0.0, "the sun runs it and charges the battery");
        assert!(w.temperature_at((3, 3, Z)) > w.outdoor_temperature() + 10.0);
        // Night: the battery carries on, until it runs dry.
        run(&mut w, 0.4 * DAY_LEN);
        assert!(w.is_night());
        assert_eq!(w.power.made, 0.0);
    }

    #[test]
    fn without_power_at_night_heaters_stay_off() {
        let mut w = colony(1, false);
        w.set_time(3.0 * SEASON_LEN + 0.05 * DAY_LEN);
        let heater = w.map.idx((4, 4, Z));
        run(&mut w, 2.0);
        assert_eq!(w.device_state(heater), Some(DeviceState::NoPower));
        assert_eq!(w.power.unpowered, 1);
        assert!(w.alerts().iter().any(|a| a.0.contains("power")));
    }

    #[test]
    fn heaters_rest_when_the_room_is_warm_and_coolers_cool_in_summer() {
        let mut w = colony(3, true);
        // A summer afternoon: the heater rests; a cooler instead cools the room.
        w.set_time(SEASON_LEN + 0.55 * DAY_LEN);
        run(&mut w, 2.0);
        let at = w.map.idx((4, 4, Z));
        assert_eq!(w.device_state(at), Some(DeviceState::Idle));
        w.map.set_feature(at, Feature::built("cooler"));
        let before = w.outdoor_temperature();
        run(&mut w, 0.15 * DAY_LEN);
        assert_eq!(w.device_state(at), Some(DeviceState::Running));
        assert!(w.temperature_at((3, 3, Z)) < before - 5.0, "{} vs {}", w.temperature_at((3, 3, Z)), before);
    }
}
