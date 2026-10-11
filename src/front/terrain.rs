//! Ground, water, rock and walls as chunked meshes with vertex colours.
//!
//! The map is split into 16×16-tile chunks. Each frame a cheap signature of
//! every chunk is compared with the last one, and only changed chunks are
//! rebuilt.

use super::look::*;
use super::sim::Sim;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use my_world::map::{Feature, Map, Terrain, Zone};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const CHUNK: usize = 16;
pub const ROCK_H: f32 = 1.6;
pub const WALL_H: f32 = 1.3;
pub const WATER_Y: f32 = -0.15;

/// Height of solid blocks; None for open ground.
pub fn block_height(f: Feature) -> Option<f32> {
    match f {
        Feature::Rock => Some(ROCK_H),
        Feature::Wall => Some(WALL_H),
        _ => None,
    }
}

/// Height of the walkable/visible surface of a tile.
pub fn surface_height(m: &Map, x: usize, y: usize) -> f32 {
    let i = m.idx((x, y));
    if let Some(h) = block_height(m.feature[i]) {
        return h;
    }
    if m.terrain[i] == Terrain::Water {
        WATER_Y
    } else {
        0.0
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

/// Everything that affects a chunk's mesh, including a one-tile border
/// (neighbouring blocks decide which side faces are visible).
fn signature(m: &Map, cx: usize, cy: usize) -> u64 {
    let mut h = DefaultHasher::new();
    let x0 = (cx * CHUNK).saturating_sub(1);
    let y0 = (cy * CHUNK).saturating_sub(1);
    let x1 = ((cx + 1) * CHUNK + 1).min(m.w);
    let y1 = ((cy + 1) * CHUNK + 1).min(m.h);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = m.idx((x, y));
            let block = match m.feature[i] {
                Feature::Rock => 1u8,
                Feature::Wall => 2,
                _ => 0,
            };
            (m.terrain[i] as u8, block, m.zone[i] as u8, m.room.get(i).map_or(false, |&r| r != 0), blood_level(m, i))
                .hash(&mut h);
        }
    }
    h.finish()
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

    fn top(&mut self, x: f32, z: f32, y: f32, color: Rgb) {
        let c = [Vec3::new(x, y, z), Vec3::new(x, y, z + 1.0), Vec3::new(x + 1.0, y, z + 1.0), Vec3::new(x + 1.0, y, z)];
        self.quad(c, Vec3::Y, color);
    }

    /// Vertical face on the tile edge in direction (dx, dz), spanning lo..hi.
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

fn ground_color(m: &Map, x: usize, y: usize) -> Rgb {
    let i = m.idx((x, y));
    let (xi, yi) = (x as i32, y as i32);
    let indoors = m.room.get(i).map_or(false, |&r| r != 0);
    let mut c = if m.terrain[i] == Terrain::Water {
        WATER.mix(Rgb(0.15, 0.33, 0.53), noise(x as f32 / 4.0, y as f32 / 4.0, 3))
    } else if m.zone[i] == Zone::Grow {
        SOIL.shade(0.92 + hash(xi, yi, 6) * 0.12)
    } else if indoors {
        FLOOR.shade(0.94 + hash(xi, yi, 7) * 0.1)
    } else {
        let n = noise(x as f32 / 6.0, y as f32 / 6.0, 1) * 0.75 + hash(xi, yi, 2) * 0.25;
        GRASS_A.mix(GRASS_B, n)
    };
    if m.zone[i] == Zone::Stockpile {
        c = c.mix(STOCKPILE, 0.22);
    }
    let blood = blood_level(m, i);
    if blood > 0 {
        c = c.mix(BLOOD, 0.22 * blood as f32);
    }
    c
}

fn build_chunk(m: &Map, cx: usize, cy: usize) -> Mesh {
    let mut b = MeshBuf::default();
    let height_at = |x: i32, y: i32| -> Option<f32> {
        (x >= 0 && y >= 0 && (x as usize) < m.w && (y as usize) < m.h).then(|| surface_height(m, x as usize, y as usize))
    };
    for y in cy * CHUNK..((cy + 1) * CHUNK).min(m.h) {
        for x in cx * CHUNK..((cx + 1) * CHUNK).min(m.w) {
            let i = m.idx((x, y));
            let (fx, fz) = (x as f32, y as f32);
            let (xi, yi) = (x as i32, y as i32);
            let h = surface_height(m, x, y);
            let (top, face) = match m.feature[i] {
                Feature::Rock => {
                    let v = 0.94 + hash(xi, yi, 31) * 0.1;
                    (ROCK_TOP.shade(v), ROCK_FACE.shade(v))
                }
                Feature::Wall => (WALL_TOP, WALL_FACE),
                // Open ground: earth shows on the sides of banks.
                _ => (ground_color(m, x, y), SOIL.shade(0.8)),
            };
            b.top(fx, fz, h, top);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                // Map edges get a side face down to below the water line.
                let below = height_at(xi + dx, yi + dz).unwrap_or(WATER_Y - 0.3);
                if below < h {
                    b.side(fx, fz, dx, dz, below, h, face);
                }
            }
        }
    }
    b.into_mesh()
}
