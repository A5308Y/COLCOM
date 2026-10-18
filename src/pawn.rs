//! Pawns (colonists and raiders): needs, skills, mood, weapons and jobs.

use crate::defs::{defs, ItemId, PlantId, Slot};
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
    Cooking,
    Crafting,
}

impl Skill {
    pub const ALL: [Skill; 9] = [
        Skill::Construction,
        Skill::Growing,
        Skill::Mining,
        Skill::Plants,
        Skill::Shooting,
        Skill::Melee,
        Skill::Medicine,
        Skill::Cooking,
        Skill::Crafting,
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
            Skill::Cooking => "Cooking",
            Skill::Crafting => "Crafting",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum WorkType {
    Doctor,
    Cooking,
    Construction,
    Growing,
    Mining,
    PlantCutting,
    Crafting,
    Hauling,
}

impl WorkType {
    /// In the order of the enum (priorities are indexed by it).
    pub const ALL: [WorkType; 8] = [
        WorkType::Doctor,
        WorkType::Cooking,
        WorkType::Construction,
        WorkType::Growing,
        WorkType::Mining,
        WorkType::PlantCutting,
        WorkType::Crafting,
        WorkType::Hauling,
    ];

    pub fn name(self) -> &'static str {
        match self {
            WorkType::Doctor => "Doctor",
            WorkType::Cooking => "Cook",
            WorkType::Construction => "Construct",
            WorkType::Growing => "Grow",
            WorkType::Mining => "Mine",
            WorkType::PlantCutting => "Cut plants",
            WorkType::Crafting => "Craft",
            WorkType::Hauling => "Haul",
        }
    }

    pub fn skill(self) -> Option<Skill> {
        match self {
            WorkType::Doctor => Some(Skill::Medicine),
            WorkType::Cooking => Some(Skill::Cooking),
            WorkType::Construction => Some(Skill::Construction),
            WorkType::Growing => Some(Skill::Growing),
            WorkType::Mining => Some(Skill::Mining),
            WorkType::PlantCutting => Some(Skill::Plants),
            WorkType::Crafting => Some(Skill::Crafting),
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
    /// Fill an open cell with soil.
    Fill,
    Construct,
    /// Take down a structure or floor.
    Deconstruct,
    /// Make the current bill's recipe at a workbench.
    Craft,
    /// Pack up furniture that is being moved (see `moving.rs`).
    Uninstall,
    /// Put up (or take down) the next thing for the wall here (see `decor.rs`).
    Decorate,
}

impl Task {
    pub fn verb(self) -> &'static str {
        match self {
            Task::Harvest(kind) => &kind.def().harvest.verb,
            Task::Sow(_) => "planting",
            Task::Mine => "mining",
            Task::Fill => "filling in",
            Task::Construct => "building",
            Task::Deconstruct => "deconstructing",
            Task::Craft => "crafting",
            Task::Uninstall => "packing up",
            Task::Decorate => "hanging up",
        }
    }
}

/// How a pawn fights: its weapon's numbers, or bare fists.
#[derive(Clone, Copy, Debug)]
pub struct Attack {
    pub range: f32,
    pub damage: (f32, f32),
    pub kind: DamageKind,
    pub cooldown: f32,
    pub ranged: bool,
    /// Heard when attacking.
    pub sound: &'static str,
}

const FISTS: Attack =
    Attack { range: 1.5, damage: (3.0, 6.0), kind: DamageKind::Bruise, cooldown: 1.0, ranged: false, sound: "swing" };

/// What a pawn is currently doing. Multi-step jobs track their stage in fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Job {
    Wander { to: Pos },
    Goto { to: Pos },
    Work { target: Pos, task: Task },
    /// Carry a stack from `from` to a stockpile cell, or (`deliver`) to a
    /// blueprint or workbench that needs it.
    Haul { from: Pos, to: Pos, deliver: bool, picked: bool },
    /// Fetch food, then eat it, sitting at a table if there's a `seat`.
    Eat { from: Pos, picked: bool, seat: Option<Pos> },
    Sleep { bed: Option<Pos> },
    /// Lie in bed (or on the ground) to be treated and recover.
    Patient { bed: Option<Pos> },
    /// Fetch medicine (if any) and treat a patient (possibly oneself).
    Tend { patient: u32, medicine: Option<Pos>, picked: bool },
    /// Carry a downed colonist to a bed.
    Rescue { patient: u32, bed: Pos, carrying: bool },
    /// Bring food to a downed colonist.
    Feed { patient: u32, food: Pos, picked: bool },
    /// Pick up a better weapon or piece of apparel and use it; `chosen` by
    /// the player rather than by the pawn.
    Equip {
        from: Pos,
        #[serde(default)]
        chosen: bool,
    },
    /// Carry a dead colonist to an empty grave and bury them.
    Bury { corpse: u32, grave: Pos, carrying: bool },
    /// Lie on an operating table and wait to be fitted with a prosthetic.
    Operation { table: Pos },
    /// Fetch a prosthetic and fit it to a patient on an operating table.
    Surgery { patient: u32, prosthetic: Pos, picked: bool },
}

impl Job {
    pub fn describe(&self) -> &'static str {
        match self {
            Job::Wander { .. } => "wandering",
            Job::Goto { .. } => "moving",
            Job::Work { task, .. } => task.verb(),
            Job::Haul { deliver: true, .. } => "delivering materials",
            Job::Haul { .. } => "hauling",
            Job::Eat { .. } => "eating",
            Job::Sleep { .. } => "sleeping",
            Job::Patient { .. } => "resting in bed",
            Job::Tend { .. } => "tending",
            Job::Rescue { .. } => "rescuing",
            Job::Feed { .. } => "feeding a patient",
            Job::Equip { .. } => "equipping",
            Job::Bury { .. } => "burying the dead",
            Job::Operation { .. } => "waiting for an operation",
            Job::Surgery { .. } => "operating",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Memory {
    /// Slept in a bedroom of this quality tier (see `rooms::TIERS`).
    Bedroom(u8),
    SleptOutside,
    SleptOnGround,
    ColonistDied,
    Catharsis,
    AteRawFood,
    AteMeal,
    /// Ate at a table in a room of this quality tier.
    DiningRoom(u8),
    AteWithoutTable,
    WonBattle,
    LostBattle,
    /// Slept in a themed room (index into `defs().rooms`) that has a thought.
    RoomThought(u8),
    SleptOnSpot,
}

impl Memory {
    pub fn label(self) -> &'static str {
        match self {
            Memory::Bedroom(tier) => [
                "Slept in an awful bedroom",
                "Slept in a dull bedroom",
                "Slept in a decent bedroom",
                "Slept in a nice bedroom",
                "Slept in an impressive bedroom",
            ][tier.min(4) as usize],
            Memory::SleptOutside => "Slept outside",
            Memory::SleptOnGround => "Slept on the ground",
            Memory::ColonistDied => "A colonist died",
            Memory::Catharsis => "Catharsis",
            Memory::AteRawFood => "Ate raw food",
            Memory::AteMeal => "Ate a cooked meal",
            Memory::DiningRoom(tier) => [
                "Ate in an awful room",
                "Ate in a dull room",
                "Ate in a decent dining room",
                "Ate in a nice dining room",
                "Ate in an impressive dining room",
            ][tier.min(4) as usize],
            Memory::AteWithoutTable => "Ate without a table",
            Memory::WonBattle => "Won a battle",
            Memory::LostBattle => "Lost a battle",
            Memory::RoomThought(k) => room_thought(k).map_or("Slept in a themed room", |t| t.0.as_str()),
            Memory::SleptOnSpot => "Slept on a sleeping spot",
        }
    }

    pub fn mood(self) -> f32 {
        match self {
            Memory::Bedroom(tier) => [-2.0, 2.0, 4.0, 6.0, 9.0][tier.min(4) as usize],
            Memory::SleptOutside => -4.0,
            Memory::SleptOnGround => -5.0,
            Memory::ColonistDied => -10.0,
            Memory::Catharsis => 10.0,
            Memory::AteRawFood => -5.0,
            Memory::AteMeal => 2.0,
            Memory::DiningRoom(tier) => [-2.0, 0.0, 2.0, 4.0, 6.0][tier.min(4) as usize],
            Memory::AteWithoutTable => -3.0,
            Memory::WonBattle => 6.0,
            Memory::LostBattle => -10.0,
            Memory::RoomThought(k) => room_thought(k).map_or(0.0, |t| t.1),
            Memory::SleptOnSpot => -2.0,
        }
    }

    pub fn duration(self) -> f32 {
        match self {
            Memory::ColonistDied => 3.0 * DAY_LEN,
            Memory::Catharsis => 1.5 * DAY_LEN,
            Memory::AteRawFood | Memory::AteMeal | Memory::DiningRoom(_) | Memory::AteWithoutTable => 0.5 * DAY_LEN,
            _ => DAY_LEN,
        }
    }

    /// Memories that replace each other: a new bedroom replaces the last one.
    fn same_kind(self, other: Memory) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

fn room_thought(k: u8) -> Option<&'static (String, f32)> {
    defs().rooms.get(k as usize)?.effects.thought.as_ref()
}

/// Walking pace in cells per second (a cell is about 1.8 m: a brisk walk).
pub const WALK_SPEED: f32 = 1.0;
/// Running pace in battles.
pub const RUN_SPEED: f32 = 4.5;

/// What a colonist does when a battle starts (see `squads.rs`, `battle.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Role {
    /// Fights as a unit of their own.
    #[default]
    Fighter,
    /// Shelters in the safe zone.
    Civilian,
    /// Leads a squad: the colonists assigned to them fight as one unit with them.
    Leader,
    /// In the squad of the leader with this pawn id.
    Member(u32),
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Fighter => "Fighter",
            Role::Civilian => "Civilian",
            Role::Leader => "Squad leader",
            Role::Member(_) => "Squad member",
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
    pub skills: [u8; Skill::ALL.len()],
    pub xp: [f32; Skill::ALL.len()],
    /// Per work type: 0 = disabled, 1 = highest .. 4 = lowest.
    pub priorities: [u8; WorkType::ALL.len()],
    pub weapon: Option<ItemId>,
    /// Worn items, at most one per slot.
    pub apparel: Vec<ItemId>,
    /// Weapon and apparel the player told them to use: kept even when
    /// something better turns up.
    #[serde(default)]
    pub chosen: Vec<ItemId>,
    pub cooldown: f32,
    pub downed: bool,
    pub downed_time: f32,
    pub dead: bool,
    /// Raider left the map; removed at the end of the tick.
    pub left: bool,
    pub sleeping: bool,
    /// Lying down awake (as a patient).
    pub resting: bool,
    pub memories: Vec<MemoryEntry>,
    /// A cell the player told them to work on (see `prioritize.rs`).
    #[serde(default)]
    pub forced: Option<Pos>,
    /// Operations the player ordered for them (see `surgery.rs`).
    #[serde(default)]
    pub operations: Vec<crate::surgery::Operation>,
    pub bed: Option<Pos>,
    pub color: [f32; 3],
    pub think_timer: f32,
    pub break_timer: f32,
    #[serde(default)]
    pub role: Role,
    /// Weapon and apparel to fight in (at most one weapon, one per slot).
    #[serde(default)]
    pub loadout: Vec<ItemId>,
    /// Battle gear taken from storage, each with what it replaced (put
    /// back on after the battle).
    #[serde(default)]
    pub stash: Vec<(ItemId, Option<ItemId>)>,
    /// Runs instead of walking (in battles).
    #[serde(default)]
    pub running: bool,
    /// A leader without a decent bedroom of their own (updated once a second).
    #[serde(skip)]
    pub poor_quarters: bool,
    /// Dead and buried in the grave at `pos` (see `graves.rs`).
    #[serde(default)]
    pub buried: bool,
    /// A dead colonist lies unburied somewhere (updated once a second).
    #[serde(skip)]
    pub unburied_dead: bool,
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
            skills: [0; Skill::ALL.len()],
            xp: [0.0; Skill::ALL.len()],
            priorities: [3; WorkType::ALL.len()],
            weapon: defs().start_weapons.first().map(|w| w.id()),
            apparel: Vec::new(),
            chosen: Vec::new(),
            cooldown: 0.0,
            downed: false,
            downed_time: 0.0,
            dead: false,
            left: false,
            sleeping: false,
            resting: false,
            memories: Vec::new(),
            operations: Vec::new(),
            forced: None,
            bed: None,
            color: [rng.range_f32(0.5, 1.0), rng.range_f32(0.45, 0.95), rng.range_f32(0.45, 0.95)],
            think_timer: rng.f32(),
            break_timer: 0.0,
            role: Role::Fighter,
            loadout: Vec::new(),
            stash: Vec::new(),
            running: false,
            poor_quarters: false,
            buried: false,
            unburied_dead: false,
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

    /// A raider's weapon by the chances in `raider_weapons`, of one kind if
    /// `ranged` says so (to arm a squad alike).
    pub fn raider_weapon(rng: &mut Rng, ranged: Option<bool>) -> Option<ItemId> {
        raider_weapon(rng, ranged)
    }

    pub fn new_raider(id: u32, name: String, pos: Pos, rng: &mut Rng) -> Pawn {
        let mut p = Pawn::base(id, name, Faction::Raider, pos, rng);
        p.weapon = raider_weapon(rng, None);
        p.skills[Skill::Shooting as usize] = 3 + rng.below(6) as u8;
        p.skills[Skill::Melee as usize] = 3 + rng.below(6) as u8;
        p.color = [0.85, 0.2, 0.2];
        p.priorities = [0; WorkType::ALL.len()];
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
        // Quicker hands make up a little for walking between jobs.
        let mut speed =
            1.25 * (0.6 + 0.07 * self.skill(s) as f32) * caps.manipulation.min(1.0) * (0.7 + 0.3 * caps.sight.min(1.0));
        if self.mood > 70.0 {
            speed *= 1.1;
        } else if self.mood < 25.0 {
            speed *= 0.85;
        }
        speed.max(0.05)
    }

    /// The weapon's numbers, or fists.
    pub fn attack(&self) -> Attack {
        match self.weapon.and_then(|w| w.def().weapon.as_ref()) {
            Some(w) => Attack {
                range: w.range,
                damage: w.damage,
                kind: w.kind,
                cooldown: w.cooldown,
                ranged: w.ranged(),
                sound: w.sound.as_deref().unwrap_or("swing"),
            },
            None => FISTS,
        }
    }

    pub fn weapon_name(&self) -> &'static str {
        self.weapon.map_or("fists", |w| w.name())
    }

    /// Share of damage stopped by what they wear.
    pub fn armor(&self) -> f32 {
        let total: f32 = self.apparel.iter().filter_map(|a| a.def().apparel.as_ref()).map(|a| a.armor).sum();
        total.min(0.8)
    }

    /// What they wear in a slot.
    pub fn worn(&self, slot: Slot) -> Option<ItemId> {
        self.apparel.iter().copied().find(|a| a.def().apparel.as_ref().is_some_and(|d| d.slot == slot))
    }

    /// Cells per second: a brisk walk around the colony, running in battle.
    pub fn move_speed(&self) -> f32 {
        let base = if self.running { RUN_SPEED } else { WALK_SPEED };
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
        match self.memories.iter_mut().find(|m| m.kind.same_kind(kind)) {
            Some(m) => {
                m.kind = kind;
                m.left = kind.duration();
            }
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
        for m in self.memories.iter().filter(|m| m.kind.mood() != 0.0) {
            t.push((m.kind.label(), m.kind.mood()));
        }
        for a in &self.apparel {
            if let Some(d) = a.def().apparel.as_ref().filter(|d| d.mood != 0.0) {
                t.push((a.name(), d.mood));
            }
        }
        if self.role == Role::Leader {
            t.push(("Responsible for a squad", -2.0));
            if self.poor_quarters {
                t.push(("No proper quarters for a leader", -8.0));
            }
        }
        if self.unburied_dead && !self.dead {
            t.push(("A colonist lies unburied", -5.0));
        }
        t
    }

    pub fn mood_target(&self) -> f32 {
        let sum: f32 = self.thoughts().iter().map(|(_, v)| v).sum();
        (50.0 + sum).clamp(0.0, 100.0)
    }

    /// A colonist who hasn't died (they may still be downed or away).
    pub fn is_living_colonist(&self) -> bool {
        self.is_colonist() && !self.dead
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
        let doing = self.job.map_or("idle", |j| j.describe());
        if self.forced.is_some() && self.job.is_some() {
            format!("{} (prioritized)", doing)
        } else {
            doing.into()
        }
    }

    /// Hold a weapon or put on apparel; returns what it replaced.
    pub fn wear(&mut self, kind: ItemId) -> Option<ItemId> {
        let d = kind.def();
        if d.weapon.is_some() {
            return self.weapon.replace(kind);
        }
        let slot = d.apparel.as_ref().map_or(Slot::Body, |a| a.slot);
        let old = self.worn(slot);
        self.apparel.retain(|&a| Some(a) != old);
        self.apparel.push(kind);
        old
    }

    /// Put down a weapon or take off apparel; false if they don't have it.
    pub fn take_off(&mut self, kind: ItemId) -> bool {
        if self.weapon == Some(kind) {
            self.weapon = None;
            return true;
        }
        let before = self.apparel.len();
        self.apparel.retain(|&a| a != kind);
        self.apparel.len() != before
    }
}


/// See `Pawn::raider_weapon`.
fn raider_weapon(rng: &mut Rng, ranged: Option<bool>) -> Option<ItemId> {
    let fits = |k: ItemId| ranged.is_none_or(|r| k.def().weapon.as_ref().is_some_and(|w| w.ranged() == r));
    let weapons: Vec<(ItemId, f32)> = defs().raider_weapons.iter().map(|(w, weight)| (w.id(), *weight)).filter(|w| fits(w.0)).collect();
    let total: f32 = weapons.iter().map(|w| w.1).sum();
    let mut roll = rng.f32() * total;
    for &(w, weight) in &weapons {
        if roll < weight {
            return Some(w);
        }
        roll -= weight;
    }
    weapons.last().map(|w| w.0)
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
