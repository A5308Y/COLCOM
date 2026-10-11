//! Colonists and raiders as small 3D figures kept in sync with the simulation.

use super::look::*;
use super::props::{lit, PropAssets};
use super::sim::Sim;
use super::tile_point;
use bevy::prelude::*;
use my_world::map::{Feature, ItemKind};
use my_world::pawn::{Faction, Job, Pawn, Weapon};
use my_world::world::World as SimWorld;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

struct PawnVis {
    root: Entity,
    body: Entity,
    head: Entity,
    hair: Entity,
    weapon: Entity,
    carry: Entity,
    weapon_kind: Option<Weapon>,
    carry_kind: Option<ItemKind>,
    dead: bool,
    yaw: f32,
}

#[derive(Resource, Default)]
struct PawnVisuals {
    generation: Option<u64>,
    map: HashMap<u32, PawnVis>,
}

#[derive(Resource)]
struct PawnAssets {
    capsule: Handle<Mesh>,
    sphere: Handle<Mesh>,
    cube: Handle<Mesh>,
    metal: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    blade: Handle<StandardMaterial>,
}

pub struct PawnsPlugin;

impl Plugin for PawnsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PawnVisuals>().add_systems(Startup, setup).add_systems(Update, sync_pawns);
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(PawnAssets {
        capsule: meshes.add(Capsule3d::new(0.17, 0.3)),
        sphere: meshes.add(Sphere::new(0.5)),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        metal: lit(&mut materials, METAL),
        wood: lit(&mut materials, WOOD),
        blade: lit(&mut materials, Rgb(0.82, 0.84, 0.88)),
    });
}

fn mesh(m: &Handle<Mesh>, mat: Handle<StandardMaterial>, tf: Transform, parent: Entity) -> impl Bundle {
    (Mesh3d(m.clone()), MeshMaterial3d(mat), tf, ChildOf(parent))
}

fn spawn_pawn(commands: &mut Commands, a: &PawnAssets, materials: &mut Assets<StandardMaterial>, p: &Pawn) -> PawnVis {
    let look = pawn_look(p);
    let root = commands.spawn((Transform::from_translation(tile_point(p.x, p.y)), Visibility::default())).id();
    let body = commands
        .spawn(mesh(&a.capsule, lit(materials, look.clothes), Transform::from_xyz(0.0, 0.34, 0.0), root))
        .id();
    let head = commands
        .spawn(mesh(&a.sphere, lit(materials, look.skin), Transform::from_xyz(0.0, 0.82, 0.0).with_scale(Vec3::splat(0.3)), root))
        .id();
    let hair = commands
        .spawn(mesh(
            &a.sphere,
            lit(materials, look.hair),
            Transform::from_xyz(0.0, 0.88, -0.03).with_scale(Vec3::new(0.32, 0.22, 0.32)),
            root,
        ))
        .id();
    let weapon = commands.spawn((mesh(&a.cube, a.metal.clone(), Transform::default(), root), Visibility::Hidden)).id();
    let carry = commands
        .spawn((
            mesh(&a.cube, a.wood.clone(), Transform::from_xyz(0.0, 0.42, 0.26).with_scale(Vec3::splat(0.22)), root),
            Visibility::Hidden,
        ))
        .id();
    PawnVis { root, body, head, hair, weapon, carry, weapon_kind: None, carry_kind: None, dead: false, yaw: 0.0 }
}

/// Where the weapon sits in the hands, and its material.
fn weapon_pose(w: Weapon, a: &PawnAssets) -> (Transform, Handle<StandardMaterial>) {
    match w {
        Weapon::Rifle => (Transform::from_xyz(0.15, 0.42, 0.2).with_scale(Vec3::new(0.05, 0.06, 0.52)), a.metal.clone()),
        Weapon::Pistol => (Transform::from_xyz(0.17, 0.42, 0.16).with_scale(Vec3::new(0.05, 0.08, 0.22)), a.metal.clone()),
        Weapon::Club => (
            Transform::from_xyz(0.2, 0.52, 0.12)
                .with_rotation(Quat::from_rotation_x(0.5))
                .with_scale(Vec3::new(0.08, 0.48, 0.08)),
            a.wood.clone(),
        ),
        Weapon::Knife => (Transform::from_xyz(0.18, 0.42, 0.2).with_scale(Vec3::new(0.03, 0.03, 0.26)), a.blade.clone()),
    }
}

/// Direction the pawn should face: where it's walking, or what it's working on.
fn facing(p: &Pawn, w: &SimWorld) -> Option<f32> {
    let target = if let Some(&(nx, ny)) = p.path.last() {
        Some((nx as f32, ny as f32))
    } else {
        match p.job {
            Some(Job::Work { target, .. }) => Some((target.0 as f32, target.1 as f32)),
            Some(Job::Attack { target } | Job::Tend { patient: target, .. } | Job::Feed { patient: target, .. }) => {
                w.pawn(target).map(|t| (t.x, t.y))
            }
            _ => None,
        }
    }?;
    let (dx, dz) = (target.0 - p.x, target.1 - p.y);
    (dx * dx + dz * dz > 1e-4).then(|| dx.atan2(dz))
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let diff = (b - a + PI).rem_euclid(TAU) - PI;
    a + diff * t
}

#[allow(clippy::too_many_arguments)]
fn sync_pawns(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    visuals: ResMut<PawnVisuals>,
    assets: Res<PawnAssets>,
    props: Res<PropAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let visuals = visuals.into_inner();
    let w = &sim.world;
    let m = &w.map;
    if visuals.generation != Some(sim.generation) {
        for (_, v) in visuals.map.drain() {
            commands.entity(v.root).despawn();
        }
        visuals.generation = Some(sim.generation);
    }

    // Drop visuals of pawns that left the map.
    visuals.map.retain(|id, v| {
        let keep = w.pawn(*id).is_some_and(|p| !p.left);
        if !keep {
            commands.entity(v.root).despawn();
        }
        keep
    });

    let dt = time.delta_secs();
    for p in w.pawns.iter().filter(|p| !p.left) {
        let v = visuals.map.entry(p.id).or_insert_with(|| spawn_pawn(&mut commands, &assets, &mut materials, p));

        if p.dead && !v.dead {
            // Grey out the body.
            let look = pawn_look(p);
            for (e, c) in [(v.body, look.clothes), (v.head, look.skin), (v.hair, look.hair)] {
                if let Ok(mut mat) = mats.get_mut(e) {
                    mat.0 = lit(&mut materials, c);
                }
            }
            v.dead = true;
        }

        if let Some(target) = facing(p, w) {
            v.yaw = lerp_angle(v.yaw, target, (dt * 10.0).min(1.0));
        }
        let lying = p.dead || p.lying();
        let in_bed = lying && !p.dead && m.feature[m.idx(p.pos)] == Feature::Bed;
        let base = tile_point(p.x, p.y);
        let root_tf = if lying {
            // Model is upright along +Y; tip it over so the head points along -Z (towards a bed's pillow).
            let yaw = if in_bed { 0.0 } else { v.yaw };
            let turn = Quat::from_rotation_y(yaw);
            let lift = 0.17 + if in_bed { 0.37 } else { 0.0 };
            Transform::from_translation(base + Vec3::Y * lift + turn * Vec3::new(0.0, 0.0, 0.42))
                .with_rotation(turn * Quat::from_rotation_x(-FRAC_PI_2))
        } else {
            let bob = if p.path.is_empty() { 0.0 } else { ((p.x + p.y) * TAU).sin().abs() * 0.06 };
            Transform::from_translation(base + Vec3::Y * bob).with_rotation(Quat::from_rotation_y(v.yaw))
        };
        if let Ok(mut tf) = transforms.get_mut(v.root) {
            *tf = root_tf;
        }

        // Weapon in hand when fighting or drafted.
        let armed = !lying && (p.drafted || p.faction == Faction::Raider || matches!(p.job, Some(Job::Attack { .. })));
        if v.weapon_kind != Some(p.weapon) {
            let (tf, mat) = weapon_pose(p.weapon, &assets);
            if let (Ok(mut t), Ok(mut mm)) = (transforms.get_mut(v.weapon), mats.get_mut(v.weapon)) {
                *t = tf;
                mm.0 = mat;
                v.weapon_kind = Some(p.weapon);
            }
        }
        if let Ok(mut vis) = visibility.get_mut(v.weapon) {
            *vis = if armed { Visibility::Inherited } else { Visibility::Hidden };
        }

        // Whatever they're carrying, held in front.
        let carried = p.carrying.map(|s| s.kind).filter(|_| !lying);
        if carried != v.carry_kind {
            if let Some(kind) = carried {
                if let Ok(mut mm) = mats.get_mut(v.carry) {
                    mm.0 = props.item_material(kind);
                }
            }
            if let Ok(mut vis) = visibility.get_mut(v.carry) {
                *vis = if carried.is_some() { Visibility::Inherited } else { Visibility::Hidden };
                v.carry_kind = carried;
            }
        }
    }
}
