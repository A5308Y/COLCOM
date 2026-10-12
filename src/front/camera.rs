//! RTS-style camera: orbit around a focus point on the ground.
//!
//! WASD/arrows pan, Q/E rotate, wheel zooms, right-drag orbits, middle-drag pans.

use super::input::UiState;
use super::sim::Sim;
use bevy::anti_alias::smaa::Smaa;
use bevy::camera::Hdr;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::{DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{egui_wants_any_keyboard_input, EguiWantsInput};

#[derive(Component)]
pub struct MainCamera;

#[derive(Resource)]
pub struct CameraRig {
    pub focus: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl CameraRig {
    fn offset(&self) -> Vec3 {
        Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos()) * self.distance
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translation(self.focus + self.offset()).looking_at(self.focus, Vec3::Y)
    }

    /// Ground directions relative to the view: (forward, right).
    pub fn axes(&self) -> (Vec3, Vec3) {
        let forward = Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        (forward, right)
    }
}

/// How far the mouse moved while the right button was held, so a right
/// *click* can be told apart from a right *drag* (camera orbit).
#[derive(Resource, Default)]
pub struct DragTracker {
    pub right_moved: f32,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraRig { focus: Vec3::ZERO, yaw: 0.6, pitch: 0.95, distance: 28.0 })
            .init_resource::<DragTracker>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    keyboard_controls.run_if(not(egui_wants_any_keyboard_input)),
                    mouse_controls,
                    apply_rig,
                )
                    .chain(),
            );
    }
}

fn spawn_camera(mut commands: Commands, mut rig: ResMut<CameraRig>, sim: Res<Sim>) {
    rig.focus = colony_center(&sim);
    commands.spawn((
        Camera3d::default(),
        MainCamera,
        rig.transform(),
        // HDR so lamps and gunfire can glow (bloom).
        Hdr,
        Bloom::NATURAL,
        // Ambient occlusion darkens corners and creases between blocks. It
        // needs multisampling off, so SMAA smooths edges instead.
        Msaa::Off,
        ScreenSpaceAmbientOcclusion::default(),
        Smaa::default(),
        // Distance haze; its colour follows the sky (see scene.rs).
        DistanceFog {
            color: Color::srgb(0.55, 0.72, 0.9),
            falloff: FogFalloff::Linear { start: 90.0, end: 240.0 },
            ..default()
        },
    ));
}

/// Average position of living colonists (or the map centre).
pub fn colony_center(sim: &Sim) -> Vec3 {
    let living: Vec<(f32, f32, f32)> =
        sim.world.pawns.iter().filter(|p| p.is_colonist() && !p.dead).map(|p| (p.x, p.y, p.z)).collect();
    if living.is_empty() {
        let m = &sim.world.map;
        return Vec3::new(m.w as f32 / 2.0, 0.0, m.h as f32 / 2.0);
    }
    let n = living.len() as f32;
    let x = living.iter().map(|p| p.0).sum::<f32>() / n;
    let y = living.iter().map(|p| p.1).sum::<f32>() / n;
    let z = living.iter().map(|p| p.2).sum::<f32>() / n;
    super::tile_point(x, y, z)
}

fn keyboard_controls(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut rig: ResMut<CameraRig>) {
    let dt = time.delta_secs();
    let (forward, right) = rig.axes();
    let mut pan = Vec3::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        pan += forward;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        pan -= forward;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        pan += right;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        pan -= right;
    }
    let speed = 0.9 * rig.distance + 6.0;
    rig.focus += pan.normalize_or_zero() * speed * dt;
    if keys.pressed(KeyCode::KeyQ) {
        rig.yaw -= 1.6 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        rig.yaw += 1.6 * dt;
    }
}

#[allow(clippy::too_many_arguments)]
fn mouse_controls(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    egui_wants_pointer: Option<Res<EguiWantsInput>>,
    ui: Res<UiState>,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut drag: ResMut<DragTracker>,
    mut rig: ResMut<CameraRig>,
) {
    let over_dock = window.is_some_and(|w| ui.over_dock(&w));
    let over_ui = egui_wants_pointer.is_some_and(|w| w.wants_any_pointer_input()) || over_dock;
    let delta = motion.delta;

    if buttons.just_pressed(MouseButton::Right) {
        drag.right_moved = 0.0;
    }
    if buttons.pressed(MouseButton::Right) {
        drag.right_moved += delta.length();
        // Small jitter shouldn't spin the camera, so a click stays a click.
        if drag.right_moved > 4.0 {
            rig.yaw -= delta.x * 0.006;
            rig.pitch = (rig.pitch + delta.y * 0.004).clamp(0.3, 1.5);
        }
    }
    if buttons.pressed(MouseButton::Middle) {
        let (forward, right) = rig.axes();
        let k = rig.distance * 0.0022;
        rig.focus += (-right * delta.x + forward * delta.y) * k;
    }
    if !over_ui && scroll.delta.y != 0.0 {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        };
        rig.distance = (rig.distance * 0.88f32.powf(lines)).clamp(5.0, 110.0);
    }
}

fn apply_rig(
    sim: Res<Sim>,
    ui: Res<UiState>,
    time: Res<Time>,
    mut rig: ResMut<CameraRig>,
    mut camera: Single<&mut Transform, With<MainCamera>>,
) {
    let m = &sim.world.map;
    rig.focus.x = rig.focus.x.clamp(0.0, m.w as f32 - 0.01);
    rig.focus.z = rig.focus.z.clamp(0.0, m.h as f32 - 0.01);
    // Glide up and down with the ground under the focus point.
    // (In the layer view, never above the viewed level.)
    let (fx, fz) = (rig.focus.x as usize, rig.focus.z as usize);
    let level = m.surface(fx, fz).unwrap_or(0).min(ui.top_level(m));
    let ground = level as f32 * super::terrain::LEVEL_H;
    let k = (time.delta_secs() * 4.0).min(1.0);
    rig.focus.y += (ground - rig.focus.y) * k;
    **camera = rig.transform();
}
