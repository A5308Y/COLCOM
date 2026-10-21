//! Per-tile plants, structures, blueprints and items: definition-selected glTF
//! scenes with shape fallbacks (`assets/defs.ron`).
//!
//! Compare map props when the map changes. Doors, beds and graves also
//! follow pawn state each frame; changed props get their entity replaced.

use super::audio::SoundQueue;
use super::input::{box_cells, hollow, order_target, Tool, UiState};
use super::look::*;
use super::sim::Sim;
use super::terrain::{FLOOR_T, LEVEL_H};
use super::tile_center;
use bevy::prelude::*;
use bevy::gltf::GltfAssetLabel;
use bevy::gltf::GltfMaterialName;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use colcom::pawn::PawnId;
use colcom::defs::{defs, BuildDef, BuildId, BuildLook, ItemId, ModelPose, Part, PlantId, PlantVariant, Shape, Special, Storage, WornPiece};
use colcom::map::{Dir, Feature, Map, Over, Pos, DIRS};
use std::collections::{HashMap, HashSet};
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, PartialEq, Debug)]
enum PropKey {
    Plant { kind: PlantId, stage: u8 },
    /// Turned so the model's +x faces `dir`. `owner`: who sleeps in
    /// a bed. `open`: a door someone is passing through, or a grave nobody
    /// is buried in yet. `links`: bit per
    /// direction (`DIRS`) with a neighbour that linked parts join up with.
    /// `snug`: how far it's drawn pushed back against a wall behind it (see `snug`).
    Built { kind: BuildId, dir: Dir, owner: Option<PawnId>, open: bool, links: u8, snug: (f32, f32) },
    /// `over`: around a wall still standing there (a slightly bigger ghost).
    /// `plant`: a plant still standing in it, to be cleared first (kind and stage).
    Blueprint { kind: BuildId, dir: Dir, over: bool, snug: (f32, f32), plant: Option<(PlantId, u8)> },
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct ItemKey {
    kind: ItemId,
    big: bool,
}

/// What lies in a cell, slot by slot (a shelf holds up to four).
type ItemsKey = [Option<ItemKey>; 4];

#[derive(Resource, Default)]
struct Props {
    generation: Option<u64>,
    changes: Option<u64>,
    /// Props whose appearance also follows pawns (doors, beds and graves).
    dynamic: Vec<usize>,
    /// Layer view level and roof cut version the visibilities were last set for.
    view: Option<(usize, u64)>,
    /// Spawned objects per cell (flat arrays: cheaper than maps when scanning every cell each frame).
    features: Vec<Option<(PropKey, Entity)>>,
    items: Vec<Option<(ItemsKey, Entity)>>,
    /// What hangs on walls, by wall cell, face and slot.
    decor: HashMap<(u32, Dir, u8), (DecorKey, Entity)>,
}

/// Something on a wall as drawn: up or planned, and what stands on it (a wall shelf).
#[derive(Clone, Copy, PartialEq, Debug)]
struct DecorKey {
    kind: BuildId,
    built: bool,
    item: Option<ItemKey>,
}

/// Shared unit meshes, materials and glTF scenes for props and equipped items.
#[derive(Resource)]
pub struct PropAssets {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    cone: Handle<Mesh>,
    /// One material per palette and glow colour of the definitions.
    paints: HashMap<String, Handle<StandardMaterial>>,
    /// Polished metal in each palette colour, for armour.
    metals: HashMap<String, Handle<StandardMaterial>>,
    blueprint: Handle<StandardMaterial>,
    blanket_free: Handle<StandardMaterial>,
    models: HashMap<String, Handle<WorldAsset>>,
}

impl PropAssets {
    pub fn model(&self, path: &str, scene: usize) -> Handle<WorldAsset> {
        self.models[&model_key(path, scene)].clone()
    }
    pub fn paint(&self, color: &str) -> Handle<StandardMaterial> {
        // Unknown colours are caught when the definitions load.
        self.paints.get(color).unwrap_or(&self.blanket_free).clone()
    }

    pub fn mesh(&self, shape: Shape) -> &Handle<Mesh> {
        match shape {
            Shape::Cube => &self.cube,
            Shape::Sphere => &self.sphere,
            Shape::Cylinder => &self.cylinder,
            Shape::Cone => &self.cone,
        }
    }
}

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Props>()
            .add_systems(Startup, setup_assets)
            .add_systems(Update, (sync_props, flicker_lights, preview_blueprints));
    }
}

pub fn lit(materials: &mut Assets<StandardMaterial>, c: Rgb) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: c.color(), perceptual_roughness: 0.85, ..default() })
}

fn setup_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    server: Res<AssetServer>,
) {
    let d = defs();
    let mut paints = HashMap::new();
    let mut metals = HashMap::new();
    for (name, &(r, g, b)) in &d.palette {
        paints.insert(name.clone(), lit(&mut materials, Rgb(r, g, b)));
        let metal = StandardMaterial { base_color: Rgb(r, g, b).color(), metallic: 0.85, perceptual_roughness: 0.32, ..default() };
        metals.insert(name.clone(), materials.add(metal));
    }
    // Bright enough to bloom.
    for (name, &(r, g, b)) in &d.glow {
        let glow = StandardMaterial {
            base_color: Color::srgb(r.min(1.0), g.min(1.0), b.min(1.0)),
            emissive: LinearRgba::rgb(r, g, b),
            ..default()
        };
        paints.insert(name.clone(), materials.add(glow));
    }
    for (name, &(r, g, b, a)) in &d.clear {
        let glass = StandardMaterial {
            base_color: Color::srgba(r, g, b, a),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.1,
            reflectance: 0.5,
            ..default()
        };
        paints.insert(name.clone(), materials.add(glass));
    }
    let blueprint = materials.add(StandardMaterial {
        base_color: Color::srgba(0.38, 0.68, 1.0, 0.35),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let mut models = HashMap::new();
    for item in &d.items {
        let stored = item.model.iter().map(|model| (model.path.as_str(), 0));
        let worn = item.apparel.iter().flat_map(|apparel| &apparel.pieces)
            .filter_map(|piece| piece.model.as_ref()).map(|model| (model.path.as_str(), model.scene));
        for (path, scene) in stored.chain(worn) {
            models.entry(model_key(path, scene)).or_insert_with(|| server.load(GltfAssetLabel::Scene(scene).from_asset(path.to_string())));
        }
        if let Some(skin) = item.apparel.as_ref().and_then(|a| a.skinned.as_ref()) {
            for path in [&skin.male, &skin.female] {
                models.entry(model_key(path, 0)).or_insert_with(|| server.load(GltfAssetLabel::Scene(0).from_asset(path.clone())));
            }
        }
    }
    for model in d.builds.iter().filter_map(|build| build.model.as_ref()) {
        for scene in model.scenes.all() {
            models.entry(model_key(&model.path, scene)).or_insert_with(|| server.load(GltfAssetLabel::Scene(scene).from_asset(model.path.clone())));
        }
    }
    for model in d.plants.iter().flat_map(|p| &p.stages).flat_map(|s| &s.variants).filter_map(|v| v.model.as_ref()) {
        models.entry(model_key(&model.path, 0)).or_insert_with(|| server.load(GltfAssetLabel::Scene(0).from_asset(model.path.clone())));
    }
    let assets = PropAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        sphere: meshes.add(Sphere::new(0.5)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        cone: meshes.add(Cone::new(0.5, 1.0)),
        paints,
        metals,
        blueprint,
        blanket_free: lit(&mut materials, Rgb(0.55, 0.58, 0.62)),
        models,
    };
    commands.insert_resource(assets);
}

/// Neighbours a structure's linked parts (fence rails) join up with: the
/// same structure, walls or solid ground. Bit `d` is set for `DIRS[d]`.
fn links(m: &Map, pos: Pos, kind: BuildId) -> u8 {
    let mut mask = 0;
    for (d, (dx, dy)) in DIRS.iter().enumerate() {
        let joins = m.at(pos.0 as i32 + dx, pos.1 as i32 + dy, pos.2 as i32).is_some_and(|q| {
            let f = m.feature[m.idx(q)];
            f.structure_id() == Some(kind) || f.is_block() || m.block[m.idx(q)].solid()
        });
        mask |= (joins as u8) << d;
    }
    mask
}

/// Torches and other flames: light that wavers around `base` intensity.
#[derive(Component)]
struct Flicker {
    base: f32,
    seed: f32,
}

fn flicker_lights(time: Res<Time>, mut lights: Query<(&Flicker, &mut PointLight)>) {
    let t = time.elapsed_secs();
    for (f, mut light) in &mut lights {
        let wave = (t * 9.0 + f.seed).sin() * 0.5 + (t * 23.0 + f.seed * 1.7).sin() * 0.3 + (t * 3.1 + f.seed).sin() * 0.2;
        light.intensity = f.base * (0.85 + 0.12 * wave);
    }
}

/// A seat beside a table faces it (if it doesn't face one already);
/// otherwise it stays as it was placed. `front`: the way its model faces.
pub fn seat_dir(m: &Map, pos: Pos, front: Dir, dir: Dir) -> Dir {
    let table = |d: Dir| {
        let (dx, dy) = d.offset();
        m.at(pos.0 as i32 + dx, pos.1 as i32 + dy, pos.2 as i32)
            .is_some_and(|t| m.feature[m.idx(t)].is_special(Special::Table))
    };
    if table(front.turned(dir)) {
        return dir;
    }
    let Some(t) = Dir::ALL.into_iter().find(|&d| table(d)) else { return dir };
    Dir::ALL.into_iter().find(|&d| front.turned(d) == t).unwrap_or(dir)
}

/// How far furniture `kind` turned to `dir` at `pos` is drawn pushed back
/// (map x, y): until it touches a wall (or window, or rock) right behind it,
/// the side away from its front. Walls are drawn thin, so it would stand
/// clear of them otherwise. Stairs, ladders and seats stay put.
pub fn snug(m: &Map, pos: Pos, kind: BuildId, dir: Dir) -> Vec2 {
    let d = kind.def();
    if !d.turnable() || matches!(d.special, Special::Stairs | Special::Seat) {
        return Vec2::ZERO;
    }
    let back = d.front.opposite();
    let (bx, by) = back.turned(dir).offset();
    let Some(behind) = m.at(pos.0 as i32 + bx, pos.1 as i32 + by, pos.2 as i32) else { return Vec2::ZERO };
    let bi = m.idx(behind);
    // Where what's behind starts, from the middle of this cell.
    let face = if m.block[bi].solid() {
        0.5
    } else if m.feature[bi].blocks_room() && !m.feature[bi].is_special(Special::Door) {
        1.0 - colcom::decor::WALL_T / 2.0
    } else {
        return Vec2::ZERO;
    };
    // How far the model reaches towards its back.
    let (lx, ly) = back.offset();
    let reach = d
        .parts()
        .iter()
        .map(|p| p.at.0 * lx as f32 + p.at.2 * ly as f32 + if lx != 0 { p.size.0 } else { p.size.2 } / 2.0)
        .fold(0.0f32, f32::max);
    let shift = (face - reach - 0.01).max(0.0);
    Vec2::new(bx as f32, by as f32) * shift
}

/// `snug` for what's built at `pos` (zero if it's not furniture).
pub fn built_snug(m: &Map, pos: Pos) -> Vec2 {
    match m.feature[m.idx(pos)] {
        colcom::map::Feature::Built { kind, dir } => snug(m, pos, kind, dir),
        _ => Vec2::ZERO,
    }
}

/// Rotation that turns +x towards `dir`.
fn facing(dir: Dir) -> Quat {
    let (dx, dy) = dir.offset();
    Quat::from_rotation_y((-dy as f32).atan2(dx as f32))
}

#[allow(clippy::too_many_arguments)]
fn sync_props(
    mut commands: Commands,
    sim: Res<Sim>,
    ui: Res<UiState>,
    props: ResMut<Props>,
    assets: Res<PropAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sounds: ResMut<SoundQueue>,
) {
    let props = props.into_inner();
    let w = &sim.world;
    let m = &w.map;
    let hidden = |pos: Pos| pos.2 > ui.top_at(m, pos.0, pos.1);
    props.reset(&mut commands, sim.generation, m.len());

    props.sync_visibility(&mut commands, m, &ui);

    let map_changed = props.changes != Some(m.changes());
    props.changes = Some(m.changes());
    let dynamic = std::mem::take(&mut props.dynamic);
    let mut owners: HashMap<Pos, (PawnId, [f32; 3])> = HashMap::new();
    for p in w.pawns.iter().filter(|p| !p.is_dead()) {
        if let Some(b) = p.bed {
            owners.insert(b, (p.id, p.color));
        }
    }
    let filled: HashSet<Pos> = w.pawns.iter().filter(|p| p.is_buried()).map(|p| p.pos).collect();

    let cells = (0..if map_changed { m.len() } else { 0 })
        .chain(dynamic.iter().copied().filter(|_| !map_changed));
    for i in cells {
        if map_changed && m.feature[i].structure().is_some_and(|d| matches!(d.special, Special::Door | Special::Bed | Special::Grave)) {
            props.dynamic.push(i);
        }
        let pos = m.pos(i);
        let want = prop_key(w, i, &owners, &filled);
        let have = props.features[i].map(|(k, _)| k);
        // A door swinging open.
        let door = matches!(m.feature[i], Feature::Built { kind, .. } if kind.def().special == Special::Door);
        if let (true, Some(PropKey::Built { open: true, .. }), Some(PropKey::Built { open: false, .. })) = (door, want, have) {
            sounds.play("door", Some(tile_center(pos) + Vec3::Y * 0.5));
        }
        if want != have {
            if let Some((_, e)) = props.features[i].take() {
                commands.entity(e).despawn();
            }
            if let Some(key) = want {
                let blanket = match key {
                    PropKey::Built { owner: Some(_), .. } => {
                        let c = owners[&pos].1;
                        lit(&mut materials, Rgb(c[0], c[1], c[2]).shade(0.8))
                    }
                    _ => assets.blanket_free.clone(),
                };
                // Doors and windows fill their cell like the walls they sit in, floor or not.
                let in_wall = match m.feature[i] {
                    Feature::Built { kind, .. } | Feature::Blueprint { kind, .. } => kind.def().along_walls,
                    Feature::None | Feature::Plant { .. } => false,
                };
                let lift = if m.floor[i] && !in_wall { FLOOR_T } else { 0.0 };
                let e = spawn_feature(&mut commands, &assets, key, pos, lift, blanket);
                if hidden(pos) {
                    commands.entity(e).insert(Visibility::Hidden);
                }
                props.features[i] = Some((key, e));
            }
        }

        sync_items(&mut commands, &assets, props, m, &ui, i);
    }

    if !map_changed {
        props.dynamic = dynamic;
        return;
    }

    sync_decor(&mut commands, &assets, props, m, &ui);
}

impl Props {
    fn reset(&mut self, commands: &mut Commands, generation: u64, len: usize) {
        if self.generation != Some(generation) || self.features.len() != len {
            for (_, e) in self.features.drain(..).flatten() {
                commands.entity(e).despawn();
            }
            for (_, e) in self.items.drain(..).flatten() {
                commands.entity(e).despawn();
            }
            for (_, (_, e)) in self.decor.drain() {
                commands.entity(e).despawn();
            }
            self.features = vec![None; len];
            self.items = vec![None; len];
            self.generation = Some(generation);
            self.view = None;
            self.changes = None;
            self.dynamic.clear();
        }
    }

    fn sync_visibility(&mut self, commands: &mut Commands, m: &Map, ui: &UiState) {
        let hidden = |pos: Pos| pos.2 > ui.top_at(m, pos.0, pos.1);
        let view = (ui.top_level(m), ui.roof_version);
        // Layer view changed: show or hide everything by level.
        if self.view != Some(view) {
            for i in 0..m.len() {
                let vis = if hidden(m.pos(i)) { Visibility::Hidden } else { Visibility::Inherited };
                for e in [self.features[i].map(|f| f.1), self.items[i].map(|f| f.1)].into_iter().flatten() {
                    commands.entity(e).insert(vis);
                }
            }
            for (&(i, _, _), &(_, e)) in &self.decor {
                let vis = if hidden(m.pos(i as usize)) { Visibility::Hidden } else { Visibility::Inherited };
                commands.entity(e).insert(vis);
            }
            self.view = Some(view);
        }
    }
}

fn prop_key(w: &colcom::world::World, i: usize, owners: &HashMap<Pos, (PawnId, [f32; 3])>, filled: &HashSet<Pos>) -> Option<PropKey> {
    let m = &w.map;
    let pos = m.pos(i);
    match m.feature[i] {
        Feature::None => None,
        Feature::Plant { kind, growth } => Some(PropKey::Plant { kind, stage: kind.def().stage(growth) as u8 }),
        // To be built over a wall that's drawn with the terrain.
        Feature::Blueprint { kind, over: Over::Structure(..), .. } => {
            Some(PropKey::Blueprint { kind, dir: Dir::PlusX, over: true, snug: (0.0, 0.0), plant: None })
        }
        // Drawn with the terrain.
        f if f.is_block() => None,
        Feature::Built { kind, dir } => Some({
            let def = kind.def();
            let owner = (def.special == Special::Bed).then(|| owners.get(&pos).map(|o| o.0)).flatten();
            let open = match def.special {
                Special::Door => w.pawns.iter().any(|p| {
                    !p.is_dead()
                        && (p.x - pos.0 as f32).abs() < 0.9
                        && (p.y - pos.1 as f32).abs() < 0.9
                        && (p.z - pos.2 as f32).abs() < 0.9
                }),
                Special::Grave => !filled.contains(&pos),
                Special::None | Special::Floor | Special::Roof | Special::Stairs | Special::Bed | Special::Table | Special::Seat | Special::Lookout => false,
            };
            // Modelled spanning x; a quarter turn when the walls run along y.
            let dir = if def.along_walls {
                if m.runs_along_x(pos) {
                    Dir::PlusX
                } else {
                    Dir::MinusY
                }
            } else if def.special == Special::Seat {
                seat_dir(m, pos, def.front, dir)
            } else {
                dir
            };
            let linked = def.parts().iter().any(|p| p.link.is_some());
            let links = if linked { links(m, pos, kind) } else { 0 };
            let snug = snug(m, pos, kind, dir);
            PropKey::Built { kind, dir, owner, open, links, snug: (snug.x, snug.y) }
        }),
        Feature::Blueprint { kind, dir, over, .. } => {
            let dir = if kind.def().turnable() { dir } else { Dir::PlusX };
            let snug = snug(m, pos, kind, dir);
            let plant = match over {
                Over::Plant { kind, growth } => Some((kind, kind.def().stage(growth) as u8)),
                Over::Nothing | Over::Structure(..) => None,
            };
            Some(PropKey::Blueprint { kind, dir, over: false, snug: (snug.x, snug.y), plant })
        }
    }
}

fn sync_items(commands: &mut Commands, assets: &PropAssets, props: &mut Props, m: &Map, ui: &UiState, i: usize) {
    let hidden = |pos: Pos| pos.2 > ui.top_at(m, pos.0, pos.1);
    let pos = m.pos(i);
    let mut want: ItemsKey = [None; 4];
    for (slot, s) in want.iter_mut().zip(m.stacks(i)) {
        *slot = Some(ItemKey { kind: s.kind, big: s.count >= 25 });
    }
    // What wall shelves hold is drawn on them (with the rest of the wall's things).
    let want_item = (want[0].is_some() && m.wall_shelves(i) == 0).then_some(want);
    let have_item = props.items[i].map(|(k, _)| k);
    if want_item != have_item {
        if let Some((_, e)) = props.items[i].take() {
            commands.entity(e).despawn();
        }
        if let Some(key) = want_item {
            let storage = m.feature[i].structure().and_then(|d| d.storage.as_ref());
            let lift = if m.floor[i] { FLOOR_T } else { 0.0 };
            let e = match storage {
                Some(st) if st.slots > 1 => spawn_shelf_items(commands, assets, key, pos, lift, st),
                _ => spawn_item(commands, assets, key[0].unwrap(), pos, lift + storage.map_or(0.0, |s| s.height)),
            };
            // On a shelf pushed back against a wall: with it.
            if storage.is_some() {
                let s = built_snug(m, pos);
                commands.entity(e).entry::<Transform>().and_modify(move |mut t| t.translation += Vec3::new(s.x, 0.0, s.y));
            }
            if hidden(pos) {
                commands.entity(e).insert(Visibility::Hidden);
            }
            props.items[i] = Some((key, e));
        }
    }
}

fn sync_decor(commands: &mut Commands, assets: &PropAssets, props: &mut Props, m: &Map, ui: &UiState) {
    let hidden = |pos: Pos| pos.2 > ui.top_at(m, pos.0, pos.1);
    // What hangs on walls: each wall shelf holds the next of its wall's stacks.
    let mut want: HashMap<(u32, Dir, u8), DecorKey> = HashMap::new();
    for (&i, list) in &m.decor {
        let mut stacks = m.stacks(i as usize);
        for d in list {
            let shelf = d.is_up() && d.kind.def().storage.is_some();
            let item = if shelf { stacks.next().map(|s| ItemKey { kind: s.kind, big: s.count >= 25 }) } else { None };
            want.insert((i, d.face, d.slot), DecorKey { kind: d.kind, built: d.is_up(), item });
        }
    }
    let stale: Vec<(u32, Dir, u8)> = props.decor.iter().filter(|(k, (key, _))| want.get(*k) != Some(key)).map(|(k, _)| *k).collect();
    for k in stale {
        if let Some((_, e)) = props.decor.remove(&k) {
            commands.entity(e).despawn();
        }
    }
    for (k, key) in want {
        if props.decor.contains_key(&k) {
            continue;
        }
        let pos = m.pos(k.0 as usize);
        let lift = if m.floor[k.0 as usize] { FLOOR_T } else { 0.0 };
        let e = spawn_decor(commands, assets, key, pos, k.1, k.2, lift);
        if hidden(pos) {
            commands.entity(e).insert(Visibility::Hidden);
        }
        props.decor.insert(k, (key, e));
    }
}

/// Blueprint ghosts shown where they'd go before clicking: under the cursor
/// while a buildable is picked, or over the whole rectangle while dragging.
#[derive(Default)]
struct Preview {
    cells: Vec<(Pos, PropKey)>,
    decor: Option<(Pos, Dir, u8, BuildId)>,
    entities: Vec<Entity>,
}

/// Most ghosts shown at once (a big drag shows its first ones).
const PREVIEW_MAX: usize = 400;

fn preview_blueprints(
    mut commands: Commands,
    sim: Res<Sim>,
    ui: Res<UiState>,
    keys: Res<ButtonInput<KeyCode>>,
    assets: Res<PropAssets>,
    mut shown: Local<Preview>,
) {
    let w = &sim.world;
    let m = &w.map;
    let mut cells = Vec::new();
    let mut decor = None;
    if let (Tool::Paint(order @ colcom::world::Order::Build(kind)), Some(h)) = (ui.tool, ui.hover) {
        let d = kind.def();
        if d.on_wall {
            decor = ui.wall_slot.filter(|&(at, f, s)| w.decor_ok(at, f, s, kind)).map(|(at, f, s)| (at, f, s, kind));
        } else {
            let to = order_target(w, h, order);
            let from = ui.drag_start.unwrap_or(to);
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            for p in box_cells(w, from, to, order, hollow(order, shift)).into_iter().take(PREVIEW_MAX) {
                if p.2 > ui.top_at(m, p.0, p.1) {
                    continue;
                }
                // Into a standing wall (a window): the shell around it, as once planned.
                let over = matches!(m.feature[m.idx(p)], Feature::Built { .. });
                let dir = if d.turnable() { ui.place_dir } else { colcom::map::Dir::PlusX };
                let key = if over {
                    PropKey::Blueprint { kind, dir: colcom::map::Dir::PlusX, over: true, snug: (0.0, 0.0), plant: None }
                } else {
                    let s = snug(m, p, kind, dir);
                    PropKey::Blueprint { kind, dir, over: false, snug: (s.x, s.y), plant: None }
                };
                cells.push((p, key));
            }
        }
    }
    if cells == shown.cells && decor == shown.decor {
        return;
    }
    for e in shown.entities.drain(..) {
        commands.entity(e).despawn();
    }
    for &(p, key) in &cells {
        let i = m.idx(p);
        let lift = if m.floor[i] && !key_along_walls(key) { FLOOR_T } else { 0.0 };
        shown.entities.push(spawn_feature(&mut commands, &assets, key, p, lift, assets.blanket_free.clone()));
    }
    if let Some((at, face, slot, kind)) = decor {
        let lift = if m.floor[m.idx(at)] { FLOOR_T } else { 0.0 };
        let key = DecorKey { kind, built: false, item: None };
        shown.entities.push(spawn_decor(&mut commands, &assets, key, at, face, slot, lift));
    }
    shown.cells = cells;
    shown.decor = decor;
}

/// Doors and windows fill their cell like the walls they sit in, floor or not.
fn key_along_walls(key: PropKey) -> bool {
    match key {
        PropKey::Blueprint { kind, .. } | PropKey::Built { kind, .. } => kind.def().along_walls,
        PropKey::Plant { .. } => false,
    }
}

/// Where slot `slot` on face `face` of the wall at `pos` is: on the face
/// (half a wall's thickness out from the middle of the cell), turned so the
/// model's +z looks out of the wall and +x runs along it.
pub fn decor_transform(pos: Pos, face: Dir, slot: u8, lift: f32) -> Transform {
    let (nx, ny) = face.offset();
    let n = Vec3::new(nx as f32, 0.0, ny as f32);
    let (_, plus) = colcom::decor::slot_axis(face);
    let (ux, uy) = plus.offset();
    let u = Vec3::new(ux as f32, 0.0, uy as f32);
    let (s0, s1) = colcom::decor::slot_span(slot);
    let at = tile_center(pos) + n * (colcom::decor::WALL_T / 2.0) + u * ((s0 + s1) / 2.0 - 0.5) + Vec3::Y * lift;
    let x = Vec3::Y.cross(n);
    Transform::from_translation(at).with_rotation(Quat::from_mat3(&Mat3::from_cols(x, Vec3::Y, n)))
}

/// Something on a wall (or the ghost of one planned), with its light and
/// what stands on it.
fn spawn_decor(commands: &mut Commands, a: &PropAssets, key: DecorKey, pos: Pos, face: Dir, slot: u8, lift: f32) -> Entity {
    let def = key.kind.def();
    let root = decor_transform(pos, face, slot, lift);
    let pose = Pose { ghost: !key.built, ..default() };
    let e = spawn_build_look(commands, a, def, root, pose, &a.blanket_free);
    if let Some(l) = def.light.as_ref().filter(|_| key.built) {
        let light = PointLight {
            color: Color::srgb(l.color.0, l.color.1, l.color.2),
            intensity: l.intensity,
            range: l.range,
            shadow_maps_enabled: false,
            ..default()
        };
        commands.entity(e).with_child((light, Transform::from_xyz(l.at.0, l.at.1, l.at.2)));
    }
    if let (Some(item), Some(st)) = (key.item, def.storage.as_ref()) {
        let s = if item.big { 0.34 } else { 0.28 };
        let t = Transform::from_xyz(0.0, st.height, 0.06).with_scale(Vec3::splat(s));
        let on = spawn_item_look(commands, a, item.kind, t);
        commands.entity(on).insert(ChildOf(e));
    }
    e
}

/// How to show a set of parts.
#[derive(Clone, Copy, Default)]
struct Pose {
    /// All in the blueprint material.
    ghost: bool,
    /// Hinged parts swung open.
    open: bool,
    /// Turn of the model, to match linked parts to neighbours.
    dir: Dir,
    /// Directions with a neighbour that linked parts join (see `links`).
    links: u8,
}

/// Parts as children of a root at `root`. `owner` replaces the "owner" colour.
fn spawn_parts(
    commands: &mut Commands,
    a: &PropAssets,
    root: Transform,
    parts: &[Part],
    pose: Pose,
    owner: &Handle<StandardMaterial>,
) -> Entity {
    let Pose { ghost, open, dir, links } = pose;
    let mut e = commands.spawn((root, Visibility::default()));
    e.with_children(|c| {
        for p in parts {
            if p.link.is_some_and(|l| links & (1 << l.turned(dir).index()) == 0) {
                continue;
            }
            // Graves: the pit while open, the mound once filled.
            if p.filled == Some(open) {
                continue;
            }
            let mat = match p.color.as_str() {
                _ if ghost => a.blueprint.clone(),
                "owner" => owner.clone(),
                color => a.paint(color),
            };
            let at = Vec3::new(p.at.0, p.at.1, p.at.2);
            let mut t = part_transform(p);
            if p.hinge && open {
                // A quarter turn around its left edge.
                t.rotate_around(at - Vec3::X * p.size.0 / 2.0, Quat::from_rotation_y(-FRAC_PI_2));
            }
            c.spawn((Mesh3d(a.mesh(p.shape).clone()), MeshMaterial3d(mat), t));
        }
    });
    e.id()
}

/// Where a part of a look sits: placed at `at`, turned by `turn` (degrees
/// around x, up and y), scaled to `size`.
pub fn part_transform(p: &Part) -> Transform {
    let (tx, ty, tz) = p.turn;
    Transform::from_xyz(p.at.0, p.at.1, p.at.2)
        .with_rotation(Quat::from_euler(EulerRot::XYZ, tx.to_radians(), ty.to_radians(), tz.to_radians()))
        .with_scale(Vec3::new(p.size.0, p.size.1, p.size.2))
}

pub fn model_transform(pose: &ModelPose) -> Transform {
    Transform::from_xyz(pose.at.0, pose.at.1, pose.at.2)
        .with_rotation(Quat::from_euler(EulerRot::XYZ, pose.turn.0.to_radians(), pose.turn.1.to_radians(), pose.turn.2.to_radians()))
        .with_scale(Vec3::splat(pose.scale))
}

#[derive(Component)]
struct ModelFallback(Entity);

fn model_key(path: &str, scene: usize) -> String {
    format!("{path}#Scene{scene}")
}

/// Keep the cheap shape visible until this particular scene instance is ready.
pub fn attach_item_model(
    commands: &mut Commands,
    a: &PropAssets,
    kind: ItemId,
    parent: Entity,
    transform: Transform,
    fallback: Entity,
) {
    let Some(model) = kind.def().model.as_ref() else { return };
    attach_model(commands, a.models[&model_key(&model.path, 0)].clone(), parent, transform, fallback);
}

fn attach_model(commands: &mut Commands, model: Handle<WorldAsset>, parent: Entity, transform: Transform, fallback: Entity) -> Entity {
    commands.spawn((
        WorldAssetRoot(model),
        transform,
        Visibility::default(),
        ChildOf(parent),
        ModelFallback(fallback),
    )).observe(hide_model_fallback).id()
}

#[derive(Component)]
struct BuildStyle {
    ghost: Option<Handle<StandardMaterial>>,
    owner: Option<(String, Handle<StandardMaterial>)>,
}

impl BuildStyle {
    fn material(&self, name: &str) -> Option<Handle<StandardMaterial>> {
        self.ghost.clone().or_else(|| self.owner.as_ref().filter(|(label, _)| label == name).map(|(_, material)| material.clone()))
    }
}

fn style_build_model(
    ready: On<WorldInstanceReady>,
    styles: Query<&BuildStyle>,
    children: Query<&Children>,
    mut surfaces: Query<(&GltfMaterialName, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let entity = ready.event().entity;
    let Ok(style) = styles.get(entity) else { return };
    for child in children.iter_descendants(entity) {
        let Ok((name, mut surface)) = surfaces.get_mut(child) else { continue };
        if let Some(material) = style.material(&name.0) {
            surface.0 = material;
        }
    }
}

fn spawn_build_look(
    commands: &mut Commands, a: &PropAssets, def: &BuildDef, transform: Transform, pose: Pose, owner: &Handle<StandardMaterial>,
) -> Entity {
    let Some(model) = &def.model else { return spawn_parts(commands, a, transform, def.parts(), pose, owner) };
    let root = commands.spawn((transform, Visibility::default())).id();
    let fallback = spawn_parts(commands, a, Transform::default(), def.parts(), pose, owner);
    commands.entity(fallback).insert(ChildOf(root));
    let selected = model.scenes.select(pose.open, pose.links, pose.dir);
    let scene = attach_model(commands, a.model(&model.path, selected), root, model_transform(&model.pose), fallback);
    commands.entity(scene).insert(BuildStyle {
        ghost: pose.ghost.then(|| a.blueprint.clone()),
        owner: model.owner_material.as_ref().map(|name| (name.clone(), owner.clone())),
    }).observe(style_build_model);
    root
}

fn hide_model_fallback(ready: On<WorldInstanceReady>, fallbacks: Query<&ModelFallback>, mut commands: Commands) {
    if let Ok(fallback) = fallbacks.get(ready.event().entity) {
        commands.entity(fallback.0).insert(Visibility::Hidden);
    }
}

/// A multi-stalk plot changes over as a whole: a failed or delayed instance
/// leaves its complete fallback visible instead of hiding plants prematurely.
#[derive(Component)]
struct PlantModels {
    fallback: Entity,
    scenes: Vec<Entity>,
    waiting: HashSet<Entity>,
}

#[derive(Component)]
struct PlantModelGroup(Entity);

fn plant_models_ready(
    ready: On<WorldInstanceReady>, members: Query<&PlantModelGroup>,
    mut groups: Query<&mut PlantModels>, mut commands: Commands,
) {
    let entity = ready.event().entity;
    let Ok(member) = members.get(entity) else { return };
    let Ok(mut group) = groups.get_mut(member.0) else { return };
    if group.waiting.remove(&entity) && group.waiting.is_empty() {
        commands.entity(group.fallback).insert(Visibility::Hidden);
        for &scene in &group.scenes {
            commands.entity(scene).insert(Visibility::Inherited);
        }
    }
}

fn spawn_plant_look(commands: &mut Commands, a: &PropAssets, transform: Transform, variant: &PlantVariant) -> Entity {
    let Some(model) = &variant.model else {
        return spawn_parts(commands, a, transform, &variant.parts, Pose::default(), &a.blanket_free);
    };
    let root = commands.spawn((transform, Visibility::default())).id();
    let fallback = spawn_parts(commands, a, Transform::default(), &variant.parts, Pose::default(), &a.blanket_free);
    commands.entity(fallback).insert(ChildOf(root));
    let scenes: Vec<_> = model.placements.iter().map(|pose| {
        commands.spawn((WorldAssetRoot(a.model(&model.path, 0)), model_transform(pose),
            Visibility::Hidden, ChildOf(root), PlantModelGroup(root))).observe(plant_models_ready).id()
    }).collect();
    commands.entity(root).insert(PlantModels { fallback, waiting: scenes.iter().copied().collect(), scenes });
    root
}

fn spawn_item_look(commands: &mut Commands, a: &PropAssets, kind: ItemId, transform: Transform) -> Entity {
    if kind.def().model.is_none() {
        return spawn_parts(commands, a, transform, &kind.def().look, Pose::default(), &a.blanket_free);
    }
    let root = commands.spawn((transform, Visibility::default())).id();
    let fallback = spawn_parts(commands, a, Transform::default(), &kind.def().look, Pose::default(), &a.blanket_free);
    commands.entity(fallback).insert(ChildOf(root));
    if let Some(model) = &kind.def().model {
        attach_item_model(commands, a, kind, root, model_transform(&model.ground), fallback);
    }
    root
}

/// A carried stack uses the same item visual as a ground or shelf stack.
pub fn replace_carried_item(commands: &mut Commands, a: &PropAssets, carrier: Entity, kind: Option<ItemId>) {
    commands.entity(carrier).despawn_children();
    if let Some(kind) = kind {
        let item = spawn_item_look(commands, a, kind, Transform::default());
        commands.entity(item).insert(ChildOf(carrier));
    }
}

/// Worn apparel: `parts` (in metres from the bone, in the character's own
/// frame) under a root turned by `turn` (undoing the bone's own rest turn),
/// in polished metal (armour) or plain colours (`cloth`: a parka, a hat).
pub fn spawn_worn(commands: &mut Commands, a: &PropAssets, transform: Transform, piece: &WornPiece) -> Entity {
    let root = commands.spawn((transform, Visibility::default())).id();
    let mut fallback = commands.spawn((Transform::default(), Visibility::default(), ChildOf(root)));
    fallback.with_children(|c| {
        for p in &piece.parts {
            let t = part_transform(p);
            let metal = a.metals.get(&p.color).filter(|_| !piece.cloth);
            let mat = metal.cloned().unwrap_or_else(|| a.paint(&p.color));
            c.spawn((Mesh3d(a.mesh(p.shape).clone()), MeshMaterial3d(mat), t));
        }
    });
    let fallback = fallback.id();
    if let Some(model) = &piece.model {
        attach_model(commands, a.models[&model_key(&model.path, model.scene)].clone(), root, model_transform(&model.pose), fallback);
    }
    root
}

fn spawn_plant(commands: &mut Commands, a: &PropAssets, kind: PlantId, stage: u8, pos: Pos, lift: f32) -> Entity {
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    let def = kind.def();
    let variants = &def.stages[stage as usize].variants;
    let variant = &variants[(hash(xi, yi, 23) * variants.len() as f32) as usize % variants.len()];
    let (lo, hi) = def.scale;
    let root = Transform::from_translation(tile_center(pos) + Vec3::Y * lift)
        .with_rotation(Quat::from_rotation_y(hash(xi, yi, 22) * TAU))
        .with_scale(Vec3::splat(lo + hash(xi, yi, 20) * (hi - lo)));
    spawn_plant_look(commands, a, root, variant)
}

fn spawn_feature(
    commands: &mut Commands,
    a: &PropAssets,
    key: PropKey,
    pos: Pos,
    lift: f32,
    blanket: Handle<StandardMaterial>,
) -> Entity {
    let pushed = match key {
        PropKey::Built { snug, .. } | PropKey::Blueprint { snug, .. } => Vec3::new(snug.0, 0.0, snug.1),
        PropKey::Plant { .. } => Vec3::ZERO,
    };
    let base = Transform::from_translation(tile_center(pos) + pushed + Vec3::Y * lift);
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    match key {
        PropKey::Plant { kind, stage } => spawn_plant(commands, a, kind, stage, pos, lift),
        // The plant still standing, with the ghost of what replaces it: both under one entity.
        PropKey::Blueprint { kind, dir, over, snug, plant: Some((plant, stage)) } => {
            let ghost = PropKey::Blueprint { kind, dir, over, snug, plant: None };
            let children = [spawn_feature(commands, a, ghost, pos, lift, blanket), spawn_plant(commands, a, plant, stage, pos, lift)];
            commands.spawn((Transform::default(), Visibility::default())).add_children(&children).id()
        }
        PropKey::Built { kind, dir, open, links, .. } => {
            let def = kind.def();
            let pose = Pose { ghost: false, open, dir, links };
            let e = spawn_build_look(commands, a, def, base.with_rotation(facing(dir)), pose, &blanket);
            if let Some(l) = &def.light {
                let light = (
                    PointLight {
                        color: Color::srgb(l.color.0, l.color.1, l.color.2),
                        intensity: l.intensity,
                        range: l.range,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_xyz(l.at.0, l.at.1, l.at.2),
                );
                if l.flicker {
                    let seed = hash(xi, yi, 25) * 100.0;
                    commands.entity(e).with_child((light, Flicker { base: l.intensity, seed }));
                } else {
                    commands.entity(e).with_child(light);
                }
            }
            e
        }
        PropKey::Blueprint { kind, dir, over, .. } => match &kind.def().look {
            BuildLook::Parts(_) if !over => {
                let pose = Pose { ghost: true, dir, ..default() };
                spawn_build_look(commands, a, kind.def(), base.with_rotation(facing(dir)), pose, &blanket)
            }
            // Over a standing wall: a shell just around it.
            _ => {
                let size = if over { 1.04 } else { 0.98 };
                let block = Part {
                    shape: Shape::Cube,
                    color: String::new(),
                    at: (0.0, LEVEL_H / 2.0, 0.0),
                    size: (size, LEVEL_H * if over { 1.02 } else { 1.0 }, size),
                    turn: (0.0, 0.0, 0.0),
                    hinge: false,
                    link: None,
                    filled: None,
                };
                spawn_parts(commands, a, base, &[block], Pose { ghost: true, ..default() }, &blanket)
            }
        },
    }
}

/// A shelf's stacks: two side by side on the top board, two on the lower one, smaller than on the ground.
fn spawn_shelf_items(commands: &mut Commands, a: &PropAssets, key: ItemsKey, pos: Pos, lift: f32, st: &Storage) -> Entity {
    let root = commands.spawn((Transform::from_translation(tile_center(pos) + Vec3::Y * lift), Visibility::default())).id();
    for (k, item) in key.iter().enumerate() {
        let Some(item) = item else { continue };
        let x = if k % 2 == 0 { -0.22 } else { 0.22 };
        let y = if k < 2 { st.height } else { st.lower.unwrap_or(st.height) };
        let s = if item.big { 0.62 } else { 0.5 };
        let t = Transform::from_xyz(x, y, 0.0).with_scale(Vec3::splat(s));
        let e = spawn_item_look(commands, a, item.kind, t);
        commands.entity(e).insert(ChildOf(root));
    }
    root
}

fn spawn_item(commands: &mut Commands, a: &PropAssets, key: ItemKey, pos: Pos, lift: f32) -> Entity {
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    let s = if key.big { 1.25 } else { 1.0 };
    let root = Transform::from_translation(tile_center(pos) + Vec3::Y * lift)
        .with_rotation(Quat::from_rotation_y(hash(xi, yi, 24) * TAU))
        .with_scale(Vec3::splat(s));
    spawn_item_look(commands, a, key.kind, root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use colcom::defs::BuildCategory;
    use colcom::map::FLAT_Z as Z;

    fn test_assets() -> PropAssets {
        let mut scenes = Assets::<WorldAsset>::default();
        let mut models = HashMap::new();
        for kind in ItemId::all() {
            let def = kind.def();
            if let Some(model) = &def.model {
                models.insert(model_key(&model.path, 0), scenes.add(WorldAsset::new(World::new())));
            }
            for piece in def.apparel.iter().flat_map(|a| &a.pieces) {
                if let Some(model) = &piece.model {
                    models.insert(model_key(&model.path, model.scene), scenes.add(WorldAsset::new(World::new())));
                }
            }
            if let Some(skin) = def.apparel.as_ref().and_then(|a| a.skinned.as_ref()) {
                for path in [&skin.male, &skin.female] {
                    models.insert(model_key(path, 0), scenes.add(WorldAsset::new(World::new())));
                }
            }
        }
        for model in defs().builds.iter().filter_map(|build| build.model.as_ref()) {
            for scene in model.scenes.all() {
                models.insert(model_key(&model.path, scene), scenes.add(WorldAsset::new(World::new())));
            }
        }
        for model in defs().plants.iter().flat_map(|p| &p.stages).flat_map(|s| &s.variants).filter_map(|v| v.model.as_ref()) {
            models.entry(model_key(&model.path, 0)).or_insert_with(|| scenes.add(WorldAsset::new(World::new())));
        }
        let mut materials = Assets::<StandardMaterial>::default();
        PropAssets {
            cube: Handle::default(), sphere: Handle::default(), cylinder: Handle::default(), cone: Handle::default(),
            paints: HashMap::new(), metals: HashMap::new(), blueprint: materials.add(StandardMaterial::default()),
            blanket_free: materials.add(StandardMaterial::default()), models,
        }
    }

    #[test]
    fn plant_plots_wait_for_every_instance_and_cleanup_with_their_root() {
        let a = test_assets();
        let mut world = World::new();
        let variant = &PlantId::named("corn").def().stages[3].variants[0];
        let transform = Transform::from_xyz(2.0, 1.0, 3.0).with_scale(Vec3::splat(0.9));
        let root = spawn_plant_look(&mut world.commands(), &a, transform, variant);
        world.flush();
        assert_eq!(world.get::<Transform>(root), Some(&transform));
        let group = world.get::<PlantModels>(root).unwrap();
        let (fallback, scenes) = (group.fallback, group.scenes.clone());
        assert_eq!(scenes.len(), 4);
        let handles: Vec<_> = scenes.iter().map(|e| world.get::<WorldAssetRoot>(*e).unwrap().0.clone()).collect();
        assert!(handles.windows(2).all(|w| w[0] == w[1]));
        let mut spawner = bevy::world_serialization::WorldInstanceSpawner::default();
        for (k, (&entity, handle)) in scenes.iter().zip(handles).enumerate() {
            world.trigger(WorldInstanceReady { entity, instance_id: spawner.spawn(handle) });
            world.flush();
            let complete = k == scenes.len() - 1;
            assert_eq!(world.get::<Visibility>(fallback), Some(&if complete { Visibility::Hidden } else { Visibility::Inherited }));
            for &scene in &scenes {
                assert_eq!(world.get::<Visibility>(scene), Some(&if complete { Visibility::Inherited } else { Visibility::Hidden }));
            }
        }
        world.despawn(root);
        assert!(world.get_entity(fallback).is_err());
        assert!(scenes.iter().all(|&e| world.get_entity(e).is_err()));
        // A missing model description uses the original shape-only path.
        let mut shapes = variant.clone();
        shapes.model = None;
        spawn_plant_look(&mut world.commands(), &a, transform, &shapes);
        world.flush();
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0);
    }

    #[test]
    fn modelled_plants_follow_growth_caching_visibility_and_removal() {
        use bevy::ecs::system::RunSystemOnce;
        let mut sim = Sim::new(None);
        let mut w = colcom::world::World::with_map(Map::flat(8, 8), colcom::rng::Rng::new(1));
        let at = (3, 3, Z);
        let i = w.map.idx(at);
        let kind = PlantId::named("corn");
        w.map.set_feature(i, Feature::Plant { kind, growth: 0.0 });
        sim.replace(w);
        let mut world = World::new();
        world.insert_resource(sim);
        world.insert_resource(UiState::new());
        world.insert_resource(Props::default());
        world.insert_resource(test_assets());
        world.insert_resource(Assets::<StandardMaterial>::default());
        world.insert_resource(SoundQueue::default());
        world.run_system_once(sync_props).unwrap();
        let first = world.resource::<Props>().features[i].unwrap().1;
        world.run_system_once(sync_props).unwrap();
        assert_eq!(world.resource::<Props>().features[i].unwrap().1, first, "idle frames retain the plot");
        world.resource_mut::<Sim>().world.map.set_feature(i, Feature::Plant { kind, growth: 0.24 });
        world.run_system_once(sync_props).unwrap();
        assert_eq!(world.resource::<Props>().features[i].unwrap().1, first, "growth inside a stage retains the plot");
        world.resource_mut::<Sim>().world.map.set_feature(i, Feature::Plant { kind, growth: 0.25 });
        world.run_system_once(sync_props).unwrap();
        let next = world.resource::<Props>().features[i].unwrap().1;
        assert_ne!(first, next);
        assert!(world.get_entity(first).is_err());
        world.resource_mut::<UiState>().view_level = Some(Z - 1);
        world.run_system_once(sync_props).unwrap();
        assert_eq!(world.get::<Visibility>(next), Some(&Visibility::Hidden));
        world.resource_mut::<Sim>().world.map.set_feature(i, Feature::None);
        world.run_system_once(sync_props).unwrap();
        assert!(world.get_entity(next).is_err());
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0);
    }

    #[test]
    fn cached_state_scenes_follow_new_fence_neighbours_and_burials() {
        use bevy::ecs::system::RunSystemOnce;
        use colcom::pawn::Life;
        let mut w = colcom::world::World::with_map(Map::flat(8, 8), colcom::rng::Rng::new(1));
        let fence = w.map.idx((3, 3, Z));
        let grave = w.map.idx((1, 1, Z));
        w.map.set_feature(fence, Feature::built("fence"));
        w.map.set_feature(grave, Feature::built("grave"));
        let id = w.spawn_colonist((1, 1, Z));
        w.pawns[0].life = Life::Dead { buried: false };
        let mut sim = Sim::new(None);
        sim.replace(w);
        let mut world = World::new();
        world.insert_resource(sim);
        world.insert_resource(UiState::new());
        world.insert_resource(Props::default());
        world.insert_resource(test_assets());
        world.insert_resource(Assets::<StandardMaterial>::default());
        world.insert_resource(SoundQueue::default());
        world.run_system_once(sync_props).unwrap();
        let old_fence = world.resource::<Props>().features[fence].unwrap().1;
        let old_grave = world.resource::<Props>().features[grave].unwrap().1;
        {
            let mut sim = world.resource_mut::<Sim>();
            let i = sim.world.map.idx((4, 3, Z));
            sim.world.map.set_feature(i, Feature::built("fence"));
        }
        world.run_system_once(sync_props).unwrap();
        let (key, new_fence) = world.resource::<Props>().features[fence].unwrap();
        assert!(matches!(key, PropKey::Built { links: 1, .. }));
        assert_ne!(new_fence, old_fence);
        assert!(world.get_entity(old_fence).is_err());
        let revision = world.resource::<Sim>().world.map.changes();
        world.resource_mut::<Sim>().world.pawns.iter_mut().find(|p| p.id == id).unwrap().life = Life::Dead { buried: true };
        world.run_system_once(sync_props).unwrap();
        let (key, new_grave) = world.resource::<Props>().features[grave].unwrap();
        assert!(matches!(key, PropKey::Built { open: false, .. }));
        assert_ne!(new_grave, old_grave);
        assert!(world.get_entity(old_grave).is_err());
        assert_eq!(world.resource::<Sim>().world.map.changes(), revision);
        let a = world.resource::<PropAssets>();
        let expected = a.model(&BuildId::named("grave").def().model.as_ref().unwrap().path, 1);
        let descendants: Vec<_> = world.get::<Children>(new_grave).unwrap().iter().collect();
        assert!(descendants.iter().any(|&e| world.get::<WorldAssetRoot>(e).is_some_and(|s| s.0 == expected)));
    }

    #[test]
    fn stateful_build_models_select_scenes_and_keep_blueprint_styles() {
        let a = test_assets();
        let mut world = World::new();
        for (name, open, links, expected) in [("grave", true, 0, 0), ("grave", false, 0, 1),
            ("fence", false, 0, 0), ("fence", false, 5, 5), ("fence", false, 15, 15)] {
            let def = BuildId::named(name).def();
            let root = spawn_build_look(&mut world.commands(), &a, def, Transform::default(),
                Pose { open, links, ghost: true, ..default() }, &a.blanket_free);
            world.flush();
            let scene = world.query::<(&WorldAssetRoot, &BuildStyle)>().iter(&world).next().unwrap();
            assert_eq!(scene.0.0, a.model(&def.model.as_ref().unwrap().path, expected));
            assert_eq!(scene.1.ghost, Some(a.blueprint.clone()));
            world.despawn(root);
        }
    }

    #[test]
    fn cached_props_follow_map_edits_pawns_visibility_and_world_replacement() {
        use bevy::ecs::system::RunSystemOnce;
        use colcom::rng::Rng;
        use colcom::world::World as SimWorld;
        let mut w = SimWorld::with_map(Map::flat(8, 8), Rng::new(1));
        let door = (3, 3, Z);
        let di = w.map.idx(door);
        w.map.set_feature(di, Feature::built("door"));
        let id = w.spawn_colonist((0, 0, Z));
        let mut sim = Sim::new(None);
        sim.replace(w);
        let mut world = World::new();
        world.insert_resource(sim);
        world.insert_resource(UiState::new());
        world.insert_resource(Props::default());
        world.insert_resource(test_assets());
        world.insert_resource(Assets::<StandardMaterial>::default());
        world.insert_resource(SoundQueue::default());
        world.run_system_once(sync_props).unwrap();
        let original = world.resource::<Props>().features[di].unwrap().1;
        world.run_system_once(sync_props).unwrap();
        assert_eq!(world.resource::<Props>().features[di].unwrap().1, original);
        // Movement changes a door's appearance without changing the map.
        let revision = world.resource::<Sim>().world.map.changes();
        {
            let mut sim = world.resource_mut::<Sim>();
            let p = sim.world.pawns.iter_mut().find(|p| p.id == id).unwrap();
            p.x = door.0 as f32;
            p.y = door.1 as f32;
        }
        world.run_system_once(sync_props).unwrap();
        let (key, open) = world.resource::<Props>().features[di].unwrap();
        assert!(matches!(key, PropKey::Built { open: true, .. }));
        assert_ne!(open, original);
        assert_eq!(world.resource::<Sim>().world.map.changes(), revision);
        world.resource_mut::<UiState>().view_level = Some(Z - 1);
        world.run_system_once(sync_props).unwrap();
        assert_eq!(world.get::<Visibility>(open), Some(&Visibility::Hidden));
        world.resource_mut::<Sim>().world.map.set_feature(di, Feature::built("shelf"));
        world.run_system_once(sync_props).unwrap();
        let shelf = world.resource::<Props>().features[di].unwrap().1;
        assert_ne!(shelf, open);
        assert_eq!(world.get::<Visibility>(shelf), Some(&Visibility::Hidden));
        let replacement = SimWorld::with_map(Map::flat(8, 8), Rng::new(2));
        world.resource_mut::<Sim>().replace(replacement);
        world.run_system_once(sync_props).unwrap();
        assert!(world.resource::<Props>().features[di].is_none());
        assert!(world.get_entity(shelf).is_err());
    }

    #[test]
    fn furniture_shares_models_and_preserves_ghosts_owner_colors_and_lights() {
        let a = test_assets();
        let mut world = World::new();
        let def = BuildId::named("bed").def();
        let built = spawn_build_look(&mut world.commands(), &a, def, Transform::default(), Pose::default(), &a.blanket_free);
        let ghost = spawn_build_look(&mut world.commands(), &a, def, Transform::default(), Pose { ghost: true, ..default() }, &a.blanket_free);
        world.flush();
        let mut count = 0;
        for (scene, style) in world.query::<(&WorldAssetRoot, &BuildStyle)>().iter(&world) {
            assert_eq!(scene.0, a.models[&model_key(&def.model.as_ref().unwrap().path, 0)]);
            if style.ghost.is_some() {
                for name in ["Timber", "Owner fabric", "Linen"] {
                    assert_eq!(style.material(name), Some(a.blueprint.clone()));
                }
            } else {
                assert_eq!(style.material("Owner fabric"), Some(a.blanket_free.clone()));
                assert!(style.material("Timber").is_none());
                assert!(style.material("Linen").is_none(), "pillows keep their original linen color");
            }
            count += 1;
        }
        assert_eq!(count, 2);
        // Exercise the ready observers with nested glTF surfaces, as the loader supplies them.
        let scenes: Vec<_> = world.query::<(Entity, &WorldAssetRoot)>().iter(&world).map(|(e, scene)| (e, scene.0.clone())).collect();
        let mut spawner = bevy::world_serialization::WorldInstanceSpawner::default();
        for (scene, handle) in scenes {
            let node = world.spawn(ChildOf(scene)).id();
            let fabric = world.spawn((ChildOf(node), GltfMaterialName("Owner fabric".into()), MeshMaterial3d::<StandardMaterial>(Handle::default()))).id();
            let linen = world.spawn((ChildOf(node), GltfMaterialName("Linen".into()), MeshMaterial3d::<StandardMaterial>(Handle::default()))).id();
            world.trigger(WorldInstanceReady { entity: scene, instance_id: spawner.spawn(handle) });
            world.flush();
            let style = world.get::<BuildStyle>(scene).unwrap();
            assert_eq!(world.get::<MeshMaterial3d<StandardMaterial>>(fabric).unwrap().0, style.ghost.clone().unwrap_or_else(|| a.blanket_free.clone()));
            assert_eq!(world.get::<MeshMaterial3d<StandardMaterial>>(linen).unwrap().0, style.ghost.clone().unwrap_or_default());
            let fallback = world.get::<ModelFallback>(scene).unwrap().0;
            assert_eq!(world.get::<Visibility>(fallback), Some(&Visibility::Hidden));
        }
        world.despawn(built);
        world.despawn(ghost);
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0);
        let lamp = BuildId::named("lamp");
        let pos = (3, 3, Z);
        spawn_feature(&mut world.commands(), &a, PropKey::Blueprint { kind: lamp, dir: colcom::map::Dir::PlusX, over: false, snug: (0.0, 0.0), plant: None }, pos, 0.0, a.blanket_free.clone());
        world.flush();
        assert_eq!(world.query::<&PointLight>().iter(&world).count(), 0, "previews do not cast light");
        spawn_feature(&mut world.commands(), &a, PropKey::Built { kind: lamp, dir: colcom::map::Dir::PlusX, owner: None, open: false, links: 0, snug: (0.0, 0.0) }, pos, 0.0, a.blanket_free.clone());
        world.flush();
        let light = world.query::<&PointLight>().single(&world).unwrap();
        assert_eq!(light.intensity, lamp.def().light.as_ref().unwrap().intensity);
        assert_eq!(light.range, lamp.def().light.as_ref().unwrap().range);
    }

    #[test]
    fn item_models_share_handles_and_equipping_removes_old_children() {
        let a = test_assets();
        let mut world = World::new();
        let rifle = ItemId::named("rifle");
        let held = world.spawn((Transform::default(), Visibility::Hidden)).id();
        let stored = {
            let mut commands = world.commands();
            super::super::pawns::equip_visual(&mut commands, &a, held, Some(rifle), true);
            spawn_item_look(&mut commands, &a, rifle, Transform::default())
        };
        world.flush();
        let models: Vec<_> = world.query::<&WorldAssetRoot>().iter(&world).map(|model| model.0.id()).collect();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0], models[1], "stored and equipped use the same asset");
        for fallback in world.query::<&ModelFallback>().iter(&world) {
            assert_eq!(world.get::<Visibility>(fallback.0), Some(&Visibility::Inherited), "fallback stays visible until loaded");
        }
        assert_eq!(world.get::<Visibility>(held), Some(&Visibility::Hidden), "equipping preserves visibility");
        for kind in ItemId::all().filter(|k| k.def().weapon.is_some()) {
            for in_hand in [true, false] {
                let old_children: Vec<_> = world.get::<Children>(held).unwrap().iter().collect();
                super::super::pawns::equip_visual(&mut world.commands(), &a, held, Some(kind), in_hand);
                world.flush();
                assert!(old_children.iter().all(|e| world.get_entity(*e).is_err()));
                assert_eq!(world.get::<Children>(held).unwrap().len(), 2, "a model and fallback for {kind:?}");
                assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 2, "only stored and equipped models remain");
            }
        }
        super::super::pawns::equip_visual(&mut world.commands(), &a, held, None, true);
        world.flush();
        assert!(world.get::<Children>(held).is_none_or(|c| c.is_empty()), "unequipping removes the visual");
        world.despawn(stored);
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0, "despawning removes the scene too");
        let wood = spawn_item_look(&mut world.commands(), &a, ItemId::named("wood"), Transform::default());
        world.flush();
        assert!(world.get::<Children>(wood).is_some());
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 1, "resources now use their own models too");
    }

    #[test]
    fn carried_items_share_models_and_clear_old_stack_visuals() {
        let a = test_assets();
        let mut world = World::new();
        let pose = Transform::from_xyz(0.0, 0.51, 0.27).with_scale(Vec3::splat(0.5));
        let carrier = world.spawn((pose, Visibility::Hidden)).id();
        for kind in ItemId::all() {
            let old = world.get::<Children>(carrier).map(|c| c.iter().collect::<Vec<_>>()).unwrap_or_default();
            replace_carried_item(&mut world.commands(), &a, carrier, Some(kind));
            world.flush();
            assert!(old.iter().all(|e| world.get_entity(*e).is_err()), "old carried stack was removed");
            assert_eq!(world.get::<Children>(carrier).unwrap().len(), 1);
            let scene = world.query::<&WorldAssetRoot>().single(&world).unwrap();
            let model = kind.def().model.as_ref().unwrap();
            assert_eq!(scene.0, a.models[&model_key(&model.path, 0)], "carrying shares the stored asset for {kind:?}");
            assert_eq!(world.get::<Transform>(carrier).unwrap(), &pose);
            assert_eq!(world.get::<Visibility>(carrier), Some(&Visibility::Hidden));
        }
        replace_carried_item(&mut world.commands(), &a, carrier, None);
        world.flush();
        assert!(world.get::<Children>(carrier).is_none_or(|c| c.is_empty()));
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0);
    }

    #[test]
    fn worn_apparel_scenes_keep_fallbacks_and_despawn_with_their_owner() {
        let a = test_assets();
        let mut world = World::new();
        for kind in ItemId::all().filter(|k| k.def().apparel.is_some()) {
            let owner = world.spawn((Transform::default(), Visibility::default())).id();
            let pieces = &kind.def().apparel.as_ref().unwrap().pieces;
            for piece in pieces {
                let at = Transform::from_rotation(Quat::from_rotation_x(0.3));
                let root = spawn_worn(&mut world.commands(), &a, at, piece);
                world.commands().entity(root).insert(ChildOf(owner));
            }
            world.flush();
            assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), pieces.len());
            for fallback in world.query::<&ModelFallback>().iter(&world) {
                assert_eq!(world.get::<Visibility>(fallback.0), Some(&Visibility::Inherited));
            }
            world.despawn(owner);
            assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0, "{kind:?} leaves no old bone pieces");
        }
        let mut cloth = ItemId::named("parka").def().apparel.as_ref().unwrap().pieces[0].clone();
        cloth.model = None;
        spawn_worn(&mut world.commands(), &a, Transform::default(), &cloth);
        world.flush();
        assert_eq!(world.query::<&WorldAssetRoot>().iter(&world).count(), 0, "ordinary clothing keeps its shape visuals");
        assert!(world.query::<&Mesh3d>().iter(&world).count() > 0);
    }

    #[test]
    fn item_assets_fit_on_the_floor_and_stay_within_budget() {
        let mut checked = 0;
        for kind in ItemId::all() {
            let model = kind.def().model.as_ref().expect("all inventory items have a model");
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(&model.path);
            let gltf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let buffer = std::fs::read(path.with_file_name(gltf["buffers"][0]["uri"].as_str().unwrap())).unwrap();
            let pose = model_transform(&model.ground);
            let mut triangles = 0;
            for mesh in gltf["meshes"].as_array().unwrap() {
                for primitive in mesh["primitives"].as_array().unwrap() {
                    triangles += gltf["accessors"][primitive["indices"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() / 3;
                }
            }
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            let mut stack: Vec<_> = gltf["scenes"][0]["nodes"].as_array().unwrap().iter().map(|n| (n.as_u64().unwrap() as usize, Vec3::ZERO)).collect();
            let mut stored_meshes = HashSet::new();
            while let Some((index, offset)) = stack.pop() {
                let node = &gltf["nodes"][index];
                let at = node["translation"].as_array().map_or(Vec3::ZERO, |v| Vec3::new(v[0].as_f64().unwrap() as f32, v[1].as_f64().unwrap() as f32, v[2].as_f64().unwrap() as f32)) + offset;
                for child in node["children"].as_array().into_iter().flatten() {
                    stack.push((child.as_u64().unwrap() as usize, at));
                }
                let Some(mesh) = node["mesh"].as_u64() else { continue };
                stored_meshes.insert(mesh);
                for primitive in gltf["meshes"][mesh as usize]["primitives"].as_array().unwrap() {
                    let positions = &gltf["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
                    let view = &gltf["bufferViews"][positions["bufferView"].as_u64().unwrap() as usize];
                    let offset = view["byteOffset"].as_u64().unwrap() as usize;
                    for vertex in buffer[offset..offset + positions["count"].as_u64().unwrap() as usize * 12].as_chunks::<12>().0 {
                        let xyz: Vec<f32> = vertex.as_chunks::<4>().0.iter().map(|v| f32::from_le_bytes(*v)).collect();
                        let p = pose.transform_point(Vec3::new(xyz[0], xyz[1], xyz[2]) + at);
                        assert!(p.is_finite());
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
            let budget = if model.path.starts_with("models/items/") { 1000 } else { 2000 };
            assert!((1..=budget).contains(&triangles), "{kind:?}: {triangles} triangles");
            assert!(gltf["materials"].as_array().unwrap().len() <= 5);
            assert!(gltf.get("textures").is_none());
            assert!(min.y >= -1e-6 && min.y < 0.01, "{kind:?} rests above the surface: {min:?}");
            assert!(min.x >= -0.5 && max.x <= 0.5 && min.z >= -0.5 && max.z <= 0.5, "{kind:?} fits inside one tile: {min:?} {max:?}");
            for piece in kind.def().apparel.iter().flat_map(|a| &a.pieces) {
                let worn = piece.model.as_ref().expect("armor has a model on every bone piece");
                assert_eq!(worn.path, model.path, "stored and worn share one glTF and its materials");
                let nodes = gltf["scenes"][worn.scene]["nodes"].as_array().expect("a bone scene exists");
                for n in nodes {
                    let node = &gltf["nodes"][n.as_u64().unwrap() as usize];
                    assert!(stored_meshes.contains(&node["mesh"].as_u64().unwrap()), "bone pieces reuse the stored meshes");
                }
            }
            checked += 1;
        }
        assert_eq!(checked, defs().items.len(), "every inventory item has been checked");
    }
    #[test]
    fn tree_models_keep_authored_heights_and_anchor_trunks_at_cell_centres() {
        let tree = PlantId::named("tree").def();
        for (variant, height) in tree.stages[0].variants.iter().zip([1.46, 1.46, 1.46, 1.6, 1.6]) {
            let model = variant.model.as_ref().unwrap();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(&model.path);
            let g: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let bytes = std::fs::read(path.with_extension("bin")).unwrap();
            let mut points = Vec::new();
            for mesh in g["meshes"].as_array().unwrap() {
                for p in mesh["primitives"].as_array().unwrap() {
                    let a = &g["accessors"][p["attributes"]["POSITION"].as_u64().unwrap() as usize];
                    let v = &g["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
                    let start = (v["byteOffset"].as_u64().unwrap_or(0) + a["byteOffset"].as_u64().unwrap_or(0)) as usize;
                    for row in bytes[start..start + a["count"].as_u64().unwrap() as usize * 12].as_chunks::<4>().0.as_chunks::<3>().0 {
                        points.push(Vec3::new(f32::from_le_bytes(row[0]), f32::from_le_bytes(row[1]), f32::from_le_bytes(row[2])));
                    }
                }
            }
            let bottom = points.iter().map(|p| p.y).reduce(f32::min).unwrap();
            let top = points.iter().map(|p| p.y).reduce(f32::max).unwrap();
            let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for &p in points.iter().filter(|p| p.y < bottom + 0.02) {
                min = min.min(p);
                max = max.max(p);
            }
            let pose = model_transform(&model.placements[0]);
            let foot = pose.transform_point(Vec3::new((min.x + max.x) / 2.0, bottom, (min.z + max.z) / 2.0));
            assert!(foot.length() < 1e-5, "{} trunk: {foot:?}", model.path);
            assert!(((top - bottom) * pose.scale.y - height).abs() < 1e-5, "{} height", model.path);
        }
    }

    #[test]
    fn world_prop_models_keep_footprints_back_reach_and_scene_references() {
        use colcom::defs::BuildScenes;
        let mut checked = 0;
        for def in defs().builds.iter().filter(|d| d.model.as_ref().is_some_and(|m| m.path.starts_with("models/world/"))) {
            let model = def.model.as_ref().unwrap();
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(&model.path);
            let g: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let data = std::fs::read(path.with_extension("bin")).unwrap();
            for scene in model.scenes.all() {
                assert!(g["scenes"].get(scene).is_some(), "{}: scene {scene}", def.id);
            }
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for mesh in g["meshes"].as_array().unwrap() {
                for primitive in mesh["primitives"].as_array().unwrap() {
                    let a = &g["accessors"][primitive["attributes"]["POSITION"].as_u64().unwrap() as usize];
                    let v = &g["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
                    let offset = v["byteOffset"].as_u64().unwrap_or(0) as usize;
                    for bytes in data[offset..offset + a["count"].as_u64().unwrap() as usize * 12].as_chunks::<12>().0 {
                        let p = model_transform(&model.pose).transform_point(Vec3::new(
                            f32::from_le_bytes(bytes[0..4].try_into().unwrap()),
                            f32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                            f32::from_le_bytes(bytes[8..12].try_into().unwrap())));
                        min = min.min(p);
                        max = max.max(p);
                    }
                }
            }
            assert!(min.x >= -0.501 && max.x <= 0.501 && min.z >= -0.501 && max.z <= 0.501, "{}: {min:?} {max:?}", def.id);
            if def.turnable() && def.special != Special::Stairs {
                let (x, z) = def.front.opposite().offset();
                let back = Vec3::new(x as f32, 0.0, z as f32);
                let actual = (if x > 0 || z > 0 { max } else { min }).dot(back);
                let expected = def.parts().iter().map(|p| p.at.0 * back.x + p.at.2 * back.z + if x != 0 { p.size.0 / 2.0 } else { p.size.2 / 2.0 }).fold(0.0f32, f32::max);
                assert!((actual - expected).abs() < 0.01, "{}: back reach {actual} vs {expected}", def.id);
            }
            assert!(matches!((&model.scenes, def.special), (BuildScenes::Filled, Special::Grave)
                | (BuildScenes::Connected, Special::None) | (BuildScenes::Static, Special::None | Special::Stairs)), "{}: scene semantics", def.id);
            checked += 1;
        }
        assert_eq!(checked, 20);
    }

    #[test]
    #[ignore = "manual CPU-only prop cache benchmark: run with --ignored --nocapture"]
    fn benchmark_dense_world_prop_sync() {
        use std::time::Instant;
        // Identical 100x70 colony, including every crop stage and all world props.
        // Empty scene assets isolate CPU spawning/cache work; this does not time
        // glTF decoding, loaded mesh propagation or GPU rendering.
        for models in [false, true] {
            let mut w = colcom::world::World::with_map(Map::flat(100, 70), colcom::rng::Rng::new(42));
            let kinds: Vec<_> = PlantId::all().collect();
            let builds: Vec<_> = BuildId::all().filter(|k| k.def().model.as_ref().is_some_and(|m| m.path.starts_with("models/world/"))).collect();
            for y in 1..69 {
                for x in 1..99 {
                    let i = w.map.idx((x, y, Z));
                    let feature = if x % 10 == 0 && y % 5 == 0 {
                        Feature::Built { kind: builds[(x + y) % builds.len()], dir: Dir::PlusX }
                    } else {
                        let kind = kinds[(x + y) % kinds.len()];
                        let growth = kind.def().stages[(x * 3 + y) % kind.def().stages.len()].from;
                        Feature::Plant { kind, growth }
                    };
                    w.map.set_feature(i, feature);
                }
            }
            let a = test_assets();
            let mut world = World::new();
            let start = Instant::now();
            let mut props = Props { features: vec![None; w.map.len()], items: vec![None; w.map.len()],
                changes: Some(w.map.changes()), ..default() };
            for y in 1..69 {
                for x in 1..99 {
                    let pos = (x, y, Z);
                    let i = w.map.idx(pos);
                    let key = prop_key(&w, i, &HashMap::new(), &HashSet::new()).unwrap();
                    let transform = Transform::from_translation(tile_center(pos));
                    let entity = match key {
                        PropKey::Plant { kind, stage } => {
                            let variants = &kind.def().stages[stage as usize].variants;
                            let mut variant = variants[(hash(x as i32, y as i32, 23) * variants.len() as f32) as usize % variants.len()].clone();
                            if !models { variant.model = None; }
                            let (lo, hi) = kind.def().scale;
                            let transform = transform.with_rotation(Quat::from_rotation_y(hash(x as i32, y as i32, 22) * TAU))
                                .with_scale(Vec3::splat(lo + hash(x as i32, y as i32, 20) * (hi - lo)));
                            spawn_plant_look(&mut world.commands(), &a, transform, &variant)
                        }
                        PropKey::Built { kind, dir, open, links, .. } => {
                            let mut def = kind.def().clone();
                            if !models { def.model = None; }
                            spawn_build_look(&mut world.commands(), &a, &def, transform.with_rotation(facing(dir)),
                                Pose { dir, open, links, ..default() }, &a.blanket_free)
                        }
                        PropKey::Blueprint { .. } => unreachable!("fixture has finished props"),
                    };
                    props.features[i] = Some((key, entity));
                }
            }
            world.flush();
            let spawn = start.elapsed();
            let mut sim = Sim::new(None);
            sim.replace(w);
            props.generation = Some(sim.generation);
            world.insert_resource(sim);
            world.insert_resource(props);
            world.insert_resource(a);
            world.insert_resource(UiState::new());
            world.insert_resource(Assets::<StandardMaterial>::default());
            world.insert_resource(SoundQueue::default());
            let system = world.register_system(sync_props);
            for _ in 0..20 { world.run_system(system).unwrap(); }
            let initial_entities = world.query::<Entity>().iter(&world).count();
            let start = Instant::now();
            for _ in 0..1000 { world.run_system(system).unwrap(); }
            let idle = start.elapsed().as_secs_f64() * 1e6 / 1000.0;
            assert_eq!(world.query::<Entity>().iter(&world).count(), initial_entities, "idle sync must not rebuild props");
            eprintln!("models={models}: 6664 props, spawn {:.2} ms; idle sync {idle:.2} us; {initial_entities} entities", spawn.as_secs_f64() * 1e3);
        }
    }

    #[test]
    fn furniture_models_preserve_placement_storage_and_contact_surfaces() {
        let mut checked = 0;
        for def in defs().builds.iter().filter(|d| matches!(d.category, BuildCategory::Furniture | BuildCategory::WallDecor | BuildCategory::Light)) {
            let model = def.model.as_ref().expect("all room furnishings have a model");
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(&model.path);
            let gltf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let buffer = std::fs::read(path.with_file_name(gltf["buffers"][0]["uri"].as_str().unwrap())).unwrap();
            let vectors = |index: &serde_json::Value| -> Vec<Vec3> {
                let accessor = &gltf["accessors"][index.as_u64().unwrap() as usize];
                let view = &gltf["bufferViews"][accessor["bufferView"].as_u64().unwrap() as usize];
                let offset = (view["byteOffset"].as_u64().unwrap_or(0) + accessor["byteOffset"].as_u64().unwrap_or(0)) as usize;
                buffer[offset..offset + accessor["count"].as_u64().unwrap() as usize * 12].as_chunks::<12>().0.iter().map(|v| {
                    Vec3::new(f32::from_le_bytes(v[0..4].try_into().unwrap()), f32::from_le_bytes(v[4..8].try_into().unwrap()), f32::from_le_bytes(v[8..12].try_into().unwrap()))
                }).collect()
            };
            let pose = model_transform(&model.pose);
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            let mut tops = Vec::new();
            let mut triangles = 0;
            for mesh in gltf["meshes"].as_array().unwrap() {
                for primitive in mesh["primitives"].as_array().unwrap() {
                    triangles += gltf["accessors"][primitive["indices"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() / 3;
                    for (p, normal) in vectors(&primitive["attributes"]["POSITION"]).into_iter().zip(vectors(&primitive["attributes"]["NORMAL"])) {
                        let p = pose.transform_point(p);
                        assert!(p.is_finite());
                        min = min.min(p);
                        max = max.max(p);
                        if (pose.rotation * normal).y > 0.999 { tops.push(p.y); }
                    }
                }
            }
            assert!((1..=2000).contains(&triangles), "{}: {triangles} triangles", def.id);
            let materials = gltf["materials"].as_array().unwrap();
            assert!(materials.len() <= 5 && gltf.get("textures").is_none());
            if let Some(name) = &model.owner_material {
                assert!(materials.iter().any(|m| m["name"].as_str() == Some(name)));
            }
            assert!(min.y >= -1e-6 && max.y <= 0.921, "{} stays below the ceiling: {min:?} {max:?}", def.id);
            assert!(min.x >= -0.5 && max.x <= 0.5 && min.z >= -0.5 && max.z <= 0.5, "{} stays in its tile", def.id);
            if def.on_wall {
                assert!(min.z >= -1e-6 && min.x >= -0.1 && max.x <= 0.1, "{} fits a wall slot", def.id);
            } else {
                assert!(min.y < 0.01, "{} rests on the floor", def.id);
                if def.turnable() && def.special != Special::Seat {
                    let (x, z) = def.front.opposite().offset();
                    let back = Vec3::new(x as f32, 0.0, z as f32);
                    let actual = (if x > 0 || z > 0 { max } else { min }).dot(back);
                    let expected = def.parts().iter().map(|p| p.at.0 * back.x + p.at.2 * back.z + if x != 0 { p.size.0 / 2.0 } else { p.size.2 / 2.0 }).fold(0.0f32, f32::max);
                    assert!((actual - expected).abs() < 0.01, "{} keeps its wall contact: {actual} vs {expected}", def.id);
                }
            }
            let contact = match def.id.as_str() { "chair" => Some(0.325), "bed" => Some(0.33), "table" => Some(0.53), "operating_table" => Some(0.475), _ => None };
            for height in contact.into_iter().chain(def.storage.iter().flat_map(|s| std::iter::once(s.height).chain(s.lower))) {
                assert!(tops.iter().any(|y| (y - height).abs() < 0.001), "{} supports its users or stored items at {height}", def.id);
            }
            checked += 1;
        }
        assert_eq!(checked, 20);
    }

    #[test]
    fn furniture_is_drawn_back_against_the_wall_behind_it() {
        let mut m = Map::flat(8, 8);
        let bed = BuildId::named("bed");
        // Unturned, a bed's head (pillow) is towards -y.
        let at = (4, 4, Z);
        assert_eq!(snug(&m, at, bed, colcom::map::Dir::PlusX), Vec2::ZERO, "nothing behind it");
        let wall = m.idx((4, 3, Z));
        m.set_feature(wall, Feature::built("wall"));
        let s = snug(&m, at, bed, colcom::map::Dir::PlusX);
        assert!(s.x == 0.0 && s.y < -0.4, "pushed back to the wall: {:?}", s);
        // Its far end then just touches the wall's face.
        assert!((0.46 - s.y - (1.0 - colcom::decor::WALL_T / 2.0)).abs() < 0.02);
        assert_eq!(snug(&m, at, bed, colcom::map::Dir::PlusY), Vec2::ZERO, "turned, its head isn't at the wall");
        m.set_feature(wall, Feature::built("door"));
        assert_eq!(snug(&m, at, bed, colcom::map::Dir::PlusX), Vec2::ZERO, "not into a doorway");
        m.set_feature(wall, Feature::built("wall"));
        assert_eq!(snug(&m, at, BuildId::named("chair"), colcom::map::Dir::PlusX), Vec2::ZERO, "chairs stay by their table");
        assert_eq!(snug(&m, at, BuildId::named("stairs"), colcom::map::Dir::MinusX), Vec2::ZERO);
    }

    #[test]
    fn decor_sits_on_its_wall_face() {
        let wall = (3, 3, Z);
        // The +y face (world +z), first slot: towards -x, a wall's half thickness out.
        let t = decor_transform(wall, colcom::map::Dir::PlusY, 0, 0.0);
        let c = tile_center(wall);
        assert!((t.translation.z - (c.z + colcom::decor::WALL_T / 2.0)).abs() < 1e-4);
        assert!((t.translation.x - (c.x - 0.4)).abs() < 1e-4);
        assert!((t.rotation * Vec3::Z - Vec3::Z).length() < 1e-4, "looking out of the wall");
        // The -x face: looking -x, its slots running along +z.
        let t = decor_transform(wall, colcom::map::Dir::MinusX, 4, 0.0);
        assert!((t.rotation * Vec3::Z - Vec3::NEG_X).length() < 1e-4);
        assert!((t.translation.z - (c.z + 0.4)).abs() < 1e-4);
        assert!((t.rotation * Vec3::Y - Vec3::Y).length() < 1e-4, "upright");
    }

    #[test]
    fn turning_matches_how_models_are_turned() {
        for l in Dir::ALL {
            for dir in Dir::ALL {
                let (lx, ly) = l.offset();
                let v = facing(dir) * Vec3::new(lx as f32, 0.0, ly as f32);
                let (x, y) = l.turned(dir).offset();
                assert!((x as f32 - v.x).abs() < 0.01 && (y as f32 - v.z).abs() < 0.01, "{:?} turned {:?}", l, dir);
            }
        }
    }

    #[test]
    fn chairs_face_their_table() {
        let mut m = Map::flat(8, 8);
        let front = defs().builds[BuildId::named("chair").index()].front;
        let chair = (4, 4, Z);
        assert_eq!(seat_dir(&m, chair, front, Dir::MinusX), Dir::MinusX, "no table: as placed");
        let t = m.idx((3, 4, Z));
        m.set_feature(t, Feature::built("table"));
        let d = seat_dir(&m, chair, front, Dir::MinusX);
        assert_eq!(front.turned(d).offset(), (-1, 0), "towards the table");
        // Facing one of two tables already: left as it is.
        let t2 = m.idx((4, 5, Z));
        m.set_feature(t2, Feature::built("table"));
        let towards_second = Dir::ALL.into_iter().find(|&d| front.turned(d).offset() == (0, 1)).unwrap();
        assert_eq!(seat_dir(&m, chair, front, towards_second), towards_second);
    }
}
