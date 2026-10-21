//! Weather as drawn: rain or snow falling around where the camera looks, wind
//! swaying plants near it, and how overcast and foggy it is (used by
//! `scene.rs` for the light and haze). Everything eases in and out, so a
//! change of weather fades rather than flips.

use super::camera::CameraRig;
use super::sim::Sim;
use super::terrain::LEVEL_H;
use bevy::prelude::*;
use colcom::weather::{Precipitation, Weather};

/// Drops (or flakes) in the pool; as many as are shown in full rain.
const POOL: usize = 800;
/// How fast the look follows the weather (share per real second).
const EASE: f32 = 0.25;

/// How the weather looks right now, each 0..1.
#[derive(Resource, Default)]
pub struct WeatherLook {
    /// Cloud over the sun: dims the light, greys the sky.
    pub overcast: f32,
    /// Fog: the haze comes close.
    pub fog: f32,
    /// How much is falling (rain or snow).
    pub falling: f32,
    /// How hard the wind blows.
    pub wind: f32,
    pub snow: bool,
}

/// A plant that leans and sways in the wind, about its base.
#[derive(Component)]
pub struct Sway {
    /// Its rotation in still air.
    base: Quat,
    /// Most it leans (radians).
    most: f32,
    phase: f32,
}

impl Sway {
    /// For a plant turned `base`, leaning at most `degrees`, at world position `at`.
    pub fn new(base: Quat, degrees: f32, at: Vec3) -> Sway {
        let seed = (at.x * 92.7 + at.z * 31.3) as i32 as u32;
        Sway { base, most: degrees.to_radians(), phase: scatter(seed) * std::f32::consts::TAU }
    }
}

#[derive(Component)]
struct Drop {
    /// Where it stops: the roof, floor or ground below it.
    floor: f32,
    /// Its own sway, for snow.
    phase: f32,
}

#[derive(Resource)]
struct DropLooks {
    rain: Handle<StandardMaterial>,
    snow: Handle<StandardMaterial>,
    rain_mesh: Handle<Mesh>,
    snow_mesh: Handle<Mesh>,
    showing_snow: bool,
}

pub struct WeatherPlugin;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WeatherLook>()
            .add_systems(Startup, spawn_drops)
            .add_systems(Update, (ease_weather, fall, sway).chain());
    }
}

/// A cheap repeatable "random" number in 0..1 from a counter.
fn scatter(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFF_FFFF) as f32 / 0x100_0000 as f32
}

fn spawn_drops(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let mut look = |c: Color| {
        materials.add(StandardMaterial { base_color: c, unlit: true, alpha_mode: AlphaMode::Blend, ..default() })
    };
    let looks = DropLooks {
        rain: look(Color::srgba(0.75, 0.8, 0.9, 0.22)),
        snow: look(Color::srgba(0.97, 0.98, 1.0, 0.9)),
        rain_mesh: meshes.add(Cuboid::new(0.01, 0.24, 0.01)),
        snow_mesh: meshes.add(Cuboid::new(0.05, 0.05, 0.05)),
        showing_snow: false,
    };
    for k in 0..POOL {
        commands.spawn((
            Drop { floor: f32::MAX, phase: scatter(k as u32 * 7 + 3) * std::f32::consts::TAU },
            Mesh3d(looks.rain_mesh.clone()),
            MeshMaterial3d(looks.rain.clone()),
            Transform::default(),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
    }
    commands.insert_resource(looks);
}

fn ease_weather(sim: Res<Sim>, time: Res<Time>, mut look: ResMut<WeatherLook>) {
    let w = &sim.world;
    let (overcast, fog, falling, wind) = match w.weather() {
        Weather::Clear => (0.0, 0.0, 0.0, 0.45),
        Weather::Fog => (0.5, 1.0, 0.0, 0.15),
        Weather::Rain => (0.6, 0.12, 1.0, 1.0),
    };
    let k = (EASE * time.delta_secs()).min(1.0);
    look.overcast += (overcast - look.overcast) * k;
    look.fog += (fog - look.fog) * k;
    look.falling += (falling - look.falling) * k;
    look.wind += (wind - look.wind) * k;
    // What falls changes at once with the temperature; only how much eases.
    look.snow = w.falling().map_or(look.snow, |f| f == Precipitation::Snow);
}

/// Top of what stands in column (x, y) under level `below`: where a drop stops.
fn floor_under(m: &colcom::map::Map, x: usize, y: usize, below: usize) -> f32 {
    for z in (0..below.min(m.d)).rev() {
        let i = m.idx((x, y, z));
        if m.block[i].solid() {
            return (z + 1) as f32 * LEVEL_H;
        }
        if m.roof[i] || m.floor[i] {
            return z as f32 * LEVEL_H + 0.1;
        }
    }
    0.0
}

fn fall(
    sim: Res<Sim>,
    time: Res<Time>,
    rig: Res<CameraRig>,
    look: Res<WeatherLook>,
    mut looks: ResMut<DropLooks>,
    mut drops: Query<(&mut Drop, &mut Transform, &mut Visibility, &mut Mesh3d, &mut MeshMaterial3d<StandardMaterial>)>,
    mut counter: Local<u32>,
) {
    let m = &sim.world.map;
    let dt = time.delta_secs().min(0.1);
    let shown = (POOL as f32 * look.falling) as usize;
    let swap = looks.showing_snow != look.snow;
    looks.showing_snow = look.snow;
    // Around where the camera looks, wider when zoomed out.
    let half = (rig.distance * 0.7).clamp(12.0, 50.0);
    let height = 18.0;
    let (speed, sway) = if look.snow { (1.6, 0.3 + 1.2 * look.wind) } else { (16.0, 0.0) };
    let t = time.elapsed_secs();
    for (k, (mut drop, mut tf, mut vis, mut mesh, mut mat)) in drops.iter_mut().enumerate() {
        if swap {
            mesh.0 = if look.snow { looks.snow_mesh.clone() } else { looks.rain_mesh.clone() };
            mat.0 = if look.snow { looks.snow.clone() } else { looks.rain.clone() };
        }
        if k >= shown {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
                drop.floor = f32::MAX;
            }
            continue;
        }
        let p = &mut tf.translation;
        p.y -= speed * dt;
        p.x += sway * (t * 0.9 + drop.phase).sin() * dt;
        let away = (p.x - rig.focus.x).abs() > half || (p.z - rig.focus.z).abs() > half;
        if drop.floor == f32::MAX || p.y <= drop.floor || away {
            // Somewhere new over the area, at a random height (so they don't fall in sheets).
            *counter = counter.wrapping_add(1);
            let n = *counter;
            let x = rig.focus.x + (scatter(n * 3) * 2.0 - 1.0) * half;
            let z = rig.focus.z + (scatter(n * 3 + 1) * 2.0 - 1.0) * half;
            let top = rig.focus.y + height;
            let (cx, cy) = (x.floor(), z.floor());
            let inside = cx >= 0.0 && cy >= 0.0 && (cx as usize) < m.w && (cy as usize) < m.h;
            drop.floor = if inside { floor_under(m, cx as usize, cy as usize, m.d) } else { 0.0 };
            *p = Vec3::new(x, drop.floor + scatter(n * 3 + 2) * (top - drop.floor).max(1.0), z);
            *vis = Visibility::Inherited;
        }
    }
}

/// Plants near where the camera looks lean downwind (towards +x) and sway,
/// in gusts that roll across the map. Those further off keep their last pose.
fn sway(time: Res<Time>, rig: Res<CameraRig>, look: Res<WeatherLook>, mut plants: Query<(&Sway, &mut Transform)>) {
    let t = time.elapsed_secs();
    let reach = (rig.distance * 1.2).max(30.0);
    for (s, mut tf) in &mut plants {
        let p = tf.translation;
        if (p.x - rig.focus.x).abs() > reach || (p.z - rig.focus.z).abs() > reach {
            continue;
        }
        let gust = 0.65 + 0.35 * (0.35 * t - 0.08 * p.x + 0.05 * p.z).sin();
        let flutter = 0.5 * (1.7 * t + s.phase).sin() + 0.2 * (2.9 * t + 1.3 * s.phase).sin();
        let lean = s.most * look.wind * gust * (0.6 + 0.4 * flutter);
        tf.rotation = Quat::from_rotation_z(-lean) * s.base;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sway_leans_downwind_within_its_limit() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(CameraRig { focus: Vec3::ZERO, yaw: 0.0, pitch: 1.0, distance: 20.0 })
            .insert_resource(WeatherLook { wind: 1.0, ..default() })
            .add_systems(Update, sway);
        let base = Quat::from_rotation_y(0.7);
        let near = app.world_mut().spawn((Sway::new(base, 3.0, Vec3::new(2.0, 0.0, 3.0)), Transform::from_xyz(2.0, 0.0, 3.0).with_rotation(base))).id();
        let far = app.world_mut().spawn((Sway::new(base, 3.0, Vec3::ZERO), Transform::from_xyz(500.0, 0.0, 0.0).with_rotation(base))).id();
        for _ in 0..30 {
            app.update();
            let tf = app.world().get::<Transform>(near).unwrap();
            let up = tf.rotation * Vec3::Y;
            assert!(up.x > 0.0, "leans towards +x: {:?}", up);
            assert!(up.angle_between(Vec3::Y) <= 3.0f32.to_radians() + 1e-4);
        }
        assert_eq!(app.world().get::<Transform>(far).unwrap().rotation, base, "far off: left alone");
    }
}
