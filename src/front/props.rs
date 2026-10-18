//! Per-tile 3D objects: plants, structures, blueprints and items, built from
//! the parts listed in their definitions (`assets/defs.ron`).
//!
//! Each frame the wanted object for every tile is compared with what is
//! spawned; changed tiles get their entity replaced.

use super::audio::SoundQueue;
use super::input::UiState;
use super::look::*;
use super::sim::Sim;
use super::terrain::{FLOOR_T, LEVEL_H};
use super::tile_center;
use bevy::prelude::*;
use my_world::defs::{defs, BuildId, BuildLook, ItemId, Part, PlantId, Shape, Special, Storage};
use my_world::map::{Feature, Map, Pos, DIRS};
use std::collections::{HashMap, HashSet};
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, PartialEq, Debug)]
enum PropKey {
    Plant { kind: PlantId, stage: u8 },
    /// Turned so the model's +x faces `DIRS[dir]`. `owner`: who sleeps in
    /// a bed. `open`: a door someone is passing through, or a grave nobody
    /// is buried in yet. `links`: bit per
    /// direction (`DIRS`) with a neighbour that linked parts join up with.
    Built { kind: BuildId, dir: u8, owner: Option<u32>, open: bool, links: u8 },
    /// `over`: around a wall still standing there (a slightly bigger ghost).
    Blueprint { kind: BuildId, dir: u8, over: bool },
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
    /// Layer view level and roof cut version the visibilities were last set for.
    view: Option<(usize, u64)>,
    /// Spawned objects per cell (flat arrays: cheaper than maps when scanning every cell each frame).
    features: Vec<Option<(PropKey, Entity)>>,
    items: Vec<Option<(ItemsKey, Entity)>>,
}

/// Shared unit meshes and materials for props (and carried items on pawns).
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
}

impl PropAssets {
    /// Material for a carried or stored item of this kind.
    pub fn item_material(&self, kind: ItemId) -> Handle<StandardMaterial> {
        self.paint(&kind.def().color)
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
            .add_systems(Update, (sync_props, flicker_lights));
    }
}

pub fn lit(materials: &mut Assets<StandardMaterial>, c: Rgb) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: c.color(), perceptual_roughness: 0.85, ..default() })
}

fn setup_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
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
    let assets = PropAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        sphere: meshes.add(Sphere::new(0.5)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        cone: meshes.add(Cone::new(0.5, 1.0)),
        paints,
        metals,
        blueprint,
        blanket_free: lit(&mut materials, Rgb(0.55, 0.58, 0.62)),
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

/// The world direction (index into `DIRS`) a model's direction `l` faces once turned by `dir`.
pub fn turned(l: u8, dir: u8) -> u8 {
    my_world::map::turn_dir(l, dir)
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
pub fn seat_dir(m: &Map, pos: Pos, front: u8, dir: u8) -> u8 {
    let table = |d: usize| {
        let (dx, dy) = DIRS[d];
        m.at(pos.0 as i32 + dx, pos.1 as i32 + dy, pos.2 as i32)
            .is_some_and(|t| m.feature[m.idx(t)].is_special(Special::Table))
    };
    if table(turned(front, dir) as usize) {
        return dir;
    }
    let Some(t) = (0..4).find(|&d| table(d)) else { return dir };
    (0..4u8).find(|&d| turned(front, d) as usize == t).unwrap_or(dir)
}

/// Rotation that turns +x towards `DIRS[dir]`.
fn facing(dir: u8) -> Quat {
    let (dx, dy) = DIRS[dir as usize % 4];
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
    let view = (ui.top_level(m), ui.roof_version);
    if props.generation != Some(sim.generation) || props.features.len() != m.len() {
        for (_, e) in props.features.drain(..).flatten() {
            commands.entity(e).despawn();
        }
        for (_, e) in props.items.drain(..).flatten() {
            commands.entity(e).despawn();
        }
        props.features = vec![None; m.len()];
        props.items = vec![None; m.len()];
        props.generation = Some(sim.generation);
        props.view = None;
    }

    // Layer view changed: show or hide everything by level.
    if props.view != Some(view) {
        for i in 0..m.len() {
            let vis = if hidden(m.pos(i)) { Visibility::Hidden } else { Visibility::Inherited };
            for e in [props.features[i].map(|f| f.1), props.items[i].map(|f| f.1)].into_iter().flatten() {
                commands.entity(e).insert(vis);
            }
        }
        props.view = Some(view);
    }

    let mut owners: HashMap<Pos, (u32, [f32; 3])> = HashMap::new();
    for p in w.pawns.iter().filter(|p| !p.dead) {
        if let Some(b) = p.bed {
            owners.insert(b, (p.id, p.color));
        }
    }
    let filled: HashSet<Pos> = w.pawns.iter().filter(|p| p.buried).map(|p| p.pos).collect();

    for i in 0..m.len() {
        let pos = m.pos(i);
        let want = match m.feature[i] {
            Feature::None => None,
            Feature::Plant { kind, growth } => Some(PropKey::Plant { kind, stage: kind.def().stage(growth) as u8 }),
            // To be built over a wall that's drawn with the terrain.
            Feature::Blueprint { kind, over: Some(_), .. } => Some(PropKey::Blueprint { kind, dir: 0, over: true }),
            // Drawn with the terrain.
            f if f.is_block() => None,
            Feature::Built { kind, dir } => Some({
                let def = kind.def();
                let owner = (def.special == Special::Bed).then(|| owners.get(&pos).map(|o| o.0)).flatten();
                let open = match def.special {
                    Special::Door => w.pawns.iter().any(|p| {
                        !p.dead
                            && (p.x - pos.0 as f32).abs() < 0.9
                            && (p.y - pos.1 as f32).abs() < 0.9
                            && (p.z - pos.2 as f32).abs() < 0.9
                    }),
                    Special::Grave => !filled.contains(&pos),
                    _ => false,
                };
                // Modelled spanning x; a quarter turn when the walls run along y.
                let dir = if def.along_walls {
                    if m.runs_along_x(pos) {
                        0
                    } else {
                        3
                    }
                } else if def.special == Special::Seat {
                    seat_dir(m, pos, def.front, dir)
                } else {
                    dir
                };
                let linked = def.parts().iter().any(|p| p.link.is_some());
                let links = if linked { links(m, pos, kind) } else { 0 };
                PropKey::Built { kind, dir, owner, open, links }
            }),
            Feature::Blueprint { kind, dir, .. } => {
                Some(PropKey::Blueprint { kind, dir: if kind.def().turnable() { dir } else { 0 }, over: false })
            }
        };
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
                    _ => false,
                };
                let lift = if m.floor[i] && !in_wall { FLOOR_T } else { 0.0 };
                let e = spawn_feature(&mut commands, &assets, key, pos, lift, blanket);
                if hidden(pos) {
                    commands.entity(e).insert(Visibility::Hidden);
                }
                props.features[i] = Some((key, e));
            }
        }

        let mut want: ItemsKey = [None; 4];
        for (slot, s) in want.iter_mut().zip(m.stacks(i)) {
            *slot = Some(ItemKey { kind: s.kind, big: s.count >= 25 });
        }
        let want_item = want[0].is_some().then_some(want);
        let have_item = props.items[i].map(|(k, _)| k);
        if want_item != have_item {
            if let Some((_, e)) = props.items[i].take() {
                commands.entity(e).despawn();
            }
            if let Some(key) = want_item {
                let storage = m.feature[i].structure().and_then(|d| d.storage.as_ref());
                let lift = if m.floor[i] { FLOOR_T } else { 0.0 };
                let e = match storage {
                    Some(st) if st.slots > 1 => spawn_shelf_items(&mut commands, &assets, key, pos, lift, st),
                    _ => spawn_item(&mut commands, &assets, key[0].unwrap(), pos, lift + storage.map_or(0.0, |s| s.height)),
                };
                if hidden(pos) {
                    commands.entity(e).insert(Visibility::Hidden);
                }
                props.items[i] = Some((key, e));
            }
        }
    }
}

/// How to show a set of parts.
#[derive(Clone, Copy, Default)]
struct Pose {
    /// All in the blueprint material.
    ghost: bool,
    /// Hinged parts swung open.
    open: bool,
    /// Turn of the model (index into `DIRS`), to match linked parts to neighbours.
    dir: u8,
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
            if p.link.is_some_and(|l| links & (1 << turned(l, dir)) == 0) {
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

/// Worn armour: `parts` (in metres from the bone, in the character's own
/// frame) under a root turned by `turn` (undoing the bone's own rest turn),
/// in polished metal.
pub fn spawn_worn(commands: &mut Commands, a: &PropAssets, turn: Quat, parts: &[Part]) -> Entity {
    let mut e = commands.spawn((Transform::from_rotation(turn), Visibility::default()));
    e.with_children(|c| {
        for p in parts {
            let t = part_transform(p);
            let mat = a.metals.get(&p.color).cloned().unwrap_or_else(|| a.paint(&p.color));
            c.spawn((Mesh3d(a.mesh(p.shape).clone()), MeshMaterial3d(mat), t));
        }
    });
    e.id()
}

fn spawn_feature(
    commands: &mut Commands,
    a: &PropAssets,
    key: PropKey,
    pos: Pos,
    lift: f32,
    blanket: Handle<StandardMaterial>,
) -> Entity {
    let base = Transform::from_translation(tile_center(pos) + Vec3::Y * lift);
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    match key {
        PropKey::Plant { kind, stage } => {
            let def = kind.def();
            let variants = &def.stages[stage as usize].variants;
            let parts = &variants[(hash(xi, yi, 23) * variants.len() as f32) as usize % variants.len()];
            let (lo, hi) = def.scale;
            let root = base
                .with_rotation(Quat::from_rotation_y(hash(xi, yi, 22) * TAU))
                .with_scale(Vec3::splat(lo + hash(xi, yi, 20) * (hi - lo)));
            spawn_parts(commands, a, root, parts, Pose::default(), &blanket)
        }
        PropKey::Built { kind, dir, open, links, .. } => {
            let def = kind.def();
            let parts = def.parts();
            let pose = Pose { ghost: false, open, dir, links };
            let e = spawn_parts(commands, a, base.with_rotation(facing(dir)), parts, pose, &blanket);
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
        PropKey::Blueprint { kind, dir, over } => match &kind.def().look {
            BuildLook::Parts(parts) if !over => {
                let pose = Pose { ghost: true, dir, ..default() };
                spawn_parts(commands, a, base.with_rotation(facing(dir)), parts, pose, &blanket)
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
        let e = spawn_parts(commands, a, t, &item.kind.def().look, Pose::default(), &a.blanket_free);
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
    spawn_parts(commands, a, root, &key.kind.def().look, Pose::default(), &a.blanket_free)
}

#[cfg(test)]
mod tests {
    use super::*;
    use my_world::map::FLAT_Z as Z;

    #[test]
    fn turning_matches_how_models_are_turned() {
        for l in 0..4u8 {
            for dir in 0..4u8 {
                let (lx, ly) = DIRS[l as usize];
                let v = facing(dir) * Vec3::new(lx as f32, 0.0, ly as f32);
                let (x, y) = DIRS[turned(l, dir) as usize];
                assert!((x as f32 - v.x).abs() < 0.01 && (y as f32 - v.z).abs() < 0.01, "{} turned {}", l, dir);
            }
        }
    }

    #[test]
    fn chairs_face_their_table() {
        let mut m = Map::flat(8, 8);
        let front = defs().builds[BuildId::named("chair").index()].front;
        let chair = (4, 4, Z);
        assert_eq!(seat_dir(&m, chair, front, 1), 1, "no table: as placed");
        let t = m.idx((3, 4, Z));
        m.set_feature(t, Feature::built("table"));
        let d = seat_dir(&m, chair, front, 1);
        assert_eq!(DIRS[turned(front, d) as usize], (-1, 0), "towards the table");
        // Facing one of two tables already: left as it is.
        let t2 = m.idx((4, 5, Z));
        m.set_feature(t2, Feature::built("table"));
        let towards_second = (0..4).find(|&d| DIRS[turned(front, d) as usize] == (0, 1)).unwrap();
        assert_eq!(seat_dir(&m, chair, front, towards_second), towards_second);
    }
}
