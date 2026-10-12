//! Colonists and raiders kept in sync with the simulation: animated
//! character models (`characters.rs`), with the old code-built figure shown
//! until a character is ready or if the models can't load.

use super::characters::{self, Anim, CharacterLibrary, Rig};
use super::input::UiState;
use super::look::*;
use super::terrain::FLOOR_T;
use super::props::{lit, PropAssets};
use super::sim::Sim;
use super::tile_point;
use bevy::prelude::*;
use my_world::defs::{ItemId, Special};
use my_world::pawn::{Faction, Job, Pawn};
use my_world::world::World as SimWorld;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Overall size of the figure (the model is built about 1.15 tall).
const SCALE: f32 = 0.85;
/// How quickly the drawn position follows the simulated one (per second, at
/// 1x speed): a short lag that rounds corners and eases starts and stops.
const FOLLOW: f32 = 14.0;
/// Below this drawn speed (cells per real second) a pawn stands still.
const STILL: f32 = 0.35;
/// Fastest turn, radians per second.
const TURN_RATE: f32 = 9.0;

struct PawnVis {
    root: Entity,
    /// The code-built figure (scaled), and the animated character.
    figure: Entity,
    rig: Entity,
    rig_weapon: Option<Entity>,
    rig_shown: bool,
    rig_dead: bool,
    /// An attack animation, the seconds it has left, and whether it starts this frame.
    action: Option<(Anim, f32, bool)>,
    last_cooldown: f32,
    /// Drawn position (cells) and its speed (cells per real second).
    shown: Vec2,
    speed: f32,
    /// This pawn's own materials (clothes, trousers, skin, hair), greyed on death.
    own: [Handle<StandardMaterial>; 4],
    /// Hip and shoulder pivots, rotated to swing the limbs.
    legs: [Entity; 2],
    arms: [Entity; 2],
    weapon: Entity,
    carry: Entity,
    /// Weapon the hand part was last set up for (`Some(None)`: unarmed).
    weapon_kind: Option<Option<ItemId>>,
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
    });
}

fn mesh(m: &Handle<Mesh>, mat: Handle<StandardMaterial>, tf: Transform, parent: Entity) -> impl Bundle {
    (Mesh3d(m.clone()), MeshMaterial3d(mat), tf, ChildOf(parent))
}

fn spawn_pawn(
    commands: &mut Commands,
    a: &PawnAssets,
    materials: &mut Assets<StandardMaterial>,
    server: &AssetServer,
    p: &Pawn,
) -> PawnVis {
    let look = pawn_look(p);
    let clothes = lit(materials, look.clothes);
    let trousers = lit(materials, look.clothes.shade(0.6));
    let skin = lit(materials, look.skin);
    let hair = lit(materials, look.hair);
    let own = [clothes.clone(), trousers.clone(), skin.clone(), hair.clone()];
    let root = commands.spawn((Transform::from_translation(tile_point(p.x, p.y, p.z)), Visibility::default())).id();
    let rig = characters::spawn_rig(commands, server, p, root);
    let figure = commands.spawn((Transform::from_scale(Vec3::splat(SCALE)), Visibility::default(), ChildOf(root))).id();
    // A pivot at a joint, with the limb hanging below it.
    let mut limb = |at: Vec3, mesh_h: &Handle<Mesh>, mat: &Handle<StandardMaterial>, hang: f32| {
        let pivot = commands.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(figure))).id();
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
    commands.spawn(mesh(&a.torso, clothes, Transform::from_xyz(0.0, 0.58, 0.0), figure));
    commands.spawn(mesh(&a.sphere, skin, Transform::from_xyz(0.0, 1.0, 0.0).with_scale(Vec3::splat(0.3)), figure));
    let cap = Transform::from_xyz(0.0, 1.06, -0.03).with_scale(Vec3::new(0.32, 0.22, 0.32));
    commands.spawn(mesh(&a.sphere, hair, cap, figure));
    let weapon = commands.spawn((mesh(&a.cube, a.metal.clone(), Transform::default(), figure), Visibility::Hidden)).id();
    // Carried things are held in front of the chest, by either body.
    let carry_tf = Transform::from_xyz(0.0, 0.6 * SCALE, 0.28 * SCALE).with_scale(Vec3::splat(0.24 * SCALE));
    let carry = commands.spawn((mesh(&a.cube, a.wood.clone(), carry_tf, root), Visibility::Hidden)).id();
    PawnVis {
        root,
        figure,
        rig,
        rig_weapon: None,
        rig_shown: false,
        rig_dead: false,
        action: None,
        last_cooldown: p.cooldown,
        shown: Vec2::new(p.x, p.y),
        speed: 0.0,
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

/// How a weapon sits in the figure's hands (and, `in_hand`, in the
/// character's right hand), from its definition, with its mesh and material.
fn weapon_pose(w: ItemId, props: &PropAssets, in_hand: bool) -> Option<(Transform, Handle<Mesh>, Handle<StandardMaterial>)> {
    let h = &w.def().weapon.as_ref()?.held;
    let (tx, ty, tz) = h.turn;
    let turn = Quat::from_euler(EulerRot::XYZ, tx.to_radians(), ty.to_radians(), tz.to_radians());
    let size = Vec3::new(h.size.0, h.size.1, h.size.2);
    let tf = if in_hand {
        characters::hand_pose(size, turn, SCALE)
    } else {
        Transform::from_xyz(h.at.0, h.at.1, h.at.2).with_rotation(turn).with_scale(size)
    };
    Some((tf, props.mesh(h.shape).clone(), props.paint(&h.color)))
}

/// Direction the pawn should face when not moving: what it's working on or
/// fighting, or the table it's eating at.
fn facing(p: &Pawn, w: &SimWorld) -> Option<f32> {
    let m = &w.map;
    let target = match p.job {
        Some(Job::Eat { seat: Some(seat), .. }) if seat == p.pos => m
            .neighbors(seat)
            .find(|&nb| m.feature[m.idx(nb)].is_special(Special::Table))
            .map(|t| (t.0 as f32, t.1 as f32)),
        Some(Job::Work { target, .. }) => Some((target.0 as f32, target.1 as f32)),
        Some(Job::Attack { target } | Job::Tend { patient: target, .. } | Job::Feed { patient: target, .. }) => {
            w.pawn(target).map(|t| (t.x, t.y))
        }
        _ => None,
    }?;
    let (dx, dz) = (target.0 - p.x, target.1 - p.y);
    (dx * dx + dz * dz > 1e-4).then(|| dx.atan2(dz))
}

/// Turn from `a` towards `b` by at most `max` radians.
fn turn_towards(a: f32, b: f32, max: f32) -> f32 {
    let diff = (b - a + PI).rem_euclid(TAU) - PI;
    a + diff.clamp(-max, max)
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
    server: Res<AssetServer>,
    library: Res<CharacterLibrary>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut rigs: Query<(&mut Rig, &mut AnimationPlayer, &mut AnimationTransitions)>,
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
    // Simulation seconds per real second: animations other than walking run at this rate.
    let sim_rate = if sim.paused { 0.0 } else { sim.speed as f32 };
    for p in w.pawns.iter().filter(|p| !p.left) {
        let v = visuals
            .map
            .entry(p.id)
            .or_insert_with(|| spawn_pawn(&mut commands, &assets, &mut materials, &server, p));

        // The character takes over from the figure once it's ready.
        let mut rig = rigs.get_mut(v.rig).ok();
        let rig_ready = rig.as_mut().is_some_and(|(r, ..)| characters::prepare(&mut commands, &library, v.rig, r));
        if rig_ready != v.rig_shown {
            v.rig_shown = rig_ready;
            for (e, show) in [(v.rig, rig_ready), (v.figure, !rig_ready)] {
                if let Ok(mut vis) = visibility.get_mut(e) {
                    *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
                }
            }
        }

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
        if let Some((r, ..)) = rig.as_ref().filter(|_| rig_ready && v.rig_dead != p.dead) {
            characters::set_dead(r, p.dead, &mut materials);
            v.rig_dead = p.dead;
        }

        // The drawn position trails the simulated one a little, which rounds
        // corners and eases into and out of walking. Big jumps snap.
        let target = Vec2::new(p.x, p.y);
        let before = v.shown;
        if v.shown.distance(target) > 2.0 || sim_rate == 0.0 && v.shown.distance(target) > 0.5 {
            v.shown = target;
        } else {
            v.shown += (target - v.shown) * (1.0 - (-dt * FOLLOW * sim_rate.max(1.0)).exp());
        }
        let velocity = if dt > 0.0 { (v.shown - before) / dt } else { Vec2::ZERO };
        v.speed += (velocity.length() - v.speed) * (dt * 10.0).min(1.0);

        let lying = p.dead || p.lying();
        let in_bed = lying && !p.dead && m.feature[m.idx(p.pos)].is_bed();
        // The character collapses on the ground when downed or killed; in
        // bed, asleep or resting it (like the figure) is laid down flat.
        let collapsed = rig_ready && (p.dead || p.downed) && !in_bed;
        let tipped = lying && !collapsed;
        let moving = !lying && v.speed > STILL;

        let turn = TURN_RATE * dt * sim_rate.max(1.0);
        if moving && velocity.length() > STILL {
            v.yaw = turn_towards(v.yaw, velocity.x.atan2(velocity.y), turn);
        } else if let Some(target) = facing(p, w).filter(|_| !lying) {
            v.yaw = turn_towards(v.yaw, target, turn);
        }

        // Standing on a built floor slab rather than the cell's bottom.
        let on_slab = m.floor[m.idx(p.pos)] && (p.z - p.pos.2 as f32).abs() < 0.01;
        let mut base = tile_point(v.shown.x, v.shown.y, p.z) + Vec3::Y * if on_slab { FLOOR_T } else { 0.0 };
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
        let falling = v.render_y > base.y + 0.05;
        base.y = v.render_y;

        v.swing += ((moving as u8 as f32) - v.swing) * (dt * 8.0).min(1.0);
        let root_tf = if tipped {
            // Upright along +Y; tip over so the head points along -Z (towards a bed's pillow).
            let yaw = if in_bed { 0.0 } else { v.yaw };
            let turn = Quat::from_rotation_y(yaw);
            let lift = if rig_ready { 0.08 } else { 0.15 } + if in_bed { 0.37 } else { 0.0 };
            Transform::from_translation(base + Vec3::Y * lift + turn * Vec3::new(0.0, 0.0, 0.48))
                .with_rotation(turn * Quat::from_rotation_x(-FRAC_PI_2))
        } else {
            Transform::from_translation(base).with_rotation(Quat::from_rotation_y(v.yaw))
        };
        if let Ok(mut tf) = transforms.get_mut(v.root) {
            *tf = root_tf;
        }
        if let Ok(mut vis) = visibility.get_mut(v.root) {
            *vis = if ui.shows(m, p.z) { Visibility::Inherited } else { Visibility::Hidden };
        }

        // Weapon in hand when fighting or drafted.
        let armed = !lying && (p.drafted || p.faction == Faction::Raider || matches!(p.job, Some(Job::Attack { .. })));

        // An attack starts the weapon's cooldown: play the shot or blow.
        if p.cooldown > v.last_cooldown + 0.05 && !lying {
            let anim = characters::attack_anim(p);
            v.action = Some((anim, library.duration(anim), true));
        }
        v.last_cooldown = p.cooldown;

        if let Some((r, player, transitions)) = rig.as_mut().filter(|_| rig_ready) {
            let (anim, rate, restart) = if collapsed {
                (Anim::Down, sim_rate, false)
            } else if tipped {
                (Anim::Idle, 0.4 * sim_rate, false)
            } else if falling {
                (Anim::Fall, 1.0, false)
            } else if moving {
                let (a, rate) = characters::locomotion(v.speed);
                (a, rate, false)
            } else if let Some((a, _, fresh)) = v.action {
                (a, sim_rate, fresh)
            } else {
                (characters::activity(p, w, armed), sim_rate, false)
            };
            characters::play(&library, r, player, transitions, anim, restart, rate);
            // Sitting leans back onto the seat: move forward and up onto the chair.
            let sit = if anim == Anim::Sit { Vec3::new(0.0, 0.07, 0.18) } else { Vec3::ZERO };
            if let Ok(mut t) = transforms.get_mut(v.rig) {
                t.translation = sit;
            }
            if v.rig_weapon.is_none() {
                if let Some(hand) = r.hand {
                    let e = commands.spawn((mesh(&assets.cube, assets.metal.clone(), Transform::default(), hand), Visibility::Hidden));
                    v.rig_weapon = Some(e.id());
                    v.weapon_kind = None;
                }
            }
        }

        if let Some((_, left, fresh)) = &mut v.action {
            *fresh = false;
            *left -= dt * sim_rate;
            if *left <= 0.0 {
                v.action = None;
            }
        }

        // Figure: legs and arms swing in opposite pairs; armed arms hold the weapon forward.
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
        let weapons = [(Some(v.weapon), false), (v.rig_weapon, true)];
        if v.weapon_kind != Some(p.weapon) {
            for (e, in_hand) in weapons {
                let (Some(e), Some((tf, mesh, mat))) = (e, p.weapon.and_then(|w| weapon_pose(w, &props, in_hand))) else {
                    continue;
                };
                if let Ok(mut t) = transforms.get_mut(e) {
                    *t = tf;
                }
                if let Ok(mut mm) = mats.get_mut(e) {
                    mm.0 = mat;
                }
                commands.entity(e).insert(Mesh3d(mesh));
            }
            v.weapon_kind = Some(p.weapon);
        }
        for e in weapons.into_iter().filter_map(|w| w.0) {
            if let Ok(mut vis) = visibility.get_mut(e) {
                *vis = if armed && p.weapon.is_some() { Visibility::Inherited } else { Visibility::Hidden };
            }
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
