//! Look test (phase 10a): the downloaded models in the game's lighting, to
//! judge the art direction before converting the game to them.
//!
//! `cargo run --release --example look` browses the downloaded source packs.
//! `cargo run --release --example look -- --world-props` reviews selected plants,
//! machinery and state scenes with their actual definition transforms.
//! `--world-manifest` exports those matrices as JSON for the CPU preview tool.
//!
//! Everything is shown at one scale (`SCALE`): a character is about one
//! cell tall, as pawns are in the game, so the source packs' relative sizes are
//! true. World-prop mode instead uses each definition's placement/scale directly.
//! A faint grid marks game cells.
//!
//! Keys: right-drag or Q/E rotate, WASD pan, wheel zoom; Left/Right change
//! the animation; T cycles textures (smooth, sharp, pixel); 1-4 time of day
//! (dawn, noon, dusk, night); G toggles the cell grid.

use bevy::anti_alias::smaa::Smaa;
use bevy::asset::AssetId;
use bevy::camera::Hdr;
use bevy::image::ImageSampler;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::pbr::{DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::render::view::Msaa;
use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::gltf::{Gltf, GltfAssetLabel};
use bevy::world_serialization::{WorldAssetRoot, WorldInstanceReady};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use std::collections::HashMap;
use std::f32::consts::TAU;

/// Model units to game cells: a 1.8 m character becomes one cell tall.
const SCALE: f32 = 0.55;
const ANIMATIONS: &str = "models/characters/animations/UAL1_Standard.glb";

fn main() {
    if std::env::args().any(|a| a == "--world-manifest") {
        println!("{}", serde_json::to_string(&world_review_entries()).unwrap());
        return;
    }
    let world_props = std::env::args().any(|a| a == "--world-props");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "My World - look test".into(), ..default() }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(ClearColor(Color::srgb(0.55, 0.72, 0.9)))
        .insert_resource(GlobalAmbientLight { brightness: 300.0, ..default() })
        .insert_resource(View { yaw: 0.6, pitch: 0.5, distance: if world_props { 32.0 } else { 14.0 }, focus: Vec3::new(0.0, 0.0, if world_props { 14.0 } else { 2.0 }) })
        .insert_resource(Settings { time_of_day: 0.4, textures: Textures::Smooth, grid: true })
        .init_resource::<WorldLabels>()
        .init_resource::<Library>()
        .init_resource::<Originals>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (controls, apply_view, apply_daylight, build_library, drive_animation, apply_textures, draw_grid),
        )
        .add_systems(EguiPrimaryContextPass, overlay)
        .run();
}

#[derive(Resource)]
struct View {
    yaw: f32,
    pitch: f32,
    distance: f32,
    focus: Vec3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Textures {
    Smooth,
    Sharp,
    Pixel,
}

#[derive(Resource)]
struct Settings {
    /// 0..1 over a day; 0.25 is sunrise.
    time_of_day: f32,
    textures: Textures,
    grid: bool,
}

/// The animation library, once loaded: a graph with every clip.
#[derive(Resource, Default)]
struct Library {
    gltf: Handle<Gltf>,
    graph: Option<Handle<AnimationGraph>>,
    clips: Vec<(String, AnimationNodeIndex)>,
    current: usize,
}

/// A character's skeleton root, animated by the library.
#[derive(Component)]
struct Rig;

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct MainCamera;

/// Full-resolution copies of model textures, so the texture modes can be undone.
#[derive(Resource, Default)]
struct Originals {
    images: HashMap<AssetId<Image>, Image>,
    applied: HashMap<AssetId<Image>, Textures>,
}

fn model(assets: &AssetServer, path: &str) -> WorldAssetRoot {
    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.to_string())))
}

fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut library: ResMut<Library>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut labels: ResMut<WorldLabels>,
) {
    let reviewing_world = std::env::args().any(|a| a == "--world-props");
    library.gltf = assets.load(ANIMATIONS);

    commands.spawn((
        Camera3d::default(),
        MainCamera,
        Hdr,
        Bloom::NATURAL,
        Msaa::Off,
        ScreenSpaceAmbientOcclusion::default(),
        Smaa::default(),
        DistanceFog {
            color: Color::srgb(0.55, 0.72, 0.9),
            falloff: FogFalloff::Linear { start: 60.0, end: 160.0 },
            ..default()
        },
    ));
    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 10_000.0, shadow_maps_enabled: true, ..default() },
        Transform::default(),
        CascadeShadowConfigBuilder { maximum_distance: 80.0, first_cascade_far_bound: 20.0, ..default() }.build(),
    ));

    // Ground, and a few game blocks (walls, rock, soil) in today's colours for comparison.
    let grass = materials.add(StandardMaterial { base_color: Color::srgb(0.38, 0.52, 0.26), perceptual_roughness: 0.95, ..default() });
    commands.spawn((Mesh3d(meshes.add(Plane3d::default().mesh().size(80.0, if reviewing_world { 90.0 } else { 60.0 }))), MeshMaterial3d(grass)));
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    for (x, z, color) in [
        (-9.5, -1.5, Color::srgb(0.6, 0.57, 0.53)),
        (-9.5, -0.5, Color::srgb(0.6, 0.57, 0.53)),
        (-9.5, 0.5, Color::srgb(0.6, 0.57, 0.53)),
        (-10.5, -1.5, Color::srgb(0.36, 0.34, 0.33)),
        (-11.5, -1.5, Color::srgb(0.42, 0.3, 0.2)),
    ] {
        let mat = materials.add(StandardMaterial { base_color: color, perceptual_roughness: 0.95, ..default() });
        commands.spawn((Mesh3d(cube.clone()), MeshMaterial3d(mat), Transform::from_xyz(x, 0.5, z)));
    }

    // Characters: two bodies, then dressed ones (outfit + head cut from the
    // body by tools/make_heads.py + hair). One animation player per character
    // drives the bones of all its parts, so they move as one.
    let people: [(&[&str], f32); 4] = [
        (&["base/Superhero_Male_FullBody"], -3.0),
        (&["base/Superhero_Female_FullBody"], -1.5),
        (&["outfits/Male_Ranger", "base/Superhero_Male_Head", "hair/Hair_SimpleParted"], 0.0),
        (&["outfits/Female_Peasant", "base/Superhero_Female_Head", "hair/Hair_Buns"], 1.5),
    ];
    for (parts, x) in people {
        let character = commands
            .spawn((AnimationPlayer::default(), Rig, Transform::from_xyz(x, 0.0, 0.0).with_scale(Vec3::splat(SCALE))))
            .id();
        for part in parts {
            let path = format!("models/characters/{}.gltf", part);
            commands.spawn((model(&assets, &path), ChildOf(character))).observe(rig_skeleton);
        }
    }

    if reviewing_world {
        world_props_review(&mut commands, &assets, &mut labels);
        return;
    }

    // Rows of everything else, all at the same scale.
    let rows: [(&str, f32, &[&str]); 6] = [
        ("trees", 4.0, &["Tree_1", "Tree_3", "Birch_2", "Birch_5", "Pine_2", "Pine_4", "DeadTree_2"]),
        (
            "nature",
            9.0,
            &["CommonTree_1", "PineTree_2", "BirchTree_1", "Willow_1", "CommonTree_Autumn_2", "PineTree_Snow_1", "TreeStump", "WoodLog"],
        ),
        (
            "nature",
            12.5,
            &["Rock_1", "Rock_Moss_3", "Rock_5", "Bush_1", "BushBerries_1", "Grass", "Flowers", "Plant_2", "Wheat"],
        ),
        (
            "crops",
            15.0,
            &["Corn_1", "Corn_2", "Corn_3", "Corn_4", "Corn_Harvested", "Wheat_2", "Wheat_4", "Pumpkin_3", "Carrot_4", "Lettuce_4", "Tomato_4"],
        ),
        ("village/buildings", 21.0, &["House_1", "House_3", "Blacksmith", "Mill"]),
        (
            "village/props",
            27.0,
            &["Barrel", "Crate", "Cart", "Well", "Bench_1", "MarketStand_1", "Hay", "Fence", "Bonfire_Lit", "Cauldron"],
        ),
    ];
    for (folder, z, names) in rows {
        let spacing = if folder == "village/buildings" { 6.0 } else { 2.2 };
        let start = -(names.len() as f32 - 1.0) * spacing / 2.0;
        for (k, name) in names.iter().enumerate() {
            let path = format!("models/{}/{}.gltf", folder, name);
            let at = Vec3::new(start + k as f32 * spacing, 0.0, z);
            commands.spawn((model(&assets, &path), Transform::from_translation(at).with_scale(Vec3::splat(SCALE))));
        }
    }
}

/// Labels project onto the selected models rather than using source-pack sizes.
#[derive(Resource, Default)]
struct WorldLabels(Vec<(Vec3, String)>);

fn review_pose(p: &my_world::defs::ModelPose) -> Transform {
    Transform::from_xyz(p.at.0, p.at.1, p.at.2)
        .with_rotation(Quat::from_euler(EulerRot::XYZ, p.turn.0.to_radians(), p.turn.1.to_radians(), p.turn.2.to_radians()))
        .with_scale(Vec3::splat(p.scale))
}

#[derive(serde::Serialize)]
struct WorldReviewEntry {
    label: String,
    path: String,
    scene: usize,
    at: [f32; 3],
    placements: Vec<[f32; 16]>,
    scale_range: [f32; 2],
}

fn world_review_entries() -> Vec<WorldReviewEntry> {
    let defs = my_world::defs::defs();
    let mut entries = Vec::new();
    for (row, plant) in defs.plants.iter().enumerate() {
        let mut col = 0;
        for stage in &plant.stages {
            for (variant, look) in stage.variants.iter().enumerate() {
                let Some(model) = &look.model else { continue };
                entries.push(WorldReviewEntry {
                    label: format!("{} {:.0}% / {}", plant.name, stage.from * 100.0, variant + 1),
                    path: model.path.clone(), scene: 0,
                    at: [-8.0 + col as f32 * 2.3, 0.0, 4.0 + row as f32 * 2.5],
                    placements: model.placements.iter().map(|p| review_pose(p).to_matrix().to_cols_array()).collect(),
                    scale_range: [plant.scale.0, plant.scale.1],
                });
                col += 1;
            }
        }
    }
    let mut col = 0;
    for build in defs.builds.iter().filter(|d| d.model.as_ref().is_some_and(|m| m.path.starts_with("models/world/"))) {
        let model = build.model.as_ref().unwrap();
        for scene in model.scenes.all() {
            entries.push(WorldReviewEntry {
                label: format!("{} / {}", build.name, scene), path: model.path.clone(), scene,
                at: [-10.0 + (col % 8) as f32 * 2.8, 0.0, 21.0 + (col / 8) as f32 * 2.8],
                placements: vec![review_pose(&model.pose).to_matrix().to_cols_array()], scale_range: [1.0, 1.0],
            });
            col += 1;
        }
    }
    entries
}

fn world_props_review(commands: &mut Commands, assets: &AssetServer, labels: &mut WorldLabels) {
    for entry in world_review_entries() {
        let at = Vec3::from_array(entry.at);
        let root = commands.spawn((Transform::from_translation(at), Visibility::default())).id();
        for placement in &entry.placements {
            commands.spawn((model_scene(assets, &entry.path, entry.scene),
                Transform::from_matrix(Mat4::from_cols_array(placement)), ChildOf(root)));
        }
        labels.0.push((at, entry.label));
    }
}

fn model_scene(assets: &AssetServer, path: &str, scene: usize) -> WorldAssetRoot {
    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(scene).from_asset(path.to_owned())))
}

/// Character files have no animations of their own, so the glTF loader
/// doesn't mark their bones. Do what it does for the animation library: name
/// each bone by its path from the `Armature`, so the library's clips find
/// them, and have the character's one player animate it.
fn rig_skeleton(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    names: Query<&Name>,
    parents: Query<&ChildOf>,
) {
    let part = ready.event().entity;
    let Ok(character) = parents.get(part).map(ChildOf::parent) else { return };
    let Some(armature) =
        children.iter_descendants(part).find(|&e| names.get(e).is_ok_and(|n| n.as_str() == "Armature"))
    else {
        return;
    };
    let mut stack = vec![(armature, vec![names.get(armature).unwrap().clone()])];
    while let Some((e, path)) = stack.pop() {
        commands.entity(e).insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(character)));
        for child in children.get(e).into_iter().flatten() {
            if let Ok(name) = names.get(*child) {
                let mut p = path.clone();
                p.push(name.clone());
                stack.push((*child, p));
            }
        }
    }
}

/// Once the animation library is loaded, put all its clips in one graph.
fn build_library(mut library: ResMut<Library>, gltfs: Res<Assets<Gltf>>, mut graphs: ResMut<Assets<AnimationGraph>>) {
    if library.graph.is_some() {
        return;
    }
    let Some(gltf) = gltfs.get(&library.gltf) else { return };
    let mut named: Vec<(String, Handle<AnimationClip>)> =
        gltf.named_animations.iter().map(|(n, h)| (n.to_string(), h.clone())).collect();
    named.sort_by(|a, b| a.0.cmp(&b.0));
    let (graph, nodes) = AnimationGraph::from_clips(named.iter().map(|(_, h)| h.clone()));
    library.clips = named.into_iter().map(|(n, _)| n).zip(nodes).collect();
    library.current = library.clips.iter().position(|(n, _)| n == "Walk_Loop").unwrap_or(0);
    library.graph = Some(graphs.add(graph));
}

/// Every character plays the chosen clip.
fn drive_animation(
    mut commands: Commands,
    library: Res<Library>,
    mut rigs: Query<(Entity, &mut AnimationPlayer, Option<&AnimationGraphHandle>), With<Rig>>,
) {
    let (Some(graph), Some((_, node))) = (&library.graph, library.clips.get(library.current)) else { return };
    for (e, mut player, has_graph) in &mut rigs {
        if has_graph.is_none() {
            commands.entity(e).insert(AnimationGraphHandle(graph.clone()));
        }
        if !player.is_playing_animation(*node) {
            player.stop_all();
            player.play(*node).repeat();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    mut view: ResMut<View>,
    mut settings: ResMut<Settings>,
    mut library: ResMut<Library>,
) {
    let dt = time.delta_secs();
    let forward = Vec3::new(-view.yaw.sin(), 0.0, -view.yaw.cos());
    let right = Vec3::new(view.yaw.cos(), 0.0, -view.yaw.sin());
    let mut pan = Vec3::ZERO;
    for (key, dir) in [(KeyCode::KeyW, forward), (KeyCode::KeyS, -forward), (KeyCode::KeyD, right), (KeyCode::KeyA, -right)] {
        if keys.pressed(key) {
            pan += dir;
        }
    }
    let speed = 0.9 * view.distance + 4.0;
    view.focus += pan.normalize_or_zero() * speed * dt;
    if keys.pressed(KeyCode::KeyQ) {
        view.yaw -= 1.6 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        view.yaw += 1.6 * dt;
    }
    if buttons.pressed(MouseButton::Right) {
        view.yaw -= motion.delta.x * 0.006;
        view.pitch = (view.pitch + motion.delta.y * 0.004).clamp(0.1, 1.5);
    }
    if scroll.delta.y != 0.0 {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        view.distance = (view.distance * 0.88f32.powf(lines)).clamp(2.0, 80.0);
    }
    let count = library.clips.len();
    if count > 0 {
        if keys.just_pressed(KeyCode::ArrowRight) {
            library.current = (library.current + 1) % count;
        }
        if keys.just_pressed(KeyCode::ArrowLeft) {
            library.current = (library.current + count - 1) % count;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) {
        settings.textures = match settings.textures {
            Textures::Smooth => Textures::Sharp,
            Textures::Sharp => Textures::Pixel,
            Textures::Pixel => Textures::Smooth,
        };
    }
    for (key, t) in [(KeyCode::Digit1, 0.27), (KeyCode::Digit2, 0.5), (KeyCode::Digit3, 0.76), (KeyCode::Digit4, 0.95)] {
        if keys.just_pressed(key) {
            settings.time_of_day = t;
        }
    }
    if keys.just_pressed(KeyCode::KeyG) {
        settings.grid = !settings.grid;
    }
}

fn apply_view(view: Res<View>, mut camera: Single<&mut Transform, With<MainCamera>>) {
    let offset = Vec3::new(view.yaw.sin() * view.pitch.cos(), view.pitch.sin(), view.yaw.cos() * view.pitch.cos());
    **camera = Transform::from_translation(view.focus + offset * view.distance).looking_at(view.focus, Vec3::Y);
}

/// The game's sun, moon, ambient light and sky for a time of day.
fn apply_daylight(
    settings: Res<Settings>,
    mut sun: Single<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut fog: Single<&mut DistanceFog>,
) {
    let t = settings.time_of_day;
    let day = 0.5 - 0.5 * (t * TAU).cos();
    let angle = (t - 0.25) * TAU;
    let elevation = angle.sin();
    let (light, transform) = &mut *sun;
    let (dir, illuminance, color) = if elevation > -0.05 {
        let dir = Vec3::new(angle.cos(), elevation.max(0.18), 0.45).normalize();
        let warmth = (1.0 - elevation.clamp(0.0, 1.0)).powi(3);
        (dir, 1_500.0 + 10_000.0 * elevation.clamp(0.0, 1.0), Color::srgb(1.0, 1.0 - 0.3 * warmth, 1.0 - 0.55 * warmth))
    } else {
        (Vec3::new(-0.4, 0.85, 0.35).normalize(), 600.0, Color::srgb(0.65, 0.72, 1.0))
    };
    light.illuminance = illuminance;
    light.color = color;
    **transform = Transform::default().looking_to(-dir, Vec3::Y);
    ambient.brightness = 25.0 + 475.0 * day;
    ambient.color = Color::srgb(0.75 + 0.25 * day, 0.8 + 0.2 * day, 1.0);
    let sky = Vec3::new(0.02, 0.03, 0.08).lerp(Vec3::new(0.55, 0.72, 0.9), day);
    clear.0 = Color::srgb(sky.x, sky.y, sky.z);
    fog.color = clear.0;
}

/// A quarter of the size (at least 16 pixels), averaging each block.
fn downsample(img: &Image, factor: u32) -> Option<Image> {
    let format = img.texture_descriptor.format;
    if !matches!(format, TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm) {
        return None;
    }
    let data = img.data.as_ref()?;
    let (w, h) = (img.width(), img.height());
    let (nw, nh) = ((w / factor).max(16).min(w), (h / factor).max(16).min(h));
    let (fx, fy) = (w / nw, h / nh);
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = [0u32; 4];
            for dy in 0..fy {
                for dx in 0..fx {
                    let i = (((y * fy + dy) * w + (x * fx + dx)) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += data[i + c] as u32;
                    }
                }
            }
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (sum[c] / (fx * fy)) as u8;
            }
        }
    }
    let mut small = img.clone();
    small.texture_descriptor.size = Extent3d { width: nw, height: nh, depth_or_array_layers: 1 };
    small.texture_descriptor.mip_level_count = 1;
    small.data = Some(out);
    Some(small)
}

/// Switch model textures between smooth, sharp (nearest) and pixel (a
/// quarter of the resolution, nearest), keeping the originals to go back.
fn apply_textures(
    settings: Res<Settings>,
    materials: Res<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut originals: ResMut<Originals>,
) {
    let mode = settings.textures;
    let mut ids: Vec<AssetId<Image>> = Vec::new();
    for (_, m) in materials.iter() {
        for h in [&m.base_color_texture, &m.normal_map_texture, &m.metallic_roughness_texture, &m.occlusion_texture]
            .into_iter()
            .flatten()
        {
            ids.push(h.id());
        }
    }
    for id in ids {
        if originals.applied.get(&id) == Some(&mode) {
            continue;
        }
        let Some(img) = images.get(id) else { continue };
        if img.data.is_none() {
            continue;
        }
        originals.images.entry(id).or_insert_with(|| img.clone());
        let original = &originals.images[&id];
        let mut next = match mode {
            Textures::Pixel => downsample(original, 4).unwrap_or_else(|| original.clone()),
            _ => original.clone(),
        };
        next.sampler = if mode == Textures::Smooth { original.sampler.clone() } else { ImageSampler::nearest() };
        if let Some(mut img) = images.get_mut(id) {
            *img = next;
        }
        originals.applied.insert(id, mode);
    }
}

fn draw_grid(settings: Res<Settings>, mut gizmos: Gizmos) {
    if settings.grid {
        let iso = Isometry3d::new(Vec3::new(0.0, 0.01, 10.0), Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
        gizmos.grid(iso, UVec2::new(40, 40), Vec2::ONE, Color::srgba(1.0, 1.0, 1.0, 0.12));
    }
}

fn overlay(mut contexts: EguiContexts, library: Res<Library>, settings: Res<Settings>, labels: Res<WorldLabels>, camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::Window::new("Look test").anchor(egui::Align2::LEFT_TOP, [8.0, 8.0]).resizable(false).show(ctx, |ui| {
        let clip = library.clips.get(library.current).map_or("loading...", |(n, _)| n.as_str());
        ui.label(format!("Animation: {} ({}/{})", clip, library.current + 1, library.clips.len()));
        ui.label(format!("Textures: {:?}", settings.textures));
        ui.label(format!("Scale: {} (a character is about one cell tall)", SCALE));
        ui.separator();
        ui.label("Left/Right: animation   T: textures   1-4: dawn, noon, dusk, night   G: grid");
        ui.label("Right-drag or Q/E: rotate   WASD: pan   wheel: zoom");
        ui.separator();
        ui.label("Front: characters (bodies, ranger, peasant) and game blocks on the left");
        if labels.0.is_empty() {
            ui.label("Behind: textured trees, low-poly trees, rocks and plants, crops, village");
        } else {
            ui.label("Behind: actual plant stages, machinery, grave states and all fence connections");
            ui.label("World props use their definition transforms; cell grid shows their gameplay footprint");
        }
    });
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("world_labels")));
    let (cam, gt) = *camera;
    for (at, name) in &labels.0 {
        if let Ok(p) = cam.world_to_viewport(gt, *at) {
            painter.text(egui::pos2(p.x, p.y + 8.0), egui::Align2::CENTER_TOP, name,
                egui::FontId::proportional(11.0), egui::Color32::WHITE);
        }
    }
    Ok(())
}
