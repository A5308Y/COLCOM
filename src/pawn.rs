//! Pawns (colonists and raiders): needs, skills, mood, weapons and jobs.

use crate::defs::PlantId;
use crate::health::{DamageKind, Health};
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
    Medicine,
}

impl Skill {
    pub const ALL: [Skill; 7] = [
        Skill::Construction,
        Skill::Growing,
        Skill::Mining,
        Skill::Plants,
        Skill::Shooting,
        Skill::Melee,
        Skill::Medicine,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Skill::Construction => "Construction",
            Skill::Growing => "Growing",
            Skill::Mining => "Mining",
            Skill::Plants => "Plants",
            Skill::Shooting => "Shooting",
            Skill::Melee => "Melee",
            Skill::Medicine => "Medicine",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WorkType {
    Doctor,
    Construction,
    Growing,
    Mining,
    PlantCutting,
    Hauling,
}

impl WorkType {
    pub const ALL: [WorkType; 6] = [
        WorkType::Doctor,
        WorkType::Construction,
        WorkType::Growing,
        WorkType::Mining,
        WorkType::PlantCutting,
        WorkType::Hauling,
    ];

    pub fn name(self) -> &'static str {
        match self {
            WorkType::Doctor => "Doctor",
            WorkType::Construction => "Construct",
            WorkType::Growing => "Grow",
            WorkType::Mining => "Mine",
            WorkType::PlantCutting => "Cut plants",
            WorkType::Hauling => "Haul",
        }
    }

    pub fn skill(self) -> Option<Skill> {
        match self {
            WorkType::Doctor => Some(Skill::Medicine),
            WorkType::Construction => Some(Skill::Construction),
            WorkType::Growing => Some(Skill::Growing),
            WorkType::Mining => Some(Skill::Mining),
            WorkType::PlantCutting => Some(Skill::Plants),
            WorkType::Hauling => None,
        }
    }
}

/// Work done at a single tile. How long it takes and which skill it uses
/// come from the plant or blueprint there (see `World::task_skill_time`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Task {
    /// Remove a grown plant for its yield (chop, forage, harvest).
    Harvest(PlantId),
    /// Plant in a grow zone.
    Sow(PlantId),
    Mine,
    Construct,
}

impl Task {
    pub fn verb(self) -> &'static str {
        match self {
            Task::Harvest(kind) => &kind.def().harvest.verb,
            Task::Sow(_) => "planting",
            Task::Mine => "mining",
            Task::Construct => "building",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Weapon {
    Rifle,
    Pistol,
    Club,
    Knife,
}

impl Weapon {
    pub fn range(self) -> f32 {
        match self {
            Weapon::Rifle => 11.0,
            Weapon::Pistol => 7.0,
            Weapon::Club | Weapon::Knife => 1.5,
        }
    }

    pub fn damage(self) -> (f32, f32) {
        match self {
            Weapon::Rifle => (9.0, 14.0),
            Weapon::Pistol => (7.0, 11.0),
            Weapon::Club => (8.0, 13.0),
            Weapon::Knife => (6.0, 11.0),
        }
    }

    pub fn damage_kind(self) -> DamageKind {
        match self {
            Weapon::Rifle | Weapon::Pistol => DamageKind::Gunshot,
            Weapon::Club => DamageKind::Bruise,
            Weapon::Knife => DamageKind::Cut,
        }
    }

    pub fn cooldown(self) -> f32 {
        match self {
            Weapon::Rifle => 1.8,
            Weapon::Pistol => 1.3,
            Weapon::Club => 1.2,
            Weapon::Knife => 1.0,
        }
    }

    pub fn ranged(self) -> bool {
        matches!(self, Weapon::Rifle | Weapon::Pistol)
    }

    pub fn name(self) -> &'static str {
        match self {
            Weapon::Rifle => "rifle",
            Weapon::Pistol => "pistol",
            Weapon::Club => "club",
            Weapon::Knife => "knife",
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
    /// Lie in bed (or on the ground) to be treated and recover.
    Patient { bed: Option<Pos> },
    /// Fetch medicine (if any) and treat a patient (possibly oneself).
    Tend { patient: u32, medicine: Option<Pos>, picked: bool },
    /// Carry a downed colonist to a bed.
    Rescue { patient: u32, bed: Pos, carrying: bool },
    /// Bring food to a downed colonist.
    Feed { patient: u32, food: Pos, picked: bool },
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
            Job::Patient { .. } => "resting in bed",
            Job::Tend { .. } => "tending",
            Job::Rescue { .. } => "rescuing",
            Job::Feed { .. } => "feeding a patient",
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
    /// Smooth position in cell units (z = level), for drawing and ranges.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Remaining path, reversed (next step is last).
    pub path: Vec<Pos>,
    pub path_goal: Option<Pos>,
    pub job: Option<Job>,
    pub job_progress: f32,
    pub carrying: Option<Stack>,
    pub hunger: f32,
    pub rest: f32,
    pub mood: f32,
    pub health: Health,
    pub skills: [u8; 7],
    pub xp: [f32; 7],
    /// Per work type: 0 = disabled, 1 = highest .. 4 = lowest.
    pub priorities: [u8; 6],
    pub weapon: Weapon,
    pub cooldown: f32,
    pub drafted: bool,
    pub downed: bool,
    pub downed_time: f32,
    pub dead: bool,
    /// Raider left the map; removed at the end of the tick.
    pub left: bool,
    pub sleeping: bool,
    /// Lying down awake (as a patient).
    pub resting: bool,
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
            z: pos.2 as f32,
            path: Vec::new(),
            path_goal: None,
            job: None,
            job_progress: 0.0,
            carrying: None,
            hunger: rng.range_f32(75.0, 100.0),
            rest: rng.range_f32(75.0, 100.0),
            mood: 55.0,
            health: Health::new(),
            skills: [0; 7],
            xp: [0.0; 7],
            priorities: [3; 6],
            weapon: Weapon::Rifle,
            cooldown: 0.0,
            drafted: false,
            downed: false,
            downed_time: 0.0,
            dead: false,
            left: false,
            sleeping: false,
            resting: false,
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
        let passion = rng.below(Skill::ALL.len());
        p.skills[passion] = (p.skills[passion] + 5 + rng.below(6) as u8).min(20);
        p
    }

    pub fn new_raider(id: u32, name: String, pos: Pos, rng: &mut Rng) -> Pawn {
        let mut p = Pawn::base(id, name, Faction::Raider, pos, rng);
        let roll = rng.f32();
        p.weapon = if roll < 0.5 {
            Weapon::Pistol
        } else if roll < 0.75 {
            Weapon::Club
        } else {
            Weapon::Knife
        };
        p.skills[Skill::Shooting as usize] = 3 + rng.below(6) as u8;
        p.skills[Skill::Melee as usize] = 3 + rng.below(6) as u8;
        p.color = [0.85, 0.2, 0.2];
        p.priorities = [0; 6];
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

    /// Lying down where a doctor can work on them.
    pub fn lying(&self) -> bool {
        self.downed || self.sleeping || self.resting
    }

    pub fn skill(&self, s: Skill) -> u8 {
        self.skills[s as usize]
    }

    pub fn work_speed(&self, s: Skill) -> f32 {
        let caps = &self.health.caps;
        let mut speed = (0.6 + 0.07 * self.skill(s) as f32) * caps.manipulation.min(1.0) * (0.7 + 0.3 * caps.sight.min(1.0));
        if self.mood > 70.0 {
            speed *= 1.1;
        } else if self.mood < 25.0 {
            speed *= 0.85;
        }
        speed.max(0.05)
    }

    pub fn move_speed(&self) -> f32 {
        let base = if self.is_colonist() { 4.0 } else { 3.6 };
        base * self.health.caps.moving.clamp(0.15, 1.0)
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
        let pain = self.health.pain;
        if pain >= 0.6 {
            t.push(("Mind-shattering pain", -20.0));
        } else if pain >= 0.4 {
            t.push(("Intense pain", -14.0));
        } else if pain >= 0.2 {
            t.push(("Serious pain", -8.0));
        } else if pain >= 0.05 {
            t.push(("Minor pain", -3.0));
        }
        if self.health.hediffs.iter().any(|h| h.kind == crate::health::HediffKind::Missing) {
            t.push(("Missing a body part", -5.0));
        }
        if self.health.has_disease() {
            t.push(("Sick", -5.0));
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
            return match self.health.downed_reason() {
                Some(r) => format!("downed ({})", r),
                None => "downed".into(),
            };
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
        let mut p = Pawn::new_colonist(1, "Test".into(), (0, 0, 1), &mut rng);
        p.add_memory(Memory::SleptOutside);
        p.memories[0].left = 1.0;
        p.add_memory(Memory::SleptOutside);
        assert_eq!(p.memories.len(), 1);
        assert_eq!(p.memories[0].left, Memory::SleptOutside.duration());
    }

    #[test]
    fn starving_tanks_mood() {
        let mut rng = Rng::new(1);
        let mut p = Pawn::new_colonist(1, "Test".into(), (0, 0, 1), &mut rng);
        p.hunger = 100.0;
        p.rest = 100.0;
        let happy = p.mood_target();
        p.hunger = 0.0;
        assert!(p.mood_target() < happy - 15.0);
    }

    #[test]
    fn injuries_slow_work_and_walking() {
        let mut rng = Rng::new(1);
        let mut p = Pawn::new_colonist(1, "Test".into(), (0, 0, 1), &mut rng);
        let (work, walk) = (p.work_speed(Skill::Mining), p.move_speed());
        p.health.injure(crate::health::Part::RightHand, 15.0, DamageKind::Cut, &mut rng);
        p.health.injure(crate::health::Part::LeftLeg, 20.0, DamageKind::Bruise, &mut rng);
        p.health.recompute();
        assert!(p.work_speed(Skill::Mining) < work);
        assert!(p.move_speed() < walk);
    }
}
