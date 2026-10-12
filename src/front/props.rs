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
use my_world::defs::{defs, BuildId, BuildLook, ItemId, Part, PlantId, Shape, Special};
use my_world::map::{Feature, Map, Pos, DIRS};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, PartialEq, Debug)]
enum PropKey {
    Plant { kind: PlantId, stage: u8 },
    /// Turned so the model's +x faces `DIRS[dir]`. `owner`: who sleeps in
    /// a bed. `open`: a door someone is passing through. `links`: bit per
    /// direction (`DIRS`) with a neighbour that linked parts join up with.
    Built { kind: BuildId, dir: u8, owner: Option<u32>, open: bool, links: u8 },
    Blueprint { kind: BuildId },
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct ItemKey {
    kind: ItemId,
    big: bool,
}

#[derive(Resource, Default)]
struct Props {
    generation: Option<u64>,
    /// Layer view level the visibilities were last set for.
    view: Option<usize>,
    /// Spawned objects per cell (flat arrays: cheaper than maps when scanning every cell each frame).
    features: Vec<Option<(PropKey, Entity)>>,
    items: Vec<Option<(ItemKey, Entity)>>,
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
    for (name, &(r, g, b)) in &d.palette {
        paints.insert(name.clone(), lit(&mut materials, Rgb(r, g, b)));
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
fn turned(l: u8, dir: u8) -> u8 {
    let (lx, ly) = DIRS[l as usize % 4];
    let v = facing(dir) * Vec3::new(lx as f32, 0.0, ly as f32);
    let found = DIRS.iter().position(|&(x, y)| (x as f32 - v.x).abs() < 0.5 && (y as f32 - v.z).abs() < 0.5);
    found.unwrap_or(l as usize) as u8
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
    let cut = ui.top_level(m);
    let hidden = |pos: Pos| pos.2 > cut;
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
    if props.view != Some(cut) {
        for i in 0..m.len() {
            let vis = if hidden(m.pos(i)) { Visibility::Hidden } else { Visibility::Inherited };
            for e in [props.features[i].map(|f| f.1), props.items[i].map(|f| f.1)].into_iter().flatten() {
                commands.entity(e).insert(vis);
            }
        }
        props.view = Some(cut);
    }

    let mut owners: HashMap<Pos, (u32, [f32; 3])> = HashMap::new();
    for p in w.pawns.iter().filter(|p| !p.dead) {
        if let Some(b) = p.bed {
            owners.insert(b, (p.id, p.color));
        }
    }

    for i in 0..m.len() {
        let pos = m.pos(i);
        let want = match m.feature[i] {
            Feature::None => None,
            Feature::Plant { kind, growth } => Some(PropKey::Plant { kind, stage: kind.def().stage(growth) as u8 }),
            // Drawn with the terrain.
            f if f.is_block() => None,
            Feature::Built { kind, dir } => Some({
                let def = kind.def();
                let owner = (def.special == Special::Bed).then(|| owners.get(&pos).map(|o| o.0)).flatten();
                let open = def.special == Special::Door && {
                    w.pawns.iter().any(|p| {
                        !p.dead
                            && (p.x - pos.0 as f32).abs() < 0.9
                            && (p.y - pos.1 as f32).abs() < 0.9
                            && (p.z - pos.2 as f32).abs() < 0.9
                    })
                };
                // Modelled spanning x; a quarter turn when the walls run along y.
                let dir = if def.along_walls {
                    if m.runs_along_x(pos) {
                        0
                    } else {
                        3
                    }
                } else {
                    dir
                };
                let linked = def.parts().iter().any(|p| p.link.is_some());
                let links = if linked { links(m, pos, kind) } else { 0 };
                PropKey::Built { kind, dir, owner, open, links }
            }),
            Feature::Blueprint { kind, .. } => Some(PropKey::Blueprint { kind }),
        };
        let have = props.features[i].map(|(k, _)| k);
        // A door swinging open.
        if let (Some(PropKey::Built { open: true, .. }), Some(PropKey::Built { open: false, .. })) = (want, have) {
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

        let want_item = m.items[i].map(|s| ItemKey { kind: s.kind, big: s.count >= 25 });
        let have_item = props.items[i].map(|(k, _)| k);
        if want_item != have_item {
            if let Some((_, e)) = props.items[i].take() {
                commands.entity(e).despawn();
            }
            if let Some(key) = want_item {
                let shelf = m.feature[i].structure().and_then(|d| d.storage.as_ref()).map_or(0.0, |s| s.height);
                let lift = if m.floor[i] { FLOOR_T } else { 0.0 } + shelf;
                let e = spawn_item(&mut commands, &assets, key, pos, lift);
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
            let mat = match p.color.as_str() {
                _ if ghost => a.blueprint.clone(),
                "owner" => owner.clone(),
                color => a.paint(color),
            };
            let (tx, ty, tz) = p.turn;
            let at = Vec3::new(p.at.0, p.at.1, p.at.2);
            let mut t = Transform::from_translation(at)
                .with_rotation(Quat::from_euler(EulerRot::XYZ, tx.to_radians(), ty.to_radians(), tz.to_radians()))
                .with_scale(Vec3::new(p.size.0, p.size.1, p.size.2));
            if p.hinge && open {
                // A quarter turn around its left edge.
                t.rotate_around(at - Vec3::X * p.size.0 / 2.0, Quat::from_rotation_y(-FRAC_PI_2));
            }
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
        PropKey::Blueprint { kind } => match &kind.def().look {
            BuildLook::Parts(parts) => {
                spawn_parts(commands, a, base, parts, Pose { ghost: true, ..default() }, &blanket)
            }
            BuildLook::Block | BuildLook::Opening { .. } => {
                let block = Part {
                    shape: Shape::Cube,
                    color: String::new(),
                    at: (0.0, LEVEL_H / 2.0, 0.0),
                    size: (0.98, LEVEL_H, 0.98),
                    turn: (0.0, 0.0, 0.0),
                    hinge: false,
                    link: None,
                };
                spawn_parts(commands, a, base, &[block], Pose { ghost: true, ..default() }, &blanket)
            }
        },
    }
}

fn spawn_item(commands: &mut Commands, a: &PropAssets, key: ItemKey, pos: Pos, lift: f32) -> Entity {
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    let s = if key.big { 1.25 } else { 1.0 };
    let root = Transform::from_translation(tile_center(pos) + Vec3::Y * lift)
        .with_rotation(Quat::from_rotation_y(hash(xi, yi, 24) * TAU))
        .with_scale(Vec3::splat(s));
    spawn_parts(commands, a, root, &key.kind.def().look, Pose::default(), &a.blanket_free)
}
