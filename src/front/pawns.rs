//! Colonists and raiders kept in sync with the simulation: animated
//! character models (`characters.rs`), with the old code-built figure shown
//! until a character is ready or if the models can't load.

use super::characters::{self, Anim, CharacterLibrary, Pose, Rig};
use super::carry::CarryAnchor;
use super::input::UiState;
use super::look::*;
use super::terrain::{FLOOR_T, LEVEL_H};
use super::props::{self, lit, PropAssets};
use super::sim::Sim;
use super::tile_point;
use bevy::prelude::*;
use colcom::defs::{Bone, ItemId, Slot, Special, WornPiece};
use colcom::map::{Feature, Map, Pos, DIRS};
use colcom::recreation::GET_UP;
use colcom::pawn::{Job, Pawn, PawnId, Task};
use colcom::world::World as SimWorld;
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
    /// What's worn that the figure and character were last dressed for.
    dressed: Option<(Vec<ItemId>, bool)>,
    /// Armour pieces on the character's bones.
    armor: Vec<Entity>,
    carry_kind: Option<ItemId>,
    dead: bool,
    yaw: f32,
    /// 0 standing still .. 1 walking; eases in and out.
    swing: f32,
    /// Drawn height, which lags behind the real one while falling.
    render_y: f32,
    fall_speed: f32,
    /// Seated last frame, seconds of sitting down still to play, and how far
    /// the character is moved onto the seat (0..1, glides).
    sitting: bool,
    sit_enter: f32,
    sit_shift: f32,
}

#[derive(Resource, Default)]
struct PawnVisuals {
    generation: Option<u64>,
    map: HashMap<PawnId, PawnVis>,
}

#[derive(Resource)]
struct PawnAssets {
    torso: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    leg: Handle<Mesh>,
    arm: Handle<Mesh>,
    sphere: Handle<Mesh>,
}

pub struct PawnsPlugin;

impl Plugin for PawnsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PawnVisuals>().add_systems(Startup, setup).add_systems(Update, sync_pawns);
    }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    commands.insert_resource(PawnAssets {
        torso: meshes.add(Capsule3d::new(0.15, 0.22)),
        leg: meshes.add(Capsule3d::new(0.065, 0.2)),
        arm: meshes.add(Capsule3d::new(0.05, 0.22)),
        sphere: meshes.add(Sphere::new(0.5)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
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
    let weapon = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(figure))).id();
    // Placement is driven by the palms after the current animation is evaluated.
    let carry = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(root))).id();
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
        dressed: None,
        armor: Vec::new(),
        carry_kind: None,
        dead: false,
        yaw: 0.0,
        swing: 0.0,
        render_y: f32::NAN,
        fall_speed: 0.0,
        sitting: false,
        sit_enter: 0.0,
        sit_shift: 0.0,
    }
}

/// How a weapon sits in the figure's hands (and, `in_hand`, in the
/// character's right hand), from its definition, with its mesh and material.
fn weapon_pose(w: ItemId, props: &PropAssets, in_hand: bool) -> Option<(Transform, Handle<Mesh>, Handle<StandardMaterial>)> {
    let h = &w.def().weapon.as_ref()?.held;
    let Transform { rotation: turn, scale: size, .. } = props::part_transform(h);
    let tf = if in_hand {
        characters::hand_pose(size, turn, SCALE)
    } else {
        Transform::from_xyz(h.at.0, h.at.1, h.at.2).with_rotation(turn).with_scale(size)
    };
    Some((tf, props.mesh(h.shape).clone(), props.paint(&h.color)))
}

pub(super) fn equip_visual(commands: &mut Commands, props: &PropAssets, root: Entity, weapon: Option<ItemId>, in_hand: bool) {
    commands.entity(root).despawn_children();
    let Some(kind) = weapon else { return };
    let Some((tf, mesh, mat)) = weapon_pose(kind, props, in_hand) else { return };
    let fallback = commands.spawn((tf, Mesh3d(mesh), MeshMaterial3d(mat), Visibility::default(), ChildOf(root))).id();
    if let Some(model) = &kind.def().model {
        let pose = props::model_transform(&model.held);
        let tf = if in_hand {
            characters::model_hand_pose(pose)
        } else {
            let h = &kind.def().weapon.as_ref().unwrap().held;
            let old = props::part_transform(h);
            let axis = if old.scale.y > old.scale.z && old.scale.y > old.scale.x {
                Vec3::Y * old.scale.y
            } else {
                Vec3::Z * old.scale.z
            };
            let at = old.translation - old.rotation * axis.normalize() * (axis.length() / 2.0 - 0.1).max(0.0);
            Transform::from_translation(at)
                .with_scale(Vec3::splat(characters::RIG_SCALE / SCALE)) * pose
        };
        props::attach_item_model(commands, props, kind, root, tf, fallback);
    }
}

/// A suit's integrated helmet and separate headgear can occupy the same bone.
/// Draw the more protective one so their shells cannot overlap and flicker.
fn worn_pieces(apparel: &[ItemId]) -> Vec<&'static WornPiece> {
    let outer = apparel.iter().any(|k| k.def().apparel.as_ref().is_some_and(|a| a.slot == Slot::Outer && !a.pieces.is_empty()));
    let head = apparel.iter().filter(|k| k.def().apparel.as_ref().is_some_and(|a| a.pieces.iter().any(|p| p.bone == Bone::Head)))
        .max_by(|a, b| a.def().apparel.as_ref().unwrap().armor.total_cmp(&b.def().apparel.as_ref().unwrap().armor));
    apparel.iter().filter(|k| !outer || k.def().apparel.as_ref().is_none_or(|a| a.slot != Slot::Shirt)).flat_map(|kind| {
        kind.def().apparel.iter().flat_map(|a| &a.pieces).filter(move |piece| piece.bone != Bone::Head || head == Some(kind))
    }).collect()
}

fn skinned_apparel(apparel: &[ItemId]) -> Option<ItemId> {
    let outer = apparel.iter().any(|k| k.def().apparel.as_ref().is_some_and(|a| a.slot == Slot::Outer && !a.pieces.is_empty()));
    apparel.iter().copied().find(|k| k.def().apparel.as_ref().is_some_and(|a| a.skinned.is_some() && a.slot != Slot::Legs && (!outer || a.slot != Slot::Shirt)))
}

fn skinned_leg_apparel(apparel: &[ItemId]) -> Option<ItemId> {
    // A sealed suit supplies its own legs even for stale/conflicting save data.
    if skinned_apparel(apparel).is_some_and(|k| k.def().apparel.as_ref().unwrap().skinned.as_ref().unwrap().full_body) {
        return None;
    }
    apparel.iter().copied().find(|k| k.def().apparel.as_ref().is_some_and(|a| a.slot == Slot::Legs && a.skinned.is_some()))
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
        Some(Job::Relax { at, .. }) => Some((at.0 as f32, at.1 as f32)),
        // Sitting on a chair: the way it faces.
        Some(Job::Sit { at, .. }) if at == p.pos => match m.feature[m.idx(at)] {
            Feature::Built { kind, dir } => {
                let (dx, dy) = kind.def().front.turned(dir).offset();
                Some((at.0 as f32 + dx as f32, at.1 as f32 + dy as f32))
            }
            _ => None,
        },
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
                (cover != colcom::battle::Cover::None).then_some((cover, cx, cy))
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

/// Where someone using furniture pushed back against a wall is drawn moved
/// along with it (see `props::snug`): asleep in such a bed, or working at
/// such a bench from in front of it.
fn snugged(p: &Pawn, m: &Map) -> Vec2 {
    if p.lying() && !p.is_dead() && m.feature[m.idx(p.pos)].is_bed() {
        return props::built_snug(m, p.pos);
    }
    match p.job {
        Some(Job::Work { target, task: Task::Craft }) if p.path.is_empty() && m.front_cell(target) == Some(p.pos) => {
            props::built_snug(m, target)
        }
        _ => Vec2::ZERO,
    }
}

/// The way a sleeper in the bed at `at` lies: along the bed, as it's turned.
fn bed_yaw(m: &Map, at: Pos) -> f32 {
    match m.feature[m.idx(at)] {
        Feature::Built { dir, .. } => {
            let (dx, dy) = dir.offset();
            (-dy as f32).atan2(dx as f32)
        }
        _ => 0.0,
    }
}

/// Height (in levels) of someone at (x, y) on level `z` (fractional, as pawns
/// move) who is on stairs: on the stairs' own cell, the height of the steps
/// under them; past the top while still on their way up or down, the level
/// above. None elsewhere.
fn stairs_height(m: &Map, x: f32, y: f32, z: f32) -> Option<f32> {
    let level = (z + 1e-4).floor();
    let (cx, cy) = (x.round() as i32, y.round() as i32);
    let ramp = |cx: i32, cy: i32| m.at(cx, cy, level as i32).and_then(|c| m.ramp_dir(c));
    if let Some((sx, sy)) = ramp(cx, cy) {
        let along = (x - cx as f32) * sx as f32 + (y - cy as f32) * sy as f32;
        return Some(level + (along + 0.5).clamp(0.0, 1.0));
    }
    // Stepped off the top, the move to the next level not quite done.
    let between = z - level > 0.001;
    let behind = DIRS.iter().any(|&(sx, sy)| ramp(cx - sx, cy - sy) == Some((sx, sy)));
    (between && behind).then_some(level + 1.0)
}

/// Turn from `a` towards `b` by at most `max` radians.
fn turn_towards(a: f32, b: f32, max: f32) -> f32 {
    let diff = (b - a + PI).rem_euclid(TAU) - PI;
    a + diff.clamp(-max, max)
}

fn sync_apparel(
    commands: &mut Commands,
    props: &PropAssets,
    p: &Pawn,
    v: &mut PawnVis,
    rig: Option<&mut Rig>,
    visibility: &mut Query<&mut Visibility>,
) {
    let rig_ready = rig.is_some();
    // Armour and helmets: pieces on the character's bones (once it's ready), and on the figure.
    let wearing = (p.apparel.clone(), rig_ready);
    if v.dressed.as_ref() != Some(&wearing) {
        for e in v.armor.drain(..) {
            commands.entity(e).despawn();
        }
        let pieces = worn_pieces(&p.apparel);
        for piece in &pieces {
            let (parent, at, turn, proportions) = match piece.bone {
                Bone::Spine03 => (v.figure, Vec3::new(0.0, 0.62, 0.0), Quat::IDENTITY, Vec3::ONE),
                Bone::Head => (v.figure, Vec3::new(0.0, 0.90, 0.0), Quat::IDENTITY, Vec3::ONE),
                Bone::UpperArmL => (v.arms[1], Vec3::ZERO, Quat::from_rotation_z(-FRAC_PI_2), Vec3::ONE),
                Bone::UpperArmR => (v.arms[0], Vec3::ZERO, Quat::from_rotation_z(FRAC_PI_2), Vec3::ONE),
                Bone::LowerArmL => (v.arms[1], Vec3::new(0.0, -0.16, 0.0), Quat::from_rotation_z(-FRAC_PI_2), Vec3::ONE),
                Bone::LowerArmR => (v.arms[0], Vec3::new(0.0, -0.16, 0.0), Quat::from_rotation_z(FRAC_PI_2), Vec3::ONE),
                // The fallback figure has shorter legs than the character rigs.
                Bone::Pelvis => (v.figure, Vec3::new(0.0, 0.36, 0.0), Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                Bone::ThighL => (v.legs[1], Vec3::ZERO, Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                Bone::ThighR => (v.legs[0], Vec3::ZERO, Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                Bone::CalfL => (v.legs[1], Vec3::new(0.0, -0.17, 0.0), Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                Bone::CalfR => (v.legs[0], Vec3::new(0.0, -0.17, 0.0), Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                Bone::FootL => (v.legs[1], Vec3::new(0.0, -0.31, 0.0), Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                Bone::FootR => (v.legs[0], Vec3::new(0.0, -0.31, 0.0), Quat::IDENTITY, Vec3::new(1.0, 0.62, 1.0)),
                // Nothing is worn on the hands; the fallback figure has none.
                Bone::HandL | Bone::HandR => continue,
            };
            let tf = Transform::from_translation(at).with_rotation(turn).with_scale(proportions * (characters::RIG_SCALE / SCALE));
            let e = props::spawn_worn(commands, props, tf, piece);
            commands.entity(e).insert(ChildOf(parent));
            v.armor.push(e);
        }
        if let Some(r) = rig {
            r.reset_garment(commands);
            let fabric = skinned_apparel(&p.apparel).map(|k| k.def().apparel.as_ref().unwrap());
            let leg_fabric = skinned_leg_apparel(&p.apparel).map(|k| k.def().apparel.as_ref().unwrap());
            let mut fabric_fallbacks = Vec::new();
            let mut leg_fallbacks = Vec::new();
            for piece in &pieces {
                let Some((bone, rest)) = r.bone(piece.bone) else {
                    warn!("no bone {:?} for armour", piece.bone);
                    continue;
                };
                // Undo the bone's turn at rest, so the parts sit in the character's own frame.
                let e = props::spawn_worn(commands, props, Transform::from_rotation(rest.inverse()), piece);
                commands.entity(e).insert(ChildOf(bone));
                v.armor.push(e);
                if piece.bone != Bone::Head && fabric.is_some_and(|a| a.pieces.iter().any(|p| std::ptr::eq(p, *piece))) {
                    fabric_fallbacks.push(e);
                }
                if leg_fabric.is_some_and(|a| a.pieces.iter().any(|p| std::ptr::eq(p, *piece))) {
                    leg_fallbacks.push(e);
                }
            }
            if let Some(skin) = fabric.and_then(|a| a.skinned.as_ref()) {
                let path = if r.female() { &skin.female } else { &skin.male };
                let e = characters::spawn_garment(commands, props.model(path, 0), v.rig, r, fabric_fallbacks, skin.full_body);
                v.armor.push(e);
            }
            if let Some(skin) = leg_fabric.and_then(|a| a.skinned.as_ref()) {
                let path = if r.female() { &skin.female } else { &skin.male };
                let e = characters::spawn_leg_garment(commands, props.model(path, 0), v.rig, r, leg_fallbacks);
                v.armor.push(e);
            }
            r.show_hair(!pieces.iter().any(|w| w.hides_hair), visibility);
        }
        v.dressed = Some(wearing);
    }
}

enum Posture {
    Collapsed,
    Lying { in_bed: bool },
    Upright,
}

struct PawnMotion {
    posture: Posture,
    moving: bool,
    falling: bool,
}

#[allow(clippy::too_many_arguments)]
fn position_pawn(
    p: &Pawn, v: &mut PawnVis, w: &SimWorld, rig_ready: bool,
    dt: f32, sim_rate: f32,
    transforms: &mut Query<&mut Transform>, visibility: &mut Query<&mut Visibility>,
) -> PawnMotion {
    let m = &w.map;
    // The drawn position trails the simulated one a little, which rounds
    // corners and eases into and out of walking. Big jumps snap.
    let target = Vec2::new(p.x, p.y) + squad_slot(p, w) + snugged(p, m);
    let before = v.shown;
    if v.shown.distance(target) > 2.0 || sim_rate == 0.0 && v.shown.distance(target) > 0.5 {
        v.shown = target;
    } else {
        v.shown += (target - v.shown) * (1.0 - (-dt * FOLLOW * sim_rate.max(1.0)).exp());
    }
    let velocity = if dt > 0.0 { (v.shown - before) / dt } else { Vec2::ZERO };
    v.speed += (velocity.length() - v.speed) * (dt * 10.0).min(1.0);

    let lying = p.is_dead() || p.lying();
    let in_bed = lying && !p.is_dead() && m.feature[m.idx(p.pos)].is_bed();
    // The character collapses on the ground when downed or killed; in
    // bed, asleep or resting it (like the figure) is laid down flat.
    let collapsed = rig_ready && (p.is_dead() || p.is_downed()) && !in_bed;
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
    match stairs_height(m, v.shown.x, v.shown.y, p.z) {
        // Up or down the steps of stairs.
        Some(h) => base.y = h * LEVEL_H,
        None => {
            // A little hop when stepping up or down a level.
            let frac = p.z - p.z.floor();
            if frac > 0.001 && frac < 0.999 {
                base.y += 0.35 * 4.0 * frac * (1.0 - frac);
            }
        }
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
        let yaw = if in_bed { bed_yaw(m, p.pos) } else { v.yaw };
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
        let bed = Transform::from_translation(base).with_rotation(Quat::from_rotation_y(bed_yaw(m, p.pos)));
        if let Ok(mut t) = transforms.get_mut(v.blanket) {
            *t = Transform::from_matrix(root_tf.to_matrix().inverse() * bed.to_matrix());
        }
    }
    PawnMotion {
        posture: if collapsed { Posture::Collapsed } else if lying { Posture::Lying { in_bed } } else { Posture::Upright },
        moving,
        falling,
    }
}

fn sync_held_equipment(
    commands: &mut Commands, props: &PropAssets, p: &Pawn, v: &mut PawnVis,
    transforms: &mut Query<&mut Transform>, visibility: &mut Query<&mut Visibility>,
    armed: bool,
) {
    let lying = p.is_dead() || p.lying();
    // Carrying also raises the fallback figure's arms to support the stack.
    let carrying = p.carrying.is_some() && !lying;
    let stride = ((p.x + p.y) * PI * 1.6).sin() * 0.6 * v.swing;
    let swings = [
        (v.legs[0], stride),
        (v.legs[1], -stride),
        (v.arms[0], if carrying { -FRAC_PI_2 } else if armed { -1.2 } else { -stride * 0.8 }),
        (v.arms[1], if carrying { -FRAC_PI_2 } else if armed { -1.3 } else { stride * 0.8 }),
    ];
    for (pivot, angle) in swings {
        if let Ok(mut t) = transforms.get_mut(pivot) {
            let inward = if carrying && pivot == v.arms[0] { 0.35 }
                else if carrying && pivot == v.arms[1] { -0.35 } else { 0.0 };
            t.rotation = Quat::from_rotation_y(inward) * Quat::from_rotation_x(angle);
        }
    }
    let weapons = [(Some(v.weapon), false), (v.rig_weapon, true)];
    if v.weapon_kind != Some(p.weapon) {
        for (e, in_hand) in weapons {
            if let Some(e) = e {
                equip_visual(commands, props, e, p.weapon, in_hand);
            }
        }
        v.weapon_kind = Some(p.weapon);
    }
    for e in weapons.into_iter().filter_map(|w| w.0) {
        if let Ok(mut vis) = visibility.get_mut(e) {
            *vis = if armed && p.weapon.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
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
    mut rigs: Query<(&mut Rig, &mut AnimationPlayer)>,
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
        let keep = w.pawn(*id).is_some_and(|p| !p.has_left() && !p.is_buried());
        if !keep {
            commands.entity(v.root).despawn();
        }
        keep
    });

    let dt = time.delta_secs();
    // Simulation seconds per real second: animations other than walking run at this rate.
    let sim_rate = sim.effective_rate();
    for p in w.pawns.iter().filter(|p| !p.has_left() && !p.is_buried()) {
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

        sync_apparel(&mut commands, &props, p, v, rig.as_mut().filter(|_| rig_ready).map(|(r, _)| &mut **r), &mut visibility);

        if p.is_dead() && !v.dead {
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
        if let Some((r, ..)) = rig.as_mut().filter(|_| rig_ready && v.rig_dead != p.is_dead()) {
            characters::set_dead(r, p.is_dead(), &mut materials);
            v.rig_dead = p.is_dead();
        }

        let PawnMotion { posture, moving, falling } = position_pawn(p, v, w, rig_ready, dt, sim_rate, &mut transforms, &mut visibility);
        let (lying, in_bed, collapsed, tipped) = match posture {
            Posture::Collapsed => (true, false, true, false),
            Posture::Lying { in_bed } => (true, in_bed, false, true),
            Posture::Upright => (false, false, false, false),
        };
        if let Ok(mut vis) = visibility.get_mut(v.root) {
            *vis = if ui.shows_at(m, p.x, p.y, p.z) { Visibility::Inherited } else { Visibility::Hidden };
        }

        // Weapon in hand when fighting in a battle (civilians just keep their heads down).
        let armed = !lying
            && w.battle.as_ref().is_some_and(|b| b.unit_of(p.id).is_some_and(|u| !b.units[u].is_civilians()));

        // An attack starts the weapon's cooldown: play the shot or blow
        // (a deployed squad fires from the knee: the shot over kneeling legs).
        let deployed = w.battle.as_ref().is_some_and(|b| b.unit_of(p.id).is_some_and(|u| b.units[u].is_deployed()));
        if p.cooldown > v.last_cooldown + 0.05 && !lying {
            let anim = characters::attack_anim(p);
            v.action = Some((anim, library.duration(anim), true));
        }
        v.last_cooldown = p.cooldown;

        // Sitting down onto a chair plays first; getting up plays while the simulation waits for it.
        let seated = w.seated(p);
        if seated && !v.sitting {
            v.sit_enter = library.duration(Anim::SitEnter);
        }
        v.sitting = seated;
        if let Some((r, player)) = rig.as_mut().filter(|_| rig_ready) {
            let (pose, rate, restart) = if collapsed {
                (Pose::full(Anim::Down), sim_rate, false)
            } else if tipped {
                (Pose::full(Anim::Idle), 0.4 * sim_rate, false)
            } else if falling {
                (Pose::full(Anim::Fall), 1.0, false)
            } else if moving {
                let (a, rate) = characters::locomotion(v.speed, sim_rate, p.running);
                (Pose::full(a), rate, false)
            } else if let Some((a, _, fresh)) = v.action {
                (if deployed { Pose::split(Anim::Kneel, a) } else { Pose::full(a) }, sim_rate, fresh)
            } else if seated && v.sit_enter > 0.0 {
                (Pose::full(Anim::SitEnter), sim_rate, false)
            } else if !seated && p.getting_up > 0.0 {
                let rate = library.duration(Anim::SitExit) / GET_UP;
                (Pose::full(Anim::SitExit), rate * sim_rate, false)
            } else {
                (characters::activity(p, w, armed), sim_rate, false)
            };
            // Arms out in front for whatever they carry.
            let pose = if p.carrying.is_some() && !lying { pose.carrying() } else { pose };
            characters::play(&library, r, player, pose, restart, rate, dt);
            characters::cover_body(r, in_bed, &mut visibility);
            characters::close_eyes(&mut commands, r, p.sleeping || p.is_downed() || p.is_dead(), &mut materials);
            // Sitting leans back onto the seat: move forward and up onto the chair (gliding there and back).
            let target = if pose.sitting() { 1.0 } else { 0.0 };
            v.sit_shift += (target - v.sit_shift) * (dt * 4.0 * sim_rate.max(1.0)).min(1.0);
            if let Ok(mut t) = transforms.get_mut(v.rig) {
                t.translation = Vec3::new(0.0, 0.07, 0.18) * v.sit_shift;
            }
            if v.rig_weapon.is_none() {
                if let Some(hand) = r.hand {
                    let e = commands.spawn((Transform::default(), Visibility::Hidden, ChildOf(hand)));
                    v.rig_weapon = Some(e.id());
                    v.weapon_kind = None;
                }
            }
        }

        v.sit_enter = (v.sit_enter - dt * sim_rate).max(0.0);
        if let Some((_, left, fresh)) = &mut v.action {
            *fresh = false;
            *left -= dt * sim_rate;
            if *left <= 0.0 {
                v.action = None;
            }
        }

        sync_held_equipment(&mut commands, &props, p, v, &mut transforms, &mut visibility, armed);

        // Whatever they're carrying, held in front.
        let carried = p.carrying.map(|s| s.kind).filter(|_| !lying);
        if carried.is_some() {
            let anchor = rig.as_ref().filter(|_| rig_ready).and_then(|(r, _)| {
                Some(CarryAnchor::rig(v.root, r.bone(Bone::HandL)?.0, r.bone(Bone::HandR)?.0))
            }).unwrap_or_else(|| CarryAnchor::figure(v.root, v.arms));
            commands.entity(v.carry).insert(anchor);
        } else {
            commands.entity(v.carry).remove::<CarryAnchor>();
        }
        if carried != v.carry_kind {
            props::replace_carried_item(&mut commands, &props, v.carry, carried);
            commands.entity(v.carry).insert(if carried.is_some() { Visibility::Inherited } else { Visibility::Hidden });
            v.carry_kind = carried;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colcom::map::{Feature, FLAT_Z as Z};

    #[test]
    fn battle_models_keep_finn_and_bram_armed() {
        use bevy::render::{RenderPlugin, settings::{RenderCreation, WgpuSettings}};
        use bevy::time::TimeUpdateStrategy;
        use bevy::winit::WinitPlugin;
        use bevy_egui::EguiPlugin;
        use std::time::{Duration, Instant};
        let mut app = App::new();
        app.add_plugins(DefaultPlugins.build().disable::<WinitPlugin>().set(RenderPlugin {
            render_creation: RenderCreation::Automatic(Box::new(WgpuSettings { backends: None, ..default() })),
            ..default()
        }))
            .add_plugins(EguiPlugin::default())
            .add_plugins(super::super::FrontPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            sim.scenario = Some("battle".into());
            let world = sim.fresh_world();
            sim.replace(world);
        }
        // Asset loaders register in finish(), even without a GPU. Merely calling
        // update() exercises fallback figures, not the imported character rigs.
        app.finish();
        app.cleanup();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            app.update();
            let world = app.world();
            let visuals = world.resource::<PawnVisuals>();
            let loaded = !visuals.map.is_empty() && visuals.map.values().all(|v| {
                world.get::<Rig>(v.rig).is_some_and(Rig::ready)
                    && v.rig_weapon.is_some_and(|root| {
                        world.get::<Children>(root).is_some_and(|children| children.iter().any(|child| {
                            world.get::<bevy::world_serialization::WorldAssetRoot>(child).is_some()
                                && world.get::<Children>(child).is_some()
                        }))
                    })
            });
            if loaded { break; }
            assert!(Instant::now() < deadline, "battle character and weapon assets did not load");
            std::thread::sleep(Duration::from_millis(5));
        }
        for _ in 0..5 { app.update(); }
        let world = app.world();
        let sim = world.resource::<Sim>();
        let visuals = world.resource::<PawnVisuals>();
        for pawn in sim.world.pawns.iter().filter(|p| {
            p.weapon.is_some() && sim.world.battle.as_ref().unwrap().unit_of(p.id)
                .is_some_and(|u| !sim.world.battle.as_ref().unwrap().units[u].is_civilians())
        }) {
            let name = &pawn.name;
            let v = &visuals.map[&pawn.id];
            let root = v.rig_weapon.unwrap();
            let hand = world.get::<Rig>(v.rig).unwrap().hand.unwrap();
            assert_eq!(world.get::<ChildOf>(root).unwrap().parent(), hand);
            assert!(world.get::<InheritedVisibility>(root).unwrap().get(), "{name}'s weapon must inherit visible ancestors");
            let mut stack = vec![root];
            let mut visible_model_meshes = 0;
            while let Some(e) = stack.pop() {
                if world.get::<Mesh3d>(e).is_some() && world.get::<InheritedVisibility>(e).is_some_and(|v| v.get()) {
                    visible_model_meshes += 1;
                }
                if let Some(children) = world.get::<Children>(e) { stack.extend(children.iter()); }
            }
            assert!(visible_model_meshes > 0, "{name}'s equipped gun model must be visible");
        }
        for pawn in sim.world.pawns.iter().filter(|p| p.apparel.contains(&ItemId::named("powered_armor"))) {
            let mut stack = vec![visuals.map[&pawn.id].rig];
            let mut checked = 0;
            while let Some(e) = stack.pop() {
                if world.get::<bevy::gltf::GltfMaterialName>(e).is_some_and(|n| n.0.starts_with("MI_")) {
                    assert!(!world.get::<InheritedVisibility>(e).unwrap().get(),
                            "{}'s full suit exposes a native body/outfit mesh {:?}", pawn.name, world.get::<Name>(e));
                    checked += 1;
                }
                if let Some(children) = world.get::<Children>(e) { stack.extend(children.iter()); }
            }
            assert!(checked > 0, "loaded the native body to check its visibility");
        }
    }

    #[test]
    fn skinned_apparel_follow_outer_layer_selection() {
        let clothes = ItemId::named("clothes");
        let parka = ItemId::named("parka");
        assert_eq!(skinned_apparel(&[clothes]), Some(clothes));
        assert_eq!(skinned_apparel(&[clothes, parka]), Some(parka));
        assert_eq!(skinned_apparel(&[clothes, ItemId::named("steel_armor")]), None);
        let powered = ItemId::named("powered_armor");
        assert_eq!(skinned_apparel(&[clothes, powered]), Some(powered));
        assert_eq!(skinned_apparel(&[powered, clothes]), Some(powered));
    }

    #[test]
    fn leg_apparel_layers_with_tops_but_not_a_full_suit() {
        for legs in ["cargo_trousers", "flak_leg_guards"] {
            let legs = ItemId::named(legs);
            for top in ["clothes", "parka", "steel_armor"] {
                let top = ItemId::named(top);
                for items in [[legs, top], [top, legs]] {
                    assert_eq!(skinned_leg_apparel(&items), Some(legs));
                    assert_ne!(skinned_apparel(&items), Some(legs), "legs cannot displace an upper garment");
                }
            }
            assert_eq!(skinned_leg_apparel(&[legs]), Some(legs));
            let suit = ItemId::named("powered_armor");
            for items in [[suit, legs], [legs, suit]] {
                assert_eq!(skinned_apparel(&items), Some(suit));
                assert_eq!(skinned_leg_apparel(&items), None, "sealed suits own their leg coverage");
            }
        }
    }

    #[test]
    fn integrated_helmets_do_not_overlap_separate_headgear() {
        let powered = ItemId::named("powered_armor");
        let helmet = ItemId::named("steel_helmet");
        let hat = ItemId::named("wool_hat");
        for apparel in [vec![powered, helmet], vec![helmet, powered], vec![hat, powered]] {
            let pieces = worn_pieces(&apparel);
            let heads: Vec<_> = pieces.iter().filter(|p| p.bone == Bone::Head).collect();
            assert_eq!(heads.len(), 1);
            assert!(heads[0].model.as_ref().unwrap().path.ends_with("PoweredArmor.gltf"));
            assert_eq!(pieces.len(), 13, "all articulated body pieces are still present");
        }
        let pieces = worn_pieces(&[ItemId::named("steel_armor"), helmet]);
        assert_eq!(pieces.iter().filter(|p| p.bone == Bone::Head).count(), 1);
        assert!(pieces.iter().any(|p| p.model.as_ref().is_some_and(|m| m.path.ends_with("SteelHelmet.gltf"))));
    }

    #[test]
    fn outer_layers_cover_the_utility_jacket_without_duplicate_collars() {
        let clothes = ItemId::named("clothes");
        assert_eq!(worn_pieces(&[clothes]).len(), 3);
        for outer in ["parka", "steel_armor", "powered_armor"] {
            let pieces = worn_pieces(&[clothes, ItemId::named(outer)]);
            assert!(pieces.iter().all(|p| !p.model.as_ref().unwrap().path.ends_with("SimpleClothes.gltf")));
            assert!(pieces.iter().any(|p| p.bone == Bone::Spine03));
        }
    }

    #[test]
    fn people_on_stairs_walk_up_the_steps() {
        let mut m = Map::flat(8, 3);
        let i = m.idx((3, 1, Z));
        m.set_feature(i, Feature::built("stairs")); // rising towards +x
        let z = Z as f32;
        assert_eq!(stairs_height(&m, 1.0, 1.0, z), None, "not on them");
        assert_eq!(stairs_height(&m, 2.5, 1.0, z), Some(z), "at the foot");
        assert_eq!(stairs_height(&m, 3.0, 1.0, z), Some(z + 0.5), "halfway");
        assert!((stairs_height(&m, 3.4, 1.0, z + 0.4).unwrap() - (z + 0.9)).abs() < 1e-4, "nearly at the top");
        assert_eq!(stairs_height(&m, 3.7, 1.0, z + 0.7), Some(z + 1.0), "stepped off onto the level above");
        // Ladders are climbed as before.
        m.set_feature(i, Feature::built("ladder"));
        assert_eq!(stairs_height(&m, 3.0, 1.0, z), None);
    }
}
