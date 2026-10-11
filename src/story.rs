//! The storyteller: schedules raids and random events.

use crate::map::{Feature, ItemKind, Pos, Stack};
use crate::pathing::bfs;
use crate::world::World;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

const RAIDER_NAMES: [&str; 12] = [
    "Grim", "Vex", "Rook", "Slag", "Mox", "Brakk", "Skarn", "Jag", "Tusk", "Rend", "Krill", "Nox",
];

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

impl World {
    pub(crate) fn storyteller_tick(&mut self) {
        if self.colony_lost() {
            return;
        }
        if self.time >= self.story.next_raid {
            self.story.next_raid = self.time + self.rng.range_f32(1.5, 3.0) * DAY_LEN;
            self.spawn_raid();
        }
        if self.time >= self.story.next_event {
            self.story.next_event = self.time + self.rng.range_f32(0.7, 1.5) * DAY_LEN;
            self.random_event();
        }
    }

    /// Rough colony value; bigger colonies attract bigger raids.
    pub fn wealth(&self) -> u32 {
        let items: u32 = self.map.items.iter().flatten().map(|s| s.count).sum();
        let structures = self.map.feature.iter().filter(|f| f.built_from().is_some()).count() as u32;
        items + structures * 10
    }

    pub fn spawn_raid(&mut self) {
        let size = (1 + self.day() as usize / 3 + self.wealth() as usize / 800).min(8);
        let Some(entry) = self.random_edge_tile() else { return };
        let df = bfs(&self.map, entry);
        let mut spots: Vec<usize> = (0..df.cost.len()).filter(|&i| df.cost[i] >= 0 && df.cost[i] <= 6).collect();
        spots.sort_by_key(|&i| df.cost[i]);
        let count = size.min(spots.len());
        for &i in spots.iter().take(count) {
            let name = RAIDER_NAMES[self.rng.below(RAIDER_NAMES.len())].to_string();
            let pos = self.map.pos(i);
            self.spawn_raider(name, pos);
        }
        self.story.raids += 1;
        self.push_log(format!("RAID! {} raiders are attacking. Draft your colonists with F!", count));
    }

    fn random_edge_tile(&mut self) -> Option<Pos> {
        let (w, h) = (self.map.w, self.map.h);
        for _ in 0..200 {
            let p = match self.rng.below(4) {
                0 => (self.rng.below(w), 0),
                1 => (self.rng.below(w), h - 1),
                2 => (0, self.rng.below(h)),
                _ => (w - 1, self.rng.below(h)),
            };
            if self.map.passable(p) {
                return Some(p);
            }
        }
        None
    }

    fn random_event(&mut self) {
        let has_crops = self.map.feature.iter().any(|f| matches!(f, Feature::Crop { .. }));
        match self.rng.below(3) {
            0 if self.colonists_alive() < 10 => self.wanderer_joins(),
            1 if has_crops => self.blight(),
            _ => self.cargo_pods(),
        }
    }

    fn wanderer_joins(&mut self) {
        let Some(pos) = self.random_edge_tile() else { return };
        let id = self.spawn_colonist(pos);
        let name = self.pawn(id).map(|p| p.name.clone()).unwrap_or_default();
        self.push_log(format!("{} wandered in and asked to join the colony.", name));
    }

    fn blight(&mut self) {
        let mut lost = 0;
        for i in 0..self.map.feature.len() {
            if matches!(self.map.feature[i], Feature::Crop { .. }) && self.rng.chance(0.5) {
                self.map.set_feature(i, Feature::None);
                lost += 1;
            }
        }
        self.push_log(format!("Blight! {} crops withered.", lost));
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
        let kind = ItemKind::ALL[self.rng.below(3)];
        let count = 15 + self.rng.below(26) as u32;
        self.map.drop_item((x, y), Stack { kind, count });
        self.push_log(format!("Cargo pods crashed nearby: {} {}.", count, kind.name().to_lowercase()));
    }
}
