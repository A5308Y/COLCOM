//! Definitions of items, buildables, plants and recipes, loaded from
//! `assets/defs.ron` (built into the game).
//!
//! The simulation refers to them by small ids (`ItemId`, `BuildId`,
//! `PlantId`, `RecipeId`): indexes into the registry returned by `defs()`. Behaviour the
//! code has to know about (walls block rooms, stairs connect levels, beds,
//! food, medicine) is described by flags and fields of the definitions, never
//! by matching on particular ones. Saves store the definitions' text ids, so
//! adding or reordering definitions keeps old saves loadable.

use crate::health::DamageKind;
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Slot {
    /// Clothes.
    Body,
    /// Armour worn over clothes.
    Outer,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApparelDef {
    pub slot: Slot,
    /// Share of each blow's damage it stops.
    pub armor: f32,
    /// Mood while worn.
    pub mood: f32,
}

impl ApparelDef {
    /// To compare apparel for the same slot.
    pub fn score(&self) -> f32 {
        self.armor * 40.0 + self.mood
    }
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
    /// How many of one item the cell holds.
    pub stack_limit: u32,
    /// Height items are shown at.
    pub height: f32,
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
    pub cost: Amount,
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
    /// Dropped when it is removed; half the cost if not given.
    #[serde(default)]
    drops: Option<Amount>,
    #[serde(default)]
    pub light: Option<Light>,
    /// Modelled spanning x; turned to run along the walls beside it.
    #[serde(default)]
    pub along_walls: bool,
    /// Lets daylight into rooms it borders (windows).
    #[serde(default)]
    pub daylight: bool,
    /// Adds to the beauty of the room it's in (or borders, for walls and windows).
    #[serde(default)]
    pub beauty: f32,
    #[serde(default)]
    pub storage: Option<Storage>,
    pub look: BuildLook,
    /// Recipes made here (filled in from the recipes); a workbench if any.
    #[serde(skip)]
    pub recipes: Vec<RecipeId>,
}

impl BuildDef {
    /// What removing it gives back.
    pub fn drops(&self) -> (ItemId, u32) {
        match &self.drops {
            Some(a) => (a.item, a.count),
            None => (self.cost.item, self.cost.count / 2),
        }
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

fn one_each() -> f32 {
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
    /// Weapons raiders carry, with relative chances.
    #[serde(default)]
    pub raider_weapons: Vec<(Ref<ItemId>, f32)>,
    pub rock_drops: Amount,
    pub ore_drops: Amount,
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
        let restricted = d.special == Special::Bed || !d.recipes.is_empty();
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
        for (r, _) in self.raider_weapons.iter_mut() {
            resolve_ref(r, "raider_weapons")?;
        }
        resolve(&mut self.rock_drops, "rock_drops")?;
        resolve(&mut self.ore_drops, "ore_drops")?;
        for b in self.builds.iter_mut() {
            resolve(&mut b.cost, &b.id)?;
            if let Some(a) = b.drops.as_mut() {
                resolve(a, &b.id)?;
            }
        }
        for p in self.plants.iter_mut() {
            resolve(&mut p.harvest.yields, &p.id)?;
        }
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
        for d in &self.items {
            if !known(&d.color) || d.color == "owner" {
                return Err(format!("{}: no colour \"{}\" in the palette", d.id, d.color));
            }
            if d.look.is_empty() {
                return Err(format!("{}: needs a look", d.id));
            }
            parts(&d.id, &d.look)?;
            if d.pods.is_some_and(|(lo, hi)| lo == 0 || lo > hi) {
                return Err(format!("{}: pods need 1 <= min <= max", d.id));
            }
            if !(0.0..=1.0).contains(&d.poison) || d.nutrition < 0.0 || d.medicine < 0.0 {
                return Err(format!("{}: poison must be 0..1, nutrition and medicine not negative", d.id));
            }
            if d.category.is_empty() {
                return Err(format!("{}: needs a category", d.id));
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
            if d.cost.count == 0 || d.work_time <= 0.0 {
                return Err(format!("{}: needs a cost and work time", d.id));
            }
            parts(&d.id, d.parts())?;
            if let BuildLook::Opening { width, bottom, top, .. } = d.look {
                if !(0.0 < width && width < 1.0 && 0.0 <= bottom && bottom < top && top <= 1.0) || d.passable {
                    return Err(format!("{}: an opening needs 0 < width < 1, 0 <= bottom < top <= 1, not passable", d.id));
                }
            }
            if d.is_block() && d.passable {
                return Err(format!("{}: a block can't be passable", d.id));
            }
            if d.category.is_empty() {
                return Err(format!("{}: needs a category", d.id));
            }
            if d.storage.as_ref().is_some_and(|s| s.stack_limit < crate::map::STACK_MAX) {
                return Err(format!("{}: storage must hold at least {}", d.id, crate::map::STACK_MAX));
            }
            if (d.storage.is_some() || d.special == Special::Seat) && !d.passable {
                return Err(format!("{}: storage and seats must be passable", d.id));
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
        let weapons = self.start_weapons.iter().chain(self.raider_weapons.iter().map(|w| &w.0));
        for w in weapons {
            if self.items[w.id().index()].weapon.is_none() {
                return Err(format!("\"{}\" is not a weapon", w.name));
            }
        }
        Ok(())
    }

    /// The buildable that makes floors, if any.
    pub fn floor(&self) -> Option<BuildId> {
        self.builds.iter().position(|d| d.special == Special::Floor).map(|i| BuildId(i as u16))
    }

    /// The buildable that makes roofs, if any.
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
