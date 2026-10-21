//! Sun, moon, sky and ambient light following the simulation's day/night cycle,
//! dimmed and greyed by cloud and closed in by fog (`weather::WeatherLook`).

use super::camera::MainCamera;
use super::sim::Sim;
use super::weather::WeatherLook;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, GlobalAmbientLight};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use colcom::DAY_LEN;
use std::f32::consts::TAU;

#[derive(Component)]
struct Sun;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(DirectionalLightShadowMap { size: 4096 })
            .insert_resource(ClearColor(Color::srgb(0.55, 0.72, 0.9)))
            .insert_resource(GlobalAmbientLight { brightness: 300.0, ..default() })
            .add_systems(Startup, spawn_sun)
            .add_systems(Update, update_daylight);
    }
}

fn spawn_sun(mut commands: Commands) {
    commands.spawn((
        Sun,
        DirectionalLight { illuminance: 10_000.0, shadow_maps_enabled: true, ..default() },
        Transform::default(),
        CascadeShadowConfigBuilder { maximum_distance: 140.0, first_cascade_far_bound: 30.0, ..default() }.build(),
    ));
}

fn update_daylight(
    sim: Res<Sim>,
    mut sun: Single<(&mut DirectionalLight, &mut Transform), With<Sun>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut clear: ResMut<ClearColor>,
    mut fog: Query<&mut DistanceFog, With<MainCamera>>,
    weather: Res<WeatherLook>,
) {
    let w = &sim.world;
    let day = w.daylight();
    // 0 at 6:00 (sunrise in the east), quarter turn at noon, half at 18:00.
    let t = (w.time() % DAY_LEN) / DAY_LEN;
    let angle = (t - 0.25) * TAU;
    let elevation = angle.sin();
    let (light, transform) = &mut *sun;

    let (dir, illuminance, color) = if elevation > -0.05 {
        // Sun: low and warm at dawn/dusk, high and white at noon.
        let dir = Vec3::new(angle.cos(), elevation.max(0.18), 0.45).normalize();
        let warmth = (1.0 - elevation.clamp(0.0, 1.0)).powi(3);
        let color = Color::srgb(1.0, 1.0 - 0.3 * warmth, 1.0 - 0.55 * warmth);
        (dir, 1_500.0 + 10_000.0 * elevation.clamp(0.0, 1.0), color)
    } else {
        // Moonlight from high up so night shadows stay readable.
        (Vec3::new(-0.4, 0.85, 0.35).normalize(), 600.0, Color::srgb(0.65, 0.72, 1.0))
    };
    // Cloud keeps most of the sun off.
    light.illuminance = illuminance * (1.0 - 0.65 * weather.overcast);
    light.color = color;
    **transform = Transform::default().looking_to(-dir, Vec3::Y);

    // Dark nights, so lamps matter.
    ambient.brightness = (25.0 + 475.0 * day) * (1.0 - 0.25 * weather.overcast);
    ambient.color = Color::srgb(0.75 + 0.25 * day, 0.8 + 0.2 * day, 1.0);
    let night = Vec3::new(0.02, 0.03, 0.08);
    let noon = Vec3::new(0.55, 0.72, 0.9);
    let grey = Vec3::new(0.56, 0.6, 0.64);
    let sky = night.lerp(noon.lerp(grey, weather.overcast.max(weather.fog)), day);
    clear.0 = Color::srgb(sky.x, sky.y, sky.z);
    // The haze comes in from far off to a few cells away in thick fog.
    let start = 90.0 + (6.0 - 90.0) * weather.fog;
    let end = 240.0 + (45.0 - 240.0) * weather.fog;
    for mut f in &mut fog {
        f.color = clear.0;
        f.falloff = FogFalloff::Linear { start, end };
    }
}
