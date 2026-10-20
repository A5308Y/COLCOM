//! Definitions of items, buildables, plants and recipes, loaded from
//! `assets/defs.ron` (built into the game).
//!
//! The simulation refers to them by small ids (`ItemId`, `BuildId`,
//! `PlantId`, `RecipeId`): indexes into the registry returned by `defs()`. Behaviour the
//! code has to know about (walls block rooms, stairs connect levels, beds,
//! food, medicine) is described by flags and fields of the definitions, never
//! by matching on particular ones. Saves store the definitions' text ids, so
//! adding or reordering definitions keeps old saves loadable.

use crate::health::{CyberSlot, DamageKind, Part as BodyPart};
use crate::map::Desig;
use crate::pawn::{Skill, WorkType};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::sync::OnceLock;

const BUILT_IN: &str = include_str!("../assets/defs.ron");

static DEFS: OnceLock<Defs> = OnceLock::new();

/// The game's definitions.
pub fn defs() -> &'static Defs {
    DEFS.get_or_init(|| Defs::parse(BUILT_IN).unwrap_or_else(|e| panic!("assets/defs.ron: {}", e)))
}

macro_rules! def_id {
    ($Id:ident, $Def:ident, $list:ident, $what:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $Id(u16);

        impl $Id {
            /// Position in the definitions list, for per-kind arrays.
            pub fn index(self) -> usize {
                self.0 as usize
            }

            pub fn def(self) -> &'static $Def {
                &defs().$list[self.0 as usize]
            }

            pub fn name(self) -> &'static str {
                &self.def().name
            }

            pub fn find(id: &str) -> Option<$Id> {
                defs().$list.iter().position(|d| d.id == id).map(|i| $Id(i as u16))
            }

            /// The definition with this id; panics if there is none (for tests
            /// and content the game can't do without).
            pub fn named(id: &str) -> $Id {
                $Id::find(id).unwrap_or_else(|| panic!("no {} with id \"{}\" in assets/defs.ron", $what, id))
            }

            pub fn all() -> impl Iterator<Item = $Id> {
                (0..defs().$list.len() as u16).map($Id)
            }
        }

        impl std::fmt::Debug for $Id {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.def().id)
            }
        }

        impl Serialize for $Id {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.def().id)
            }
        }

        impl<'de> Deserialize<'de> for $Id {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let id = String::deserialize(d)?;
                $Id::find(&id).ok_or_else(|| D::Error::custom(format!("unknown {} \"{}\"", $what, id)))
            }
        }
    };
}

def_id!(ItemId, ItemDef, items, "item");
def_id!(BuildId, BuildDef, builds, "buildable");
def_id!(PlantId, PlantDef, plants, "plant");
def_id!(RecipeId, RecipeDef, recipes, "recipe");

/// A reference to another definition by its id, resolved once everything is read.
#[derive(Clone, Debug, Deserialize)]
#[serde(from = "String", bound = "")]
pub struct Ref<T> {
    name: String,
    id: Option<T>,
}

impl<T> From<String> for Ref<T> {
    fn from(name: String) -> Ref<T> {
        Ref { name, id: None }
    }
}

impl<T: Copy> Ref<T> {
    pub fn id(&self) -> T {
        self.id.expect("references are resolved when the definitions load")
    }
}

/// Some number of an item, written `("wood", 10)`. The item is looked up by
/// name once all definitions are read.
#[derive(Clone, Debug, Deserialize)]
#[serde(from = "(String, u32)")]
pub struct Amount {
    pub item: ItemId,
    pub count: u32,
    name: String,
}

impl From<(String, u32)> for Amount {
    fn from((name, count): (String, u32)) -> Amount {
        Amount { item: ItemId(u16::MAX), count, name }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum Shape {
    Cube,
    Sphere,
    Cylinder,
    Cone,
}

/// One piece of a look: a unit shape scaled to `size`, turned by `turn`
/// degrees around x, up and y, placed at `at` from the cell's floor centre.
#[derive(Clone, Debug, Deserialize)]
pub struct Part {
    pub shape: Shape,
    /// Palette or glow colour name, or "owner" (a bed's sleeper).
    pub color: String,
    pub at: (f32, f32, f32),
    pub size: (f32, f32, f32),
    #[serde(default)]
    pub turn: (f32, f32, f32),
    /// A door leaf: swings a quarter turn around its left edge when open.
    #[serde(default)]
    pub hinge: bool,
    /// Only shown when the neighbour towards `DIRS[link]` (before turning)
    /// is the same structure or a wall: fence rails joining up.
    #[serde(default)]
    pub link: Option<u8>,
    /// Only shown while a grave is filled (`Some(true)`) or still open (`Some(false)`).
    #[serde(default)]
    pub filled: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WeaponDef {
    pub range: f32,
    pub damage: (f32, f32),
    pub kind: DamageKind,
    /// Seconds between attacks.
    pub cooldown: f32,
    /// How it looks in hand (relative to the pawn's feet, facing +z).
    pub held: Part,
    /// Sound of an attack (see `assets/sounds.ron`); a swing if not given.
    #[serde(default)]
    pub sound: Option<String>,
    /// Shots (or blows) per attack in a battle.
    #[serde(default = "one_shot")]
    pub shots: u32,
    /// Light enough to carry as a sidearm besides the main weapon.
    #[serde(default)]
    pub sidearm: bool,
}

fn one_shot() -> u32 {
    1
}

impl WeaponDef {
    pub fn ranged(&self) -> bool {
        self.range > 2.0
    }

    /// Rough damage per second, to compare weapons.
    pub fn score(&self) -> f32 {
        (self.damage.0 + self.damage.1) / 2.0 / self.cooldown * if self.ranged() { 1.5 } else { 1.0 }
    }
}

/// Where on the body a piece of apparel is worn; one piece per slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Slot {
    /// Helmets and hats.
    Head,
    /// Goggles and visors.
    Face,
    /// The inner torso: shirts and clothes.
    #[serde(alias = "Body")]
    Shirt,
    /// The outer torso, over the shirt: jackets, vests, parkas, armour.
    Outer,
    /// Trousers and leg guards.
    Legs,
    /// Backpacks: carrying on expeditions, and specialist kit.
    Back,
}

impl Slot {
    pub const ALL: [Slot; 6] = [Slot::Head, Slot::Face, Slot::Shirt, Slot::Outer, Slot::Legs, Slot::Back];

    pub fn name(self) -> &'static str {
        match self {
            Slot::Head => "Head",
            Slot::Face => "Face",
            Slot::Shirt => "Shirt",
            Slot::Outer => "Jacket / vest",
            Slot::Legs => "Legs",
            Slot::Back => "Back",
        }
    }

    /// The outer body parts armour in this slot protects (blows that go
    /// deeper are stopped where they came in). Hands are bare but for sealed suits.
    pub fn protects(self) -> &'static [BodyPart] {
        match self {
            Slot::Head => &[BodyPart::Head, BodyPart::LeftEar, BodyPart::RightEar],
            Slot::Face => &[BodyPart::LeftEye, BodyPart::RightEye, BodyPart::Nose, BodyPart::Jaw],
            Slot::Shirt | Slot::Outer => {
                &[BodyPart::Torso, BodyPart::Neck, BodyPart::LeftShoulder, BodyPart::LeftArm, BodyPart::RightShoulder, BodyPart::RightArm]
            }
            Slot::Legs => &[BodyPart::LeftLeg, BodyPart::LeftFoot, BodyPart::RightLeg, BodyPart::RightFoot],
            Slot::Back => &[BodyPart::Torso],
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApparelDef {
    pub slot: Slot,
    /// Further slots it takes up (a sealed suit covers legs, head and face too).
    #[serde(default)]
    pub also: Vec<Slot>,
    /// Share of each blow's damage it stops.
    pub armor: f32,
    /// Mood while worn.
    pub mood: f32,
    /// How much warmer the wearer feels in the cold (°C; see `temperature::felt`).
    #[serde(default)]
    pub warmth: f32,
    /// How fast the wearer walks (and moves in battle) with it on: 1 unchanged.
    #[serde(default = "full")]
    pub speed: f32,
    /// Added to the chance to hit (0.05 = 5 percentage points).
    #[serde(default)]
    pub aim: f32,
    /// Covers the whole body, hands and feet too (a sealed suit): its armour
    /// protects every part, not just its slots' (`Slot::protects`).
    #[serde(default)]
    pub sealed: bool,
    /// How many more things the wearer carries on an expedition (backpacks).
    #[serde(default)]
    pub carry: u32,
    /// Added to the quality of the wearer's tending, in and out of battle (a medic pack).
    #[serde(default)]
    pub tend: f32,
    /// Action points their unit gets on top each battle round (a field radio; doesn't add up).
    #[serde(default)]
    pub ap: i32,
    /// What shows on the wearer: shapes fixed to the character's bones.
    #[serde(default)]
    pub pieces: Vec<WornPiece>,
    /// A body-specific skin; pieces remain loading fallbacks and headgear.
    #[serde(default)]
    pub skinned: Option<SkinnedApparel>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SkinnedApparel {
    pub male: String,
    pub female: String,
    /// Replace the native body and head under a sealed suit.
    #[serde(default)]
    pub full_body: bool,
}

/// Part of a piece of apparel as worn, following one bone of the character
/// (`spine_03` the chest, `Head`, `upperarm_l`/`_r`). `parts` are placed
/// from the bone in the character's own frame, in metres (+y up, +z the way
/// they face; about 1.8 m tall), drawn in polished metal.
#[derive(Clone, Debug, Deserialize)]
pub struct WornPiece {
    pub bone: String,
    pub parts: Vec<Part>,
    /// A shared scene authored around this bone; parts remain the loading fallback.
    #[serde(default)]
    pub model: Option<WornModel>,
    /// Hair is hidden under it (helmets).
    #[serde(default)]
    pub hides_hair: bool,
    /// Drawn as cloth rather than polished metal (a parka, a hat).
    #[serde(default)]
    pub cloth: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WornModel {
    pub path: String,
    /// Scene zero is the stored item; scenes 1..N are separate bone pieces.
    pub scene: usize,
    pub pose: ModelPose,
}

/// Cyberware: a prosthetic or bionic part, or an implant, installed by a
/// doctor at an operating table into one of the body's cyberware slots.
#[derive(Clone, Debug, Deserialize)]
pub struct CyberwareDef {
    /// The slots it can go into (an arm fits either arm).
    pub fits: Vec<CyberSlot>,
    /// How well a replaced part works (a healthy natural part is 1; bionics do better).
    #[serde(default = "full")]
    pub efficiency: f32,
    /// Share of each blow's damage it stops, like worn armour.
    #[serde(default)]
    pub armor: f32,
    /// Added to the chance to hit.
    #[serde(default)]
    pub aim: f32,
    /// How fast they walk with it: 1 unchanged.
    #[serde(default = "full")]
    pub speed: f32,
}

/// How raiders of one strength come equipped. Each rolls a weapon by the
/// relative chances, then each listed piece by its chance (later apparel
/// replaces earlier pieces it overlaps; the first sidearm and belt item that
/// come up are taken; cyberware goes into a free slot it fits).
#[derive(Clone, Debug, Deserialize)]
pub struct RaiderTier {
    pub name: String,
    /// Colony threat (`World::threat`) from which raids come like this.
    pub threat: f32,
    pub weapons: Vec<(Ref<ItemId>, f32)>,
    #[serde(default)]
    pub apparel: Vec<(Ref<ItemId>, f32)>,
    #[serde(default)]
    pub sidearms: Vec<(Ref<ItemId>, f32)>,
    /// Belt items: kind, how many, chance.
    #[serde(default)]
    pub belt: Vec<(Ref<ItemId>, u32, f32)>,
    #[serde(default)]
    pub cyberware: Vec<(Ref<ItemId>, f32)>,
}

/// Something carried on the belt and used up in battle.
#[derive(Clone, Debug, Deserialize)]
pub struct UtilityDef {
    pub effect: UtilityEffect,
    /// How many one colonist carries.
    pub carry: u32,
    /// Action points to use one.
    pub ap: i32,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum UtilityEffect {
    /// Thrown at a unit up to `range` cells away: everyone in it takes `damage`.
    Grenade { damage: (f32, f32), range: f32 },
    /// Tends the wounds of everyone in the user's unit with this quality.
    Medkit { quality: f32 },
}

impl ItemDef {
    /// Something to use or wear: a weapon, apparel or a utility item.
    pub fn is_gear(&self) -> bool {
        self.weapon.is_some() || self.apparel.is_some() || self.utility.is_some()
    }
}

impl ApparelDef {
    /// Every slot it takes up: its own, then the `also` ones.
    pub fn slots(&self) -> impl Iterator<Item = Slot> + '_ {
        std::iter::once(self.slot).chain(self.also.iter().copied())
    }

    /// Whether it takes up `slot`.
    pub fn covers(&self, slot: Slot) -> bool {
        self.slots().any(|s| s == slot)
    }

    /// Whether its armour protects outer body part `part`.
    pub fn protects(&self, part: BodyPart) -> bool {
        self.sealed || self.slots().any(|s| s.protects().contains(&part))
    }

    /// Whether it and `other` can't be worn together.
    pub fn overlaps(&self, other: &ApparelDef) -> bool {
        self.slots().any(|s| other.covers(s))
    }

    /// To compare apparel for the same slot, with it `outdoors` °C outside:
    /// armour and mood, and warmth the more the colder it is.
    pub fn score_at(&self, outdoors: f32) -> f32 {
        self.armor * 40.0 + self.mood + self.warmth * 3.0 * crate::temperature::chill(outdoors)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelPose {
    pub at: (f32, f32, f32),
    /// Euler degrees, in the same order as `Part::turn`.
    pub turn: (f32, f32, f32),
    pub scale: f32,
}

/// Optional shared glTF visual. Metres, +Y up, +Z forward, origin at the grip.
#[derive(Clone, Debug, Deserialize)]
pub struct ItemModel {
    /// Asset-relative path, without the scene label.
    pub path: String,
    /// Model to floor coordinates (also used by shelf displays).
    pub ground: ModelPose,
    /// Model to grip coordinates, before cancelling the hand bone's rotation.
    pub held: ModelPose,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    /// Group in stockpile settings.
    pub category: String,
    /// Worth of one, for what raiders demand and take.
    #[serde(default = "one_each")]
    pub value: f32,
    /// Palette colour for small views (an item being carried).
    pub color: String,
    /// Hunger restored per unit eaten; 0 = not food.
    #[serde(default)]
    pub nutrition: f32,
    /// Raw food: eating it is unpleasant.
    #[serde(default)]
    pub raw: bool,
    /// Chance per unit eaten of food poisoning.
    #[serde(default)]
    pub poison: f32,
    /// How much it improves tending; 0 = not medicine.
    #[serde(default)]
    pub medicine: f32,
    /// Range of amounts arriving in cargo pods (never, if not given).
    #[serde(default)]
    pub pods: Option<(u32, u32)>,
    #[serde(default)]
    pub weapon: Option<WeaponDef>,
    #[serde(default)]
    pub apparel: Option<ApparelDef>,
    #[serde(default, alias = "prosthetic")]
    pub cyberware: Option<CyberwareDef>,
    #[serde(default)]
    pub utility: Option<UtilityDef>,
    /// One line on what it is (components: what they stand for).
    #[serde(default)]
    pub about: Option<String>,
    #[serde(default)]
    pub model: Option<ItemModel>,
    pub look: Vec<Part>,
}

/// How much cover something gives in battle (like MENACE's light, medium
/// and heavy cover). Heavy cover blocks the view completely.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Deserialize)]
pub enum CoverKind {
    None,
    Light,
    Medium,
    Heavy,
}

/// Behaviour of a buildable that the game handles in code.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
pub enum Special {
    #[default]
    None,
    /// A slab at the bottom of the cell (`Map::floor`), built in the air.
    Floor,
    /// A roof over a room (`Map::roof`). Never placed by the player: enclosing
    /// a room marks its roof (`World::mark_roofs`), and builders put it up.
    Roof,
    /// Connects its cell with the one above.
    Stairs,
    /// Sleeping and recovering.
    Bed,
    /// Swings open when someone passes; faces along its walls.
    Door,
    /// Colonists eat at tables, sitting on a seat beside one.
    Table,
    Seat,
    /// Dug into soil; haulers bury a dead colonist in it (`Job::Bury`).
    Grave,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Light {
    pub color: (f32, f32, f32),
    pub intensity: f32,
    pub range: f32,
    pub at: (f32, f32, f32),
    /// Flickers like a flame.
    #[serde(default)]
    pub flicker: bool,
}

/// Storage furniture: holds more of an item in its cell.
#[derive(Clone, Debug, Deserialize)]
pub struct Storage {
    /// How many stacks (of up to `STACK_MAX` each, usually of different
    /// items) the cell holds: shown two on top, two below.
    pub slots: u32,
    /// Height items are shown at (the top shelf).
    pub height: f32,
    /// Height of a lower shelf for slots 3 and 4.
    #[serde(default)]
    pub lower: Option<f32>,
}

#[derive(Clone, Debug, Deserialize)]
pub enum BuildLook {
    /// A full solid block, drawn with the terrain.
    Block,
    Parts(Vec<Part>),
    /// A wall block with a hole through it (windows), drawn with the terrain
    /// so it joins the walls beside it; `parts` fill the hole (frame, glass).
    /// The hole is `width` wide (of the cell) and runs from `bottom` to `top`
    /// (of the level height), centred along the wall.
    Opening { width: f32, bottom: f32, top: f32, parts: Vec<Part> },
}

fn construction() -> WorkType {
    WorkType::Construction
}

fn construction_skill() -> Skill {
    Skill::Construction
}

#[derive(Clone, Debug, Deserialize)]
pub struct BuildModel {
    pub path: String,
    pub pose: ModelPose,
    /// Material replaced with the bed owner's blanket colour, when present.
    #[serde(default)]
    pub owner_material: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BuildDef {
    pub id: String,
    pub name: String,
    /// Cover it gives in battle; if not given: heavy if it blocks the view,
    /// medium if it can't be walked through, else light.
    #[serde(default)]
    pub cover: Option<CoverKind>,
    /// Toolbar menu it's listed under.
    pub category: String,
    /// Toolbar shortcut.
    #[serde(default)]
    pub key: Option<String>,
    /// What it takes, delivered in this order: one amount (`("steel", 4)`) or
    /// a list (`[("steel", 30), ("structural_components", 8)]`).
    #[serde(deserialize_with = "cost_list")]
    pub cost: Vec<Amount>,
    /// Seconds of work at speed 1.
    pub work_time: f32,
    #[serde(default = "construction")]
    pub work: WorkType,
    #[serde(default = "construction_skill")]
    pub skill: Skill,
    /// Sound of each stroke while building it; hammering if not given.
    #[serde(default)]
    pub sound: Option<String>,
    pub passable: bool,
    #[serde(default)]
    pub blocks_room: bool,
    #[serde(default)]
    pub blocks_sight: bool,
    /// Holds up floors on the level above (see `Map::floor_supported`).
    #[serde(default)]
    pub holds_floors: bool,
    #[serde(default)]
    pub special: Special,
    /// Makes, stores or uses power (see `power.rs`).
    #[serde(default)]
    pub power: Option<Power>,
    /// Must stand under open sky (solar panels).
    #[serde(default)]
    pub open_sky: bool,
    /// A bed only for treating patients (an operating table): nobody owns
    /// it or sleeps in it.
    #[serde(default)]
    pub medical: bool,
    /// How good a bed it is, 0..1: a sleeping spot on the ground is less
    /// restful than a bed (1), heals like lying anywhere, and counts for no room theme.
    #[serde(default = "full")]
    pub comfort: f32,
    /// Dropped when it is removed; half the cost if not given.
    #[serde(default)]
    drops: Option<Amount>,
    #[serde(default)]
    pub light: Option<Light>,
    /// Modelled spanning x; turned to run along the walls beside it.
    #[serde(default)]
    pub along_walls: bool,
    /// The side it's used from, as modelled (index into `map::DIRS`: 0 +x,
    /// 1 -x, 2 +y, 3 -y; +y if not given): the arrow while placing points
    /// there, crafters stand there at workbenches, seats face their table
    /// with it. Stairs: the way up.
    #[serde(default = "plus_y")]
    pub front: u8,
    /// Looks the same from every side (a lamp, a table): no front, nothing to turn.
    #[serde(default)]
    pub any_way: bool,
    /// A ladder: stairs (the same way on, straight up into the cell above)
    /// that look like a ladder against the side they rise towards.
    #[serde(default)]
    pub climb: bool,
    /// Hung on a wall (a painting, a wall lamp) in one of its slots rather
    /// than standing in a cell (see `decor.rs`). Modelled with x along the
    /// wall, y up from the floor and z out of the wall's face.
    #[serde(default)]
    pub on_wall: bool,
    /// Lets daylight into rooms it borders (windows).
    #[serde(default)]
    pub daylight: bool,
    /// Adds to the beauty of the room it's in (or borders, for walls and windows).
    #[serde(default)]
    pub beauty: f32,
    /// Colonists have fun at it (see `recreation.rs`): how fast, 1 = normal.
    #[serde(default)]
    pub fun: f32,
    /// Warms (or, below 0, cools) the room it's in: °C for a small room, less
    /// in bigger ones. What burns always; what uses power while it runs.
    #[serde(default)]
    pub heat: f32,
    #[serde(default)]
    pub storage: Option<Storage>,
    #[serde(default)]
    pub model: Option<BuildModel>,
    pub look: BuildLook,
    /// Recipes made here (filled in from the recipes); a workbench if any.
    #[serde(skip)]
    pub recipes: Vec<RecipeId>,
}

/// What a buildable does for power: watts made at best (solar: at noon),
/// watt-hours stored.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Power {
    pub output: f32,
    pub storage: f32,
    /// Watts it uses while running (heaters, coolers).
    pub draw: f32,
}

fn cost_list<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Amount>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Cost {
        One(Amount),
        Many(Vec<Amount>),
    }
    Ok(match Cost::deserialize(d)? {
        Cost::One(a) => vec![a],
        Cost::Many(v) => v,
    })
}

impl BuildDef {
    /// Costs nothing to build.
    pub fn free(&self) -> bool {
        self.total_cost() == 0
    }

    /// Units of material it takes, all told.
    pub fn total_cost(&self) -> u32 {
        self.cost.iter().map(|a| a.count).sum()
    }

    /// What's still missing once `delivered` units are in (materials come in
    /// the order of `cost`): the next material and how many of it.
    pub fn missing(&self, delivered: u32) -> Option<(ItemId, u32)> {
        let mut left = delivered;
        for a in &self.cost {
            if left < a.count {
                return Some((a.item, a.count - left));
            }
            left -= a.count;
        }
        None
    }

    /// What `delivered` units are, material by material (for refunds).
    pub fn delivered_split(&self, delivered: u32) -> Vec<(ItemId, u32)> {
        let mut left = delivered;
        let mut out = Vec::new();
        for a in &self.cost {
            let n = left.min(a.count);
            if n > 0 {
                out.push((a.item, n));
            }
            left -= n;
        }
        out
    }

    /// What it does for power ("Makes up to 1 kW in daylight", "Stores 10 kWh"), if anything.
    pub fn power_text(&self) -> Option<String> {
        let p = self.power?;
        let kilo = |v: f32| if v >= 1000.0 { format!("{:.1} k", v / 1000.0).replace(".0 k", " k") } else { format!("{:.0} ", v) };
        let mut parts = Vec::new();
        if p.output > 0.0 {
            let when = if self.open_sky { " in daylight" } else { "" };
            parts.push(format!("Makes up to {}W{}", kilo(p.output), when));
        }
        if p.storage > 0.0 {
            parts.push(format!("Stores {}Wh", kilo(p.storage)));
        }
        if p.draw > 0.0 {
            parts.push(format!("Uses {}W while running", kilo(p.draw)));
        }
        Some(parts.join("; "))
    }

    /// "30 steel, 8 structural components".
    pub fn cost_text(&self) -> String {
        if self.free() {
            return "free".into();
        }
        let parts: Vec<String> = self.cost.iter().map(|a| format!("{} {}", a.count, a.item.name().to_lowercase())).collect();
        parts.join(", ")
    }

    /// Cover it gives in battle: as given, else heavy if it blocks the view,
    /// none for stairs, medium if it can't be walked through, else light.
    pub fn cover_kind(&self) -> CoverKind {
        self.cover.unwrap_or(if self.blocks_sight {
            CoverKind::Heavy
        } else if self.special == Special::Stairs {
            CoverKind::None
        } else if !self.passable {
            CoverKind::Medium
        } else {
            CoverKind::Light
        })
    }

    /// A proper bed (not a sleeping spot).
    pub fn proper_bed(&self) -> bool {
        self.special == Special::Bed && self.comfort >= 1.0
    }

    /// What removing it gives back: `drops` if given, else half of each material.
    pub fn drops(&self) -> Vec<(ItemId, u32)> {
        match &self.drops {
            Some(a) => vec![(a.item, a.count)],
            None => self.cost.iter().map(|a| (a.item, a.count / 2)).filter(|d| d.1 > 0).collect(),
        }
    }

    /// Can be turned while placing: modelled furniture with a front and
    /// stairs, not walls, doors or windows (which follow their walls), floors or roofs.
    pub fn turnable(&self) -> bool {
        matches!(self.look, BuildLook::Parts(_))
            && !self.any_way
            && !self.on_wall
            && !self.along_walls
            && !matches!(self.special, Special::Floor | Special::Roof | Special::Grave)
    }

    /// Drawn as a solid block with the terrain (walls).
    pub fn is_block(&self) -> bool {
        matches!(self.look, BuildLook::Block)
    }

    /// The parts drawn as a model (none for blocks).
    pub fn parts(&self) -> &[Part] {
        match &self.look {
            BuildLook::Block => &[],
            BuildLook::Parts(p) | BuildLook::Opening { parts: p, .. } => p,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sow {
    pub work: WorkType,
    pub skill: Skill,
    pub time: f32,
    /// Sound while planting; rustling if not given.
    #[serde(default)]
    pub sound: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Wild {
    /// Share of open soil covered when the map is made.
    pub density: f32,
    /// Relative chance of being the plant that regrows somewhere.
    pub regrow: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Harvest {
    /// Only on cells with this designation; automatic once grown if none.
    #[serde(default)]
    pub desig: Option<Desig>,
    pub work: WorkType,
    pub skill: Skill,
    pub time: f32,
    pub yields: Amount,
    /// Shown as what the pawn is doing.
    pub verb: String,
    /// Sound while harvesting; rustling if not given.
    #[serde(default)]
    pub sound: Option<String>,
    /// Sound when it comes down (a tree falling).
    #[serde(default)]
    pub done_sound: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Stage {
    /// Growth from which this stage shows.
    pub from: f32,
    /// One is picked per cell.
    pub variants: Vec<Vec<Part>>,
}

fn plus_y() -> u8 {
    2
}

fn one_each() -> f32 {
    1.0
}

fn full() -> f32 {
    1.0
}

fn one() -> (f32, f32) {
    (1.0, 1.0)
}

#[derive(Clone, Debug, Deserialize)]
pub struct PlantDef {
    pub id: String,
    pub name: String,
    pub passable: bool,
    /// Cover it gives in battle; if not given: medium if it can't be walked
    /// through (trees), else none.
    #[serde(default)]
    pub cover: Option<CoverKind>,
    /// Seconds of daylight from sown to grown.
    #[serde(default)]
    pub grow_time: f32,
    #[serde(default)]
    pub sow: Option<Sow>,
    #[serde(default)]
    pub wild: Option<Wild>,
    pub harvest: Harvest,
    /// Random size range.
    #[serde(default = "one")]
    pub scale: (f32, f32),
    pub stages: Vec<Stage>,
}

impl PlantDef {
    /// Cover it gives in battle: as given, else medium if it can't be walked
    /// through (trees), else none.
    pub fn cover_kind(&self) -> CoverKind {
        self.cover.unwrap_or(if self.passable { CoverKind::None } else { CoverKind::Medium })
    }

    /// Index of the look stage for a growth value.
    pub fn stage(&self, growth: f32) -> usize {
        self.stages.iter().rposition(|s| growth >= s.from).unwrap_or(0)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct RecipeDef {
    pub id: String,
    pub name: String,
    pub bench: Ref<BuildId>,
    pub work: WorkType,
    pub skill: Skill,
    /// Seconds of work at speed 1.
    pub time: f32,
    pub ingredients: Vec<Amount>,
    pub products: Vec<Amount>,
    /// Sound while making it; generic crafting if not given.
    #[serde(default)]
    pub sound: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Defs {
    /// sRGB colours by name.
    pub palette: BTreeMap<String, (f32, f32, f32)>,
    /// Glowing colours (linear, may exceed 1) by name.
    #[serde(default)]
    pub glow: BTreeMap<String, (f32, f32, f32)>,
    /// See-through colours (sRGB and opacity) by name: glass.
    #[serde(default)]
    pub clear: BTreeMap<String, (f32, f32, f32, f32)>,
    pub items: Vec<ItemDef>,
    pub start_items: Vec<Amount>,
    /// Weapons the first colonists land with, one each in turn (the first
    /// is also what new colonists get by default; wanderers get one at random).
    #[serde(default)]
    pub start_weapons: Vec<Ref<ItemId>>,
    /// How well raiders come equipped, from the weakest (threat 0) up
    /// (`World::raid_tier`).
    pub raider_tiers: Vec<RaiderTier>,
    pub rock_drops: Amount,
    pub ore_drops: Amount,
    /// What mining copper ore gives.
    pub copper_drops: Amount,
    /// Where things made from components are taken apart (see `add_salvage`).
    #[serde(default)]
    pub salvage_bench: Option<String>,
    pub builds: Vec<BuildDef>,
    pub plants: Vec<PlantDef>,
    #[serde(default)]
    pub recipes: Vec<RecipeDef>,
    /// Themed rooms, in the order they're tried (see `rooms.rs`).
    #[serde(default)]
    pub rooms: Vec<RoomDef>,
}

/// A kind of themed room: what it needs, and how its floor looks.
#[derive(Debug, Deserialize)]
pub struct RoomDef {
    pub id: String,
    pub name: String,
    /// One line on what it's for.
    pub about: String,
    pub needs: Vec<RoomNeed>,
    /// Floor tint (sRGB).
    pub floor: (f32, f32, f32),
    /// Floor laid out in tiles.
    #[serde(default)]
    pub checker: bool,
    /// What being this kind of room does.
    #[serde(default)]
    pub effects: RoomEffects,
    /// The inside of its walls.
    #[serde(default)]
    pub wallpaper: Option<Wallpaper>,
}

/// A simple repeating pattern in two colours (sRGB) on a room's walls.
#[derive(Debug, Deserialize)]
pub struct Wallpaper {
    pub pattern: Pattern,
    pub base: (f32, f32, f32),
    pub accent: (f32, f32, f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Pattern {
    /// Just the base colour (and the skirting board).
    Plain,
    /// Thin upright stripes.
    Stripes,
    /// A chequerboard of small squares.
    Checks,
    /// Dots in offset rows.
    Dots,
    /// Square tiles with grout lines.
    Tiles,
    /// Bricks with mortar.
    Bricks,
    /// Upright wooden planks, every other one darker.
    Planks,
    /// A diamond lattice.
    Diamonds,
}

impl Pattern {
    /// Whether wallpaper square (`u` along the wall, `v` up from the floor,
    /// 8 to a cell) is in the accent colour.
    pub fn accent(self, u: i64, v: i64) -> bool {
        match self {
            Pattern::Plain => false,
            Pattern::Stripes => u.rem_euclid(4) == 0,
            Pattern::Checks => (u.div_euclid(2) + v.div_euclid(2)).rem_euclid(2) == 1,
            Pattern::Dots => (u + 2 * v.div_euclid(4).rem_euclid(2)).rem_euclid(4) == 2 && v.rem_euclid(4) == 2,
            Pattern::Tiles => u.rem_euclid(4) == 0 || v.rem_euclid(4) == 0,
            Pattern::Bricks => v.rem_euclid(3) == 0 || (u + 3 * v.div_euclid(3).rem_euclid(2)).rem_euclid(6) == 0,
            Pattern::Planks => u.rem_euclid(4) == 0,
            Pattern::Diamonds => (u.rem_euclid(6) - 3).abs() + (v.rem_euclid(6) - 3).abs() == 3,
        }
    }

    /// Brightness of the base colour there: planks and bricks vary a little.
    pub fn tone(self, u: i64, v: i64) -> f32 {
        match self {
            Pattern::Planks => [1.0, 0.9][u.div_euclid(4).rem_euclid(2) as usize],
            Pattern::Bricks => [1.0, 0.93, 0.97][(u.div_euclid(6) + v.div_euclid(3)).rem_euclid(3) as usize],
            _ => 1.0,
        }
    }
}

/// What a themed room does for the colonists who sleep or work in it.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RoomEffects {
    /// Rest gained per second in a bed here (multiplier).
    pub rest: f32,
    /// What colonists think after sleeping in a bed here, and its mood.
    pub thought: Option<(String, f32)>,
    /// Work speed at workbenches here (multiplier).
    pub work: f32,
    /// Skill learned working at benches here (multiplier).
    pub learn: f32,
    /// Chance of one more of each product when a recipe is done here.
    pub extra: f32,
    /// Wounds of patients lying in its beds heal faster (multiplier).
    pub heal: f32,
    /// Added to the quality of tending a patient lying in its beds.
    pub tend: f32,
    /// Added to the chance that an operation here goes well.
    pub surgery: f32,
    /// Fun comes this much faster here (multiplier).
    pub fun: f32,
}

impl Default for RoomEffects {
    fn default() -> Self {
        RoomEffects { rest: 1.0, thought: None, work: 1.0, learn: 1.0, extra: 0.0, heal: 1.0, tend: 0.0, surgery: 0.0, fun: 1.0 }
    }
}

impl RoomEffects {
    /// One line per effect, for the UI: "Rest 25% faster".
    pub fn lines(&self) -> Vec<String> {
        let pct = |m: f32| ((m - 1.0) * 100.0).round() as i32;
        let mut out = Vec::new();
        if self.rest != 1.0 {
            out.push(format!("Rest {}% faster in its beds", pct(self.rest)));
        }
        if let Some((text, mood)) = &self.thought {
            out.push(format!("\"{}\": mood {:+}", text, mood));
        }
        if self.work != 1.0 {
            out.push(format!("Work at its benches {}% faster", pct(self.work)));
        }
        if self.learn != 1.0 {
            out.push(format!("Skills learned {}% faster at its benches", pct(self.learn)));
        }
        if self.extra > 0.0 {
            out.push(format!("{}% chance of a spare product", (self.extra * 100.0).round() as i32));
        }
        if self.heal != 1.0 {
            out.push(format!("Patients in its beds heal {}% faster", pct(self.heal)));
        }
        if self.tend > 0.0 {
            out.push(format!("Tending patients in its beds +{}% quality", (self.tend * 100.0).round() as i32));
        }
        if self.fun != 1.0 {
            out.push(format!("Fun {}% faster", pct(self.fun)));
        }
        if self.surgery > 0.0 {
            out.push(format!("Operations +{}% likelier to go well", (self.surgery * 100.0).round() as i32));
        }
        out
    }

    /// A room for the sick: patients look for its beds first.
    pub fn medical(&self) -> bool {
        self.heal > 1.0 || self.tend > 0.0 || self.surgery > 0.0
    }
}

/// At least `min` (and at most `max`) of any of `things` in a themed room.
#[derive(Debug, Deserialize)]
pub struct RoomNeed {
    pub things: Vec<Ref<BuildId>>,
    pub min: u32,
    #[serde(default)]
    pub max: Option<u32>,
}

impl RoomNeed {
    pub fn includes(&self, kind: BuildId) -> bool {
        self.things.iter().any(|t| t.id() == kind)
    }

    /// What it asks for: "Shelf", "Sawing bench, Stonecutter or Crafting bench".
    pub fn things_text(&self) -> String {
        let names: Vec<&str> = self.things.iter().map(|t| t.id().name()).collect();
        match names.len() {
            1 => names[0].to_string(),
            n => format!("{} or {}", names[..n - 1].join(", "), names[n - 1]),
        }
    }

    /// How many: "exactly 1", "2 or more", "1 to 3".
    pub fn count_text(&self) -> String {
        match self.max {
            Some(m) if m == self.min => format!("exactly {}", m),
            Some(m) => format!("{} to {}", self.min, m),
            None => format!("{} or more", self.min),
        }
    }

    pub fn met(&self, have: u32) -> bool {
        have >= self.min && self.max.is_none_or(|m| have <= m)
    }
}

impl RoomDef {
    /// Whether a bed or workbench of `kind` may be in this kind of room.
    pub fn allows(&self, kind: BuildId) -> bool {
        let d = kind.def();
        // Sleeping spots go anywhere.
        let restricted = d.proper_bed() || !d.recipes.is_empty();
        !restricted || self.needs.iter().any(|n| n.includes(kind))
    }
}

impl Defs {
    /// Read and check definitions. References to items are resolved here.
    pub fn parse(text: &str) -> Result<Defs, String> {
        let mut d: Defs = ron::from_str(text).map_err(|e| e.to_string())?;
        d.resolve()?;
        d.check()?;
        Ok(d)
    }

    /// For every recipe that makes one thing out of components (items of the
    /// "Components" category), a recipe at the salvage bench that takes it
    /// apart again: half of each ingredient back (at least one of the first
    /// component), in half the time.
    fn add_salvage(&mut self) {
        let Some(bench) = self.salvage_bench.clone() else { return };
        let component = |name: &str| self.items.iter().any(|i| i.id == name && i.category == "Components");
        let item_name = |id: &str| self.items.iter().find(|i| i.id == id).map_or(id.to_string(), |i| i.name.clone());
        let mut salvage = Vec::new();
        for r in &self.recipes {
            let [product] = &r.products[..] else { continue };
            if product.count != 1 || !r.ingredients.iter().any(|a| component(&a.name)) {
                continue;
            }
            let mut back: Vec<Amount> = r
                .ingredients
                .iter()
                .filter(|a| a.count >= 2)
                .map(|a| Amount::from((a.name.clone(), a.count / 2)))
                .collect();
            if back.is_empty() {
                let first = r.ingredients.iter().find(|a| component(&a.name)).unwrap();
                back.push(Amount::from((first.name.clone(), 1)));
            }
            salvage.push(RecipeDef {
                id: format!("salvage_{}", product.name),
                name: format!("Salvage {}", item_name(&product.name).to_lowercase()),
                bench: Ref::from(bench.clone()),
                work: WorkType::Crafting,
                skill: Skill::Crafting,
                time: (r.time / 2.0).max(1.0),
                ingredients: vec![Amount::from((product.name.clone(), 1))],
                products: back,
                sound: None,
            });
        }
        self.recipes.extend(salvage);
    }

    fn resolve(&mut self) -> Result<(), String> {
        let ids: Vec<String> = self.items.iter().map(|i| i.id.clone()).collect();
        let find = |name: &str, whose: &str| match ids.iter().position(|i| i == name) {
            Some(k) => Ok(ItemId(k as u16)),
            None => Err(format!("{}: no item \"{}\"", whose, name)),
        };
        let resolve = |a: &mut Amount, whose: &str| {
            a.item = find(&a.name, whose)?;
            Ok::<(), String>(())
        };
        let resolve_ref = |r: &mut Ref<ItemId>, whose: &str| {
            r.id = Some(find(&r.name, whose)?);
            Ok::<(), String>(())
        };
        for a in self.start_items.iter_mut() {
            resolve(a, "start_items")?;
        }
        for r in self.start_weapons.iter_mut() {
            resolve_ref(r, "start_weapons")?;
        }
        for t in self.raider_tiers.iter_mut() {
            let what = format!("raider tier {}", t.name);
            for r in t.weapons.iter_mut().chain(t.apparel.iter_mut()).chain(t.sidearms.iter_mut()).chain(t.cyberware.iter_mut()) {
                resolve_ref(&mut r.0, &what)?;
            }
            for r in t.belt.iter_mut() {
                resolve_ref(&mut r.0, &what)?;
            }
        }
        resolve(&mut self.rock_drops, "rock_drops")?;
        resolve(&mut self.ore_drops, "ore_drops")?;
        resolve(&mut self.copper_drops, "copper_drops")?;
        for b in self.builds.iter_mut() {
            for a in b.cost.iter_mut() {
                resolve(a, &b.id)?;
            }
            if let Some(a) = b.drops.as_mut() {
                resolve(a, &b.id)?;
            }
        }
        for p in self.plants.iter_mut() {
            resolve(&mut p.harvest.yields, &p.id)?;
        }
        self.add_salvage();
        for (k, r) in self.recipes.iter_mut().enumerate() {
            for a in r.ingredients.iter_mut().chain(r.products.iter_mut()) {
                resolve(a, &r.id)?;
            }
            let Some(b) = self.builds.iter().position(|b| b.id == r.bench.name) else {
                return Err(format!("{}: no buildable \"{}\"", r.id, r.bench.name));
            };
            r.bench.id = Some(BuildId(b as u16));
            self.builds[b].recipes.push(RecipeId(k as u16));
        }
        for room in self.rooms.iter_mut() {
            for need in room.needs.iter_mut() {
                for t in need.things.iter_mut() {
                    let Some(b) = self.builds.iter().position(|b| b.id == t.name) else {
                        return Err(format!("room {}: no buildable \"{}\"", room.id, t.name));
                    };
                    t.id = Some(BuildId(b as u16));
                }
                if need.things.is_empty() || need.max.is_some_and(|m| m < need.min.max(1)) {
                    return Err(format!("room {}: each need wants things, and max at least min (and 1)", room.id));
                }
            }
            let e = &room.effects;
            if e.rest <= 0.0 || e.work <= 0.0 || e.learn <= 0.0 || e.heal <= 0.0 || !(0.0..=1.0).contains(&e.extra) {
                return Err(format!("room {}: rest, work, learn and heal must be above 0, extra 0..1", room.id));
            }
        }
        Ok(())
    }

    /// Everything the game relies on: unique ids, known colours, sensible numbers.
    fn check(&self) -> Result<(), String> {
        fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a String>) -> Result<(), String> {
            let mut seen = std::collections::HashSet::new();
            for id in ids {
                if !seen.insert(id) {
                    return Err(format!("two {} with id \"{}\"", what, id));
                }
            }
            Ok(())
        }
        unique("items", self.items.iter().map(|d| &d.id))?;
        unique("builds", self.builds.iter().map(|d| &d.id))?;
        unique("plants", self.plants.iter().map(|d| &d.id))?;
        unique("recipes", self.recipes.iter().map(|d| &d.id))?;
        unique("build keys", self.builds.iter().filter_map(|d| d.key.as_ref()))?;
        if self.items.len() >= u16::MAX as usize || self.builds.len() >= u16::MAX as usize {
            return Err("too many definitions".into());
        }

        let known = |c: &str| {
            c == "owner" || self.palette.contains_key(c) || self.glow.contains_key(c) || self.clear.contains_key(c)
        };
        let parts = |whose: &str, parts: &[Part]| -> Result<(), String> {
            match parts.iter().find(|p| !known(&p.color)) {
                Some(p) => Err(format!("{}: no colour \"{}\" in the palette", whose, p.color)),
                None => Ok(()),
            }
        };
        let check_model = |whose: &str, path: &str, poses: &[&ModelPose]| -> Result<(), String> {
            if path.is_empty() || !path.ends_with(".gltf") && !path.ends_with(".glb") {
                return Err(format!("{}: model needs a glTF asset path", whose));
            }
            for pose in poses {
                let values = [pose.at.0, pose.at.1, pose.at.2, pose.turn.0, pose.turn.1, pose.turn.2, pose.scale];
                if pose.scale <= 0.0 || values.iter().any(|v| !v.is_finite()) {
                    return Err(format!("{}: model transforms need finite values and positive scale", whose));
                }
            }
            Ok(())
        };
        for d in &self.items {
            if !known(&d.color) || d.color == "owner" {
                return Err(format!("{}: no colour \"{}\" in the palette", d.id, d.color));
            }
            if d.look.is_empty() {
                return Err(format!("{}: needs a look", d.id));
            }
            parts(&d.id, &d.look)?;
            if let Some(model) = &d.model {
                check_model(&d.id, &model.path, &[&model.ground, &model.held])?;
            }
            if d.pods.is_some_and(|(lo, hi)| lo == 0 || lo > hi) {
                return Err(format!("{}: pods need 1 <= min <= max", d.id));
            }
            if !(0.0..=1.0).contains(&d.poison) || d.nutrition < 0.0 || d.medicine < 0.0 {
                return Err(format!("{}: poison must be 0..1, nutrition and medicine not negative", d.id));
            }
            if d.category.is_empty() {
                return Err(format!("{}: needs a category", d.id));
            }
            for piece in d.apparel.iter().flat_map(|a| &a.pieces) {
                parts(&d.id, &piece.parts)?;
                if let Some(model) = &piece.model {
                    check_model(&d.id, &model.path, &[&model.pose])?;
                }
            }
            if let Some(skin) = d.apparel.as_ref().and_then(|a| a.skinned.as_ref()) {
                check_model(&d.id, &skin.male, &[])?;
                check_model(&d.id, &skin.female, &[])?;
            }
            if let Some(c) = &d.cyberware {
                if c.fits.is_empty() || !(c.efficiency > 0.0 && c.efficiency <= 2.0) || c.speed <= 0.0 || !(0.0..0.9).contains(&c.armor) {
                    return Err(format!("{}: cyberware fits some slots, with efficiency 0..2, speed above 0 and armour 0 to 0.9", d.id));
                }
                if c.fits.iter().any(|s| !s.replaces()) && c.efficiency != 1.0 {
                    return Err(format!("{}: an add-on implant replaces nothing, so its efficiency stays 1", d.id));
                }
            }
            if let Some(u) = &d.utility {
                let ok = match u.effect {
                    UtilityEffect::Grenade { damage, range } => damage.0 > 0.0 && damage.0 <= damage.1 && range > 0.0,
                    UtilityEffect::Medkit { quality } => quality > 0.0 && quality <= 1.0,
                };
                if !ok || u.carry == 0 || u.ap <= 0 || d.weapon.is_some() || d.apparel.is_some() {
                    return Err(format!("{}: a utility item needs a sensible effect, carry and AP (and is nothing else)", d.id));
                }
            }
            if d.apparel.as_ref().is_some_and(|a| !(0..40).contains(&a.ap) || a.tend < 0.0) {
                return Err(format!("{}: a pack's AP bonus is 0 to 39 (less than pinning takes away), and tending can't get worse", d.id));
            }
            if d.apparel.as_ref().is_some_and(|a| a.also.contains(&a.slot)) {
                return Err(format!("{}: `also` repeats the apparel's own slot", d.id));
            }
            if d.weapon.as_ref().is_some_and(|w| w.sidearm && w.shots > 2) {
                return Err(format!("{}: a sidearm is light (at most two shots)", d.id));
            }
            if let Some(w) = &d.weapon {
                if w.range <= 0.0 || w.cooldown <= 0.0 || w.damage.0 <= 0.0 || w.damage.0 > w.damage.1 {
                    return Err(format!("{}: weapon needs range, cooldown and 0 < min <= max damage", d.id));
                }
                parts(&d.id, std::slice::from_ref(&w.held))?;
            }
            if d.apparel.as_ref().is_some_and(|a| !(0.0..0.9).contains(&a.armor)) {
                return Err(format!("{}: armour must be 0 to 0.9", d.id));
            }
        }
        for d in &self.builds {
            if let Some(model) = &d.model {
                check_model(&d.id, &model.path, &[&model.pose])?;
                if !matches!(d.look, BuildLook::Parts(_)) || d.parts().iter().any(|p| p.hinge || p.link.is_some() || p.filled.is_some()) {
                    return Err(format!("{}: a build model needs static Parts as its loading fallback", d.id));
                }
            }
            if d.medical && d.special != Special::Bed {
                return Err(format!("{}: only beds can be medical", d.id));
            }
            if d.cost.is_empty() {
                return Err(format!("{}: needs a cost (an amount, or a list of them)", d.id));
            }
            if d.work_time <= 0.0 {
                return Err(format!("{}: needs a work time", d.id));
            }
            if d.front > 3 {
                return Err(format!("{}: front must be 0 to 3", d.id));
            }
            if d.climb && d.special != Special::Stairs {
                return Err(format!("{}: a ladder is a kind of stairs (special: Stairs)", d.id));
            }
            if d.on_wall && (!matches!(d.look, BuildLook::Parts(_)) || d.blocks_room || d.special != Special::None) {
                return Err(format!("{}: things on walls are modelled parts, nothing more", d.id));
            }
            if d.on_wall && d.storage.as_ref().is_some_and(|s| s.slots != 1) {
                return Err(format!("{}: a wall shelf holds one stack", d.id));
            }
            if !(d.comfort > 0.0 && d.comfort <= 1.0) {
                return Err(format!("{}: comfort must be above 0 and up to 1", d.id));
            }
            parts(&d.id, d.parts())?;
            if let BuildLook::Opening { width, bottom, top, .. } = d.look {
                // Walked through only if it's a door.
                if !(0.0 < width && width < 1.0 && 0.0 <= bottom && bottom < top && top <= 1.0) || d.passable != (d.special == Special::Door) {
                    return Err(format!("{}: an opening needs 0 < width < 1, 0 <= bottom < top <= 1, and to be passable only if a door", d.id));
                }
            }
            if d.is_block() && d.passable {
                return Err(format!("{}: a block can't be passable", d.id));
            }
            if d.category.is_empty() {
                return Err(format!("{}: needs a category", d.id));
            }
            if d.storage.as_ref().is_some_and(|s| !(1..=4).contains(&s.slots)) {
                return Err(format!("{}: storage holds 1 to 4 stacks", d.id));
            }
            if d.special == Special::Seat && !d.passable {
                return Err(format!("{}: seats must be passable", d.id));
            }
            if d.parts().iter().any(|p| p.link.is_some_and(|l| l > 3)) {
                return Err(format!("{}: link must be 0 to 3", d.id));
            }
            if matches!(d.special, Special::Floor | Special::Roof | Special::Stairs | Special::Bed | Special::Door | Special::Grave)
                && !d.passable
            {
                return Err(format!("{}: {:?} must be passable", d.id, d.special));
            }
        }
        if self.builds.iter().filter(|d| d.special == Special::Floor).count() > 1 {
            return Err("only one buildable can be the floor".into());
        }
        if self.builds.iter().filter(|d| d.special == Special::Roof).count() > 1 {
            return Err("only one buildable can be the roof".into());
        }
        let mut wild = 0.0;
        for d in &self.plants {
            if d.stages.is_empty() || d.stages[0].from != 0.0 || d.stages.iter().any(|s| s.variants.is_empty()) {
                return Err(format!("{}: stages must start at 0 and each have a variant", d.id));
            }
            if d.stages.windows(2).any(|w| w[0].from >= w[1].from) {
                return Err(format!("{}: stages must be in order of growth", d.id));
            }
            for s in &d.stages {
                for v in &s.variants {
                    parts(&d.id, v)?;
                }
            }
            if d.sow.is_some() && d.grow_time <= 0.0 {
                return Err(format!("{}: sown plants need a grow time", d.id));
            }
            if d.harvest.yields.count == 0 || d.harvest.time <= 0.0 {
                return Err(format!("{}: harvest needs a yield and a time", d.id));
            }
            if d.scale.0 <= 0.0 || d.scale.0 > d.scale.1 {
                return Err(format!("{}: scale needs 0 < min <= max", d.id));
            }
            wild += d.wild.as_ref().map_or(0.0, |w| w.density);
        }
        if wild >= 1.0 {
            return Err("wild plant densities add up to 1 or more".into());
        }
        for r in &self.recipes {
            if r.ingredients.is_empty() || r.products.is_empty() || r.time <= 0.0 {
                return Err(format!("{}: needs ingredients, products and a work time", r.id));
            }
            if r.ingredients.iter().chain(&r.products).any(|a| a.count == 0) {
                return Err(format!("{}: amounts must be at least 1", r.id));
            }
            if r.ingredients.iter().any(|a| a.count > crate::map::STACK_MAX) {
                return Err(format!("{}: at most {} of an ingredient", r.id, crate::map::STACK_MAX));
            }
            // (`self`, not `defs()`: this runs while the registry is being made.)
            if self.builds[r.bench.id().index()].is_block() {
                return Err(format!("{}: a workbench can't be a block", r.id));
            }
        }
        let weapons = self.start_weapons.iter().chain(self.raider_tiers.iter().flat_map(|t| t.weapons.iter().map(|w| &w.0)));
        for w in weapons {
            if self.items[w.id().index()].weapon.is_none() {
                return Err(format!("\"{}\" is not a weapon", w.name));
            }
        }
        if self.raider_tiers.first().is_none_or(|t| t.threat != 0.0) || self.raider_tiers.windows(2).any(|w| w[0].threat >= w[1].threat) {
            return Err("raider_tiers: the first at threat 0, then rising".into());
        }
        for t in &self.raider_tiers {
            let item = |r: &Ref<ItemId>| &self.items[r.id().index()];
            let bad = t.weapons.is_empty()
                || t.apparel.iter().any(|a| item(&a.0).apparel.is_none())
                || t.sidearms.iter().any(|a| !item(&a.0).weapon.as_ref().is_some_and(|w| w.sidearm))
                || t.belt.iter().any(|a| item(&a.0).utility.is_none())
                || t.cyberware.iter().any(|a| item(&a.0).cyberware.is_none());
            if bad {
                return Err(format!("raider tier {}: weapons, apparel, sidearms, belt items and cyberware of the right kinds", t.name));
            }
        }
        Ok(())
    }

    /// The buildable that makes floors, if any.
    pub fn floor(&self) -> Option<BuildId> {
        self.builds.iter().position(|d| d.special == Special::Floor).map(|i| BuildId(i as u16))
    }

    /// The buildable that makes roofs, if any.
    pub fn stairs(&self) -> Option<BuildId> {
        self.builds.iter().position(|d| d.special == Special::Stairs && !d.climb).map(|i| BuildId(i as u16))
    }

    pub fn ladder(&self) -> Option<BuildId> {
        self.builds.iter().position(|d| d.special == Special::Stairs && d.climb).map(|i| BuildId(i as u16))
    }

    pub fn roof(&self) -> Option<BuildId> {
        self.builds.iter().position(|d| d.special == Special::Roof).map(|i| BuildId(i as u16))
    }

    /// What grow zones are planted with.
    pub fn sown(&self) -> Option<PlantId> {
        self.plants.iter().position(|d| d.sow.is_some()).map(|i| PlantId(i as u16))
    }

    pub fn is_food(&self, item: ItemId) -> bool {
        self.items[item.0 as usize].nutrition > 0.0
    }

    pub fn is_medicine(&self, item: ItemId) -> bool {
        self.items[item.0 as usize].medicine > 0.0
    }

    pub fn is_cyberware(&self, item: ItemId) -> bool {
        self.items[item.0 as usize].cyberware.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladders_are_stairs_that_look_different() {
        let (ladder, stairs) = (defs().ladder().unwrap(), defs().stairs().unwrap());
        assert_ne!(ladder, stairs);
        assert_eq!(ladder.def().special, Special::Stairs);
        assert!(ladder.def().climb && !stairs.def().climb);
        assert!(ladder.def().total_cost() < stairs.def().total_cost(), "cheaper");
    }

    #[test]
    fn furniture_fronts_make_sense() {
        for k in BuildId::all() {
            let d = k.def();
            assert!(d.front <= 3, "{}", d.id);
            if d.any_way {
                assert!(!d.turnable(), "{}: looks the same every way, so isn't turned", d.id);
            }
        }
        // Workbenches are turned so crafters stand at their front.
        assert!(BuildId::all().filter(|k| !k.def().recipes.is_empty()).all(|k| k.def().turnable()));
        assert!(!BuildId::named("table").def().turnable() && BuildId::named("stairs").def().turnable());
    }

    #[test]
    fn built_in_definitions_load() {
        let d = defs();
        assert!(!d.items.is_empty() && !d.builds.is_empty() && !d.plants.is_empty());
        assert!(d.floor().is_some());
        assert!(d.roof().is_some(), "rooms get roofs");
        assert!(d.sown().is_some());
        assert!(ItemId::all().any(|i| d.is_food(i)), "something to eat");
        assert!(ItemId::all().any(|i| d.is_medicine(i)), "some medicine");
        assert!(BuildId::all().any(|b| b.def().special == Special::Bed), "somewhere to sleep");
        assert!(BuildId::all().any(|b| b.def().special == Special::Stairs), "a way up");
    }

    #[test]
    fn item_models_are_optional_and_have_valid_transforms() {
        let rifle = ItemId::named("rifle").def();
        let model = rifle.model.as_ref().unwrap();
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(&model.path).is_file());
        let legacy: ItemDef = serde_json::from_str(r#"{"id":"legacy","name":"Legacy","category":"Materials","color":"wood","look":[]}"#).unwrap();
        assert!(legacy.model.is_none(), "old definitions can still omit the model field");
        let mut d = Defs::parse(BUILT_IN).unwrap();
        d.items[ItemId::named("rifle").index()].model.as_mut().unwrap().held.scale = 0.0;
        assert!(d.check().unwrap_err().contains("positive scale"));
        let mut d = Defs::parse(BUILT_IN).unwrap();
        let helmet = &mut d.items[ItemId::named("steel_helmet").index()];
        helmet.apparel.as_mut().unwrap().pieces[0].model.as_mut().unwrap().pose.scale = f32::NAN;
        assert!(d.check().unwrap_err().contains("finite values"));
    }

    #[test]
    fn build_models_are_optional_and_require_valid_static_fallbacks() {
        let legacy: BuildDef = serde_json::from_str(r#"{"id":"legacy","name":"Legacy","category":"Furniture","cost":["wood",1],"work_time":1.0,"passable":false,"look":{"Parts":[]}}"#).unwrap();
        assert!(legacy.model.is_none());
        let mut d = Defs::parse(BUILT_IN).unwrap();
        let bed = BuildId::named("bed").index();
        d.builds[bed].model.as_mut().unwrap().pose.scale = 0.0;
        assert!(d.check().unwrap_err().contains("positive scale"));
        d.builds[bed].model.as_mut().unwrap().pose.scale = 1.0;
        let BuildLook::Parts(parts) = &mut d.builds[bed].look else { panic!() };
        parts[0].hinge = true;
        assert!(d.check().unwrap_err().contains("static Parts"));
    }

    #[test]
    fn armor_targets_bones_present_in_both_character_skeletons() {
        for body in ["Superhero_Male_FullBody", "Superhero_Female_FullBody"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("assets/models/characters/base/{body}.gltf"));
            let gltf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            for piece in ItemId::all().flat_map(|k| k.def().apparel.iter().flat_map(|a| &a.pieces)) {
                assert!(gltf["nodes"].as_array().unwrap().iter().any(|n| n["name"].as_str() == Some(&piece.bone)), "{} missing {}", body, piece.bone);
            }
        }
    }

    #[test]
    fn ids_round_trip_by_name() {
        for i in ItemId::all() {
            let json = serde_json::to_string(&i).unwrap();
            assert_eq!(json, format!("\"{}\"", i.def().id));
            assert_eq!(serde_json::from_str::<ItemId>(&json).unwrap(), i);
        }
        for b in BuildId::all() {
            assert_eq!(BuildId::named(&b.def().id), b);
        }
        assert!(serde_json::from_str::<PlantId>("\"no such plant\"").is_err());
    }

    #[test]
    fn mistakes_are_reported() {
        let broken = BUILT_IN.replacen("cost: (\"stone\", 4)", "cost: (\"stoen\", 4)", 1);
        let err = Defs::parse(&broken).unwrap_err();
        assert!(err.contains("stoen"), "{}", err);
        let broken = BUILT_IN.replacen("color: \"trunk\"", "color: \"bark\"", 1);
        let err = Defs::parse(&broken).unwrap_err();
        assert!(err.contains("bark"), "{}", err);
        let broken = BUILT_IN.replacen("id: \"door\"", "id: \"wall\"", 1);
        assert!(Defs::parse(&broken).is_err());
    }

    #[test]
    fn growth_picks_the_right_stage() {
        let crop = defs().sown().unwrap().def();
        assert_eq!(crop.stage(0.0), 0);
        assert_eq!(crop.stage(0.99), crop.stages.len() - 2);
        assert_eq!(crop.stage(1.0), crop.stages.len() - 1);
    }

    /// Buildables stay inside their cell's height (1.0), so nothing pokes
    /// through a lintel or the floor above. Standing on a built floor lifts
    /// them by the slab (0.08), except doors and windows, which fill their
    /// cell like walls.
    #[test]
    fn buildables_fit_under_the_ceiling() {
        // Half the height of a box turned by Euler angles (degrees, X then Y then Z,
        // as the front end turns parts): the Y row of Rx·Ry·Rz against the half sizes.
        let half_height = |p: &Part| {
            let (x, y, z) = (p.turn.0.to_radians(), p.turn.1.to_radians(), p.turn.2.to_radians());
            let row = [
                x.cos() * z.sin() + x.sin() * y.sin() * z.cos(),
                x.cos() * z.cos() - x.sin() * y.sin() * z.sin(),
                -x.sin() * y.cos(),
            ];
            (row[0].abs() * p.size.0 + row[1].abs() * p.size.1 + row[2].abs() * p.size.2) / 2.0
        };
        for kind in BuildId::all() {
            let def = kind.def();
            if def.special == Special::Stairs {
                continue; // they lead up to the level above
            }
            let ceiling = if def.along_walls { 1.0 } else { 1.0 - 0.08 };
            for p in def.parts() {
                let half = half_height(p);
                assert!(p.at.1 + half <= ceiling + 1e-4, "{}: a part reaches {:.2}", def.id, p.at.1 + half);
            }
        }
    }

    /// Everything that can be had from the land (mined, chopped, grown), then
    /// made at benches that can themselves be built from what's had so far,
    /// leaving salvage out: the benches form a ladder, and every component
    /// and the endgame gear are on it.
    #[test]
    fn every_bench_and_component_can_be_reached_without_salvage() {
        let d = defs();
        let mut have: Vec<bool> = vec![false; d.items.len()];
        for a in [&d.rock_drops, &d.ore_drops, &d.copper_drops].into_iter().chain(d.plants.iter().map(|p| &p.harvest.yields)) {
            have[a.item.index()] = true;
        }
        let mut benches: Vec<bool> = vec![false; d.builds.len()];
        let mut order: Vec<String> = Vec::new();
        loop {
            let mut changed = false;
            for (b, def) in d.builds.iter().enumerate() {
                if !benches[b] && def.cost.iter().all(|a| have[a.item.index()]) {
                    benches[b] = true;
                    changed = true;
                    if !def.recipes.is_empty() {
                        order.push(def.id.clone());
                    }
                }
            }
            for r in d.recipes.iter().filter(|r| !r.id.starts_with("salvage_")) {
                let ready = benches[r.bench.id().index()] && r.ingredients.iter().all(|a| have[a.item.index()]);
                for a in r.products.iter().filter(|_| ready) {
                    if !have[a.item.index()] {
                        have[a.item.index()] = true;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for (b, def) in d.builds.iter().enumerate() {
            assert!(benches[b] || def.special == Special::Roof, "{} can't be built without salvage", def.id);
        }
        for item in d.items.iter().filter(|i| i.category == "Components") {
            assert!(have[ItemId::named(&item.id).index()], "{} can't be made", item.id);
        }
        for id in ["assault_rifle", "powered_armor", "laser_rifle", "silicon", "rubber", "copper"] {
            assert!(have[ItemId::named(id).index()], "{} can't be made", id);
        }
        // The fabricator comes last of the benches: an achievement.
        assert_eq!(order.last().map(String::as_str), Some("fabricator"), "{:?}", order);
    }

    #[test]
    fn gear_made_from_components_can_be_salvaged() {
        let d = defs();
        for id in ["laser_rifle", "powered_armor", "rifle", "knife", "steel_armor"] {
            let item = ItemId::named(id);
            let made = d.recipes.iter().find(|r| r.products.iter().any(|a| a.item == item)).unwrap();
            let salvage = d.recipes.iter().find(|r| r.id == format!("salvage_{}", id)).expect("a salvage recipe");
            assert_eq!(salvage.bench.id(), BuildId::named("salvage_bench"));
            assert!(!salvage.products.is_empty(), "{}: something back", id);
            for back in &salvage.products {
                let put_in = made.ingredients.iter().find(|a| a.item == back.item).map_or(0, |a| a.count);
                assert!(back.count >= 1 && back.count <= put_in, "{}: {} back of {}", id, back.count, put_in);
            }
        }
        assert!(d.recipes.iter().all(|r| r.id != "salvage_mechanical_components"), "components themselves aren't");
        assert!(ItemId::named("mechanical_components").def().about.as_deref().is_some_and(|a| a.contains("Gears")));
    }

}
