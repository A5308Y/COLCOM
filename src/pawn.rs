//! Pawns (colonists and raiders): needs, skills, mood, weapons and jobs.

use crate::defs::{defs, ItemId, PlantId, RoomId, Slot};
use crate::health::{DamageKind, Health, Part};
use crate::map::{Pos, Stack};
use crate::rng::Rng;
use crate::DAY_LEN;
use crate::per_key::{Key, PerKey};
use serde::{Deserialize, Serialize};

/// A value per skill, and per kind of work (saved by name, see `per_key`).
pub type PerSkill<V> = PerKey<Skill, V, { Skill::ALL.len() }>;
pub type PerWork<V> = PerKey<WorkType, V, { WorkType::ALL.len() }>;

/// Identifies a pawn for as long as it exists. Distinct from expedition and
/// offer ids, which come from the same counter but name different things.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PawnId(std::num::NonZeroU32);

impl PawnId {
    /// Ids count up from 1 and are handed out by `World::spawn_colonist` and
    /// `World::spawn_raider`; build one directly only in tests.
    pub fn new(raw: u32) -> PawnId {
        PawnId(std::num::NonZeroU32::new(raw).expect("pawn ids start at 1"))
    }

    /// The raw number, for deterministic per-pawn variety (looks, hashing).
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

/// When a colonist takes on a kind of work: first to fourth, or never. Not
/// ordered: job choice sorts by `rank`. Saved as 0 (never) or 1 to 4.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum WorkPriority {
    Never,
    First,
    Second,
    /// What new colonists start with (and work added since a save was made).
    #[default]
    Third,
    Fourth,
}

impl WorkPriority {
    /// 1 (first) to 4 (last), or `None` for never.
    pub fn rank(self) -> Option<u8> {
        match self {
            WorkPriority::Never => None,
            p => Some(u8::from(p)),
        }
    }

    pub fn is_enabled(self) -> bool {
        self != WorkPriority::Never
    }

    /// One step sooner: never → 4 → 3 → 2 → 1 (and 1 stays 1).
    pub fn raised(self) -> WorkPriority {
        match self {
            WorkPriority::Never => WorkPriority::Fourth,
            WorkPriority::Fourth => WorkPriority::Third,
            WorkPriority::Third => WorkPriority::Second,
            WorkPriority::Second | WorkPriority::First => WorkPriority::First,
        }
    }

    /// One step later: 1 → 2 → 3 → 4 → never (and never stays never).
    pub fn lowered(self) -> WorkPriority {
        match self {
            WorkPriority::First => WorkPriority::Second,
            WorkPriority::Second => WorkPriority::Third,
            WorkPriority::Third => WorkPriority::Fourth,
            WorkPriority::Fourth | WorkPriority::Never => WorkPriority::Never,
        }
    }
}

impl TryFrom<u8> for WorkPriority {
    type Error = String;
    fn try_from(n: u8) -> Result<WorkPriority, String> {
        match n {
            0 => Ok(WorkPriority::Never),
            1 => Ok(WorkPriority::First),
            2 => Ok(WorkPriority::Second),
            3 => Ok(WorkPriority::Third),
            4 => Ok(WorkPriority::Fourth),
            _ => Err(format!("work priority {n} is not 0 to 4")),
        }
    }
}

impl From<WorkPriority> for u8 {
    fn from(p: WorkPriority) -> u8 {
        p as u8
    }
}

/// Whether a pawn is up and about, and if not, why. Asleep or resting are
/// separate: those pawns are still up.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub enum Life {
    #[default]
    Up,
    /// Collapsed or knocked down; `time` counts the seconds spent down.
    Downed { time: f32 },
    /// `buried`: in the grave at the pawn's `pos` (see `graves.rs`).
    Dead { buried: bool },
    /// A raider who left the map; removed at the end of the tick.
    Left,
}

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

impl Key for Skill {
    const ALL: &'static [Skill] = &Skill::ALL;
    fn index(self) -> usize {
        self as usize
    }
}

impl Key for WorkType {
    const ALL: &'static [WorkType] = &WorkType::ALL;
    fn index(self) -> usize {
        self as usize
    }
}

impl Skill {
    /// In the order of the enum (`Key::index`).
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
    /// A rifle-sized gun (ranged, too big for a sidearm): clumsy against
    /// someone right next to them (`battle::CLOSE_QUARTERS`).
    pub long_gun: bool,
    /// Heard when attacking.
    pub sound: &'static str,
}

/// Things anyone carries on an expedition without a backpack.
pub const CARRY_EACH: u32 = 60;

const FISTS: Attack =
    Attack { range: 1.5, damage: (3.0, 6.0), kind: DamageKind::Bruise, cooldown: 1.0, ranged: false, long_gun: false, sound: "swing" };

/// The numbers of weapon `kind`, or fists.
pub fn attack_with(kind: Option<ItemId>) -> Attack {
    match kind.and_then(|w| w.def().weapon.as_ref()) {
        Some(w) => Attack {
            range: w.range,
            damage: w.damage,
            kind: w.kind,
            cooldown: w.cooldown,
            ranged: w.ranged(),
            long_gun: w.ranged() && !w.sidearm,
            sound: w.sound.as_deref().unwrap_or("swing"),
        },
        None => FISTS,
    }
}

/// What a pawn is currently doing. Multi-step jobs track their stage in fields.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
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
    Tend { patient: PawnId, medicine: Option<Pos>, picked: bool },
    /// Carry a downed colonist to a bed.
    Rescue { patient: PawnId, bed: Pos, carrying: bool },
    /// Bring food to a downed colonist.
    Feed { patient: PawnId, food: Pos, picked: bool },
    /// Pick up a weapon, piece of apparel or belt item and use it: their loadout's, or
    /// a better one for a slot it leaves free; `chosen` by the player (it goes
    /// into their loadout).
    Equip {
        from: Pos,
        #[serde(default)]
        chosen: bool,
        /// Chosen to carry as the sidearm (a light weapon).
        #[serde(default)]
        sidearm: bool,
    },
    /// Carry a dead colonist to an empty grave and bury them.
    Bury { corpse: PawnId, grave: Pos, carrying: bool },
    /// Lie on an operating table and wait for an operation (see `surgery.rs`).
    Operation { table: Pos },
    /// Operate on a patient on an operating table: fetch the cyberware at
    /// `prosthetic` and install it, or (`None`) take some out.
    Surgery { patient: PawnId, prosthetic: Option<Pos>, picked: bool },
    /// Have fun at something built for it at `at`, standing at `stand` (see `recreation.rs`).
    Relax { at: Pos, stand: Pos },
    /// Stroll outdoors for fun, on to `to`.
    Stroll { to: Pos },
    /// Sit on the chair at `at` for a while, until game time `until` (idle; see `recreation.rs`).
    Sit { at: Pos, until: f32 },
    /// Freezing: get warm at `to` (see `temperature.rs`).
    WarmUp { to: Pos },
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
            Job::Relax { .. } => "having fun",
            Job::Stroll { .. } => "strolling",
            Job::Sit { .. } => "sitting",
            Job::WarmUp { .. } => "warming up",
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
    /// Slept in a themed room that has a thought.
    RoomThought(RoomId),
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
            Memory::Bedroom(_) | Memory::SleptOutside | Memory::SleptOnGround | Memory::WonBattle
            | Memory::LostBattle | Memory::RoomThought(_) | Memory::SleptOnSpot => DAY_LEN,
        }
    }

    /// Memories that replace each other: a new bedroom replaces the last one.
    fn same_kind(self, other: Memory) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

fn room_thought(k: RoomId) -> Option<&'static (String, f32)> {
    k.def().effects.thought.as_ref()
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
    Member(PawnId),
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
    pub id: PawnId,
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
    pub skills: PerSkill<u8>,
    pub xp: PerSkill<f32>,
    pub priorities: PerWork<WorkPriority>,
    pub weapon: Option<ItemId>,
    /// A light second weapon, drawn in battle when it suits better.
    #[serde(default)]
    pub sidearm: Option<ItemId>,
    /// The sidearm is in hand rather than the main weapon (in battle).
    #[serde(default)]
    pub sidearm_drawn: bool,
    /// What they carry on the belt to use in battle (grenades, medkits, a shield).
    #[serde(default)]
    pub utility: Option<Stack>,
    /// Damage their shield has taken since it was last full (see `shield`).
    #[serde(default)]
    pub shield_drain: f32,
    /// Worn items, at most one per slot (`Slot`; one item can take several).
    pub apparel: Vec<ItemId>,
    /// Gear the player told them to keep, from before that was the loadout:
    /// moved into the loadout when loading (`World::load_json`), never saved.
    #[serde(default, skip_serializing)]
    pub chosen: Vec<ItemId>,
    pub cooldown: f32,
    pub life: Life,
    pub sleeping: bool,
    /// Lying down awake (as a patient).
    pub resting: bool,
    pub memories: Vec<MemoryEntry>,
    /// A cell the player told them to work on (see `prioritize.rs`).
    #[serde(default)]
    pub forced: Option<Pos>,
    /// Operations the player ordered for them (see `surgery.rs`). Saved
    /// under a new name: orders from before cyberware slots are dropped.
    #[serde(default)]
    pub surgeries: Vec<crate::surgery::Operation>,
    pub bed: Option<Pos>,
    pub color: [f32; 3],
    pub think_timer: f32,
    pub break_timer: f32,
    #[serde(default)]
    pub role: Role,
    /// The gear they use (at most one weapon, one utility item, one piece
    /// per apparel slot): they fetch it from storage and keep it on; apparel
    /// slots left free they fill with the best they find.
    #[serde(default)]
    pub loadout: Vec<ItemId>,
    /// The sidearm in their loadout (apart, as a pistol could also be the main weapon).
    #[serde(default)]
    pub loadout_sidearm: Option<ItemId>,
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
    /// A dead colonist lies unburied somewhere (updated once a second).
    #[serde(skip)]
    pub unburied_dead: bool,
    /// Their need for fun, 0..100 (see `recreation.rs`).
    #[serde(default = "content_joy")]
    pub joy: f32,
    /// What each hour of their day is for.
    #[serde(default = "crate::recreation::default_schedule")]
    pub schedule: [crate::recreation::Block; 24],
    /// Seconds left standing up from a chair (they don't walk off before; see `recreation::GET_UP`).
    #[serde(default)]
    pub getting_up: f32,
    /// The temperature they feel (°C, updated every tick; see `temperature.rs`).
    #[serde(skip, default = "mild")]
    pub felt: f32,
}

fn mild() -> f32 {
    20.0
}

fn content_joy() -> f32 {
    70.0
}

impl Pawn {
    fn base(id: PawnId, name: String, faction: Faction, pos: Pos, rng: &mut Rng) -> Pawn {
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
            skills: PerSkill::filled(0),
            xp: PerSkill::filled(0.0),
            priorities: PerWork::filled(WorkPriority::Third),
            weapon: defs().start_weapons.first().map(|w| w.id()),
            sidearm: None,
            sidearm_drawn: false,
            utility: None,
            shield_drain: 0.0,
            apparel: Vec::new(),
            chosen: Vec::new(),
            cooldown: 0.0,
            life: Life::Up,
            sleeping: false,
            resting: false,
            memories: Vec::new(),
            surgeries: Vec::new(),
            forced: None,
            bed: None,
            color: [rng.range_f32(0.5, 1.0), rng.range_f32(0.45, 0.95), rng.range_f32(0.45, 0.95)],
            think_timer: rng.f32(),
            break_timer: 0.0,
            role: Role::Fighter,
            loadout: Vec::new(),
            loadout_sidearm: None,
            stash: Vec::new(),
            running: false,
            poor_quarters: false,
            unburied_dead: false,
            felt: mild(),
            joy: content_joy(),
            schedule: crate::recreation::default_schedule(),
            getting_up: 0.0,
        }
    }

    pub fn new_colonist(id: PawnId, name: String, pos: Pos, rng: &mut Rng) -> Pawn {
        let mut p = Pawn::base(id, name, Faction::Colony, pos, rng);
        for s in p.skills.values_mut() {
            *s = rng.below(7) as u8;
        }
        // One thing they're genuinely good at.
        let passion = Skill::ALL[rng.below(Skill::ALL.len())];
        p.skills[passion] = (p.skills[passion] + 5 + rng.below(6) as u8).min(20);
        p
    }

    /// A raider's weapon by the chances of raider tier `tier`, of one kind
    /// if `ranged` says so (to arm a squad alike).
    pub fn raider_weapon(rng: &mut Rng, tier: usize, ranged: Option<bool>) -> Option<ItemId> {
        raider_weapon(rng, tier, ranged)
    }

    pub fn new_raider(id: PawnId, name: String, pos: Pos, rng: &mut Rng) -> Pawn {
        let mut p = Pawn::base(id, name, Faction::Raider, pos, rng);
        p.weapon = raider_weapon(rng, 0, None);
        p.skills[Skill::Shooting] = 3 + rng.below(6) as u8;
        p.skills[Skill::Melee] = 3 + rng.below(6) as u8;
        p.color = [0.85, 0.2, 0.2];
        p.priorities = PerWork::filled(WorkPriority::Never);
        p
    }

    pub fn is_colonist(&self) -> bool {
        self.faction == Faction::Colony
    }

    /// Alive, conscious and still on the map.
    pub fn active(&self) -> bool {
        self.life == Life::Up
    }

    pub fn is_dead(&self) -> bool {
        matches!(self.life, Life::Dead { .. })
    }

    /// Dead and buried in the grave at `pos`.
    pub fn is_buried(&self) -> bool {
        self.life == Life::Dead { buried: true }
    }

    pub fn is_downed(&self) -> bool {
        matches!(self.life, Life::Downed { .. })
    }

    pub fn has_left(&self) -> bool {
        self.life == Life::Left
    }

    /// Alive and on the map, perhaps downed.
    pub fn present(&self) -> bool {
        matches!(self.life, Life::Up | Life::Downed { .. })
    }

    /// Lying down where a doctor can work on them.
    pub fn lying(&self) -> bool {
        self.is_downed() || self.sleeping || self.resting
    }

    pub fn skill(&self, s: Skill) -> u8 {
        self.skills[s]
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

    /// The personal shield on their belt, if any: (energy when full, recharge a round).
    pub fn shield(&self) -> Option<(f32, f32)> {
        match self.utility?.kind.def().utility.as_ref()?.effect {
            crate::defs::UtilityEffect::Shield { energy, recharge } => Some((energy, recharge)),
            _ => None,
        }
    }

    /// Energy left in their shield (0 without one).
    pub fn shield_energy(&self) -> f32 {
        self.shield().map_or(0.0, |(energy, _)| (energy - self.shield_drain).max(0.0))
    }

    /// Whether they can fight with weapon `kind`: not a ranged one through their own shield.
    pub fn can_use(&self, kind: Option<ItemId>) -> bool {
        self.shield().is_none() || !attack_with(kind).ranged
    }

    /// The weapon in hand: the sidearm while it's drawn, else the main one
    /// (none, so fists, if their shield stops it).
    pub fn held(&self) -> Option<ItemId> {
        let held = if self.sidearm_drawn && self.sidearm.is_some() { self.sidearm } else { self.weapon };
        held.filter(|&k| self.can_use(Some(k)))
    }

    /// The numbers of the weapon in hand, or fists.
    pub fn attack(&self) -> Attack {
        attack_with(self.held())
    }

    /// The numbers of the main weapon (fists if their shield stops it): how
    /// they fight (at range or not).
    pub fn main_attack(&self) -> Attack {
        attack_with(self.weapon.filter(|&k| self.can_use(Some(k))))
    }

    /// What they could fight with: the main weapon (or fists), and the
    /// sidearm if they have one; ranged weapons not through a shield.
    pub fn weapons(&self) -> impl Iterator<Item = Option<ItemId>> + '_ {
        let main = self.weapon.filter(|&k| self.can_use(Some(k)));
        std::iter::once(main).chain(self.sidearm.filter(|&k| self.can_use(Some(k))).map(Some))
    }

    pub fn weapon_name(&self) -> &'static str {
        self.weapon.map_or("fists", |w| w.name())
    }

    /// The definitions of the cyberware they have installed.
    pub fn implant_defs(&self) -> impl Iterator<Item = &'static crate::defs::CyberwareDef> + '_ {
        self.health.implants().filter_map(|(_, k)| k.def().cyberware.as_ref())
    }

    fn apparel_defs(&self) -> impl Iterator<Item = &'static crate::defs::ApparelDef> + '_ {
        self.apparel.iter().filter_map(|k| k.def().apparel.as_ref())
    }

    /// Share of the damage of a blow landing on outer part `part` stopped by
    /// what protects it there, and dermal implants (which cover everything).
    pub fn armor_at(&self, part: Part) -> f32 {
        let worn: f32 = self.apparel_defs().filter(|a| a.protects(part)).map(|a| a.armor).sum();
        (worn + self.implant_defs().map(|c| c.armor).sum::<f32>()).min(0.8)
    }

    /// Share of damage stopped on average, over where blows land.
    pub fn armor(&self) -> f32 {
        let outer = Part::ALL.iter().filter(|p| !p.def().inside);
        let (total, weight) = outer.fold((0.0, 0.0), |(t, w), &p| (t + self.armor_at(p) * p.def().hit_weight, w + p.def().hit_weight));
        total / weight
    }

    /// What their gear adds to the quality of their tending (a medic pack).
    pub fn tend_bonus(&self) -> f32 {
        self.apparel_defs().map(|a| a.tend).sum()
    }

    /// How many things they carry on an expedition: `CARRY_EACH`, and more with a backpack.
    pub fn carry(&self) -> u32 {
        CARRY_EACH + self.apparel_defs().map(|a| a.carry).sum::<u32>()
    }

    /// Added to their chance to hit, from gear and implants.
    pub fn aim_bonus(&self) -> f32 {
        self.apparel_defs().map(|a| a.aim).sum::<f32>() + self.implant_defs().map(|c| c.aim).sum::<f32>()
    }

    /// What they wear in a slot.
    pub fn worn(&self, slot: Slot) -> Option<ItemId> {
        self.apparel.iter().copied().find(|a| a.def().apparel.as_ref().is_some_and(|d| d.covers(slot)))
    }

    /// What wearing apparel `kind` would take off.
    pub fn displaced_by(&self, kind: ItemId) -> Vec<ItemId> {
        let Some(new) = kind.def().apparel.as_ref() else { return Vec::new() };
        self.apparel.iter().copied().filter(|k| k.def().apparel.as_ref().is_some_and(|a| a.overlaps(new))).collect()
    }

    /// Cells per second: a brisk walk around the colony, running in battle.
    pub fn move_speed(&self) -> f32 {
        let base = if self.running { RUN_SPEED } else { WALK_SPEED };
        base * self.pace()
    }

    /// Share of normal speed they move at: how well they can move (legs,
    /// pain...), and what their gear does for it.
    pub fn pace(&self) -> f32 {
        self.health.caps.moving.clamp(0.15, 1.0) * self.gear_speed()
    }

    /// How warm what they wear keeps them in the cold (°C).
    pub fn warmth(&self) -> f32 {
        self.apparel.iter().filter_map(|k| k.def().apparel.as_ref()).map(|a| a.warmth).sum()
    }

    /// How much what they wear and their implants speed them up or slow them down (1: not at all).
    pub fn gear_speed(&self) -> f32 {
        self.apparel_defs().map(|a| a.speed).product::<f32>() * self.implant_defs().map(|c| c.speed).product::<f32>()
    }

    /// Add experience; returns true if the skill levelled up.
    pub fn learn(&mut self, s: Skill, amount: f32) -> bool {
        let i = s;
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
        if self.is_colonist() {
            if self.joy < 5.0 {
                t.push(("Very bored", -12.0));
            } else if self.joy < 25.0 {
                t.push(("Bored", -6.0));
            } else if self.joy > 85.0 {
                t.push(("Entertained", 3.0));
            }
        }
        use crate::temperature::{COLD, FREEZING, HOT};
        if self.felt < FREEZING {
            t.push(("Freezing", -12.0));
        } else if self.felt < COLD {
            t.push(("Cold", -4.0));
        } else if self.felt > HOT {
            t.push(("Hot", -4.0));
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
        if self.unburied_dead && !self.is_dead() {
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
        self.is_colonist() && !self.is_dead()
    }

    pub fn status(&self) -> String {
        if self.is_dead() {
            return "dead".into();
        }
        if self.is_downed() {
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

    /// Whether they're using `kind` (as their weapon or sidearm, wearing it, or on the belt).
    pub fn has_on(&self, kind: ItemId) -> bool {
        self.weapon == Some(kind) || self.sidearm == Some(kind) || self.apparel.contains(&kind) || self.utility.is_some_and(|s| s.kind == kind)
    }

    /// Hold a weapon or put on apparel; returns what it replaced (apparel
    /// taking several slots can replace several pieces).
    pub fn wear(&mut self, kind: ItemId) -> Vec<ItemId> {
        if kind.def().weapon.is_some() {
            return self.weapon.replace(kind).into_iter().collect();
        }
        if kind.def().apparel.is_none() {
            return Vec::new();
        }
        let old = self.displaced_by(kind);
        self.apparel.retain(|a| !old.contains(a));
        self.apparel.push(kind);
        old
    }

    /// Carry weapon `kind` as the sidearm; returns the one it replaced.
    pub fn equip_sidearm(&mut self, kind: ItemId) -> Option<ItemId> {
        self.sidearm_drawn = false;
        self.sidearm.replace(kind)
    }

    /// Put `stack` on the belt (topping up the same kind); returns what had
    /// to come off for it, and what's left over above what they carry.
    pub fn take_utility(&mut self, stack: Stack) -> Vec<Stack> {
        let carry = stack.kind.def().utility.as_ref().map_or(0, |u| u.carry);
        let mut out = Vec::new();
        let have = match self.utility.take() {
            Some(s) if s.kind == stack.kind => s.count,
            Some(s) => {
                out.push(s);
                0
            }
            None => 0,
        };
        let total = have + stack.count;
        let keep = total.min(carry);
        if keep > 0 {
            self.utility = Some(Stack { kind: stack.kind, count: keep });
        }
        if total > keep {
            out.push(Stack { kind: stack.kind, count: total - keep });
        }
        out
    }

    /// How many more of utility item `kind` they'd take (topping up, or in place of something else).
    pub fn utility_room(&self, kind: ItemId) -> u32 {
        let carry = kind.def().utility.as_ref().map_or(0, |u| u.carry);
        carry.saturating_sub(self.utility.filter(|s| s.kind == kind).map_or(0, |s| s.count))
    }

    /// Use up one utility item; returns which.
    pub fn use_utility(&mut self) -> Option<ItemId> {
        let s = self.utility.as_mut()?;
        let kind = s.kind;
        s.count -= 1;
        if s.count == 0 {
            self.utility = None;
        }
        Some(kind)
    }

    /// Put down a weapon or sidearm, take off apparel, or take everything of
    /// `kind` off the belt; false if they don't have it.
    pub fn take_off(&mut self, kind: ItemId) -> bool {
        if self.weapon == Some(kind) {
            self.weapon = None;
            return true;
        }
        if self.sidearm == Some(kind) {
            self.sidearm = None;
            self.sidearm_drawn = false;
            return true;
        }
        if self.utility.is_some_and(|s| s.kind == kind) {
            self.utility = None;
            return true;
        }
        let before = self.apparel.len();
        self.apparel.retain(|&a| a != kind);
        self.apparel.len() != before
    }
}


/// See `Pawn::raider_weapon`.
fn raider_weapon(rng: &mut Rng, tier: usize, ranged: Option<bool>) -> Option<ItemId> {
    let fits = |k: ItemId| ranged.is_none_or(|r| k.def().weapon.as_ref().is_some_and(|w| w.ranged() == r));
    let tiers = &defs().raider_tiers;
    let weapons: Vec<(ItemId, f32)> =
        tiers[tier.min(tiers.len() - 1)].weapons.iter().map(|(w, weight)| (w.id(), *weight)).filter(|w| fits(w.0)).collect();
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
        let mut p = Pawn::new_colonist(PawnId::new(1), "Test".into(), (0, 0, 1), &mut rng);
        p.add_memory(Memory::SleptOutside);
        p.memories[0].left = 1.0;
        p.add_memory(Memory::SleptOutside);
        assert_eq!(p.memories.len(), 1);
        assert_eq!(p.memories[0].left, Memory::SleptOutside.duration());
    }

    #[test]
    fn starving_tanks_mood() {
        let mut rng = Rng::new(1);
        let mut p = Pawn::new_colonist(PawnId::new(1), "Test".into(), (0, 0, 1), &mut rng);
        p.hunger = 100.0;
        p.rest = 100.0;
        let happy = p.mood_target();
        p.hunger = 0.0;
        assert!(p.mood_target() < happy - 15.0);
    }

    #[test]
    fn injuries_slow_work_and_walking() {
        let mut rng = Rng::new(1);
        let mut p = Pawn::new_colonist(PawnId::new(1), "Test".into(), (0, 0, 1), &mut rng);
        let (work, walk) = (p.work_speed(Skill::Mining), p.move_speed());
        p.health.injure(crate::health::Part::RightHand, 15.0, DamageKind::Cut, &mut rng);
        p.health.injure(crate::health::Part::LeftLeg, 20.0, DamageKind::Bruise, &mut rng);
        p.health.recompute();
        assert!(p.work_speed(Skill::Mining) < work);
        assert!(p.move_speed() < walk);
    }
}
