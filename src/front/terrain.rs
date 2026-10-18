//! Soil, rock, water and walls as chunked meshes with vertex colours.
//!
//! The map is split into 16×16-column chunks covering every level. Only faces
//! between a solid cell and an open one are emitted. Each frame a signature of
//! every chunk is compared with the last one, and only changed chunks are
//! rebuilt.

use super::input::UiState;
use super::look::*;
use super::sim::Sim;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use my_world::defs::{defs, BuildLook, Wallpaper};
use my_world::map::{Block, Map, Pos, Zone};

const CHUNK: usize = 16;
/// World units per level.
pub const LEVEL_H: f32 = 1.0;
/// Water surfaces sit a little below the top of their cell.
const WATER_DIP: f32 = 0.12;
/// Thickness of a built floor slab.
pub const FLOOR_T: f32 = 0.08;

/// What a cell looks like to the mesher.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look3 {
    Open,
    Soil,
    Rock,
    Ore,
    Copper,
    Wall,
    /// A wall with a hole through it (a window), running along x or y.
    Opening { along_x: bool },
    Water,
}

/// What's hidden: everything above `level` (layer view), and in columns
/// over roofed rooms everything above `columns` (hidden roofs, `u8::MAX` = none).
/// Also each room's themed kind (by room id: 0 = none, else index + 1 into
/// `defs().rooms`), which colours its floor.
#[derive(Clone, Copy)]
struct Cut<'a> {
    level: usize,
    columns: &'a [u8],
    kinds: &'a [u8],
}

impl Cut<'_> {
    const NONE: Cut<'static> = Cut { level: usize::MAX, columns: &[], kinds: &[] };

    /// The themed kind of the room cell `i` is in, if any.
    fn kind(&self, m: &Map, i: usize) -> Option<usize> {
        let r = *m.room.get(i)? as usize;
        self.kinds.get(r).filter(|&&k| k > 0).map(|&k| k as usize - 1)
    }

    /// Highest level shown in column (x, y).
    fn top(&self, m: &Map, x: usize, y: usize) -> usize {
        match self.columns.get(y * m.w + x) {
            Some(&c) if c != u8::MAX => self.level.min(c as usize),
            _ => self.level,
        }
    }

    fn hides(&self, m: &Map, p: Pos) -> bool {
        p.2 > self.top(m, p.0, p.1)
    }
}

/// How a cell looks with everything `cut` hides left out.
fn look_at(m: &Map, p: Pos, cut: Cut) -> Look3 {
    if cut.hides(m, p) {
        return Look3::Open;
    }
    let i = m.idx(p);
    match (m.block[i], m.feature[i]) {
        (Block::Soil, _) => Look3::Soil,
        (Block::Rock, _) => Look3::Rock,
        (Block::Ore, _) => Look3::Ore,
        (Block::Copper, _) => Look3::Copper,
        (Block::Water, _) => Look3::Water,
        (Block::Air, f) if f.is_block() => Look3::Wall,
        (Block::Air, f) if f.structure().is_some_and(|d| matches!(d.look, BuildLook::Opening { .. })) => {
            Look3::Opening { along_x: m.runs_along_x(p) }
        }
        (Block::Air, _) => Look3::Open,
    }
}

impl Look3 {
    /// Hides faces above and below it.
    fn opaque(self) -> bool {
        matches!(self, Look3::Soil | Look3::Rock | Look3::Ore | Look3::Copper | Look3::Wall | Look3::Opening { .. })
    }

    /// Hides the face of a neighbour that meets it going (dx, dz): an
    /// opening's ends are solid, its front and back have the hole.
    fn covers(self, dx: i32, dz: i32) -> bool {
        match self {
            Look3::Opening { along_x } => (dx != 0) == along_x && (dz != 0) != along_x,
            l => l.opaque(),
        }
    }

    /// Small number for chunk signatures.
    fn code(self) -> u8 {
        match self {
            Look3::Open => 0,
            Look3::Soil => 1,
            Look3::Rock => 2,
            Look3::Ore => 3,
            Look3::Copper => 8,
            Look3::Wall => 4,
            Look3::Opening { along_x } => 5 + along_x as u8,
            Look3::Water => 7,
        }
    }
}

#[derive(Resource)]
struct Chunks {
    generation: u64,
    cols: usize,
    rows: usize,
    sigs: Vec<u64>,
    entities: Vec<Option<Entity>>,
    /// Water surfaces, separate so they can be see-through and glossy.
    water: Vec<Option<Entity>>,
    material: Handle<StandardMaterial>,
    water_material: Handle<StandardMaterial>,
}

pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, update_chunks);
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.95, ..default() });
    let water_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.08,
        reflectance: 0.6,
        ..default()
    });
    commands.insert_resource(Chunks {
        generation: u64::MAX,
        cols: 0,
        rows: 0,
        sigs: Vec::new(),
        entities: Vec::new(),
        water: Vec::new(),
        material,
        water_material,
    });
}

fn update_chunks(
    mut commands: Commands,
    sim: Res<Sim>,
    ui: Res<UiState>,
    chunks: ResMut<Chunks>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let m = &sim.world.map;
    let kinds: Vec<u8> = sim.world.room_stats.iter().map(|r| r.kind.map_or(0, |k| k as u8 + 1)).collect();
    let cut = Cut { level: ui.top_level(m), columns: &ui.roof_cut, kinds: &kinds };
    let chunks = chunks.into_inner();
    let (cols, rows) = (m.w.div_ceil(CHUNK), m.h.div_ceil(CHUNK));
    if chunks.generation != sim.generation || chunks.cols != cols || chunks.rows != rows {
        for e in chunks.entities.drain(..).chain(chunks.water.drain(..)).flatten() {
            commands.entity(e).despawn();
        }
        chunks.entities = vec![None; cols * rows];
        chunks.water = vec![None; cols * rows];
        chunks.sigs = vec![u64::MAX; cols * rows];
        chunks.generation = sim.generation;
        chunks.cols = cols;
        chunks.rows = rows;
    }
    for cy in 0..rows {
        for cx in 0..cols {
            let ci = cy * cols + cx;
            let sig = signature(m, cx, cy, cut);
            if sig == chunks.sigs[ci] {
                continue;
            }
            chunks.sigs[ci] = sig;
            let (solid, water) = build_chunk(m, cx, cy, cut);
            let solid = meshes.add(solid);
            match chunks.entities[ci] {
                Some(e) => {
                    commands.entity(e).insert(Mesh3d(solid));
                }
                None => {
                    let e = commands
                        .spawn((Mesh3d(solid), MeshMaterial3d(chunks.material.clone()), Transform::default()))
                        .id();
                    chunks.entities[ci] = Some(e);
                }
            }
            match (water, chunks.water[ci]) {
                (Some(mesh), Some(e)) => {
                    commands.entity(e).insert(Mesh3d(meshes.add(mesh)));
                }
                (Some(mesh), None) => {
                    let bundle = (Mesh3d(meshes.add(mesh)), MeshMaterial3d(chunks.water_material.clone()), Transform::default());
                    chunks.water[ci] = Some(commands.spawn(bundle).id());
                }
                (None, Some(e)) => {
                    commands.entity(e).despawn();
                    chunks.water[ci] = None;
                }
                (None, None) => {}
            }
        }
    }
}

/// Zones as small numbers for chunk signatures (they decide ground colour).
fn zone_code(z: Zone) -> u8 {
    match z {
        Zone::None => 0,
        Zone::Stockpile(_) => 1,
        Zone::Grow(_) => 2,
        Zone::Safe => 3,
    }
}

fn blood_level(m: &Map, i: usize) -> u8 {
    (m.blood.get(i).copied().unwrap_or(0.0) * 3.0).ceil() as u8
}

/// Everything that affects a chunk's mesh, including a one-column border
/// (neighbouring cells decide which faces are visible).
fn signature(m: &Map, cx: usize, cy: usize, cut: Cut) -> u64 {
    // FNV-1a over small codes: cheap enough to run on every chunk every frame.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u8| h = (h ^ v as u64).wrapping_mul(0x0000_0100_0000_01b3);
    mix(cut.level as u8);
    let x0 = (cx * CHUNK).saturating_sub(1);
    let y0 = (cy * CHUNK).saturating_sub(1);
    let x1 = ((cx + 1) * CHUNK + 1).min(m.w);
    let y1 = ((cy + 1) * CHUNK + 1).min(m.h);
    for y in y0..y1 {
        for x in x0..x1 {
            mix(cut.top(m, x, y) as u8);
        }
    }
    for z in 0..m.d {
        for y in y0..y1 {
            for x in x0..x1 {
                let p = (x, y, z);
                let look = look_at(m, p, cut);
                mix(look.code());
                if look == Look3::Open && !cut.hides(m, p) {
                    // Open cells colour the ground below them, or carry a floor.
                    let i = m.idx(p);
                    mix(m.floor[i] as u8);
                    mix(zone_code(m.zone[i]));
                    mix(m.room.get(i).is_some_and(|&r| r != 0) as u8);
                    mix(cut.kind(m, i).map_or(0, |k| k as u8 + 1));
                    mix(blood_level(m, i));
                }
            }
        }
    }
    h
}

struct MeshBuf {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
    /// Opacity given to every vertex.
    alpha: f32,
}

impl MeshBuf {
    fn new(alpha: f32) -> Self {
        MeshBuf { pos: Vec::new(), nrm: Vec::new(), col: Vec::new(), idx: Vec::new(), alpha }
    }

    /// A flat quad; corner order is fixed up so it faces along `normal`.
    fn quad(&mut self, mut c: [Vec3; 4], normal: Vec3, color: Rgb) {
        if (c[1] - c[0]).cross(c[2] - c[0]).dot(normal) < 0.0 {
            c.reverse();
        }
        let base = self.pos.len() as u32;
        let mut color = color.vertex();
        color[3] = self.alpha;
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

    /// A wall's face on the cell edge in direction (dx, dz) papered with
    /// `paper`: 8 squares across and 8 up, the bottom row a skirting board.
    /// The pattern runs on along the wall from cell to cell.
    #[allow(clippy::too_many_arguments)]
    fn papered(&mut self, x: f32, z: f32, dx: i32, dz: i32, lo: f32, hi: f32, paper: &Wallpaper) {
        const N: i64 = 8;
        let (ax, az, bx, bz) = match (dx, dz) {
            (1, 0) => (x + 1.0, z, x + 1.0, z + 1.0),
            (-1, 0) => (x, z, x, z + 1.0),
            (0, 1) => (x, z + 1.0, x + 1.0, z + 1.0),
            _ => (x, z, x + 1.0, z),
        };
        // Along the wall: z for faces looking along x, else x.
        let along = if dx != 0 { z } else { x } as i64;
        let (base, accent) = (Rgb(paper.base.0, paper.base.1, paper.base.2), Rgb(paper.accent.0, paper.accent.1, paper.accent.2));
        let normal = Vec3::new(dx as f32, 0.0, dz as f32);
        for k in 0..N {
            let (t0, t1) = (k as f32 / N as f32, (k + 1) as f32 / N as f32);
            let (x0, z0) = (ax + (bx - ax) * t0, az + (bz - az) * t0);
            let (x1, z1) = (ax + (bx - ax) * t1, az + (bz - az) * t1);
            for j in 0..N {
                let (y0, y1) = (lo + (hi - lo) * j as f32 / N as f32, lo + (hi - lo) * (j + 1) as f32 / N as f32);
                let u = along * N + k;
                let color = if j == 0 {
                    accent.shade(0.7)
                } else if paper.pattern.accent(u, j) {
                    accent
                } else {
                    base.shade(paper.pattern.tone(u, j))
                };
                let c = [Vec3::new(x0, y0, z0), Vec3::new(x1, y0, z1), Vec3::new(x1, y1, z1), Vec3::new(x0, y1, z0)];
                self.quad(c, normal, color);
            }
        }
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

/// Floor colour of a themed room: its tint, in tiles if it's laid out so.
fn themed_floor(kind: usize, x: usize, y: usize, z: usize) -> Rgb {
    let room = &defs().rooms[kind];
    let (r, g, b) = room.floor;
    let jitter = hash(x as i32, y as i32 + 57 * z as i32, 13);
    let tile = if room.checker && (x + y) % 2 == 1 { 0.86 } else { 1.0 };
    Rgb(r, g, b).shade((0.94 + jitter * 0.08) * tile)
}

/// Colour of the top of solid ground, from what's on it (zones, floors, blood).
fn ground_color(m: &Map, solid: Pos, look: Look3, cut: Cut) -> Rgb {
    let (x, y, z) = solid;
    let (xi, yi) = (x as i32, y as i32);
    let above = m.above(solid).map(|a| m.idx(a));
    let zone = above.map_or(Zone::None, |a| m.zone[a]);
    let indoors = above.is_some_and(|a| m.room.get(a).is_some_and(|&r| r != 0));
    let jitter = hash(xi, yi + 97 * z as i32, 7);
    let underwater = above.is_some_and(|a| m.block[a] == Block::Water);
    let mut c = if underwater {
        Rgb(0.62, 0.56, 0.4).shade(0.9 + jitter * 0.12)
    } else if zone.crop().is_some() {
        SOIL.shade(0.92 + jitter * 0.12)
    } else if let Some(k) = above.and_then(|a| cut.kind(m, a)) {
        themed_floor(k, x, y, z + 1)
    } else if indoors {
        FLOOR.shade(0.94 + jitter * 0.1)
    } else if matches!(look, Look3::Rock | Look3::Ore | Look3::Copper) {
        ROCK_TOP.shade(0.94 + jitter * 0.1)
    } else {
        let n = noise(x as f32 / 6.0, y as f32 / 6.0, 1) * 0.75 + hash(xi, yi, 2) * 0.25;
        GRASS_A.mix(GRASS_B, n)
    };
    if let Some(a) = above {
        if zone.stockpile().is_some() {
            c = c.mix(STOCKPILE, 0.22);
        }
        if zone == Zone::Safe {
            c = c.mix(SAFE, 0.25);
        }
        let blood = blood_level(m, a);
        if blood > 0 {
            c = c.mix(BLOOD, 0.22 * blood as f32);
        }
    }
    c
}

/// Built floors: planks (or a themed room's floor) with whatever zone or blood is on them.
fn floor_color(m: &Map, p: Pos, cut: Cut) -> Rgb {
    let i = m.idx(p);
    let mut c = match cut.kind(m, i) {
        Some(k) => themed_floor(k, p.0, p.1, p.2),
        None => FLOOR.shade(0.9 + hash(p.0 as i32, p.1 as i32 + 57 * p.2 as i32, 9) * 0.1),
    };
    match m.zone[i] {
        Zone::Stockpile(_) => c = c.mix(STOCKPILE, 0.22),
        Zone::Grow(_) => c = c.mix(SOIL, 0.5),
        Zone::Safe => c = c.mix(SAFE, 0.25),
        Zone::None => {}
    }
    let blood = blood_level(m, i);
    if blood > 0 {
        c = c.mix(BLOOD, 0.22 * blood as f32);
    }
    c
}

/// The chunk's opaque mesh, and its water surface (if there is any water).
fn build_chunk(m: &Map, cx: usize, cy: usize, cut: Cut) -> (Mesh, Option<Mesh>) {
    let mut b = MeshBuf::new(1.0);
    let mut water = MeshBuf::new(0.72);
    let look_off = |x: i32, y: i32, z: i32| m.at(x, y, z).map(|p| look_at(m, p, cut));
    // A built floor that isn't hidden.
    let floor_shown = |q: Pos| m.floor[m.idx(q)] && !cut.hides(m, q);
    for z in 0..=cut.level.min(m.d - 1) {
        for y in cy * CHUNK..((cy + 1) * CHUNK).min(m.h) {
            for x in cx * CHUNK..((cx + 1) * CHUNK).min(m.w) {
                let p = (x, y, z);
                let look = look_at(m, p, cut);
                let (fx, fz) = (x as f32, y as f32);
                let (xi, yi, zi) = (x as i32, y as i32, z as i32);
                let (lo, hi) = (z as f32 * LEVEL_H, (z + 1) as f32 * LEVEL_H);
                if look == Look3::Open {
                    if floor_shown(p) {
                        let top = lo + FLOOR_T;
                        b.flat(fx, fz, top, Vec3::Y, floor_color(m, p, cut));
                        b.flat(fx, fz, lo, Vec3::NEG_Y, FLOOR.shade(0.6));
                        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let edge = m.at(xi + dx, yi + dz, zi).is_none_or(|q| !floor_shown(q));
                            if edge && !look_off(xi + dx, yi + dz, zi).is_some_and(|l| l.covers(-dx, -dz)) {
                                b.side(fx, fz, dx, dz, lo, top, FLOOR.shade(0.7));
                            }
                        }
                    }
                    continue;
                }
                let above = look_off(xi, yi, zi + 1);
                // Cut open by the layer view: show the cross-section darker.
                let sliced = z == cut.top(m, x, y) && m.above(p).is_some_and(|a| look_at(m, a, Cut::NONE).opaque());

                if look == Look3::Water {
                    // Only the surface, where open air meets it.
                    if matches!(above, None | Some(Look3::Open)) {
                        let shade = WATER.mix(Rgb(0.15, 0.33, 0.53), noise(fx / 4.0, fz / 4.0, 3));
                        water.flat(fx, fz, hi - WATER_DIP, Vec3::Y, shade);
                    }
                    continue;
                }

                if let Look3::Opening { along_x } = look {
                    let BuildLook::Opening { width, bottom, top, .. } = m.feature[m.idx(p)].structure().unwrap().look else {
                        continue;
                    };
                    let hole = (width, lo + bottom * LEVEL_H, lo + top * LEVEL_H);
                    let face_open = |dx: i32, dz: i32| !look_off(xi + dx, yi + dz, zi).is_some_and(|l| l.covers(-dx, -dz));
                    let top_open = !above.is_some_and(|a| a.opaque());
                    let below_open = z > 0 && !look_off(xi, yi, zi - 1).is_some_and(|l| l.opaque());
                    let top_color = if sliced { WALL_TOP.shade(0.55) } else { WALL_TOP };
                    opening(&mut b, (fx, fz, lo, hi), along_x, hole, top_color, &face_open, top_open, below_open);
                    continue;
                }
                let tone = 0.95 + hash(xi, yi + 31 * zi, 31) * 0.08;
                let (top, face) = match look {
                    Look3::Rock => (ground_color(m, p, look, cut), ROCK_FACE.shade(tone)),
                    // Rust-coloured streaks in the rock.
                    Look3::Ore => {
                        let fleck = hash(xi * 3 + 1, yi * 5 + zi, 41);
                        (ground_color(m, p, look, cut).mix(ORE, 0.35), ROCK_FACE.mix(ORE, 0.45 + 0.3 * fleck).shade(tone))
                    }
                    // Green-blue copper streaks.
                    Look3::Copper => {
                        let fleck = hash(xi * 5 + 2, yi * 3 + zi, 43);
                        (ground_color(m, p, look, cut).mix(COPPER, 0.35), ROCK_FACE.mix(COPPER, 0.45 + 0.3 * fleck).shade(tone))
                    }
                    Look3::Wall => (WALL_TOP, WALL_FACE),
                    _ => (ground_color(m, p, look, cut), SOIL.shade(0.78 * tone)),
                };
                if !above.is_some_and(|a| a.opaque()) {
                    b.flat(fx, fz, hi, Vec3::Y, if sliced { top.shade(0.55) } else { top });
                }
                if z > 0 && !look_off(xi, yi, zi - 1).is_some_and(|l| l.opaque()) {
                    // Ceiling of a cave or overhang.
                    b.flat(fx, fz, lo, Vec3::NEG_Y, face.shade(0.7));
                }
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let beside = look_off(xi + dx, yi + dz, zi);
                    if beside.is_some_and(|l| l.covers(-dx, -dz)) {
                        continue;
                    }
                    // A wall facing into a themed room is papered on that side.
                    let paper = (look == Look3::Wall)
                        .then(|| m.at(xi + dx, yi + dz, zi))
                        .flatten()
                        .and_then(|q| cut.kind(m, m.idx(q)))
                        .and_then(|k| defs().rooms[k].wallpaper.as_ref());
                    match paper {
                        Some(paper) => b.papered(fx, fz, dx, dz, lo, hi, paper),
                        None => b.side(fx, fz, dx, dz, lo, hi, face),
                    }
                }
            }
        }
    }
    let water = (!water.idx.is_empty()).then(|| water.into_mesh());
    (b.into_mesh(), water)
}

/// A wall cell with a hole through it, like the wall blocks beside it.
/// `cell` is (x, y, bottom, top) in world units; `hole` is (width as a share
/// of the cell, bottom, top). Coordinates below are local: `u` runs along the
/// wall, `v` through it.
#[allow(clippy::too_many_arguments)]
fn opening(
    b: &mut MeshBuf,
    (x, z, lo, hi): (f32, f32, f32, f32),
    along_x: bool,
    (width, yb, yt): (f32, f32, f32),
    top_color: Rgb,
    face_open: &dyn Fn(i32, i32) -> bool,
    top_open: bool,
    below_open: bool,
) {
    let world = |u: f32, y: f32, v: f32| if along_x { Vec3::new(x + u, y, z + v) } else { Vec3::new(x + v, y, z + u) };
    let dir = |du: i32, dv: i32| if along_x { (du, dv) } else { (dv, du) };
    let normal = |du: i32, dy: i32, dv: i32| {
        let (dx, dz) = dir(du, dv);
        Vec3::new(dx as f32, dy as f32, dz as f32)
    };
    let (h0, h1) = (0.5 - width / 2.0, 0.5 + width / 2.0);
    let reveal = WALL_FACE.shade(0.85);
    // Faces across `v` (front and back), with the hole cut out.
    let across = |b: &mut MeshBuf, v: f32, dv: i32, u0: f32, u1: f32, y0: f32, y1: f32| {
        let c = [world(u0, y0, v), world(u1, y0, v), world(u1, y1, v), world(u0, y1, v)];
        b.quad(c, normal(0, 0, dv), WALL_FACE);
    };
    for (v, dv) in [(0.0, -1), (1.0, 1)] {
        let (dx, dz) = dir(0, dv);
        if face_open(dx, dz) {
            across(b, v, dv, 0.0, 1.0, lo, yb);
            across(b, v, dv, 0.0, 1.0, yt, hi);
            across(b, v, dv, 0.0, h0, yb, yt);
            across(b, v, dv, h1, 1.0, yb, yt);
        }
    }
    // The ends, where it meets the walls beside it.
    for (u, du) in [(0.0, -1), (1.0, 1)] {
        let (dx, dz) = dir(du, 0);
        if face_open(dx, dz) {
            let c = [world(u, lo, 0.0), world(u, lo, 1.0), world(u, hi, 1.0), world(u, hi, 0.0)];
            b.quad(c, normal(du, 0, 0), WALL_FACE);
        }
    }
    // Inside the hole: sill, lintel and the two sides.
    b.quad([world(h0, yb, 0.0), world(h1, yb, 0.0), world(h1, yb, 1.0), world(h0, yb, 1.0)], Vec3::Y, reveal);
    b.quad([world(h0, yt, 0.0), world(h1, yt, 0.0), world(h1, yt, 1.0), world(h0, yt, 1.0)], Vec3::NEG_Y, reveal.shade(0.8));
    for (u, du) in [(h0, 1), (h1, -1)] {
        let c = [world(u, yb, 0.0), world(u, yb, 1.0), world(u, yt, 1.0), world(u, yt, 0.0)];
        b.quad(c, normal(du, 0, 0), reveal);
    }
    if top_open {
        b.flat(x, z, hi, Vec3::Y, top_color);
    }
    if below_open {
        b.flat(x, z, lo, Vec3::NEG_Y, WALL_FACE.shade(0.7));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use my_world::map::{Feature, FLAT_Z as Z};

    #[test]
    fn themed_rooms_are_papered() {
        // A roofed 3×3 room (walls around 2..=6).
        let mut m = Map::flat(10, 10);
        for y in 2..=6 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 2 || y == 6 {
                    let i = m.idx((x, y, Z));
                    m.set_feature(i, Feature::built(if (x, y) == (2, 4) { "door" } else { "wall" }));
                }
                let i = m.idx((x, y, Z + 1));
                m.set_roof(i, true);
            }
        }
        m.recompute_rooms();
        let colors = |kinds: &[u8]| {
            let cut = Cut { level: Z, columns: &[], kinds };
            let (mesh, _) = build_chunk(&m, 0, 0, cut);
            let Some(bevy::mesh::VertexAttributeValues::Float32x4(c)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR) else {
                panic!("vertex colours")
            };
            c.clone()
        };
        let room = m.room[m.idx((4, 4, Z))] as usize;
        let k = defs().rooms.iter().position(|r| r.id == "workshop").unwrap();
        let mut kinds = vec![0u8; room + 1];
        let plain = colors(&kinds);
        kinds[room] = k as u8 + 1;
        let papered = colors(&kinds);
        assert!(papered.len() > plain.len(), "the inside faces are made of many squares");
        let paper = defs().rooms[k].wallpaper.as_ref().unwrap();
        let accent = Rgb(paper.accent.0, paper.accent.1, paper.accent.2).vertex();
        assert!(papered.iter().any(|c| c == &accent), "in its colours");
        assert!(!plain.iter().any(|c| c == &accent));
    }

    #[test]
    fn every_pattern_shows_both_colours() {
        use my_world::defs::Pattern::*;
        for p in [Stripes, Checks, Dots, Tiles, Bricks, Planks, Diamonds] {
            let squares: Vec<bool> = (0..16).flat_map(|u| (1..8).map(move |v| p.accent(u, v))).collect();
            assert!(squares.contains(&true) && squares.contains(&false), "{:?}", p);
        }
        assert!(defs().rooms.iter().all(|r| r.wallpaper.is_some()), "every theme has one");
    }
}
