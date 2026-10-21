//! Weather as drawn: rain or snow falling around where the camera looks, and
//! how overcast and foggy it is (used by `scene.rs` for the light and haze).
//! Everything eases in and out, so a change of weather fades rather than flips.

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
    pub snow: bool,
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
            .add_systems(Update, (ease_weather, fall).chain());
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
    let (overcast, fog, falling) = match w.weather() {
        Weather::Clear => (0.0, 0.0, 0.0),
        Weather::Fog => (0.5, 1.0, 0.0),
        Weather::Rain => (0.6, 0.12, 1.0),
    };
    let k = (EASE * time.delta_secs()).min(1.0);
    look.overcast += (overcast - look.overcast) * k;
    look.fog += (fog - look.fog) * k;
    look.falling += (falling - look.falling) * k;
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
    let (speed, sway) = if look.snow { (1.6, 0.4) } else { (16.0, 0.0) };
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
