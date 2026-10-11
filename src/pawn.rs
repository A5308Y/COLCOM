//! Pawns (colonists and raiders): needs, skills, mood, weapons and jobs.

use crate::map::{Pos, Stack};
use crate::rng::Rng;
use crate::DAY_LEN;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Faction {
    Colony,
    Raider,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Skill {
    Construction,
    Growing,
    Mining,
    Plants,
    Shooting,
    Melee,
}

impl Skill {
    pub const ALL: [Skill; 6] = [
        Skill::Construction,
        Skill::Growing,
        Skill::Mining,
        Skill::Plants,
        Skill::Shooting,
        Skill::Melee,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Skill::Construction => "Construction",
            Skill::Growing => "Growing",
            Skill::Mining => "Mining",
            Skill::Plants => "Plants",
            Skill::Shooting => "Shooting",
            Skill::Melee => "Melee",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WorkType {
    Construction,
    Growing,
    Mining,
    PlantCutting,
    Hauling,
}

impl WorkType {
    pub const ALL: [WorkType; 5] = [
        WorkType::Construction,
        WorkType::Growing,
        WorkType::Mining,
        WorkType::PlantCutting,
        WorkType::Hauling,
    ];

    pub fn name(self) -> &'static str {
        match self {
            WorkType::Construction => "Construct",
            WorkType::Growing => "Grow",
            WorkType::Mining => "Mine",
            WorkType::PlantCutting => "Cut plants",
            WorkType::Hauling => "Haul",
        }
    }

    pub fn skill(self) -> Option<Skill> {
        match self {
            WorkType::Construction => Some(Skill::Construction),
            WorkType::Growing => Some(Skill::Growing),
            WorkType::Mining => Some(Skill::Mining),
            WorkType::PlantCutting => Some(Skill::Plants),
            WorkType::Hauling => None,
        }
    }
}

/// Work done at a single tile.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Task {
    Chop,
    Mine,
    Forage,
    Plant,
    HarvestCrop,
    Construct,
}

impl Task {
    /// Base seconds of work at skill-speed 1.0. Construction uses the build kind instead.
    pub fn work_time(self) -> f32 {
        match self {
            Task::Chop => 3.0,
            Task::Mine => 5.0,
            Task::Forage => 1.5,
            Task::Plant => 1.5,
            Task::HarvestCrop => 1.5,
            Task::Construct => 3.0,
        }
    }

    pub fn skill(self) -> Skill {
        match self {
            Task::Chop => Skill::Plants,
            Task::Mine => Skill::Mining,
            Task::Forage | Task::Plant | Task::HarvestCrop => Skill::Growing,
            Task::Construct => Skill::Construction,
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Task::Chop => "chopping",
            Task::Mine => "mining",
            Task::Forage => "foraging",
            Task::Plant => "planting",
            Task::HarvestCrop => "harvesting",
            Task::Construct => "building",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Weapon {
    Rifle,
    Pistol,
    Club,
}

impl Weapon {
    pub fn range(self) -> f32 {
        match self {
            Weapon::Rifle => 11.0,
            Weapon::Pistol => 7.0,
            Weapon::Club => 1.5,
        }
    }

    pub fn damage(self) -> (f32, f32) {
        match self {
            Weapon::Rifle => (9.0, 14.0),
            Weapon::Pistol => (7.0, 11.0),
            Weapon::Club => (8.0, 13.0),
        }
    }

    pub fn cooldown(self) -> f32 {
        match self {
            Weapon::Rifle => 1.8,
            Weapon::Pistol => 1.3,
            Weapon::Club => 1.2,
        }
    }

    pub fn ranged(self) -> bool {
        self != Weapon::Club
    }

    pub fn name(self) -> &'static str {
        match self {
            Weapon::Rifle => "rifle",
            Weapon::Pistol => "pistol",
            Weapon::Club => "club",
        }
    }
}

/// What a pawn is currently doing. Multi-step jobs track their stage in fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Job {
    Wander { to: Pos },
    Goto { to: Pos },
    Work { target: Pos, task: Task },
    /// Carry a stack from `from` to a stockpile cell or a blueprint.
    Haul { from: Pos, to: Pos, to_blueprint: bool, picked: bool },
    Eat { from: Pos, picked: bool },
    Sleep { bed: Option<Pos> },
    Attack { target: u32 },
    Leave { to: Pos },
}

impl Job {
    pub fn describe(&self) -> &'static str {
        match self {
            Job::Wander { .. } => "wandering",
            Job::Goto { .. } => "moving",
            Job::Work { task, .. } => task.verb(),
            Job::Haul { to_blueprint: true, .. } => "delivering materials",
            Job::Haul { .. } => "hauling",
            Job::Eat { .. } => "eating",
            Job::Sleep { .. } => "sleeping",
            Job::Attack { .. } => "fighting",
            Job::Leave { .. } => "leaving",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Memory {
    SleptInBedroom,
    SleptOutside,
    SleptOnGround,
    ColonistDied,
    Catharsis,
}

impl Memory {
    pub fn label(self) -> &'static str {
        match self {
            Memory::SleptInBedroom => "Slept in a bedroom",
            Memory::SleptOutside => "Slept outside",
            Memory::SleptOnGround => "Slept on the ground",
            Memory::ColonistDied => "A colonist died",
            Memory::Catharsis => "Catharsis",
        }
    }

    pub fn mood(self) -> f32 {
        match self {
            Memory::SleptInBedroom => 6.0,
            Memory::SleptOutside => -4.0,
            Memory::SleptOnGround => -5.0,
            Memory::ColonistDied => -10.0,
            Memory::Catharsis => 10.0,
        }
    }

    pub fn duration(self) -> f32 {
        match self {
            Memory::ColonistDied => 3.0 * DAY_LEN,
            Memory::Catharsis => 1.5 * DAY_LEN,
            _ => DAY_LEN,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub kind: Memory,
    pub left: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Pawn {
    pub id: u32,
    pub name: String,
    pub faction: Faction,
    /// Last tile fully reached.
    pub pos: Pos,
    /// Smooth position in tile units, for drawing and ranges.
    pub x: f32,
    pub y: f32,
    /// Remaining path, reversed (next step is last).
    pub path: Vec<Pos>,
    pub path_goal: Option<Pos>,
    pub job: Option<Job>,
    pub job_progress: f32,
    pub carrying: Option<Stack>,
    pub hunger: f32,
    pub rest: f32,
    pub mood: f32,
    pub health: f32,
    pub skills: [u8; 6],
    pub xp: [f32; 6],
    /// Per work type: 0 = disabled, 1 = highest .. 4 = lowest.
    pub priorities: [u8; 5],
    pub weapon: Weapon,
    pub cooldown: f32,
    pub drafted: bool,
    pub downed: bool,
    pub dead: bool,
    /// Raider left the map; removed at the end of the tick.
    pub left: bool,
    pub sleeping: bool,
    pub memories: Vec<MemoryEntry>,
    pub bed: Option<Pos>,
    pub color: [f32; 3],
    pub think_timer: f32,
    pub break_timer: f32,
    pub leave_timer: f32,
}

impl Pawn {
    fn base(id: u32, name: String, faction: Faction, pos: Pos, rng: &mut Rng) -> Pawn {
        Pawn {
            id,
            name,
            faction,
            pos,
            x: pos.0 as f32,
            y: pos.1 as f32,
            path: Vec::new(),
            path_goal: None,
            job: None,
            job_progress: 0.0,
            carrying: None,
            hunger: rng.range_f32(75.0, 100.0),
            rest: rng.range_f32(75.0, 100.0),
            mood: 55.0,
            health: 100.0,
            skills: [0; 6],
            xp: [0.0; 6],
            priorities: [3; 5],
            weapon: Weapon::Rifle,
            cooldown: 0.0,
            drafted: false,
            downed: false,
            dead: false,
            left: false,
            sleeping: false,
            memories: Vec::new(),
            bed: None,
            color: [rng.range_f32(0.5, 1.0), rng.range_f32(0.45, 0.95), rng.range_f32(0.45, 0.95)],
            think_timer: rng.f32(),
            break_timer: 0.0,
            leave_timer: 0.0,
        }
    }

    pub fn new_colonist(id: u32, name: String, pos: Pos, rng: &mut Rng) -> Pawn {
        let mut p = Pawn::base(id, name, Faction::Colony, pos, rng);
        for s in p.skills.iter_mut() {
            *s = rng.below(7) as u8;
        }
        // One thing they're genuinely good at.
        let passion = rng.below(6);
        p.skills[passion] = (p.skills[passion] + 5 + rng.below(6) as u8).min(20);
        p
    }

    pub fn new_raider(id: u32, name: String, pos: Pos, rng: &mut Rng) -> Pawn {
        let mut p = Pawn::base(id, name, Faction::Raider, pos, rng);
        p.weapon = if rng.chance(0.6) { Weapon::Pistol } else { Weapon::Club };
        p.skills[Skill::Shooting as usize] = 3 + rng.below(6) as u8;
        p.skills[Skill::Melee as usize] = 3 + rng.below(6) as u8;
        p.color = [0.85, 0.2, 0.2];
        p.priorities = [0; 5];
        p.leave_timer = 100.0;
        p
    }

    pub fn is_colonist(&self) -> bool {
        self.faction == Faction::Colony
    }

    /// Alive, conscious and still on the map.
    pub fn active(&self) -> bool {
        !self.dead && !self.downed && !self.left
    }

    pub fn skill(&self, s: Skill) -> u8 {
        self.skills[s as usize]
    }

    pub fn work_speed(&self, s: Skill) -> f32 {
        let mut speed = 0.6 + 0.07 * self.skill(s) as f32;
        if self.health < 50.0 {
            speed *= 0.7;
        }
        if self.mood > 70.0 {
            speed *= 1.1;
        } else if self.mood < 25.0 {
            speed *= 0.85;
        }
        speed
    }

    pub fn move_speed(&self) -> f32 {
        let base = if self.is_colonist() { 4.0 } else { 3.6 };
        if self.health < 50.0 {
            base * 0.6
        } else {
            base
        }
    }

    /// Add experience; returns true if the skill levelled up.
    pub fn learn(&mut self, s: Skill, amount: f32) -> bool {
        let i = s as usize;
        if self.skills[i] >= 20 {
            return false;
        }
        self.xp[i] += amount;
        let need = 20.0 + 8.0 * self.skills[i] as f32;
        if self.xp[i] >= need {
            self.xp[i] -= need;
            self.skills[i] += 1;
            return true;
        }
        false
    }

    pub fn add_memory(&mut self, kind: Memory) {
        match self.memories.iter_mut().find(|m| m.kind == kind) {
            Some(m) => m.left = kind.duration(),
            None => self.memories.push(MemoryEntry { kind, left: kind.duration() }),
        }
    }

    /// Everything currently affecting mood, with its effect.
    pub fn thoughts(&self) -> Vec<(&'static str, f32)> {
        let mut t = Vec::new();
        if self.hunger <= 0.0 {
            t.push(("Starving", -20.0));
        } else if self.hunger < 25.0 {
            t.push(("Hungry", -6.0));
        }
        if self.rest < 5.0 {
            t.push(("Exhausted", -12.0));
        } else if self.rest < 25.0 {
            t.push(("Tired", -5.0));
        }
        if self.health < 60.0 {
            t.push(("In pain", -8.0));
        }
        for m in &self.memories {
            t.push((m.kind.label(), m.kind.mood()));
        }
        t
    }

    pub fn mood_target(&self) -> f32 {
        let sum: f32 = self.thoughts().iter().map(|(_, v)| v).sum();
        (50.0 + sum).clamp(0.0, 100.0)
    }

    pub fn status(&self) -> String {
        if self.dead {
            return "dead".into();
        }
        if self.downed {
            return "downed".into();
        }
        if self.break_timer > 0.0 {
            return "mental break".into();
        }
        let job = self.job.map_or("idle", |j| j.describe());
        if self.drafted {
            format!("drafted, {}", job)
        } else {
            job.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memories_refresh_instead_of_stacking() {
        let mut rng = Rng::new(1);
        let mut p = Pawn::new_colonist(1, "Test".into(), (0, 0), &mut rng);
        p.add_memory(Memory::SleptOutside);
        p.memories[0].left = 1.0;
        p.add_memory(Memory::SleptOutside);
        assert_eq!(p.memories.len(), 1);
        assert_eq!(p.memories[0].left, Memory::SleptOutside.duration());
    }

    #[test]
    fn starving_tanks_mood() {
        let mut rng = Rng::new(1);
        let mut p = Pawn::new_colonist(1, "Test".into(), (0, 0), &mut rng);
        p.hunger = 100.0;
        p.rest = 100.0;
        let happy = p.mood_target();
        p.hunger = 0.0;
        assert!(p.mood_target() < happy - 15.0);
    }
}
