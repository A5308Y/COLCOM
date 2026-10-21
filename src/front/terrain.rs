//! Soil, rock, water and walls as chunked meshes with vertex colours.
//!
//! Walls (and windows) are drawn thin through the middle of their cell
//! (`decor::WALL_T`), joined to what's beside them, with the floor showing
//! around them in the colour of the room on each side.
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
use colcom::defs::{defs, BuildLook, Wallpaper};
use colcom::decor::{WALL_A, WALL_B};
use colcom::seasons::Season;
use colcom::map::{Block, Map, Pos, Zone, DIRS};

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
    /// The season, which colours the grass (index into `Season::ALL`).
    season: u8,
}

impl Cut<'_> {
    const NONE: Cut<'static> = Cut { level: usize::MAX, columns: &[], kinds: &[], season: 0 };

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

    /// Hides the face of a neighbour that meets it going (dx, dz). Walls and
    /// windows are thin, so they hide nothing beside them.
    fn covers(self, _dx: i32, _dz: i32) -> bool {
        match self {
            Look3::Wall | Look3::Opening { .. } => false,
            l => l.opaque(),
        }
    }

    /// Drawn thin through its cell (walls, windows).
    fn thin(self) -> bool {
        matches!(self, Look3::Wall | Look3::Opening { .. })
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
    let kinds: Vec<u8> = sim.world.derived.room_stats.iter().map(|r| r.kind.map_or(0, |k| k.index() as u8 + 1)).collect();
    let season = Season::ALL.iter().position(|&s| s == sim.world.season()).unwrap_or(0) as u8;
    let cut = Cut { level: ui.top_level(m), columns: &ui.roof_cut, kinds: &kinds, season };
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
    mix(cut.season);
    let x0 = (cx * CHUNK).saturating_sub(1);
    let y0 = (cy * CHUNK).saturating_sub(1);
    let x1 = ((cx + 1) * CHUNK + 1).min(m.w);
    let y1 = ((cy + 1) * CHUNK + 1).min(m.h);
    for y in y0..y1 {
        for x in x0..x1 {
            mix(cut.top(m, x, y) as u8);
            mix(m.quarry_at(x, y).is_some() as u8);
        }
    }
    for z in 0..m.d {
        for y in y0..y1 {
            for x in x0..x1 {
                let p = (x, y, z);
                let look = look_at(m, p, cut);
                mix(look.code());
                if look.thin() {
                    mix(m.floor[m.idx(p)] as u8);
                }
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

    /// A vertical wall face looking (dx, dz), from `a` to `b` (x, z) along
    /// the wall, spanning lo..hi: plain `face`, or papered with `paper` in
    /// squares of an eighth of a cell, the bottom row a skirting board. The
    /// pattern runs on along the wall from piece to piece and cell to cell.
    #[allow(clippy::too_many_arguments)]
    fn wall_face(&mut self, a: (f32, f32), b: (f32, f32), n: (i32, i32), lo: f32, hi: f32, face: Rgb, paper: Option<&Wallpaper>) {
        self.wall_part(a, b, n, (lo, hi), (lo, hi), face, paper);
    }

    /// Part of a wall face (`wall_face`): only heights `y0..y1` of a wall
    /// running `lo..hi`, its paper lined up with the rest of the wall (around a window's hole).
    #[allow(clippy::too_many_arguments)]
    fn wall_part(
        &mut self,
        a: (f32, f32),
        b: (f32, f32),
        (dx, dz): (i32, i32),
        (lo, hi): (f32, f32),
        (y_from, y_to): (f32, f32),
        face: Rgb,
        paper: Option<&Wallpaper>,
    ) {
        let normal = Vec3::new(dx as f32, 0.0, dz as f32);
        let corners = |u0: f32, u1: f32, y0: f32, y1: f32| {
            let at = |u: f32, y: f32| if dx != 0 { Vec3::new(a.0, y, u) } else { Vec3::new(u, y, a.1) };
            [at(u0, y0), at(u1, y0), at(u1, y1), at(u0, y1)]
        };
        // Along the wall: z for faces looking along x, else x.
        let (s, e) = if dx != 0 { (a.1.min(b.1), a.1.max(b.1)) } else { (a.0.min(b.0), a.0.max(b.0)) };
        if y_to <= y_from || e <= s {
            return;
        }
        let Some(paper) = paper else {
            self.quad(corners(s, e, y_from, y_to), normal, face);
            return;
        };
        const N: f32 = 8.0;
        let (base, accent) = (Rgb(paper.base.0, paper.base.1, paper.base.2), Rgb(paper.accent.0, paper.accent.1, paper.accent.2));
        let mut k = (s * N + 1e-3).floor() as i64;
        while (k as f32) < e * N - 1e-3 {
            let (u0, u1) = ((k as f32 / N).max(s), ((k + 1) as f32 / N).min(e));
            for j in 0..N as i64 {
                let (y0, y1) = (lo + (hi - lo) * j as f32 / N, lo + (hi - lo) * (j + 1) as f32 / N);
                let (y0, y1) = (y0.max(y_from), y1.min(y_to));
                if y1 <= y0 {
                    continue;
                }
                let color = if j == 0 {
                    accent.shade(0.7)
                } else if paper.pattern.accent(k, j) {
                    accent
                } else {
                    base.shade(paper.pattern.tone(k, j))
                };
                self.quad(corners(u0, u1, y0, y1), normal, color);
            }
            k += 1;
        }
    }

    /// A horizontal rectangle from (x0, z0) to (x1, z1) at height `y`.
    fn rect(&mut self, (x0, z0): (f32, f32), (x1, z1): (f32, f32), y: f32, normal: Vec3, color: Rgb) {
        let c = [Vec3::new(x0, y, z0), Vec3::new(x0, y, z1), Vec3::new(x1, y, z1), Vec3::new(x1, y, z0)];
        self.quad(c, normal, color);
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
        let grass = GRASS_A.mix(GRASS_B, n);
        // Golden in autumn, under snow in winter.
        match Season::ALL[cut.season as usize % 4] {
            Season::Autumn => grass.mix(GRASS_AUTUMN, 0.55 + 0.2 * n),
            Season::Winter => grass.mix(SNOW, 0.8 + 0.1 * n),
            _ => grass,
        }
    };
    if let Some(a) = above {
        if zone.stockpile().is_some() {
            c = c.mix(STOCKPILE, 0.22);
        }
        if zone == Zone::Safe {
            c = c.mix(SAFE, 0.25);
        }
        if m.quarry_at(x, y).is_some() {
            c = c.mix(QUARRY, 0.25);
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

                if look.thin() {
                    let top_open = !above.is_some_and(|a| a.opaque());
                    let top_color = top_open.then_some(if sliced { WALL_TOP.shade(0.55) } else { WALL_TOP });
                    under_wall(&mut b, m, p, cut, lo);
                    if above.is_some_and(|a| a.opaque() && !a.thin()) {
                        // Rock overhead (a wall in a cave): its underside is the ceiling here.
                        b.flat(fx, fz, hi, Vec3::NEG_Y, WALL_FACE.shade(0.7));
                    }
                    match look {
                        Look3::Opening { along_x } => {
                            let BuildLook::Opening { width, bottom, top, .. } = m.feature[m.idx(p)].structure().unwrap().look else {
                                continue;
                            };
                            let hole = (width, lo + bottom * LEVEL_H, lo + top * LEVEL_H);
                            let links = m.wall_links(p);
                            let end_open = |dx: i32, dz: i32| !DIRS.iter().position(|&d| d == (dx, dz)).is_some_and(|k| links[k]);
                            // Papered like the walls on each side, where that's a themed room.
                            let paper = |dv: i32| {
                                let (dx, dz) = if along_x { (0, dv) } else { (dv, 0) };
                                paper_at(m, m.at(xi + dx, yi + dz, zi), cut)
                            };
                            let papers = [paper(-1), paper(1)];
                            opening(&mut b, (fx, fz, lo, hi), along_x, hole, top_color, &end_open, papers);
                        }
                        _ => thin_wall(&mut b, m, p, cut, (lo, hi), top_color),
                    }
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
                    if !beside.is_some_and(|l| l.covers(-dx, -dz)) {
                        b.side(fx, fz, dx, dz, lo, hi, face);
                    }
                }
            }
        }
    }
    let water = (!water.idx.is_empty()).then(|| water.into_mesh());
    (b.into_mesh(), water)
}

/// The wallpaper of the themed room cell `q` is in, if any.
fn paper_at(m: &Map, q: Option<Pos>, cut: Cut) -> Option<&'static Wallpaper> {
    let k = cut.kind(m, m.idx(q?))?;
    defs().rooms[k].wallpaper.as_ref()
}

/// The cell a piece of the wall at `p` looks into going `n` (a `DIRS`
/// index): straight across, or, past a wall running off that way, the cell
/// diagonally in front on the `side` the piece is on.
fn facing_cell(m: &Map, p: Pos, links: &[bool; 4], n: usize, side: Option<usize>) -> Option<Pos> {
    let (nx, ny) = DIRS[n];
    let (x, y, z) = (p.0 as i32 + nx, p.1 as i32 + ny, p.2 as i32);
    match side.filter(|_| links[n]) {
        Some(s) => m.at(x + DIRS[s].0, y + DIRS[s].1, z),
        None => m.at(x, y, z),
    }
}

/// A wall through the middle of its cell: a square core with an arm out to
/// each side it joins (`Map::wall_links`), papered where it faces a themed room.
fn thin_wall(b: &mut MeshBuf, m: &Map, p: Pos, cut: Cut, (lo, hi): (f32, f32), top: Option<Rgb>) {
    let (x, z) = (p.0 as f32, p.1 as f32);
    let links = m.wall_links(p);
    let at = |u: f32, v: f32| (x + u, z + v);
    let (a, c) = (WALL_A, WALL_B);
    let face = |b: &mut MeshBuf, from: (f32, f32), to: (f32, f32), n: usize, side: Option<usize>| {
        let paper = paper_at(m, facing_cell(m, p, &links, n, side), cut);
        b.wall_face(from, to, DIRS[n], lo, hi, WALL_FACE, paper);
    };
    // The core: its own sides where nothing joins.
    for (n, from, to) in [(0, at(c, a), at(c, c)), (1, at(a, a), at(a, c)), (2, at(a, c), at(c, c)), (3, at(a, a), at(c, a))] {
        if !links[n] {
            face(b, from, to, n, None);
        }
    }
    if let Some(t) = top {
        b.rect(at(a, a), at(c, c), hi, Vec3::Y, t);
    }
    // The arms, each with a side looking either way.
    for d in (0..4).filter(|&d| links[d]) {
        let (s, e) = match d {
            0 | 2 => (c, 1.0),
            _ => (0.0, a),
        };
        let (lo_corner, hi_corner, sides) = if d < 2 {
            (at(s, a), at(e, c), [(3, at(s, a), at(e, a)), (2, at(s, c), at(e, c))])
        } else {
            (at(a, s), at(c, e), [(1, at(a, s), at(a, e)), (0, at(c, s), at(c, e))])
        };
        for (n, from, to) in sides {
            face(b, from, to, n, Some(d));
        }
        if let Some(t) = top {
            b.rect(lo_corner, hi_corner, hi, Vec3::Y, t);
        }
    }
}

/// What the floor looks like in cell `q` (a built floor, or the ground
/// under it), if there's one to see.
fn floor_around(m: &Map, q: Pos, cut: Cut) -> Option<Rgb> {
    if cut.hides(m, q) {
        return None;
    }
    if m.floor[m.idx(q)] {
        return Some(floor_color(m, q, cut));
    }
    let below = m.below(q)?;
    let look = look_at(m, below, cut);
    matches!(look, Look3::Soil | Look3::Rock | Look3::Ore | Look3::Copper).then(|| ground_color(m, below, look, cut))
}

/// The floor around a thin wall or window at `p` (bottom `lo`): each
/// quarter of the cell in the colour of the floor beside it on that side
/// (indoors on one side, grass on the other), on a slab if the wall stands
/// on a built floor. Only natural ground is shown all round: elsewhere (an
/// upper storey, on top of a lower wall) a quarter with no floor beside it
/// stays open, so nothing sticks out past the wall.
fn under_wall(b: &mut MeshBuf, m: &Map, p: Pos, cut: Cut, lo: f32) {
    let i = m.idx(p);
    let below = m.below(p).map(|q| look_at(m, q, cut));
    // (Ground, or a wall, window or door below.)
    let on_ground = below.is_some_and(|l| l.opaque());
    let natural = below.is_some_and(|l| l.opaque() && !l.thin());
    if !m.floor[i] && !on_ground {
        return;
    }
    let y = if m.floor[i] && !on_ground { lo + FLOOR_T } else { lo };
    let (x, z, zi) = (p.0 as f32, p.1 as f32, p.2 as i32);
    let links = m.wall_links(p);
    let cell = |d: (i32, i32)| m.at(p.0 as i32 + d.0, p.1 as i32 + d.1, zi);
    let fallback = m.below(p).filter(|_| natural).map(|q| ground_color(m, q, look_at(m, q, cut), cut));
    let no_floor = |d: (i32, i32)| {
        cell(d).is_none_or(|q| {
            let l = look_at(m, q, cut);
            !m.floor[m.idx(q)] && !l.opaque()
        })
    };
    for (xs, ys) in [(0, 2), (1, 2), (0, 3), (1, 3)] {
        let (ax, ay) = (DIRS[xs], DIRS[ys]);
        let mut candidates = Vec::new();
        if !links[ys] {
            candidates.push(cell(ay));
        }
        if !links[xs] {
            candidates.push(cell(ax));
        }
        candidates.push(cell((ax.0 + ay.0, ax.1 + ay.1)));
        let Some(color) = candidates.into_iter().flatten().find_map(|q| floor_around(m, q, cut)).or(fallback) else {
            continue;
        };
        let (u0, u1) = if ax.0 > 0 { (0.5, 1.0) } else { (0.0, 0.5) };
        let (v0, v1) = if ay.1 > 0 { (0.5, 1.0) } else { (0.0, 0.5) };
        let (c0, c1) = ((x + u0, z + v0), (x + u1, z + v1));
        b.rect(c0, c1, y, Vec3::Y, color);
        if y > lo {
            // This quarter of the slab: its underside, and its edges where no floor goes on.
            b.rect(c0, c1, lo, Vec3::NEG_Y, FLOOR.shade(0.6));
            let ex = if ax.0 > 0 { x + 1.0 } else { x };
            let ez = if ay.1 > 0 { z + 1.0 } else { z };
            if no_floor(ax) {
                b.wall_face((ex, c0.1), (ex, c1.1), ax, lo, y, FLOOR.shade(0.7), None);
            }
            if no_floor(ay) {
                b.wall_face((c0.0, ez), (c1.0, ez), ay, lo, y, FLOOR.shade(0.7), None);
            }
        }
    }
}

/// A window: a thin wall with a hole through it. `cell` is (x, y, bottom,
/// top) in world units; `hole` is (width as a share of the cell, bottom,
/// top). Coordinates below are local: `u` runs along the wall, `v` through
/// it (only its middle, as thick as the walls). The ends are drawn where
/// `end_open` says nothing joins.
fn opening(
    b: &mut MeshBuf,
    (x, z, lo, hi): (f32, f32, f32, f32),
    along_x: bool,
    (width, yb, yt): (f32, f32, f32),
    top_color: Option<Rgb>,
    end_open: &dyn Fn(i32, i32) -> bool,
    papers: [Option<&Wallpaper>; 2],
) {
    let (va, vb) = (WALL_A, WALL_B);
    let world = |u: f32, y: f32, v: f32| if along_x { Vec3::new(x + u, y, z + v) } else { Vec3::new(x + v, y, z + u) };
    let dir = |du: i32, dv: i32| if along_x { (du, dv) } else { (dv, du) };
    let normal = |du: i32, dy: i32, dv: i32| {
        let (dx, dz) = dir(du, dv);
        Vec3::new(dx as f32, dy as f32, dz as f32)
    };
    let (h0, h1) = (0.5 - width / 2.0, 0.5 + width / 2.0);
    let reveal = WALL_FACE.shade(0.85);
    // Faces across `v` (front and back), with the hole cut out, papered like the walls beside.
    let across = |b: &mut MeshBuf, v: f32, dv: i32, u0: f32, u1: f32, y0: f32, y1: f32, paper: Option<&Wallpaper>| {
        let (a, c) = (world(u0, 0.0, v), world(u1, 0.0, v));
        b.wall_part((a.x, a.z), (c.x, c.z), dir(0, dv), (lo, hi), (y0, y1), WALL_FACE, paper);
    };
    for (v, dv, paper) in [(va, -1, papers[0]), (vb, 1, papers[1])] {
        across(b, v, dv, 0.0, 1.0, lo, yb, paper);
        across(b, v, dv, 0.0, 1.0, yt, hi, paper);
        across(b, v, dv, 0.0, h0, yb, yt, paper);
        across(b, v, dv, h1, 1.0, yb, yt, paper);
    }
    // The ends, unless they meet the walls beside it.
    for (u, du) in [(0.0, -1), (1.0, 1)] {
        let (dx, dz) = dir(du, 0);
        if end_open(dx, dz) {
            let c = [world(u, lo, va), world(u, lo, vb), world(u, hi, vb), world(u, hi, va)];
            b.quad(c, normal(du, 0, 0), WALL_FACE);
        }
    }
    // Inside the hole: sill (not for doors, open to the floor), lintel and the two sides.
    if yb > lo + 1e-4 {
        b.quad([world(h0, yb, va), world(h1, yb, va), world(h1, yb, vb), world(h0, yb, vb)], Vec3::Y, reveal);
    }
    b.quad([world(h0, yt, va), world(h1, yt, va), world(h1, yt, vb), world(h0, yt, vb)], Vec3::NEG_Y, reveal.shade(0.8));
    for (u, du) in [(h0, 1), (h1, -1)] {
        let c = [world(u, yb, va), world(u, yb, vb), world(u, yt, vb), world(u, yt, va)];
        b.quad(c, normal(du, 0, 0), reveal);
    }
    if let Some(top) = top_color {
        b.quad([world(0.0, hi, va), world(1.0, hi, va), world(1.0, hi, vb), world(0.0, hi, vb)], Vec3::Y, top);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colcom::map::{Feature, FLAT_Z as Z};

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
            let cut = Cut { level: Z, columns: &[], kinds, season: 0 };
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
    fn windows_are_papered_like_their_walls() {
        // The roofed room of `themed_rooms_are_papered`, with a window in its north wall at (4, 2).
        let mut m = Map::flat(10, 10);
        for y in 2..=6 {
            for x in 2..=6 {
                if x == 2 || x == 6 || y == 2 || y == 6 {
                    let i = m.idx((x, y, Z));
                    let id = match (x, y) {
                        (2, 4) => "door",
                        (4, 2) => "window",
                        _ => "wall",
                    };
                    m.set_feature(i, Feature::built(id));
                }
                let i = m.idx((x, y, Z + 1));
                m.set_roof(i, true);
            }
        }
        m.recompute_rooms();
        let room = m.room[m.idx((4, 4, Z))] as usize;
        let k = defs().rooms.iter().position(|r| r.id == "workshop").unwrap();
        let mut kinds = vec![0u8; room + 1];
        kinds[room] = k as u8 + 1;
        let (mesh, _) = build_chunk(&m, 0, 0, Cut { level: Z, columns: &[], kinds: &kinds, season: 0 });
        let (Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)), Some(bevy::mesh::VertexAttributeValues::Float32x4(col))) =
            (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_COLOR))
        else {
            panic!("positions and colours")
        };
        let paper = defs().rooms[k].wallpaper.as_ref().unwrap();
        let accent = Rgb(paper.accent.0, paper.accent.1, paper.accent.2).vertex();
        let window_face = |z: f32| pos.iter().zip(col).filter(move |(p, _)| p[0] > 4.0 && p[0] < 5.0 && (p[2] - z).abs() < 1e-4);
        assert!(window_face(2.0 + WALL_B).any(|(_, c)| c == &accent), "papered on the room's side");
        assert!(!window_face(2.0 + WALL_A).any(|(_, c)| c == &accent), "plain outside");
    }

    #[test]
    fn doors_are_papered_like_their_walls_with_the_room_floor_beside_them() {
        // The roofed room again, door in its west wall at (2, 4).
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
        let room = m.room[m.idx((4, 4, Z))] as usize;
        let k = defs().rooms.iter().position(|r| r.id == "workshop").unwrap();
        let mut kinds = vec![0u8; room + 1];
        kinds[room] = k as u8 + 1;
        let cut = Cut { level: Z, columns: &[], kinds: &kinds, season: 0 };
        let (mesh, _) = build_chunk(&m, 0, 0, cut);
        let (Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)), Some(bevy::mesh::VertexAttributeValues::Float32x4(col))) =
            (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_COLOR))
        else {
            panic!("positions and colours")
        };
        let paper = defs().rooms[k].wallpaper.as_ref().unwrap();
        let accent = Rgb(paper.accent.0, paper.accent.1, paper.accent.2).vertex();
        // The door runs along y: its room side looks +x, at x = 2 + WALL_B.
        let face = |x: f32| pos.iter().zip(col).filter(move |(p, _)| p[2] > 4.0 && p[2] < 5.0 && (p[0] - x).abs() < 1e-4);
        assert!(face(2.0 + WALL_B).any(|(_, c)| c == &accent), "papered on the room's side");
        assert!(!face(2.0 + WALL_A).any(|(_, c)| c == &accent), "plain outside");
        // The floor in the doorway: on the room's side like the room's floor, outside like the grass.
        let under = |q: Pos| ground_color(&m, q, Look3::Soil, cut).vertex();
        let floor_at = |x: f32, z: f32| {
            pos.iter().zip(col).filter(move |(p, _)| (p[1] - Z as f32 * LEVEL_H).abs() < 1e-4 && (p[0] - x).abs() < 1e-4 && (p[2] - z).abs() < 1e-4).map(|(_, c)| *c)
        };
        // (Down the middle of the doorway, where its two halves meet.)
        assert!(floor_at(2.5, 4.5).any(|c| c == under((3, 4, Z - 1))), "the room's floor reaches into the doorway");
        assert!(floor_at(2.5, 4.5).any(|c| c == under((1, 4, Z - 1))), "and the grass from outside");
    }

    #[test]
    fn walls_are_thin_with_the_floor_showing_around_them() {
        // One straight wall along x at y = 4, standing on the ground.
        let mut m = Map::flat(10, 10);
        for x in 2..=6 {
            let i = m.idx((x, 4, Z));
            m.set_feature(i, Feature::built("wall"));
        }
        let (mesh, _) = build_chunk(&m, 0, 0, Cut::NONE);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions")
        };
        let (lo, hi) = (Z as f32 * LEVEL_H, (Z + 1) as f32 * LEVEL_H);
        // Everything standing up in the middle cell lies within the wall's thickness.
        let upright: Vec<&[f32; 3]> = pos.iter().filter(|p| p[1] > lo + 0.01 && (4.0..=5.0).contains(&p[0]) && (4.0..=5.0).contains(&p[2])).collect();
        assert!(!upright.is_empty());
        assert!(upright.iter().all(|p| (4.0 + WALL_A - 1e-4..=4.0 + WALL_B + 1e-4).contains(&p[2])), "thin");
        assert!(upright.iter().any(|p| (p[1] - hi).abs() < 1e-4), "with a top");
        // And the ground under it is drawn all across the cell.
        let ground = pos.iter().filter(|p| (p[1] - lo).abs() < 1e-4 && (4.0..=5.0).contains(&p[0]) && (p[2] - 4.0).abs() < 1e-4).count();
        assert!(ground > 0, "the floor shows at the cell's edge");
    }

    #[test]
    fn upstairs_the_floor_stops_at_the_wall() {
        // A wall along x at y = 4 with a floored room on its +y side (y 5..=6),
        // a storey up: once on a floor slab, once stacked on a wall below.
        for stacked in [false, true] {
            let mut m = Map::flat(10, 10);
            for x in 2..=6 {
                let up = m.idx((x, 4, Z + 1));
                m.set_feature(up, Feature::built("wall"));
                if stacked {
                    let below = m.idx((x, 4, Z));
                    m.set_feature(below, Feature::built("wall"));
                } else {
                    m.set_floor(up, true);
                }
                for y in 5..=6 {
                    let i = m.idx((x, y, Z + 1));
                    m.set_floor(i, true);
                }
            }
            let (mesh, _) = build_chunk(&m, 0, 0, Cut::NONE);
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
                panic!("positions")
            };
            let lo = (Z + 1) as f32 * LEVEL_H;
            // At the bottom of the upper storey, under the middle wall cell: nothing on the outside (-y) half.
            let outside = pos.iter().filter(|p| {
                (p[1] - lo).abs() < FLOOR_T + 1e-3 && p[0] > 4.0 && p[0] < 5.0 && p[2] < 4.0 + WALL_A - 1e-3
            });
            assert_eq!(outside.count(), 0, "nothing sticks out (stacked: {})", stacked);
            let inside = pos.iter().filter(|p| (p[1] - lo).abs() < FLOOR_T + 1e-3 && p[0] > 4.0 && p[0] < 5.0 && p[2] > 4.0 + WALL_B);
            assert!(inside.count() > 0, "the room's floor reaches the wall (stacked: {})", stacked);
        }
    }

    #[test]
    fn a_wall_over_a_door_has_the_floor_beside_it() {
        // A wall along x at y = 4 a storey up, over a door at (4, 4); the room's floor on its +y side.
        let mut m = Map::flat(10, 10);
        for x in 2..=6 {
            let below = m.idx((x, 4, Z));
            m.set_feature(below, Feature::built(if x == 4 { "door" } else { "wall" }));
            let up = m.idx((x, 4, Z + 1));
            m.set_feature(up, Feature::built("wall"));
            for y in 5..=6 {
                let i = m.idx((x, y, Z + 1));
                m.set_floor(i, true);
            }
        }
        let (mesh, _) = build_chunk(&m, 0, 0, Cut::NONE);
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else {
            panic!("positions")
        };
        let lo = (Z + 1) as f32 * LEVEL_H;
        // Over the door, on the room's side of the wall: floor, no hole.
        // (The floor's quarter of the wall's cell on that side starts halfway across it.)
        let quarter = |p: &&[f32; 3]| (p[1] - lo).abs() < 1e-3 && (p[0] - 4.5).abs() < 1e-3 && (p[2] - 4.5).abs() < 1e-3;
        assert!(pos.iter().filter(quarter).count() > 0, "the room's floor reaches the wall over the door");
    }

    #[test]
    fn grass_turns_golden_in_autumn_and_snowy_in_winter() {
        let m = Map::flat(6, 6);
        let colour = |season: Season| {
            let cut = Cut { season: Season::ALL.iter().position(|&s| s == season).unwrap() as u8, ..Cut::NONE };
            ground_color(&m, (3, 3, Z - 1), Look3::Soil, cut)
        };
        let (summer, autumn, winter) = (colour(Season::Summer), colour(Season::Autumn), colour(Season::Winter));
        assert_eq!(colour(Season::Spring).vertex(), summer.vertex());
        assert!(autumn.0 > summer.0, "more yellow");
        assert!(winter.0 > 0.8 && winter.2 > 0.8, "nearly white");
    }

    #[test]
    fn every_pattern_shows_both_colours() {
        use colcom::defs::Pattern::*;
        for p in [Stripes, Checks, Dots, Tiles, Bricks, Planks, Diamonds] {
            let squares: Vec<bool> = (0..16).flat_map(|u| (1..8).map(move |v| p.accent(u, v))).collect();
            assert!(squares.contains(&true) && squares.contains(&false), "{:?}", p);
        }
        assert!(defs().rooms.iter().all(|r| r.wallpaper.is_some()), "every theme has one");
    }
}
