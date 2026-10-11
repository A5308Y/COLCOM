//! Soil, rock, water and walls as chunked meshes with vertex colours.
//!
//! The map is split into 16×16-column chunks covering every level. Only faces
//! between a solid cell and an open one are emitted. Each frame a signature of
//! every chunk is compared with the last one, and only changed chunks are
//! rebuilt.

use super::look::*;
use super::sim::Sim;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use my_world::map::{Block, Feature, Map, Pos, Zone};

const CHUNK: usize = 16;
/// World units per level.
pub const LEVEL_H: f32 = 1.0;
/// Water surfaces sit a little below the top of their cell.
const WATER_DIP: f32 = 0.12;

/// What a cell looks like to the mesher.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look3 {
    Open,
    Soil,
    Rock,
    Wall,
    Water,
}

fn look_at(m: &Map, p: Pos) -> Look3 {
    let i = m.idx(p);
    match (m.block[i], m.feature[i]) {
        (Block::Soil, _) => Look3::Soil,
        (Block::Rock, _) => Look3::Rock,
        (Block::Water, _) => Look3::Water,
        (Block::Air, Feature::Wall) => Look3::Wall,
        (Block::Air, _) => Look3::Open,
    }
}

impl Look3 {
    /// Hides faces behind it.
    fn opaque(self) -> bool {
        matches!(self, Look3::Soil | Look3::Rock | Look3::Wall)
    }
}

#[derive(Resource)]
struct Chunks {
    generation: u64,
    cols: usize,
    rows: usize,
    sigs: Vec<u64>,
    entities: Vec<Option<Entity>>,
    material: Handle<StandardMaterial>,
}

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, update_chunks);
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() });
    commands.insert_resource(Chunks { generation: u64::MAX, cols: 0, rows: 0, sigs: Vec::new(), entities: Vec::new(), material });
}

fn update_chunks(mut commands: Commands, sim: Res<Sim>, chunks: ResMut<Chunks>, mut meshes: ResMut<Assets<Mesh>>) {
    let m = &sim.world.map;
    let chunks = chunks.into_inner();
    let (cols, rows) = (m.w.div_ceil(CHUNK), m.h.div_ceil(CHUNK));
    if chunks.generation != sim.generation || chunks.cols != cols || chunks.rows != rows {
        for e in chunks.entities.drain(..).flatten() {
            commands.entity(e).despawn();
        }
        chunks.entities = vec![None; cols * rows];
        chunks.sigs = vec![u64::MAX; cols * rows];
        chunks.generation = sim.generation;
        chunks.cols = cols;
        chunks.rows = rows;
    }
    for cy in 0..rows {
        for cx in 0..cols {
            let ci = cy * cols + cx;
            let sig = signature(m, cx, cy);
            if sig == chunks.sigs[ci] {
                continue;
            }
            chunks.sigs[ci] = sig;
            let mesh = meshes.add(build_chunk(m, cx, cy));
            match chunks.entities[ci] {
                Some(e) => {
                    commands.entity(e).insert(Mesh3d(mesh));
                }
                None => {
                    let e = commands
                        .spawn((Mesh3d(mesh), MeshMaterial3d(chunks.material.clone()), Transform::default()))
                        .id();
                    chunks.entities[ci] = Some(e);
                }
            }
        }
    }
}

fn blood_level(m: &Map, i: usize) -> u8 {
    (m.blood.get(i).copied().unwrap_or(0.0) * 3.0).ceil() as u8
}

/// Everything that affects a chunk's mesh, including a one-column border
/// (neighbouring cells decide which faces are visible).
fn signature(m: &Map, cx: usize, cy: usize) -> u64 {
    // FNV-1a over small codes: cheap enough to run on every chunk every frame.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u8| h = (h ^ v as u64).wrapping_mul(0x0000_0100_0000_01b3);
    let x0 = (cx * CHUNK).saturating_sub(1);
    let y0 = (cy * CHUNK).saturating_sub(1);
    let x1 = ((cx + 1) * CHUNK + 1).min(m.w);
    let y1 = ((cy + 1) * CHUNK + 1).min(m.h);
    for z in 0..m.d {
        for y in y0..y1 {
            for x in x0..x1 {
                let p = (x, y, z);
                let look = look_at(m, p);
                mix(look as u8);
                if look == Look3::Open {
                    // Open cells colour the ground below them.
                    let i = m.idx(p);
                    mix(m.zone[i] as u8);
                    mix(m.room.get(i).is_some_and(|&r| r != 0) as u8);
                    mix(blood_level(m, i));
                }
            }
        }
    }
    h
}

#[derive(Default)]
struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl MeshBuf {
    /// A flat quad; corner order is fixed up so it faces along `normal`.
    fn quad(&mut self, mut c: [Vec3; 4], normal: Vec3, color: Rgb) {
        if (c[1] - c[0]).cross(c[2] - c[0]).dot(normal) < 0.0 {
            c.reverse();
        }
        let base = self.pos.len() as u32;
        let color = color.vertex();
        for p in c {
            self.pos.push(p.to_array());
            self.nrm.push(normal.to_array());
            self.col.push(color);
        }
        self.idx.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Horizontal face at height `y` covering the cell's footprint.
    fn flat(&mut self, x: f32, z: f32, y: f32, normal: Vec3, color: Rgb) {
        let c = [Vec3::new(x, y, z), Vec3::new(x, y, z + 1.0), Vec3::new(x + 1.0, y, z + 1.0), Vec3::new(x + 1.0, y, z)];
        self.quad(c, normal, color);
    }

    /// Vertical face on the cell edge in direction (dx, dz), spanning lo..hi.
    #[allow(clippy::too_many_arguments)]
    fn side(&mut self, x: f32, z: f32, dx: i32, dz: i32, lo: f32, hi: f32, color: Rgb) {
        let (ax, az, bx, bz) = match (dx, dz) {
            (1, 0) => (x + 1.0, z, x + 1.0, z + 1.0),
            (-1, 0) => (x, z, x, z + 1.0),
            (0, 1) => (x, z + 1.0, x + 1.0, z + 1.0),
            _ => (x, z, x + 1.0, z),
        };
        let c = [Vec3::new(ax, lo, az), Vec3::new(bx, lo, bz), Vec3::new(bx, hi, bz), Vec3::new(ax, hi, az)];
        self.quad(c, Vec3::new(dx as f32, 0.0, dz as f32), color);
    }

    fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        mesh.insert_indices(Indices::U32(self.idx));
        mesh
    }
}

/// Colour of the top of solid ground, from what's on it (zones, floors, blood).
fn ground_color(m: &Map, solid: Pos, look: Look3) -> Rgb {
    let (x, y, z) = solid;
    let (xi, yi) = (x as i32, y as i32);
    let above = m.above(solid).map(|a| m.idx(a));
    let zone = above.map_or(Zone::None, |a| m.zone[a]);
    let indoors = above.is_some_and(|a| m.room.get(a).is_some_and(|&r| r != 0));
    let jitter = hash(xi, yi + 97 * z as i32, 7);
    let mut c = if zone == Zone::Grow {
        SOIL.shade(0.92 + jitter * 0.12)
    } else if indoors {
        FLOOR.shade(0.94 + jitter * 0.1)
    } else if look == Look3::Rock {
        ROCK_TOP.shade(0.94 + jitter * 0.1)
    } else {
        let n = noise(x as f32 / 6.0, y as f32 / 6.0, 1) * 0.75 + hash(xi, yi, 2) * 0.25;
        GRASS_A.mix(GRASS_B, n)
    };
    if let Some(a) = above {
        if zone == Zone::Stockpile {
            c = c.mix(STOCKPILE, 0.22);
        }
        let blood = blood_level(m, a);
        if blood > 0 {
            c = c.mix(BLOOD, 0.22 * blood as f32);
        }
    }
    c
}

fn build_chunk(m: &Map, cx: usize, cy: usize) -> Mesh {
    let mut b = MeshBuf::default();
    let look_off = |x: i32, y: i32, z: i32| m.at(x, y, z).map(|p| look_at(m, p));
    for z in 0..m.d {
        for y in cy * CHUNK..((cy + 1) * CHUNK).min(m.h) {
            for x in cx * CHUNK..((cx + 1) * CHUNK).min(m.w) {
                let p = (x, y, z);
                let look = look_at(m, p);
                if look == Look3::Open {
                    continue;
                }
                let (fx, fz) = (x as f32, y as f32);
                let (xi, yi, zi) = (x as i32, y as i32, z as i32);
                let (lo, hi) = (z as f32 * LEVEL_H, (z + 1) as f32 * LEVEL_H);
                let above = look_off(xi, yi, zi + 1);

                if look == Look3::Water {
                    // Only the surface, where open air meets it.
                    if matches!(above, None | Some(Look3::Open)) {
                        let shade = WATER.mix(Rgb(0.15, 0.33, 0.53), noise(fx / 4.0, fz / 4.0, 3));
                        b.flat(fx, fz, hi - WATER_DIP, Vec3::Y, shade);
                    }
                    continue;
                }

                let tone = 0.95 + hash(xi, yi + 31 * zi, 31) * 0.08;
                let (top, face) = match look {
                    Look3::Rock => (ground_color(m, p, look), ROCK_FACE.shade(tone)),
                    Look3::Wall => (WALL_TOP, WALL_FACE),
                    _ => (ground_color(m, p, look), SOIL.shade(0.78 * tone)),
                };
                if !above.is_some_and(|a| a.opaque()) {
                    b.flat(fx, fz, hi, Vec3::Y, top);
                }
                if z > 0 && !look_off(xi, yi, zi - 1).is_some_and(|l| l.opaque()) {
                    // Ceiling of a cave or overhang.
                    b.flat(fx, fz, lo, Vec3::NEG_Y, face.shade(0.7));
                }
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let beside = look_off(xi + dx, yi + dz, zi);
                    if !beside.is_some_and(|l| l.opaque()) {
                        b.side(fx, fz, dx, dz, lo, hi, face);
                    }
                }
            }
        }
    }
    b.into_mesh()
}
