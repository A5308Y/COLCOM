//! Per-tile 3D objects: trees, bushes, crops, beds, doors, blueprints and items.
//!
//! Each frame the wanted object for every tile is compared with what is
//! spawned; changed tiles get their entity replaced.

use super::input::UiState;
use super::look::*;
use super::sim::Sim;
use super::terrain::{FLOOR_T, LEVEL_H as WALL_H};
use super::tile_center;
use bevy::prelude::*;
use my_world::map::{BuildKind, Feature, ItemKind, Map, Pos};
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, PartialEq, Debug)]
enum PropKey {
    Tree,
    Bush,
    Crop { stage: u8 },
    Bed { owner: Option<u32> },
    Door { horizontal: bool, open: bool },
    Stairs,
    Blueprint { kind: BuildKind },
}

#[derive(Clone, Copy, PartialEq, Debug)]
struct ItemKey {
    kind: ItemKind,
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
    pub cube: Handle<Mesh>,
    pub sphere: Handle<Mesh>,
    pub cylinder: Handle<Mesh>,
    pub cone: Handle<Mesh>,
    trunk: Handle<StandardMaterial>,
    leaf: [Handle<StandardMaterial>; 3],
    pine: [Handle<StandardMaterial>; 2],
    berry: Handle<StandardMaterial>,
    veg: Handle<StandardMaterial>,
    wheat: Handle<StandardMaterial>,
    sprout: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    wood_light: Handle<StandardMaterial>,
    sheet: Handle<StandardMaterial>,
    pillow: Handle<StandardMaterial>,
    stone: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    wall: Handle<StandardMaterial>,
    blueprint: Handle<StandardMaterial>,
    blanket_free: Handle<StandardMaterial>,
}

impl PropAssets {
    /// Material for a carried or stored item of this kind.
    pub fn item_material(&self, kind: ItemKind) -> Handle<StandardMaterial> {
        match kind {
            ItemKind::Wood => self.wood.clone(),
            ItemKind::Stone => self.stone.clone(),
            ItemKind::Food => self.berry.clone(),
            ItemKind::Medicine => self.white.clone(),
        }
    }
}

pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Props>().add_systems(Startup, setup_assets).add_systems(Update, sync_props);
    }
}

pub fn lit(materials: &mut Assets<StandardMaterial>, c: Rgb) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: c.color(), perceptual_roughness: 0.85, ..default() })
}

fn setup_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let blueprint = materials.add(StandardMaterial {
        base_color: Color::srgba(0.38, 0.68, 1.0, 0.35),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let mut mat = |c: Rgb| lit(&mut materials, c);
    let assets = PropAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        sphere: meshes.add(Sphere::new(0.5)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        cone: meshes.add(Cone::new(0.5, 1.0)),
        trunk: mat(WOOD.shade(0.8)),
        leaf: [mat(LEAF), mat(LEAF.mix(LEAF_LIGHT, 0.5)), mat(LEAF_LIGHT)],
        pine: [mat(PINE), mat(PINE.shade(1.2))],
        berry: mat(BERRY),
        veg: mat(VEG),
        wheat: mat(WHEAT),
        sprout: mat(LEAF_LIGHT.shade(1.1)),
        wood: mat(WOOD),
        wood_light: mat(WOOD_LIGHT),
        sheet: mat(SHEET),
        pillow: mat(Rgb(0.97, 0.97, 0.97)),
        stone: mat(ROCK_TOP.shade(1.15)),
        white: mat(Rgb(0.95, 0.95, 0.95)),
        red: mat(BERRY),
        wall: mat(WALL_FACE),
        blueprint,
        blanket_free: mat(Rgb(0.55, 0.58, 0.62)),
    };
    commands.insert_resource(assets);
}

fn crop_stage(growth: f32) -> u8 {
    if growth >= 1.0 {
        3
    } else if growth >= 0.6 {
        2
    } else if growth >= 0.25 {
        1
    } else {
        0
    }
}

/// A wall or door (or solid ground) beside a door decides which way it faces.
fn is_wallish(m: &Map, x: i32, y: i32, z: usize) -> bool {
    m.at(x, y, z as i32).is_some_and(|p| {
        let i = m.idx(p);
        m.block[i].solid() || matches!(m.feature[i], Feature::Wall | Feature::Door)
    })
}

#[allow(clippy::too_many_arguments)]
fn sync_props(
    mut commands: Commands,
    sim: Res<Sim>,
    ui: Res<UiState>,
    props: ResMut<Props>,
    assets: Res<PropAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
        let (xi, yi) = (pos.0 as i32, pos.1 as i32);
        let want = match m.feature[i] {
            Feature::Tree => Some(PropKey::Tree),
            Feature::Bush => Some(PropKey::Bush),
            Feature::Crop { growth } => Some(PropKey::Crop { stage: crop_stage(growth) }),
            Feature::Bed => Some(PropKey::Bed { owner: owners.get(&pos).map(|o| o.0) }),
            Feature::Door => {
                let open = w
                    .pawns
                    .iter()
                    .any(|p| {
                        !p.dead
                            && (p.x - pos.0 as f32).abs() < 0.9
                            && (p.y - pos.1 as f32).abs() < 0.9
                            && (p.z - pos.2 as f32).abs() < 0.9
                    });
                let horizontal = is_wallish(m, xi - 1, yi, pos.2) || is_wallish(m, xi + 1, yi, pos.2);
                Some(PropKey::Door { horizontal, open })
            }
            Feature::Stairs => Some(PropKey::Stairs),
            Feature::Blueprint { kind, .. } => Some(PropKey::Blueprint { kind }),
            Feature::None | Feature::Wall => None,
        };
        let have = props.features[i].map(|(k, _)| k);
        if want != have {
            if let Some((_, e)) = props.features[i].take() {
                commands.entity(e).despawn();
            }
            if let Some(key) = want {
                let blanket = match key {
                    PropKey::Bed { owner: Some(_) } => {
                        let c = owners[&pos].1;
                        lit(&mut materials, Rgb(c[0], c[1], c[2]).shade(0.8))
                    }
                    _ => assets.blanket_free.clone(),
                };
                let lift = if m.floor[i] { FLOOR_T } else { 0.0 };
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
                let lift = if m.floor[i] { FLOOR_T } else { 0.0 };
                let e = spawn_item(&mut commands, &assets, key, pos, lift);
                if hidden(pos) {
                    commands.entity(e).insert(Visibility::Hidden);
                }
                props.items[i] = Some((key, e));
            }
        }
    }
}

/// A child part: unit mesh scaled and placed relative to the prop's root.
fn part(mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, at: Vec3, scale: Vec3) -> impl Bundle {
    (Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(at).with_scale(scale))
}

fn part_rot(mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, at: Vec3, rot: Quat, scale: Vec3) -> impl Bundle {
    (
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_translation(at).with_rotation(rot).with_scale(scale),
    )
}

fn spawn_feature(
    commands: &mut Commands,
    a: &PropAssets,
    key: PropKey,
    pos: Pos,
    lift: f32,
    blanket: Handle<StandardMaterial>,
) -> Entity {
    let base = tile_center(pos) + Vec3::Y * lift;
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    let natural = matches!(key, PropKey::Tree | PropKey::Bush | PropKey::Crop { .. });
    let yaw = if natural { hash(xi, yi, 22) * TAU } else { 0.0 };
    let mut root = commands.spawn((
        Transform::from_translation(base).with_rotation(Quat::from_rotation_y(yaw)),
        Visibility::default(),
    ));
    match key {
        PropKey::Tree => {
            let size = 0.85 + hash(xi, yi, 20) * 0.35;
            let conifer = hash(xi, yi, 21) < 0.35;
            root.with_children(|c| {
                c.spawn(part(&a.cylinder, &a.trunk, Vec3::new(0.0, 0.35 * size, 0.0), Vec3::new(0.16, 0.7, 0.16) * size));
                if conifer {
                    let mat = &a.pine[(hash(xi, yi, 23) * 2.0) as usize % 2];
                    for (base, width, height) in [(0.4, 0.95, 0.75), (0.75, 0.75, 0.65), (1.05, 0.5, 0.55)] {
                        let at = Vec3::new(0.0, (base + height / 2.0) * size, 0.0);
                        c.spawn(part(&a.cone, mat, at, Vec3::new(width, height, width) * size));
                    }
                } else {
                    let mat = &a.leaf[(hash(xi, yi, 23) * 3.0) as usize % 3];
                    let blobs = [(-0.16, 0.9, 0.05, 0.62), (0.17, 0.85, -0.1, 0.56), (0.0, 1.15, 0.0, 0.62), (0.05, 0.82, 0.18, 0.5)];
                    for (x, y, z, d) in blobs {
                        c.spawn(part(&a.sphere, mat, Vec3::new(x, y, z) * size, Vec3::splat(d * size)));
                    }
                }
            });
        }
        PropKey::Bush => {
            root.with_children(|c| {
                for (x, z, d) in [(-0.12, 0.05, 0.38), (0.13, 0.06, 0.34), (0.0, -0.1, 0.4)] {
                    c.spawn(part(&a.sphere, &a.leaf[0], Vec3::new(x, 0.17, z), Vec3::new(d, d * 0.8, d)));
                }
                for (x, y, z) in [(-0.2, 0.25, 0.1), (0.15, 0.3, -0.12), (0.05, 0.33, 0.15), (0.22, 0.2, 0.08)] {
                    c.spawn(part(&a.sphere, &a.berry, Vec3::new(x, y, z), Vec3::splat(0.08)));
                }
            });
        }
        PropKey::Crop { stage } => {
            root.with_children(|c| {
                for (x, z) in [(-0.22, -0.22), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)] {
                    if stage == 0 {
                        c.spawn(part(&a.cylinder, &a.sprout, Vec3::new(x, 0.07, z), Vec3::new(0.04, 0.14, 0.04)));
                        continue;
                    }
                    let d = 0.12 + 0.07 * stage as f32;
                    c.spawn(part(&a.sphere, &a.sprout, Vec3::new(x, d * 0.45, z), Vec3::new(d, d * 0.9, d)));
                    if stage == 3 {
                        c.spawn(part(&a.sphere, &a.wheat, Vec3::new(x, d * 0.95, z), Vec3::splat(0.1)));
                    }
                }
            });
        }
        PropKey::Bed { .. } => {
            root.with_children(|c| {
                c.spawn(part(&a.cube, &a.wood, Vec3::new(0.0, 0.13, 0.0), Vec3::new(0.82, 0.26, 0.92)));
                c.spawn(part(&a.cube, &a.sheet, Vec3::new(0.0, 0.29, 0.0), Vec3::new(0.74, 0.08, 0.84)));
                c.spawn(part(&a.cube, &a.pillow, Vec3::new(0.0, 0.35, -0.3), Vec3::new(0.52, 0.08, 0.2)));
                c.spawn(part(&a.cube, &blanket, Vec3::new(0.0, 0.33, 0.13), Vec3::new(0.76, 0.1, 0.56)));
            });
        }
        PropKey::Door { horizontal, open } => {
            // Modelled spanning X; rotated a quarter turn when the walls run along Z.
            root.insert(
                Transform::from_translation(base)
                    .with_rotation(Quat::from_rotation_y(if horizontal { 0.0 } else { FRAC_PI_2 })),
            );
            root.with_children(|c| {
                for x in [-0.45, 0.45] {
                    c.spawn(part(&a.cube, &a.wall, Vec3::new(x, WALL_H / 2.0, 0.0), Vec3::new(0.1, WALL_H, 0.34)));
                }
                c.spawn(part(&a.cube, &a.wall, Vec3::new(0.0, WALL_H - 0.08, 0.0), Vec3::new(1.0, 0.16, 0.34)));
                if open {
                    // Swung open against the frame.
                    let at = Vec3::new(-0.36, 0.55, 0.36);
                    c.spawn(part_rot(&a.cube, &a.wood_light, at, Quat::from_rotation_y(FRAC_PI_2), Vec3::new(0.78, 1.1, 0.07)));
                } else {
                    c.spawn(part(&a.cube, &a.wood_light, Vec3::new(0.0, 0.55, 0.0), Vec3::new(0.8, 1.1, 0.07)));
                }
            });
        }
        PropKey::Stairs => {
            // Four steps rising to the floor of the level above.
            root.with_children(|c| {
                for k in 0..4 {
                    let h = (k + 1) as f32 * WALL_H / 4.0;
                    let at = Vec3::new(-0.375 + k as f32 * 0.25, h / 2.0, 0.0);
                    c.spawn(part(&a.cube, &a.wood_light, at, Vec3::new(0.25, h, 0.9)));
                }
            });
        }
        PropKey::Blueprint { kind } => {
            let (size, y) = match kind {
                BuildKind::Wall => (Vec3::new(0.98, WALL_H, 0.98), WALL_H / 2.0),
                BuildKind::Door => (Vec3::new(0.98, WALL_H, 0.3), WALL_H / 2.0),
                BuildKind::Bed => (Vec3::new(0.82, 0.36, 0.92), 0.18),
                BuildKind::Floor => (Vec3::new(0.98, FLOOR_T, 0.98), FLOOR_T / 2.0),
                BuildKind::Stairs => (Vec3::new(0.98, WALL_H, 0.9), WALL_H / 2.0),
            };
            root.with_children(|c| {
                c.spawn(part(&a.cube, &a.blueprint, Vec3::new(0.0, y, 0.0), size));
            });
        }
    }
    root.id()
}

fn spawn_item(commands: &mut Commands, a: &PropAssets, key: ItemKey, pos: Pos, lift: f32) -> Entity {
    let (xi, yi) = (pos.0 as i32, pos.1 as i32);
    let yaw = hash(xi, yi, 24) * TAU;
    let s = if key.big { 1.25 } else { 1.0 };
    let mut root = commands.spawn((
        Transform::from_translation(tile_center(pos) + Vec3::Y * lift)
            .with_rotation(Quat::from_rotation_y(yaw))
            .with_scale(Vec3::splat(s)),
        Visibility::default(),
    ));
    root.with_children(|c| match key.kind {
        ItemKind::Wood => {
            let lying = Quat::from_rotation_z(FRAC_PI_2);
            for (y, z) in [(0.07, -0.08), (0.07, 0.08), (0.19, 0.0)] {
                c.spawn(part_rot(&a.cylinder, &a.wood, Vec3::new(0.0, y, z), lying, Vec3::new(0.14, 0.6, 0.14)));
            }
        }
        ItemKind::Stone => {
            for (x, y, z, d) in [(-0.12, 0.09, 0.0, 0.2), (0.12, 0.08, 0.05, 0.18), (0.0, 0.24, -0.02, 0.17)] {
                c.spawn(part(&a.cube, &a.stone, Vec3::new(x, y, z), Vec3::splat(d)));
            }
        }
        ItemKind::Food => {
            for (k, (x, z)) in [(-0.12, 0.0), (0.1, 0.06), (0.0, -0.12), (0.02, 0.12), (0.13, -0.08)].into_iter().enumerate() {
                let mat = if k % 2 == 0 { &a.berry } else { &a.veg };
                c.spawn(part(&a.sphere, mat, Vec3::new(x, 0.08, z), Vec3::splat(0.16)));
            }
        }
        ItemKind::Medicine => {
            c.spawn(part(&a.cube, &a.white, Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.4, 0.2, 0.3)));
            c.spawn(part(&a.cube, &a.red, Vec3::new(0.0, 0.205, 0.0), Vec3::new(0.08, 0.02, 0.22)));
            c.spawn(part(&a.cube, &a.red, Vec3::new(0.0, 0.205, 0.0), Vec3::new(0.24, 0.02, 0.08)));
        }
    });
    root.id()
}
