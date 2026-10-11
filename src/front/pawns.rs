//! Colonists and raiders as small 3D figures kept in sync with the simulation.

use super::input::UiState;
use super::look::*;
use super::terrain::FLOOR_T;
use super::props::{lit, PropAssets};
use super::sim::Sim;
use super::tile_point;
use bevy::prelude::*;
use my_world::defs::ItemId;
use my_world::pawn::{Faction, Job, Pawn, Weapon};
use my_world::world::World as SimWorld;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Overall size of the figure (the model is built about 1.15 tall).
const SCALE: f32 = 0.85;

struct PawnVis {
    root: Entity,
    /// This pawn's own materials (clothes, trousers, skin, hair), greyed on death.
    own: [Handle<StandardMaterial>; 4],
    /// Hip and shoulder pivots, rotated to swing the limbs.
    legs: [Entity; 2],
    arms: [Entity; 2],
    weapon: Entity,
    carry: Entity,
    weapon_kind: Option<Weapon>,
    carry_kind: Option<ItemId>,
    dead: bool,
    yaw: f32,
    /// 0 standing still .. 1 walking; eases in and out.
    swing: f32,
    /// Drawn height, which lags behind the real one while falling.
    render_y: f32,
    fall_speed: f32,
}

#[derive(Resource, Default)]
struct PawnVisuals {
    generation: Option<u64>,
    map: HashMap<u32, PawnVis>,
}

#[derive(Resource)]
struct PawnAssets {
    torso: Handle<Mesh>,
    leg: Handle<Mesh>,
    arm: Handle<Mesh>,
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
        torso: meshes.add(Capsule3d::new(0.15, 0.22)),
        leg: meshes.add(Capsule3d::new(0.065, 0.2)),
        arm: meshes.add(Capsule3d::new(0.05, 0.22)),
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
    let clothes = lit(materials, look.clothes);
    let trousers = lit(materials, look.clothes.shade(0.6));
    let skin = lit(materials, look.skin);
    let hair = lit(materials, look.hair);
    let own = [clothes.clone(), trousers.clone(), skin.clone(), hair.clone()];
    let root = commands
        .spawn((Transform::from_translation(tile_point(p.x, p.y, p.z)).with_scale(Vec3::splat(SCALE)), Visibility::default()))
        .id();
    // A pivot at a joint, with the limb hanging below it.
    let mut limb = |at: Vec3, mesh_h: &Handle<Mesh>, mat: &Handle<StandardMaterial>, hang: f32| {
        let pivot = commands.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(root))).id();
        commands.spawn(mesh(mesh_h, mat.clone(), Transform::from_xyz(0.0, -hang, 0.0), pivot));
        pivot
    };
    let legs = [
        limb(Vec3::new(-0.08, 0.36, 0.0), &a.leg, &trousers, 0.17),
        limb(Vec3::new(0.08, 0.36, 0.0), &a.leg, &trousers, 0.17),
    ];
    let arms = [
        limb(Vec3::new(-0.21, 0.76, 0.0), &a.arm, &clothes, 0.16),
        limb(Vec3::new(0.21, 0.76, 0.0), &a.arm, &clothes, 0.16),
    ];
    commands.spawn(mesh(&a.torso, clothes, Transform::from_xyz(0.0, 0.58, 0.0), root));
    commands.spawn(mesh(&a.sphere, skin, Transform::from_xyz(0.0, 1.0, 0.0).with_scale(Vec3::splat(0.3)), root));
    let cap = Transform::from_xyz(0.0, 1.06, -0.03).with_scale(Vec3::new(0.32, 0.22, 0.32));
    commands.spawn(mesh(&a.sphere, hair, cap, root));
    let weapon = commands.spawn((mesh(&a.cube, a.metal.clone(), Transform::default(), root), Visibility::Hidden)).id();
    let carry = commands
        .spawn((
            mesh(&a.cube, a.wood.clone(), Transform::from_xyz(0.0, 0.6, 0.28).with_scale(Vec3::splat(0.24)), root),
            Visibility::Hidden,
        ))
        .id();
    PawnVis {
        root,
        own,
        legs,
        arms,
        weapon,
        carry,
        weapon_kind: None,
        carry_kind: None,
        dead: false,
        yaw: 0.0,
        swing: 0.0,
        render_y: f32::NAN,
        fall_speed: 0.0,
    }
}

/// Where the weapon sits in the hands, and its material.
fn weapon_pose(w: Weapon, a: &PawnAssets) -> (Transform, Handle<StandardMaterial>) {
    match w {
        Weapon::Rifle => (Transform::from_xyz(0.15, 0.62, 0.22).with_scale(Vec3::new(0.05, 0.06, 0.52)), a.metal.clone()),
        Weapon::Pistol => (Transform::from_xyz(0.17, 0.62, 0.2).with_scale(Vec3::new(0.05, 0.08, 0.22)), a.metal.clone()),
        Weapon::Club => (
            Transform::from_xyz(0.22, 0.72, 0.14)
                .with_rotation(Quat::from_rotation_x(0.5))
                .with_scale(Vec3::new(0.08, 0.48, 0.08)),
            a.wood.clone(),
        ),
        Weapon::Knife => (Transform::from_xyz(0.2, 0.62, 0.22).with_scale(Vec3::new(0.03, 0.03, 0.26)), a.blade.clone()),
    }
}

/// Direction the pawn should face: where it's walking, or what it's working on.
fn facing(p: &Pawn, w: &SimWorld) -> Option<f32> {
    let target = if let Some(&(nx, ny, _)) = p.path.last() {
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
    ui: Res<UiState>,
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
            // Grey out the body (pawn_look already greys a dead pawn's colours).
            let look = pawn_look(p);
            let colors = [look.clothes, look.clothes.shade(0.6), look.skin, look.hair];
            for (handle, c) in v.own.iter().zip(colors) {
                if let Some(mut mat) = materials.get_mut(handle) {
                    mat.base_color = c.color();
                }
            }
            v.dead = true;
        }

        if let Some(target) = facing(p, w) {
            v.yaw = lerp_angle(v.yaw, target, (dt * 10.0).min(1.0));
        }
        let lying = p.dead || p.lying();
        let in_bed = lying && !p.dead && m.feature[m.idx(p.pos)].is_bed();
        // Standing on a built floor slab rather than the cell's bottom.
        let on_slab = m.floor[m.idx(p.pos)] && (p.z - p.pos.2 as f32).abs() < 0.01;
        let mut base = tile_point(p.x, p.y, p.z) + Vec3::Y * if on_slab { FLOOR_T } else { 0.0 };
        // A little hop when stepping up or down a level.
        let frac = p.z - p.z.floor();
        if frac > 0.001 && frac < 0.999 {
            base.y += 0.35 * 4.0 * frac * (1.0 - frac);
        }
        // Falls drop with gravity instead of snapping down.
        if v.render_y.is_nan() || v.render_y < base.y + 0.3 {
            v.render_y = base.y;
            v.fall_speed = 0.0;
        } else {
            v.fall_speed += 22.0 * dt;
            v.render_y = (v.render_y - v.fall_speed * dt).max(base.y);
        }
        base.y = v.render_y;

        let walking = !lying && !p.path.is_empty();
        v.swing += ((walking as u8 as f32) - v.swing) * (dt * 8.0).min(1.0);
        let root_tf = if lying {
            // Model is upright along +Y; tip it over so the head points along -Z (towards a bed's pillow).
            let yaw = if in_bed { 0.0 } else { v.yaw };
            let turn = Quat::from_rotation_y(yaw);
            let lift = 0.15 + if in_bed { 0.37 } else { 0.0 };
            Transform::from_translation(base + Vec3::Y * lift + turn * Vec3::new(0.0, 0.0, 0.48))
                .with_rotation(turn * Quat::from_rotation_x(-FRAC_PI_2))
        } else {
            Transform::from_translation(base).with_rotation(Quat::from_rotation_y(v.yaw))
        };
        if let Ok(mut tf) = transforms.get_mut(v.root) {
            *tf = root_tf.with_scale(Vec3::splat(SCALE));
        }
        if let Ok(mut vis) = visibility.get_mut(v.root) {
            *vis = if ui.shows(m, p.z) { Visibility::Inherited } else { Visibility::Hidden };
        }

        // Weapon in hand when fighting or drafted.
        let armed = !lying && (p.drafted || p.faction == Faction::Raider || matches!(p.job, Some(Job::Attack { .. })));

        // Legs and arms swing in opposite pairs; armed arms hold the weapon forward.
        let stride = ((p.x + p.y) * PI * 1.6).sin() * 0.6 * v.swing;
        let swings = [
            (v.legs[0], stride),
            (v.legs[1], -stride),
            (v.arms[0], if armed { -1.2 } else { -stride * 0.8 }),
            (v.arms[1], if armed { -1.3 } else { stride * 0.8 }),
        ];
        for (pivot, angle) in swings {
            if let Ok(mut t) = transforms.get_mut(pivot) {
                t.rotation = Quat::from_rotation_x(angle);
            }
        }
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
