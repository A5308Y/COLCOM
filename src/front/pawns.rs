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
use my_world::pawn::{Job, Pawn};
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
    /// The blanket over a character in bed (its body hidden underneath).
    blanket: Entity,
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
    cylinder: Handle<Mesh>,
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
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
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
    let blanket_mat = lit(materials, Rgb(p.color[0], p.color[1], p.color[2]).shade(0.8));
    // Placed at the bed's floor point each frame; the parts are relative to it
    // (bed head towards -Z): a lump over the body, and the blanket's edge
    // rolled over under the chin so the head doesn't look detached.
    let blanket = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(root))).id();
    let lump = Transform::from_xyz(0.0, 0.36, 0.13).with_scale(Vec3::new(0.54, 0.2, 0.68));
    commands.spawn(mesh(&a.sphere, blanket_mat.clone(), lump, blanket));
    let fold = Transform::from_xyz(0.0, 0.42, -0.17)
        .with_rotation(Quat::from_rotation_z(FRAC_PI_2))
        .with_scale(Vec3::new(0.12, 0.5, 0.12));
    commands.spawn(mesh(&a.cylinder, blanket_mat, fold, blanket));
    PawnVis {
        root,
        figure,
        rig,
        blanket,
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

/// Direction the pawn should face when not moving: the nearest enemy in a
/// battle, what it's working on, or the table it's eating at.
fn facing(p: &Pawn, w: &SimWorld) -> Option<f32> {
    let m = &w.map;
    let enemy = || {
        let b = w.battle.as_ref()?;
        b.unit_of(p.id)?;
        w.pawns
            .iter()
            .filter(|o| o.faction != p.faction && o.active() && b.unit_of(o.id).is_some())
            .min_by(|a, c| {
                let d = |o: &&Pawn| (o.x - p.x).powi(2) + (o.y - p.y).powi(2);
                d(a).total_cmp(&d(c))
            })
            .map(|o| (o.x, o.y))
    };
    let target = match p.job {
        // Whoever they're aiming at in an attack, else the nearest enemy.
        _ if w.battle.is_some() => w.aim_target(p.id).and_then(|t| w.pawn(t)).map(|t| (t.x, t.y)).or_else(enemy),
        Some(Job::Eat { seat: Some(seat), .. }) if seat == p.pos => m
            .neighbors(seat)
            .find(|&nb| m.feature[m.idx(nb)].is_special(Special::Table))
            .map(|t| (t.0 as f32, t.1 as f32)),
        Some(Job::Work { target, .. }) => Some((target.0 as f32, target.1 as f32)),
        Some(Job::Tend { patient: target, .. } | Job::Feed { patient: target, .. }) => {
            w.pawn(target).map(|t| (t.x, t.y))
        }
        _ => None,
    }?;
    let (dx, dz) = (target.0 - p.x, target.1 - p.y);
    (dx * dx + dz * dz > 1e-4).then(|| dx.atan2(dz))
}

/// Where a squad member stands within the squad's one cell, as an offset in
/// cells: in a row against the side with cover facing the enemy, else (and
/// while the squad walks, together) in a huddle.
fn squad_slot(p: &Pawn, w: &SimWorld) -> Vec2 {
    let Some(b) = &w.battle else { return Vec2::ZERO };
    let Some(unit) = b.unit_of(p.id).map(|u| &b.units[u]) else { return Vec2::ZERO };
    let n = unit.members.len();
    let Some(k) = unit.members.iter().position(|&id| id == p.id) else { return Vec2::ZERO };
    if n < 2 {
        return Vec2::ZERO;
    }
    let walking = !p.path.is_empty() || b.moving.iter().any(|m| m.pawn == p.id);
    let m = &w.map;
    // The side facing the nearest enemy, and what cover it has.
    let enemy = w
        .pawns
        .iter()
        .filter(|o| o.faction != p.faction && o.active() && b.unit_of(o.id).is_some())
        .min_by(|a, c| {
            let d = |o: &&Pawn| (o.x - p.x).powi(2) + (o.y - p.y).powi(2);
            d(a).total_cmp(&d(c))
        });
    let cover = enemy.filter(|_| !walking).and_then(|e| {
        let (dx, dy) = ((e.pos.0 as i32 - p.pos.0 as i32).signum(), (e.pos.1 as i32 - p.pos.1 as i32).signum());
        [(dx, 0), (0, dy)]
            .into_iter()
            .filter(|&d| d != (0, 0))
            .filter_map(|(cx, cy)| {
                let c = m.at(p.pos.0 as i32 + cx, p.pos.1 as i32 + cy, p.pos.2 as i32)?;
                let cover = w.cover_at(c);
                (cover != my_world::battle::Cover::None).then_some((cover, cx, cy))
            })
            .max_by_key(|c| c.0)
    });
    if let Some((_, cx, cy)) = cover {
        // Shoulder to shoulder against the cover.
        let toward = Vec2::new(cx as f32, cy as f32);
        let along = Vec2::new(-toward.y, toward.x);
        let t = k as f32 - (n as f32 - 1.0) / 2.0;
        return toward * 0.2 + along * t * 0.27;
    }
    const HUDDLE: [[(f32, f32); 4]; 3] = [
        [(-0.22, 0.0), (0.22, 0.0), (0.0, 0.0), (0.0, 0.0)],
        [(0.0, -0.2), (-0.22, 0.18), (0.22, 0.18), (0.0, 0.0)],
        [(-0.22, -0.22), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)],
    ];
    let (x, y) = HUDDLE[n.clamp(2, 4) - 2][k.min(3)];
    Vec2::new(x, y)
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
    let sim_rate = if sim.paused { 0.0 } else { sim.rate() as f32 };
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
        let target = Vec2::new(p.x, p.y) + squad_slot(p, w);
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
            // Upright along +Y; tip over so the head points along -Z (towards a
            // bed's pillow). A character in bed rests the back of its head on the pillow.
            let yaw = if in_bed { 0.0 } else { v.yaw };
            let turn = Quat::from_rotation_y(yaw);
            let (lift, along) = match (rig_ready, in_bed) {
                (true, true) => (0.44, 0.58),
                (true, false) => (0.08, 0.48),
                (false, true) => (0.52, 0.48),
                (false, false) => (0.15, 0.48),
            };
            Transform::from_translation(base + Vec3::Y * lift + turn * Vec3::new(0.0, 0.0, along))
                .with_rotation(turn * Quat::from_rotation_x(-FRAC_PI_2))
        } else {
            Transform::from_translation(base).with_rotation(Quat::from_rotation_y(v.yaw))
        };
        if let Ok(mut tf) = transforms.get_mut(v.root) {
            *tf = root_tf;
        }
        // In bed the character is tucked in: a blanket up to the chin, only the head showing.
        let tucked = rig_ready && in_bed;
        if let Ok(mut vis) = visibility.get_mut(v.blanket) {
            *vis = if tucked { Visibility::Inherited } else { Visibility::Hidden };
        }
        if tucked {
            let bed = Transform::from_translation(base);
            if let Ok(mut t) = transforms.get_mut(v.blanket) {
                *t = Transform::from_matrix(root_tf.to_matrix().inverse() * bed.to_matrix());
            }
        }
        if let Ok(mut vis) = visibility.get_mut(v.root) {
            *vis = if ui.shows_at(m, p.x, p.y, p.z) { Visibility::Inherited } else { Visibility::Hidden };
        }

        // Weapon in hand when fighting in a battle (civilians just keep their heads down).
        let armed = !lying
            && w.battle.as_ref().is_some_and(|b| b.unit_of(p.id).is_some_and(|u| !b.units[u].passive));

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
                let (a, rate) = characters::locomotion(v.speed, sim_rate, p.running);
                (a, rate, false)
            } else if let Some((a, _, fresh)) = v.action {
                (a, sim_rate, fresh)
            } else {
                (characters::activity(p, w, armed), sim_rate, false)
            };
            characters::play(&library, r, player, transitions, anim, restart, rate);
            characters::cover_body(r, in_bed, &mut visibility);
            characters::close_eyes(&mut commands, r, p.sleeping || p.downed || p.dead, &mut materials);
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
                // Through commands: the hand's weapon may have been spawned this frame.
                commands.entity(e).insert((tf, Mesh3d(mesh), MeshMaterial3d(mat)));
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
