//! The storyteller: schedules raids and random events.

use crate::defs::ItemId;
use crate::map::{Feature, Pos, Stack};
use crate::world::World;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

const RAIDER_NAMES: [&str; 12] = [
    "Grim", "Vex", "Rook", "Slag", "Mox", "Brakk", "Skarn", "Jag", "Tusk", "Rend", "Krill", "Nox",
];

/// Largest raid at size ×1 (bigger sizes raise it in proportion).
const MAX_RAID: usize = 8;
/// Range of the raid size multiplier.
pub const RAID_SIZES: std::ops::RangeInclusive<f32> = 0.25..=3.0;

/// Settings for testing and challenge, saved with the game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Difficulty {
    /// Raiders come at all (off: their timer runs on, but nobody comes).
    pub raids: bool,
    /// How many come, as a multiple of the usual (`RAID_SIZES`).
    pub raid_size: f32,
}

impl Default for Difficulty {
    fn default() -> Self {
        Difficulty { raids: true, raid_size: 1.0 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Storyteller {
    pub next_raid: f32,
    pub next_event: f32,
    pub raids: u32,
}

impl Storyteller {
    pub fn new() -> Self {
        Storyteller { next_raid: DAY_LEN * 2.5, next_event: DAY_LEN * 0.8, raids: 0 }
    }
}

impl Default for Storyteller {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn raider_name(rng: &mut crate::rng::Rng) -> String {
    RAIDER_NAMES[rng.below(RAIDER_NAMES.len())].to_string()
}

impl World {
    pub(crate) fn storyteller_tick(&mut self) {
        if self.colony_lost() {
            return;
        }
        if self.time >= self.story.next_raid {
            self.story.next_raid = self.time + self.rng.range_f32(1.5, 3.0) * DAY_LEN;
            if self.difficulty.raids {
                self.spawn_raid();
            }
        }
        if self.time >= self.story.next_event {
            self.story.next_event = self.time + self.rng.range_f32(0.7, 1.5) * DAY_LEN;
            self.random_event();
        }
    }

    /// Rough colony value; bigger colonies attract bigger raids.
    pub fn wealth(&self) -> u32 {
        let items: u32 = self.map.count_items_where(|_| true);
        let structures = self.map.feature.iter().filter(|f| f.structure().is_some()).count() as u32;
        items + structures * 10
    }

    /// Raiders arrive at the edge of the map and make their demand (see
    /// `battle.rs`): pay, or fight.
    /// How many raiders the next raid brings: more as the colony grows older
    /// and richer, scaled by the difficulty's raid size.
    pub fn raid_size(&self) -> usize {
        let usual = (1 + self.day() as usize / 3 + self.wealth() as usize / 800).min(MAX_RAID) as f32;
        let scale = self.difficulty.raid_size.clamp(*RAID_SIZES.start(), *RAID_SIZES.end());
        let most = (MAX_RAID as f32 * scale.max(1.0)).round();
        (usual * scale).round().clamp(1.0, most) as usize
    }

    /// A raid now (unless one is under way already). Returns whether one came.
    pub fn raid_now(&mut self) -> bool {
        if self.parley.is_some() || self.battle.is_some() || self.colony_lost() {
            return false;
        }
        self.spawn_raid();
        true
    }

    pub fn spawn_raid(&mut self) {
        let size = self.raid_size();
        let Some(entry) = self.random_edge_tile() else { return };
        self.story.raids += 1;
        self.open_parley(size, entry);
        let who = if size == 1 { "A raider is".to_string() } else { format!("{} raiders are", size) };
        self.push_log(format!("Raiders! {} at the edge of the colony, demanding goods.", who));
    }

    fn random_edge_tile(&mut self) -> Option<Pos> {
        let (w, h) = (self.map.w, self.map.h);
        for _ in 0..200 {
            let (x, y) = match self.rng.below(4) {
                0 => (self.rng.below(w), 0),
                1 => (self.rng.below(w), h - 1),
                2 => (0, self.rng.below(h)),
                _ => (w - 1, self.rng.below(h)),
            };
            if let Some(z) = self.map.surface(x, y) {
                return Some((x, y, z));
            }
        }
        None
    }

    fn random_event(&mut self) {
        let has_crops = self.map.feature.iter().any(|f| f.is_crop());
        match self.rng.below(4) {
            0 if self.colonists_alive() < 10 => self.wanderer_joins(),
            1 if has_crops => self.blight(),
            // Flu is rare: most of its draws bring cargo pods instead.
            2 if self.day() >= 5 && self.rng.chance(0.4) => self.flu_outbreak(),
            _ => self.cargo_pods(),
        }
    }

    fn wanderer_joins(&mut self) {
        let Some(pos) = self.random_edge_tile() else { return };
        let id = self.spawn_colonist(pos);
        // They bring one of the starting weapons, at random.
        let weapons = &crate::defs::defs().start_weapons;
        if !weapons.is_empty() {
            let w = weapons[self.rng.below(weapons.len())].id();
            let i = self.pawn_index(id).unwrap();
            self.pawns[i].weapon = Some(w);
        }
        let name = self.pawn(id).map(|p| p.name.clone()).unwrap_or_default();
        self.push_log(format!("{} wandered in and asked to join the colony.", name));
        self.sound("joined", None);
    }

    fn blight(&mut self) {
        let mut lost = 0;
        for i in 0..self.map.feature.len() {
            if self.map.feature[i].is_crop() && self.rng.chance(0.5) {
                self.map.set_feature(i, Feature::None);
                lost += 1;
            }
        }
        self.push_log(format!("Blight! {} crops withered.", lost));
    }

    fn flu_outbreak(&mut self) {
        let candidates: Vec<usize> =
            (0..self.pawns.len()).filter(|&i| self.pawns[i].is_colonist() && !self.pawns[i].dead).collect();
        if candidates.is_empty() {
            return;
        }
        let victims = 1 + self.rng.chance(0.2) as usize;
        let mut names = Vec::new();
        for _ in 0..victims {
            let i = candidates[self.rng.below(candidates.len())];
            if !self.pawns[i].health.has_disease() {
                self.pawns[i].health.add_flu();
                names.push(self.pawns[i].name.clone());
            }
        }
        if !names.is_empty() {
            self.push_log(format!("Flu! {} fell ill. Doctors can treat it; bed rest helps.", names.join(" and ")));
        }
    }

    fn cargo_pods(&mut self) {
        let colonists: Vec<Pos> = self.pawns.iter().filter(|p| p.is_colonist() && !p.dead).map(|p| p.pos).collect();
        if colonists.is_empty() {
            return;
        }
        let cx = colonists.iter().map(|p| p.0).sum::<usize>() / colonists.len();
        let cy = colonists.iter().map(|p| p.1).sum::<usize>() / colonists.len();
        let x = (cx as i32 + self.rng.below(13) as i32 - 6).clamp(0, self.map.w as i32 - 1) as usize;
        let y = (cy as i32 + self.rng.below(13) as i32 - 6).clamp(0, self.map.h as i32 - 1) as usize;
        let Some(z) = self.map.surface(x, y) else { return };
        let kinds: Vec<(ItemId, (u32, u32))> = ItemId::all().filter_map(|k| k.def().pods.map(|r| (k, r))).collect();
        if kinds.is_empty() {
            return;
        }
        let (kind, (lo, hi)) = kinds[self.rng.below(kinds.len())];
        let count = lo + self.rng.below((hi - lo + 1) as usize) as u32;
        self.map.drop_item((x, y, z), Stack { kind, count });
        self.sound("pods", Some((x, y, z)));
        self.push_log(format!("Cargo pods crashed nearby: {} {}.", count, kind.name().to_lowercase()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, FLAT_Z as Z};
    use crate::rng::Rng;
    use crate::TICK;

    fn test_world() -> World {
        let mut w = World::with_map(Map::flat(24, 24), Rng::new(6));
        w.spawn_colonist((5, 5, Z));
        w.story.next_event = f32::MAX;
        w.regrow_timer = f32::MAX;
        w
    }

    #[test]
    fn raids_can_be_switched_off() {
        let mut w = test_world();
        w.difficulty.raids = false;
        w.story.next_raid = 1.0;
        for _ in 0..(3.0 / TICK) as usize {
            w.tick(TICK);
        }
        assert!(w.parley.is_none() && w.battle.is_none(), "nobody came");
        assert!(w.story.next_raid > w.time, "the timer runs on");
        // Switched back on, the next one comes as planned.
        w.difficulty.raids = true;
        w.story.next_raid = w.time + 1.0;
        for _ in 0..(3.0 / TICK) as usize {
            w.tick(TICK);
        }
        assert!(w.parley.is_some(), "raiders at the door");
    }

    #[test]
    fn raid_size_scales_with_the_slider() {
        let mut w = test_world();
        w.time = 12.0 * DAY_LEN; // day 13: usually 5 raiders
        let usual = w.raid_size();
        assert_eq!(usual, 5);
        w.difficulty.raid_size = 2.0;
        assert_eq!(w.raid_size(), 10, "twice as many, past the usual cap of 8");
        w.difficulty.raid_size = 0.25;
        assert_eq!(w.raid_size(), 1, "at least one");
        w.time = 100.0 * DAY_LEN;
        w.difficulty.raid_size = 3.0;
        assert_eq!(w.raid_size(), 24, "capped at 3 × 8");
    }

    #[test]
    fn raids_come_on_demand_and_settings_are_saved() {
        let mut w = test_world();
        w.difficulty = Difficulty { raids: false, raid_size: 1.5 };
        assert!(w.raid_now(), "even with raids off");
        let size = w.parley.as_ref().unwrap().raiders();
        assert_eq!(size, w.raid_size());
        assert!(!w.raid_now(), "one at a time");
        let loaded = crate::save::from_json(&crate::save::to_json(&w).unwrap()).unwrap();
        assert_eq!(loaded.difficulty, w.difficulty);
    }
}
