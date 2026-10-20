//! Rigged, animated character models for pawns (phase 10b): Quaternius
//! outfits with a head cut from the base body (`tools/make_heads.py`) and
//! hair, animated by the Universal Animation Library.
//!
//! The character files carry no animations, so when a part is spawned its
//! bones are named by path for the library's clips and handed to the
//! character's one `AnimationPlayer` (see `rig_part`). Until every part is
//! in place (or if the models can't load, e.g. without a GPU) the pawn keeps
//! its code-built figure.

use super::look::{hash, pawn_look, Rgb};
use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::gltf::{Gltf, GltfAssetLabel, GltfMaterialName, GltfNode};
use bevy::prelude::*;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use my_world::defs::ItemId;
use my_world::map::{Desig, Feature};
use my_world::pawn::{Faction, Job, Pawn, Task};
use my_world::world::World as SimWorld;
use std::collections::HashMap;
use std::time::Duration;

/// Model units (metres) to cells: a 1.8 m character is about one cell tall.
pub const RIG_SCALE: f32 = 0.55;
const LIBRARY: &str = "models/characters/animations/UAL1_Standard.glb";
/// Second colour scheme for the peasant outfit, for variety.
const PEASANT_2: &str = "models/characters/outfits/T_Peasant_2_BaseColor.png";
/// Skin colour the body textures are painted in; other skin tones tint relative to it.
const TEXTURE_SKIN: Rgb = Rgb(0.93, 0.74, 0.58);
/// World rotation of the right hand bone while aiming (`Pistol_Idle_Loop`),
/// measured from the library. Its inverse turns a weapon held in the hand so
/// it points forward (+Z) with its top up (+Y) in that pose.
const AIM_HAND: Quat = Quat::from_xyzw(-0.007, 0.77, 0.638, -0.025);
/// How fast each locomotion clip moves, in cells per second (root motion of
/// the clip in metres per cycle / cycle length × `RIG_SCALE`).
const WALK_SPEED: f32 = 1.3 / 1.333 * RIG_SCALE;
const JOG_SPEED: f32 = 5.0 / 0.933 * RIG_SCALE;
const SPRINT_SPEED: f32 = 5.5 / 0.667 * RIG_SCALE;
/// Cross-fade between animations.
const BLEND: Duration = Duration::from_millis(250);

/// What a character's body is doing.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Anim {
    Idle,
    Walk,
    Jog,
    Sprint,
    /// Chopping and mining: an overhead swing, looped.
    Swing,
    /// Kneeling work: sowing, harvesting crops, floors, tending patients.
    Kneel,
    /// Standing work at a wall or workbench.
    Interact,
    /// Eating without a table.
    Crouch,
    Sit,
    /// Ranged weapon ready.
    Aim,
    /// Melee weapon ready.
    Guard,
    Shoot,
    Strike,
    Punch,
    /// Collapsing and lying on the ground (downed, dead); holds the last frame.
    Down,
    Fall,
    /// Sitting down onto a chair, and getting up again (once each).
    SitEnter,
    SitExit,
    /// Sitting and gesturing (playing chess).
    SitTalk,
    /// Arms held out in front, for the upper body while carrying something.
    Carry,
}

/// Which bones a clip node animates: all of them, only the legs and hips
/// (`Lower`), or only the spine, arms and head (`Upper`, from `spine_01` up).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Layer {
    Full,
    Lower,
    Upper,
}

/// Mask groups of the library's bones.
const UPPER_GROUP: u32 = 0;
const LOWER_GROUP: u32 = 1;
/// The bone the upper body hangs from.
const UPPER_ROOT: &str = "spine_01";

/// What a character's body is doing: one clip for all of it, or (`upper`)
/// the legs from `body` and the upper body from another clip, e.g. kneeling
/// while aiming, or walking with the arms held out to carry something.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pose {
    pub body: Anim,
    pub upper: Option<Anim>,
}

impl Pose {
    pub fn full(body: Anim) -> Pose {
        Pose { body, upper: None }
    }

    pub fn split(body: Anim, upper: Anim) -> Pose {
        Pose { body, upper: Some(upper) }
    }

    /// Any part of it sitting on a chair (the character is moved onto the seat).
    pub fn sitting(self) -> bool {
        matches!(self.body, Anim::Sit | Anim::SitTalk | Anim::SitEnter | Anim::SitExit)
    }

    /// The same, with the arms held out to carry something (over walking or standing).
    pub fn carrying(self) -> Pose {
        match (self.body, self.upper) {
            (Anim::Idle | Anim::Walk | Anim::Jog | Anim::Sprint, None) => Pose::split(self.body, Anim::Carry),
            _ => self,
        }
    }

    /// The clip nodes it plays, with their layers.
    fn layers(self) -> Vec<(Anim, Layer)> {
        match self.upper {
            None => vec![(self.body, Layer::Full)],
            Some(u) => vec![(self.body, Layer::Lower), (u, Layer::Upper)],
        }
    }
}

impl Anim {
    const ALL: [Anim; 20] = [
        Anim::Idle,
        Anim::Walk,
        Anim::Jog,
        Anim::Sprint,
        Anim::Swing,
        Anim::Kneel,
        Anim::Interact,
        Anim::Crouch,
        Anim::Sit,
        Anim::Aim,
        Anim::Guard,
        Anim::Shoot,
        Anim::Strike,
        Anim::Punch,
        Anim::Down,
        Anim::Fall,
        Anim::SitEnter,
        Anim::SitExit,
        Anim::SitTalk,
        Anim::Carry,
    ];

    fn clip(self) -> &'static str {
        match self {
            Anim::Idle => "Idle_Loop",
            Anim::Walk => "Walk_Loop",
            Anim::Jog => "Jog_Fwd_Loop",
            Anim::Sprint => "Sprint_Loop",
            Anim::Swing | Anim::Strike => "Sword_Attack",
            Anim::Kneel => "Fixing_Kneeling",
            Anim::Interact => "Interact",
            Anim::Crouch => "Crouch_Idle_Loop",
            Anim::Sit => "Sitting_Idle_Loop",
            Anim::Aim => "Pistol_Idle_Loop",
            Anim::Guard => "Sword_Idle",
            Anim::Shoot => "Pistol_Shoot",
            Anim::Punch => "Punch_Cross",
            Anim::Down => "Death01",
            Anim::Fall => "Jump_Loop",
            Anim::SitEnter => "Sitting_Enter",
            Anim::SitExit => "Sitting_Exit",
            Anim::SitTalk => "Sitting_Talking_Loop",
            // Arms forward at chest height, as if holding a wheel: holding a load.
            Anim::Carry => "Driving_Loop",
        }
    }

    /// Played once (and held at the end) rather than looped.
    fn once(self) -> bool {
        matches!(self, Anim::Shoot | Anim::Strike | Anim::Punch | Anim::Down | Anim::SitEnter | Anim::SitExit)
    }

    /// The clip's own speed in cells per second, for locomotion.
    fn moves(self) -> Option<f32> {
        match self {
            Anim::Walk => Some(WALK_SPEED),
            Anim::Jog => Some(JOG_SPEED),
            Anim::Sprint => Some(SPRINT_SPEED),
            _ => None,
        }
    }
}

/// The animation library, once loaded: one graph with every clip we use.
#[derive(Resource)]
pub struct CharacterLibrary {
    gltf: Handle<Gltf>,
    peasant_2: Handle<Image>,
    graph: Option<Handle<AnimationGraph>>,
    /// Each clip three times: for the whole body, the legs only, the upper body only.
    nodes: HashMap<(Anim, Layer), (AnimationNodeIndex, f32)>,
    /// Which clip each node plays.
    clip_of: HashMap<AnimationNodeIndex, &'static str>,
}

impl CharacterLibrary {
    /// Length of an animation in seconds.
    pub fn duration(&self, a: Anim) -> f32 {
        self.nodes.get(&(a, Layer::Full)).map_or(1.0, |n| n.1)
    }
}

/// On a pawn's character entity (which holds its `AnimationPlayer`).
#[derive(Component)]
pub struct Rig {
    /// Parts still being spawned.
    pending: usize,
    /// The right hand bone, where weapons go.
    pub hand: Option<Entity>,
    /// The outfit part (the body below the head), hidden under a blanket in bed.
    outfit: Entity,
    covered: bool,
    dead: bool,
    female: bool,
    /// Native torso/arms are replaced only after deforming clothing is ready.
    upper_body: Vec<Entity>,
    garment: Option<Entity>,
    garment_ready: bool,
    garment_materials: Vec<AssetId<StandardMaterial>>,
    /// Eye meshes and their own material; closed eyes are painted like eyelids.
    eyes: Vec<(Entity, Handle<StandardMaterial>)>,
    lids: Option<Handle<StandardMaterial>>,
    lid_color: Color,
    closed: bool,
    clothes: Color,
    skin: Color,
    hair: Color,
    /// Use the second peasant colour scheme.
    variant: bool,
    /// This character's own materials and their living colour, for greying on death.
    own: Vec<(Handle<StandardMaterial>, Color)>,
    /// Bones by name, with their turn in the character's frame at rest (for armour).
    bones: HashMap<String, (Entity, Quat)>,
    /// Garment bind matrices match the outfit, not the head/hair skeletons.
    outfit_bones: HashMap<String, Entity>,
    /// Hair parts (not the beard), hidden under a helmet.
    hair_parts: Vec<Entity>,
    /// The animation graph is attached.
    animated: bool,
    /// Clip nodes playing and their weight, fading in (wanted) or out.
    layers: Vec<(AnimationNodeIndex, f32)>,
}

impl Rig {
    /// Every part is spawned and the animations are attached.
    pub fn ready(&self) -> bool {
        self.pending == 0 && self.animated
    }
}

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, build_library);
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(CharacterLibrary {
        gltf: assets.load(LIBRARY),
        peasant_2: assets.load(PEASANT_2),
        graph: None,
        nodes: HashMap::new(),
        clip_of: HashMap::new(),
    });
}

/// Once the library is loaded, put the clips we use in one graph: each
/// clip for the whole body, and masked to the legs or the upper body (bones
/// grouped by whether they hang from `spine_01`).
fn build_library(
    mut library: ResMut<CharacterLibrary>,
    gltfs: Res<Assets<Gltf>>,
    gltf_nodes: Res<Assets<GltfNode>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
) {
    if library.graph.is_some() {
        return;
    }
    let Some(gltf) = gltfs.get(&library.gltf) else {
        return;
    };
    let mut graph = AnimationGraph::new();
    match gltf.named_nodes.get("Armature").and_then(|h| gltf_nodes.get(h)) {
        Some(armature) => {
            let mut stack = vec![(armature, vec![Name::new(armature.name.clone())], false)];
            while let Some((node, path, upper)) = stack.pop() {
                let upper = upper || node.name == UPPER_ROOT;
                let group = if upper { UPPER_GROUP } else { LOWER_GROUP };
                graph.add_target_to_mask_group(AnimationTargetId::from_names(path.iter()), group);
                for child in node.children.iter().filter_map(|h| gltf_nodes.get(h)) {
                    let mut p = path.clone();
                    p.push(Name::new(child.name.clone()));
                    stack.push((child, p, upper));
                }
            }
        }
        None => warn!("animation library has no Armature: no layered poses"),
    }
    let mut nodes = HashMap::new();
    let mut clip_of = HashMap::new();
    for a in Anim::ALL {
        let Some(clip) = gltf.named_animations.get(a.clip()) else {
            warn!("animation library has no {}", a.clip());
            continue;
        };
        let length = clips.get(clip).map_or(1.0, |c| c.duration());
        for (layer, mask) in [(Layer::Full, 0), (Layer::Lower, 1 << UPPER_GROUP), (Layer::Upper, 1 << LOWER_GROUP)] {
            let node = graph.add_clip_with_mask(clip.clone(), mask, 1.0, graph.root);
            nodes.insert((a, layer), (node, length));
            clip_of.insert(node, a.clip());
        }
    }
    library.nodes = nodes;
    library.clip_of = clip_of;
    library.graph = Some(graphs.add(graph));
}

/// Spawns a pawn's character (hidden until ready) under `parent`.
pub fn spawn_rig(commands: &mut Commands, assets: &AssetServer, p: &Pawn, parent: Entity) -> Entity {
    let h = |salt: u32| hash(p.id as i32, 41, salt);
    let female = h(1) < 0.5;
    let sex = if female { "Female" } else { "Male" };
    let raider = p.faction == Faction::Raider;
    let mut parts = vec![
        format!("outfits/{}_{}", sex, if raider { "Ranger" } else { "Peasant" }),
        format!("base/Superhero_{}_Head", sex),
    ];
    // Raiders wear the ranger's hood instead of hair.
    if !raider {
        let styles: &[&str] = if female {
            &["Hair_Buns", "Hair_Long", "Hair_BuzzedFemale"]
        } else {
            &["Hair_SimpleParted", "Hair_Buzzed", "Hair_Long"]
        };
        parts.push(format!("hair/{}", styles[(h(2) * styles.len() as f32) as usize % styles.len()]));
        if !female && h(3) < 0.4 {
            parts.push("hair/Hair_Beard".into());
        }
    }

    let look = pawn_look(p);
    let clothes = if raider { Rgb(1.0, 0.72, 0.68) } else { Rgb(1.0, 1.0, 1.0).mix(look.clothes, 0.5) };
    let (skin, base) = (look.skin.color().to_linear(), TEXTURE_SKIN.color().to_linear());
    let rig = commands
        .spawn((
            AnimationPlayer::default(),
            Transform::from_scale(Vec3::splat(RIG_SCALE)),
            Visibility::Hidden,
            ChildOf(parent),
        ))
        .id();
    let hairy: Vec<bool> = parts.iter().map(|p| p.starts_with("hair/") && !p.ends_with("Beard")).collect();
    let parts: Vec<Entity> = parts
        .into_iter()
        .map(|part| {
            let path = format!("models/characters/{}.gltf", part);
            commands
                .spawn((WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))), ChildOf(rig)))
                .observe(rig_part)
                .id()
        })
        .collect();
    let hair_parts: Vec<Entity> = parts.iter().zip(&hairy).filter(|(_, h)| **h).map(|(e, _)| *e).collect();
    commands.entity(rig).insert(Rig {
        pending: parts.len(),
        hand: None,
        outfit: parts[0],
        covered: false,
        dead: false,
        female,
        upper_body: Vec::new(),
        garment: None,
        garment_ready: false,
        garment_materials: Vec::new(),
        eyes: Vec::new(),
        lids: None,
        // The textures' skin is darker than the tone it's tinted by.
        lid_color: look.skin.shade(0.78).color(),
        closed: false,
        clothes: clothes.color(),
        skin: Color::linear_rgb(skin.red / base.red, skin.green / base.green, skin.blue / base.blue),
        hair: look.hair.color(),
        variant: !raider && h(4) < 0.5,
        own: Vec::new(),
        bones: HashMap::new(),
        outfit_bones: HashMap::new(),
        hair_parts,
        animated: false,
        layers: Vec::new(),
    });
    rig
}

/// A part has spawned. Character files have no animations of their own, so
/// the glTF loader doesn't mark their bones: name each bone by its path from
/// the `Armature`, as the loader does for the library, and have the
/// character's player animate it. Then give the part this pawn's colours.
#[allow(clippy::too_many_arguments)]
fn rig_part(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
    transforms: Query<&Transform>,
    meshes: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    mut rigs: Query<&mut Rig>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    library: Res<CharacterLibrary>,
) {
    let part = ready.event().entity;
    let Ok(character) = parents.get(part).map(ChildOf::parent) else {
        return;
    };
    let Ok(mut rig) = rigs.get_mut(character) else {
        return;
    };
    rig.pending = rig.pending.saturating_sub(1);
    if part == rig.outfit {
        rig.upper_body.extend(children.iter_descendants(part).filter(|&e| {
            names.get(e).is_ok_and(|n| n.as_str().ends_with("_Body") || n.as_str().ends_with("_Arms"))
        }));
    }

    if let Some(armature) =
        children.iter_descendants(part).find(|&e| names.get(e).is_ok_and(|n| n.as_str() == "Armature"))
    {
        let turn = |e: Entity| transforms.get(e).map_or(Quat::IDENTITY, |t| t.rotation);
        let mut stack = vec![(armature, vec![names.get(armature).unwrap().clone()], turn(armature))];
        while let Some((e, path, rest)) = stack.pop() {
            commands.entity(e).insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(character)));
            if rig.hand.is_none() && path.last().is_some_and(|n| n.as_str() == "hand_r") {
                rig.hand = Some(e);
            }
            // Preserve the attachment lookup, but bind fabric only to the
            // outfit: some heads have different arm rest transforms.
            if let Some(name) = path.last() {
                rig.bones.entry(name.as_str().to_string()).or_insert((e, rest));
                if part == rig.outfit {
                    rig.outfit_bones.insert(name.as_str().to_string(), e);
                }
            }
            for child in children.get(e).into_iter().flatten() {
                if let Ok(name) = names.get(*child) {
                    let mut p = path.clone();
                    p.push(name.clone());
                    stack.push((*child, p, rest * turn(*child)));
                }
            }
        }
    }

    for e in children.iter_descendants(part) {
        let Ok((name, mat)) = meshes.get(e) else {
            continue;
        };
        let name = name.0.as_str();
        let tint = if name.starts_with("MI_Eyes") {
            rig.eyes.push((e, mat.0.clone()));
            continue;
        } else if name.starts_with("MI_Hair") {
            rig.hair
        } else if name.starts_with("MI_Superhero") || name.starts_with("MI_Regular") {
            rig.skin
        } else {
            rig.clothes
        };
        let Some(mut own) = materials.get(&mat.0).cloned() else {
            continue;
        };
        own.base_color = tint;
        if rig.variant && name.starts_with("MI_Peasant") {
            own.base_color_texture = Some(library.peasant_2.clone());
        }
        let handle = materials.add(own);
        rig.own.push((handle.clone(), tint));
        commands.entity(e).insert(MeshMaterial3d(handle));
    }
}

/// Attach the animation graph once the library and every part are ready.
/// Returns whether the character can be shown.
pub fn prepare(commands: &mut Commands, library: &CharacterLibrary, rig_entity: Entity, rig: &mut Rig) -> bool {
    if !rig.animated && rig.pending == 0 {
        if let Some(graph) = &library.graph {
            commands.entity(rig_entity).insert(AnimationGraphHandle(graph.clone()));
            rig.animated = true;
        }
    }
    rig.ready()
}

/// Grey a dead pawn's character, or restore its colours.
impl Rig {
    pub fn female(&self) -> bool { self.female }

    pub fn reset_garment(&mut self, commands: &mut Commands) {
        self.garment = None;
        self.garment_ready = false;
        self.own.retain(|(handle, _)| !self.garment_materials.contains(&handle.id()));
        self.garment_materials.clear();
        for &e in &self.upper_body {
            commands.entity(e).insert(Visibility::Inherited);
        }
    }

    /// A bone, and its turn in the character's frame at rest.
    pub fn bone(&self, name: &str) -> Option<(Entity, Quat)> {
        self.bones.get(name).copied()
    }

    /// Show or hide the hair (under a helmet).
    pub fn show_hair(&self, show: bool, visibility: &mut Query<&mut Visibility>) {
        for &e in &self.hair_parts {
            if let Ok(mut v) = visibility.get_mut(e) {
                *v = if show { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}

fn living_or_dead(color: Color, dead: bool) -> Color {
    let c = color.to_linear();
    if dead { Color::linear_rgb(c.red * 0.45, c.green * 0.45, c.blue * 0.5) } else { color }
}

pub fn set_dead(rig: &mut Rig, dead: bool, materials: &mut Assets<StandardMaterial>) {
    rig.dead = dead;
    for (handle, color) in &rig.own {
        if let Some(mut m) = materials.get_mut(handle) {
            m.base_color = living_or_dead(*color, dead);
        }
    }
}

/// Sleeping or unconscious characters close their eyes.
pub fn close_eyes(commands: &mut Commands, rig: &mut Rig, closed: bool, materials: &mut Assets<StandardMaterial>) {
    if rig.closed == closed {
        return;
    }
    rig.closed = closed;
    let color = rig.lid_color;
    let lids = rig
        .lids
        .get_or_insert_with(|| {
            materials.add(StandardMaterial { base_color: color, perceptual_roughness: 0.7, ..default() })
        })
        .clone();
    for (e, open) in &rig.eyes {
        commands.entity(*e).insert(MeshMaterial3d(if closed { lids.clone() } else { open.clone() }));
    }
}

/// Hide the body (everything but head and hair) under a blanket.
pub fn cover_body(rig: &mut Rig, covered: bool, visibility: &mut Query<&mut Visibility>) {
    if rig.covered == covered {
        return;
    }
    rig.covered = covered;
    if let Ok(mut vis) = visibility.get_mut(rig.outfit) {
        *vis = if covered { Visibility::Hidden } else { Visibility::Inherited };
    }
    if let Some(e) = rig.garment.filter(|_| rig.garment_ready) {
        if let Ok(mut vis) = visibility.get_mut(e) {
            *vis = if covered { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
}

#[derive(Component)]
struct GarmentBinding {
    character: Entity,
    fallbacks: Vec<Entity>,
}

/// Clothing is a single skin driven by the live character skeleton, not rigid sleeves.
pub fn spawn_garment(commands: &mut Commands, scene: Handle<WorldAsset>, character: Entity, rig: &mut Rig, fallbacks: Vec<Entity>) -> Entity {
    let e = commands.spawn((WorldAssetRoot(scene), Transform::default(), Visibility::Hidden,
        ChildOf(character), GarmentBinding { character, fallbacks })).observe(bind_garment).id();
    rig.garment = Some(e);
    e
}

fn bind_garment(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    bindings: Query<&GarmentBinding>,
    children: Query<&Children>,
    names: Query<&Name>,
    skins: Query<&SkinnedMesh>,
    surfaces: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    mut rigs: Query<&mut Rig>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = ready.event().entity;
    let Ok(binding) = bindings.get(root) else { return };
    let Ok(mut rig) = rigs.get_mut(binding.character) else { return };
    if rig.garment != Some(root) || rig.garment_ready { return; }
    let descendants: Vec<_> = children.iter_descendants(root).collect();
    let mut replacements = Vec::new();
    for &e in &descendants {
        let Ok(skin) = skins.get(e) else { continue };
        let joints: Option<Vec<_>> = skin.joints.iter().map(|&joint| {
            let name = names.get(joint).ok()?;
            rig.outfit_bones.get(name.as_str()).copied()
        }).collect();
        let Some(joints) = joints else {
            warn!("garment has a joint missing from the character skeleton");
            return;
        };
        replacements.push((e, SkinnedMesh { joints, ..skin.clone() }));
    }
    if replacements.is_empty() { return; }
    for (e, skin) in replacements { commands.entity(e).insert(skin); }
    // Imported bind-pose bones are no longer needed once the skin uses the live rig.
    for &e in &descendants {
        if names.get(e).is_ok_and(|n| n.as_str() == "Armature") { commands.entity(e).despawn(); }
        if let Ok((name, surface)) = surfaces.get(e) {
            if let Some(mut own) = materials.get(&surface.0).cloned() {
                let tint = if name.0.starts_with("MI_Regular") || name.0.starts_with("MI_Superhero") {
                    rig.skin
                } else if name.0.starts_with("MI_") {
                    rig.clothes
                } else {
                    own.base_color
                };
                own.base_color = living_or_dead(tint, rig.dead);
                let handle = materials.add(own);
                rig.garment_materials.push(handle.id());
                rig.own.push((handle.clone(), tint));
                commands.entity(e).insert(MeshMaterial3d(handle));
            }
        }
    }
    for &e in binding.fallbacks.iter().chain(&rig.upper_body) { commands.entity(e).insert(Visibility::Hidden); }
    commands.entity(root).insert(if rig.covered { Visibility::Hidden } else { Visibility::Inherited });
    rig.garment_ready = true;
}

/// Play `pose`: its clip nodes fade in over `BLEND` (real time `dt`) and
/// whatever else was playing fades out. Once-only clips start over if
/// `restart`. A node starting while the same clip plays in another node (the
/// legs of a walk when a carry starts) picks up at the same point, so
/// nothing jumps. `rate` scales its speed (walk speed, simulation speed; 0 pauses).
pub fn play(library: &CharacterLibrary, rig: &mut Rig, player: &mut AnimationPlayer, pose: Pose, restart: bool, rate: f32, dt: f32) {
    let mut wanted = Vec::new();
    for (anim, layer) in pose.layers() {
        let Some(&(node, _)) = library.nodes.get(&(anim, layer)) else { continue };
        wanted.push(node);
        let fresh = !player.is_playing_animation(node);
        if fresh || (restart && anim.once()) {
            let clip = anim.clip();
            let along = rig
                .layers
                .iter()
                .filter(|l| l.0 != node && library.clip_of.get(&l.0) == Some(&clip))
                .find_map(|l| player.animation(l.0))
                .map(|a| a.seek_time());
            // The very first pose shows at once; later ones fade in.
            let weight = if rig.layers.is_empty() { 1.0 } else { 0.0 };
            let active = player.start(node);
            if anim.once() {
                active.set_repeat(bevy::animation::RepeatAnimation::Never);
            } else {
                active.repeat();
            }
            match along {
                Some(t) if !restart => {
                    active.seek_to(t);
                }
                _ => {}
            }
            if fresh {
                active.set_weight(weight);
                rig.layers.retain(|l| l.0 != node);
                rig.layers.push((node, weight));
            }
        }
        if let Some(active) = player.animation_mut(node) {
            active.set_speed(rate);
        }
    }
    let step = dt / BLEND.as_secs_f32();
    for (node, weight) in rig.layers.iter_mut() {
        *weight = if wanted.contains(node) { (*weight + step).min(1.0) } else { (*weight - step).max(0.0) };
        if let Some(active) = player.animation_mut(*node) {
            active.set_weight(*weight);
        }
    }
    rig.layers.retain(|&(node, weight)| {
        let keep = weight > 0.0 || wanted.contains(&node);
        if !keep {
            player.stop(node);
        }
        keep
    });
}

/// Picks the locomotion clip, walking around the colony or running in a
/// battle (sprinting if very fast), and the rate to play it at so the feet
/// keep pace with the ground. `speed` is in cells per real second, so a
/// faster game plays the same clip faster.
pub fn locomotion(speed: f32, sim_rate: f32, running: bool) -> (Anim, f32) {
    let anim = if !running {
        Anim::Walk
    } else if speed / sim_rate.max(1.0) < 6.0 {
        Anim::Jog
    } else {
        Anim::Sprint
    };
    // Running in battle takes longer strides than the clip: quick moves at a calm step.
    let stride = if running { RUN_STRIDE } else { 1.0 };
    (anim, (speed / (anim.moves().unwrap() * stride)).max(0.3))
}

/// How much longer a running stride is than the clip's own (a little foot
/// slide, hardly visible from above, for a calmer step at battle speed).
pub const RUN_STRIDE: f32 = 1.6;

/// The pose for what a pawn is doing in place (not moving, not lying
/// down, not falling, no attack being played, not sitting down or getting up).
pub fn activity(p: &Pawn, w: &SimWorld, armed: bool) -> Pose {
    let m = &w.map;
    let arrived = p.path.is_empty();
    // In a battle: crouched behind cover when hunkered (civilians always are),
    // down on one knee aiming when deployed.
    if let Some(b) = &w.battle {
        if let Some(u) = b.unit_of(p.id) {
            if b.units[u].deployed {
                return Pose::split(Anim::Kneel, Anim::Aim);
            }
            if b.units[u].hunkered {
                return Pose::full(Anim::Crouch);
            }
        }
    }
    // On a chair: playing chess gestures, crafting works with the hands.
    if w.seated(p) {
        return match p.job {
            Some(Job::Relax { .. }) => Pose::full(Anim::SitTalk),
            Some(Job::Work { .. }) => Pose::split(Anim::Sit, Anim::Interact),
            _ => Pose::full(Anim::Sit),
        };
    }
    let anim = match p.job {
        Some(Job::Work { target, task }) if arrived => match task {
            Task::Harvest(plant) if plant.def().harvest.desig == Some(Desig::Chop) => Anim::Swing,
            Task::Mine | Task::Fill => Anim::Swing,
            Task::Harvest(_) | Task::Sow(_) => Anim::Kneel,
            Task::Construct | Task::Deconstruct | Task::Uninstall => match m.feature[m.idx(target)] {
                Feature::Blueprint { kind, .. } | Feature::Built { kind, .. } if kind.def().is_block() => Anim::Interact,
                _ => Anim::Kneel,
            },
            Task::Craft | Task::Decorate => Anim::Interact,
        },
        // Eating without a seat.
        Some(Job::Eat { picked: true, .. }) if arrived => Anim::Crouch,
        Some(Job::Tend { picked: true, .. } | Job::Feed { picked: true, .. }) if arrived => Anim::Kneel,
        _ if armed => match p.weapon.and_then(|w: ItemId| w.def().weapon.as_ref()) {
            Some(wd) if wd.ranged() => Anim::Aim,
            Some(_) => Anim::Guard,
            None => Anim::Idle,
        },
        _ => Anim::Idle,
    };
    Pose::full(anim)
}

/// The attack animation for a pawn's weapon.
pub fn attack_anim(p: &Pawn) -> Anim {
    match p.weapon.and_then(|w| w.def().weapon.as_ref()) {
        Some(wd) if wd.ranged() => Anim::Shoot,
        Some(_) => Anim::Strike,
        None => Anim::Punch,
    }
}

/// Where a weapon sits in the right hand: the held look from the
/// definitions (sized for the old figure, `figure_scale` cells per unit),
/// turned to point forward while aiming, gripped near one end of its
/// longest side.
pub fn hand_pose(size: Vec3, turn: Quat, figure_scale: f32) -> Transform {
    let size = size * figure_scale / RIG_SCALE;
    let axis = if size.z >= size.x && size.z >= size.y {
        Vec3::Z * size.z
    } else if size.y >= size.x {
        Vec3::Y * size.y
    } else {
        Vec3::X * size.x
    };
    let grip = AIM_HAND.normalize().inverse();
    let along = axis.length();
    let offset = turn * axis.normalize() * (along / 2.0 - 0.1).max(0.0);
    Transform::from_translation(grip * (offset + Vec3::Y * 0.02)).with_rotation(grip * turn).with_scale(size)
}

/// A model already authored in metres around the grip needs no inferred offset.
pub fn model_hand_pose(pose: Transform) -> Transform {
    let grip = AIM_HAND.normalize().inverse();
    Transform {
        translation: grip * pose.translation,
        rotation: grip * pose.rotation,
        scale: pose.scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rig(outfit: Entity) -> Rig {
        Rig {
            pending: 0, hand: None, outfit, covered: false, dead: false, female: false,
            upper_body: Vec::new(), garment: None, garment_ready: false, garment_materials: Vec::new(),
            eyes: Vec::new(), lids: None, lid_color: Color::WHITE, closed: false,
            clothes: Color::WHITE, skin: Color::WHITE, hair: Color::WHITE, variant: false,
            own: Vec::new(), bones: HashMap::new(), outfit_bones: HashMap::new(),
            hair_parts: Vec::new(), animated: true, layers: Vec::new(),
        }
    }

    fn instance_ready(world: &mut World, entity: Entity) {
        let mut spawner = bevy::world_serialization::WorldInstanceSpawner::default();
        world.flush();
        world.trigger(WorldInstanceReady { entity, instance_id: spawner.spawn(Handle::<WorldAsset>::default()) });
        world.flush();
    }

    #[test]
    fn deforming_clothes_use_live_joints_and_restore_the_native_outfit() {
        let mut world = World::new();
        world.init_resource::<Assets<StandardMaterial>>();
        let outfit = world.spawn(Visibility::Inherited).id();
        let torso = world.spawn((Visibility::Inherited, ChildOf(outfit))).id();
        let arms = world.spawn((Visibility::Inherited, ChildOf(outfit))).id();
        let live = world.spawn((Name::new("upperarm_l"), Transform::default())).id();
        let mut rig = test_rig(outfit);
        rig.upper_body = vec![torso, arms];
        rig.outfit_bones.insert("upperarm_l".into(), live);
        let character = world.spawn_empty().id();
        let fallback = world.spawn(Visibility::Inherited).id();
        let scene = Handle::<WorldAsset>::default();
        let garment = spawn_garment(&mut world.commands(), scene.clone(), character, &mut rig, vec![fallback]);
        world.entity_mut(character).insert(rig);
        world.flush();
        let armature = world.spawn((Name::new("Armature"), ChildOf(garment))).id();
        let imported = world.spawn((Name::new("upperarm_l"), ChildOf(armature))).id();
        let cloth = world.spawn((SkinnedMesh { joints: vec![imported], ..default() }, ChildOf(garment))).id();
        assert_eq!(world.get::<Visibility>(torso), Some(&Visibility::Inherited), "loading retains the original torso");
        assert_eq!(world.get::<Visibility>(garment), Some(&Visibility::Hidden));
        let mut spawner = bevy::world_serialization::WorldInstanceSpawner::default();
        world.trigger(WorldInstanceReady { entity: garment, instance_id: spawner.spawn(scene) });
        world.flush();
        assert_eq!(world.get::<SkinnedMesh>(cloth).unwrap().joints, vec![live], "the imported bind-pose bones never drive the worn skin");
        assert!(world.get_entity(armature).is_err());
        for e in [torso, arms, fallback] {
            assert_eq!(world.get::<Visibility>(e), Some(&Visibility::Hidden), "no doubled torso or rigid sleeve");
        }
        assert_eq!(world.get::<Visibility>(garment), Some(&Visibility::Inherited));
        let mut rig = world.entity_mut(character).take::<Rig>().unwrap();
        let mut state = bevy::ecs::system::SystemState::<Query<&mut Visibility>>::new(&mut world);
        cover_body(&mut rig, true, &mut state.get_mut(&mut world).unwrap());
        assert_eq!(world.get::<Visibility>(garment), Some(&Visibility::Hidden), "clothes stay under the bed blanket");
        cover_body(&mut rig, false, &mut state.get_mut(&mut world).unwrap());
        assert_eq!(world.get::<Visibility>(garment), Some(&Visibility::Inherited));
        rig.reset_garment(&mut world.commands());
        world.despawn(garment);
        world.flush();
        for e in [torso, arms] { assert_eq!(world.get::<Visibility>(e), Some(&Visibility::Inherited)); }
        assert!(world.get_entity(cloth).is_err(), "switching outfits removes the old skin");
        assert!(rig.garment.is_none() && !rig.garment_ready);
    }

    #[test]
    fn clothing_binding_uses_the_outfit_in_both_part_load_orders() {
        for head_first in [true, false] {
            let mut world = World::new();
            world.init_resource::<Assets<StandardMaterial>>();
            world.insert_resource(CharacterLibrary {
                gltf: Handle::default(), peasant_2: Handle::default(), graph: None,
                nodes: HashMap::new(), clip_of: HashMap::new(),
            });
            let character = world.spawn_empty().id();
            let outfit = world.spawn((ChildOf(character), Visibility::Inherited)).observe(rig_part).id();
            let head = world.spawn(ChildOf(character)).observe(rig_part).id();
            let mut bones = Vec::new();
            for part in [outfit, head] {
                let armature = world.spawn((Name::new("Armature"), Transform::default(), ChildOf(part))).id();
                bones.push(world.spawn((Name::new("upperarm_l"), Transform::from_xyz(if part == head { 0.02 } else { 0.0 }, 0.0, 0.0), ChildOf(armature))).id());
            }
            let mut rig = test_rig(outfit);
            rig.pending = 2;
            world.entity_mut(character).insert(rig);
            for part in if head_first { [head, outfit] } else { [outfit, head] } {
                instance_ready(&mut world, part);
            }
            let mut rig = world.entity_mut(character).take::<Rig>().unwrap();
            assert_eq!(rig.outfit_bones["upperarm_l"], bones[0]);
            assert_eq!(rig.bones["upperarm_l"].0, bones[if head_first { 1 } else { 0 }], "attachment lookup retains its existing behavior");
            let garment = spawn_garment(&mut world.commands(), Handle::default(), character, &mut rig, Vec::new());
            world.entity_mut(character).insert(rig);
            world.flush();
            let imported = world.spawn((Name::new("upperarm_l"), ChildOf(garment))).id();
            let cloth = world.spawn((SkinnedMesh { joints: vec![imported], ..default() }, ChildOf(garment))).id();
            instance_ready(&mut world, garment);
            assert_eq!(world.get::<SkinnedMesh>(cloth).unwrap().joints, vec![bones[0]]);
        }
    }

    #[test]
    fn failed_and_replaced_garments_do_not_hide_the_loading_fallbacks() {
        let mut world = World::new();
        world.init_resource::<Assets<StandardMaterial>>();
        let outfit = world.spawn(Visibility::Inherited).id();
        let fallback = world.spawn(Visibility::Inherited).id();
        let live = world.spawn_empty().id();
        let mut rig = test_rig(outfit);
        rig.upper_body.push(outfit);
        rig.outfit_bones.insert("upperarm_l".into(), live);
        let character = world.spawn_empty().id();
        let garment = spawn_garment(&mut world.commands(), Handle::default(), character, &mut rig, vec![fallback]);
        world.entity_mut(character).insert(rig);
        world.flush();
        let valid = world.spawn((Name::new("upperarm_l"), ChildOf(garment))).id();
        let missing = world.spawn((Name::new("missing"), ChildOf(garment))).id();
        let cloth = world.spawn((SkinnedMesh { joints: vec![valid], ..default() }, ChildOf(garment))).id();
        let invalid = world.spawn((SkinnedMesh { joints: vec![missing], ..default() }, ChildOf(garment))).id();
        instance_ready(&mut world, garment);
        assert_eq!(world.get::<SkinnedMesh>(cloth).unwrap().joints, vec![valid], "a bad skin prevents partial binding");
        assert!(!world.get::<Rig>(character).unwrap().garment_ready);
        assert_eq!(world.get::<Visibility>(garment), Some(&Visibility::Hidden));
        for e in [outfit, fallback] { assert_eq!(world.get::<Visibility>(e), Some(&Visibility::Inherited)); }

        world.despawn(invalid);
        let mut rig = world.entity_mut(character).take::<Rig>().unwrap();
        rig.reset_garment(&mut world.commands());
        let replacement = spawn_garment(&mut world.commands(), Handle::default(), character, &mut rig, vec![fallback]);
        world.entity_mut(character).insert(rig);
        world.flush();
        instance_ready(&mut world, garment);
        assert_eq!(world.get::<SkinnedMesh>(cloth).unwrap().joints, vec![valid], "a stale completion cannot take over the replacement");
        assert_eq!(world.get::<Rig>(character).unwrap().garment, Some(replacement));
        for e in [outfit, fallback] { assert_eq!(world.get::<Visibility>(e), Some(&Visibility::Inherited)); }
    }

    #[test]
    fn garments_loaded_while_covered_or_dead_keep_visibility_and_material_state() {
        let mut world = World::new();
        world.init_resource::<Assets<StandardMaterial>>();
        let outfit = world.spawn(Visibility::Hidden).id();
        let live = world.spawn_empty().id();
        let original = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.4, 0.5), ..default()
        });
        let mut rig = test_rig(outfit);
        rig.covered = true;
        set_dead(&mut rig, true, &mut world.resource_mut::<Assets<StandardMaterial>>());
        rig.outfit_bones.insert("upperarm_l".into(), live);
        let character = world.spawn_empty().id();
        let garment = spawn_garment(&mut world.commands(), Handle::default(), character, &mut rig, Vec::new());
        world.entity_mut(character).insert(rig);
        world.flush();
        let imported = world.spawn((Name::new("upperarm_l"), ChildOf(garment))).id();
        let cloth = world.spawn((SkinnedMesh { joints: vec![imported], ..default() },
            GltfMaterialName("Fabric".into()), MeshMaterial3d(original.clone()), ChildOf(garment))).id();
        instance_ready(&mut world, garment);
        assert_eq!(world.get::<Visibility>(garment), Some(&Visibility::Hidden));
        let owned = world.get::<MeshMaterial3d<StandardMaterial>>(cloth).unwrap().0.clone();
        let materials = world.resource::<Assets<StandardMaterial>>();
        assert_ne!(owned, original);
        assert_eq!(materials.get(&owned).unwrap().base_color, living_or_dead(materials.get(&original).unwrap().base_color, true));
        let mut rig = world.entity_mut(character).take::<Rig>().unwrap();
        set_dead(&mut rig, false, &mut world.resource_mut::<Assets<StandardMaterial>>());
        assert_eq!(world.resource::<Assets<StandardMaterial>>().get(&owned).unwrap().base_color,
            world.resource::<Assets<StandardMaterial>>().get(&original).unwrap().base_color);
        rig.reset_garment(&mut world.commands());
        assert!(rig.own.is_empty() && rig.garment_materials.is_empty(), "removed garment materials leave the rig's ownership list");
    }

    #[test]
    fn deforming_clothing_assets_match_the_outfits_and_have_valid_skin_weights() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for item in ["clothes", "parka"] {
            let skin = ItemId::named(item).def().apparel.as_ref().unwrap().skinned.as_ref().unwrap();
            for (sex, path) in [("Male", &skin.male), ("Female", &skin.female)] {
                let path = assets.join(path);
                let model: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                let native_path = assets.join(format!("models/characters/outfits/{sex}_Peasant.gltf"));
                let native: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&native_path).unwrap()).unwrap();
                let bytes = std::fs::read(path.with_file_name(model["buffers"][0]["uri"].as_str().unwrap())).unwrap();
                let native_bytes = std::fs::read(native_path.with_file_name(native["buffers"][0]["uri"].as_str().unwrap())).unwrap();
                let slice = |g: &serde_json::Value, accessor: usize| {
                    let a = &g["accessors"][accessor];
                    let v = &g["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
                    (v["byteOffset"].as_u64().unwrap_or(0) + a["byteOffset"].as_u64().unwrap_or(0)) as usize
                };
                let joints = model["skins"][0]["joints"].as_array().unwrap();
                let native_joints = native["skins"][0]["joints"].as_array().unwrap();
                assert_eq!(joints.len(), native_joints.len());
                for (a, b) in joints.iter().zip(native_joints) {
                    assert_eq!(model["nodes"][a.as_u64().unwrap() as usize]["name"], native["nodes"][b.as_u64().unwrap() as usize]["name"]);
                }
                let bind = slice(&model, model["skins"][0]["inverseBindMatrices"].as_u64().unwrap() as usize);
                let native_bind = slice(&native, native["skins"][0]["inverseBindMatrices"].as_u64().unwrap() as usize);
                assert_eq!(&bytes[bind..bind + joints.len() * 64], &native_bytes[native_bind..native_bind + joints.len() * 64], "{item}/{sex} keeps its matching bind pose");
                let fabric = model["meshes"][0]["primitives"].as_array().unwrap();
                let triangles: u64 = fabric.iter().map(|p| model["accessors"][p["indices"].as_u64().unwrap() as usize]["count"].as_u64().unwrap() / 3).sum();
                assert!((1..=1800).contains(&triangles));
                for primitive in model["meshes"].as_array().unwrap().iter().flat_map(|mesh| mesh["primitives"].as_array().unwrap()) {
                let attrs = &primitive["attributes"];
                let weight = attrs["WEIGHTS_0"].as_u64().unwrap() as usize;
                let joint = attrs["JOINTS_0"].as_u64().unwrap() as usize;
                let count = model["accessors"][weight]["count"].as_u64().unwrap() as usize;
                assert_eq!(model["accessors"][joint]["count"].as_u64().unwrap() as usize, count);
                let wo = slice(&model, weight);
                let jo = slice(&model, joint);
                for i in 0..count {
                    let weights: Vec<_> = bytes[wo + i * 16..wo + (i + 1) * 16].chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
                    assert!(weights.iter().all(|w| w.is_finite() && (0.0..=1.0).contains(w)));
                    assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
                    for b in bytes[jo + i * 8..jo + (i + 1) * 8].chunks_exact(2) {
                        assert!((u16::from_le_bytes(b.try_into().unwrap()) as usize) < joints.len());
                    }
                }
                }
                let arms = model["meshes"][1]["primitives"].as_array().unwrap();
                assert!(!arms.is_empty());
                for primitive in arms {
                    let material = primitive["material"].as_u64().unwrap() as usize;
                    let name = model["materials"][material]["name"].as_str().unwrap();
                    assert!(name.starts_with("MI_Regular") || name.starts_with("MI_Superhero"), "the arm mesh contains skin, without the Peasant outfit's exposed wraps");
                }
                for image in model["images"].as_array().unwrap() {
                    assert!(path.parent().unwrap().join(image["uri"].as_str().unwrap()).is_file());
                }
            }
        }
    }

    /// The models only use vertex attributes Bevy loads: anything else makes
    /// it warn on every load (extra colour and texture sets, more than four
    /// bones per vertex). Strip those from new models.
    #[test]
    fn models_only_use_vertex_attributes_bevy_loads() {
        const KNOWN: [&str; 8] = ["POSITION", "NORMAL", "TANGENT", "TEXCOORD_0", "TEXCOORD_1", "COLOR_0", "JOINTS_0", "WEIGHTS_0"];
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/models")];
        let mut checked = 0;
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "gltf") {
                    continue;
                }
                let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                for mesh in json["meshes"].as_array().into_iter().flatten() {
                    for prim in mesh["primitives"].as_array().into_iter().flatten() {
                        for name in prim["attributes"].as_object().into_iter().flatten().map(|(k, _)| k) {
                            assert!(KNOWN.contains(&name.as_str()), "{}: vertex attribute {}", path.display(), name);
                        }
                    }
                }
                checked += 1;
            }
        }
        assert!(checked > 50, "found the models");
    }

    #[test]
    fn locomotion_matches_speed() {
        use my_world::pawn::{RUN_SPEED, WALK_SPEED};
        // Walking, a clip played at its own speed doesn't slide.
        let (a, rate) = locomotion(super::WALK_SPEED, 1.0, false);
        assert_eq!(a, Anim::Walk);
        assert!((rate - 1.0).abs() < 1e-4);
        // Around the colony they walk, a little quicker than the clip's own stroll.
        let (a, rate) = locomotion(WALK_SPEED, 1.0, false);
        assert_eq!(a, Anim::Walk);
        assert!(rate > 1.5 && rate < 2.2, "{}", rate);
        // In battle they run with long strides: an easy jog's step rate at a good pace.
        let (a, rate) = locomotion(RUN_SPEED, 1.0, true);
        assert_eq!(a, Anim::Jog);
        assert!(rate > 0.6 && rate <= 1.0, "{}", rate);
        // A faster game plays the same clip faster.
        assert_eq!(locomotion(3.0 * WALK_SPEED, 3.0, false).0, Anim::Walk);
        assert_eq!(locomotion(3.0 * RUN_SPEED, 3.0, true).0, Anim::Jog);
    }

    #[test]
    fn poses_layer_legs_and_upper_body() {
        use my_world::map::{Map, FLAT_Z as Z};
        use my_world::pawn::Job;
        // Carrying: arms out over walking or standing, nothing else changed.
        assert_eq!(Pose::full(Anim::Walk).carrying(), Pose::split(Anim::Walk, Anim::Carry));
        assert_eq!(Pose::full(Anim::Kneel).carrying(), Pose::full(Anim::Kneel));
        assert_eq!(Pose::split(Anim::Walk, Anim::Carry).layers(), vec![(Anim::Walk, Layer::Lower), (Anim::Carry, Layer::Upper)]);

        // Seated: chess gestures, crafting with the hands, otherwise just sitting.
        let mut w = SimWorld::with_map(Map::flat(12, 12), my_world::rng::Rng::new(1));
        let at = (5, 5, Z);
        let i = w.map.idx(at);
        w.map.set_feature(i, Feature::built("chair"));
        let id = w.spawn_colonist(at);
        let k = w.pawn_index(id).unwrap();
        w.pawns[k].job = Some(Job::Relax { at: (5, 6, Z), stand: at });
        assert_eq!(activity(&w.pawns[k], &w, false), Pose::full(Anim::SitTalk));
        w.pawns[k].job = Some(Job::Sit { at, until: u32::MAX });
        assert_eq!(activity(&w.pawns[k], &w, false), Pose::full(Anim::Sit));
        assert!(activity(&w.pawns[k], &w, false).sitting());

        // A deployed squad kneels, aiming.
        let mut w = my_world::scenarios::build("battle").unwrap();
        let b = w.battle.as_ref().unwrap();
        let u = (0..b.units.len()).find(|&u| w.can_deploy(u)).expect("a ranged squad");
        let member = b.units[u].members[0];
        w.battle.as_mut().unwrap().units[u].deployed = true;
        let p = w.pawn(member).unwrap();
        assert_eq!(activity(p, &w, true), Pose::split(Anim::Kneel, Anim::Aim));
    }

    #[test]
    fn rifle_points_forward_from_the_hand() {
        let tf = hand_pose(Vec3::new(0.05, 0.06, 0.52), Quat::IDENTITY, 0.85);
        // In the aiming pose the hand's world rotation is AIM_HAND.
        let world = AIM_HAND.normalize() * tf.rotation;
        assert!((world * Vec3::Z).dot(Vec3::Z) > 0.99);
        assert!((world * Vec3::Y).dot(Vec3::Y) > 0.99);
        // The barrel reaches forward past the hand.
        assert!((AIM_HAND.normalize() * tf.translation).z > 0.2);
        // Grip-authored models keep the hand at their origin and retain metre scale.
        let pose = super::super::props::model_transform(&ItemId::named("rifle").def().model.as_ref().unwrap().held);
        let held = model_hand_pose(pose);
        let world = AIM_HAND.normalize() * held.rotation;
        assert!((world * Vec3::Z - Vec3::Z).length() < 1e-5);
        assert!((world * Vec3::Y - Vec3::Y).length() < 1e-5);
        assert_eq!(held.translation, Vec3::ZERO);
        assert_eq!(held.scale, Vec3::ONE);
    }
}
