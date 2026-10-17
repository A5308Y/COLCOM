//! Roofs over rooms, drawn as hipped roofs.
//!
//! The height of every point comes from its distance to the roof's edge, so
//! any floor plan works: rectangles get a ridge, squares a pyramid, L-shapes
//! a valley where the wings meet, and big halls a flat top (the height is
//! capped). Points sit on a half-cell grid, so odd widths get a ridge in the
//! middle of a cell rather than a flat strip. A roof also covers the tops of
//! the walls around it and overhangs them a little (the eaves).
//!
//! All roofs are one mesh, rebuilt only when roofs or rooms change, or what's
//! shown does (layer view, hidden roofs).

use super::input::UiState;
use super::look::*;
use super::sim::Sim;
use super::terrain::LEVEL_H;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use my_world::map::{Block, Feature, Map};
use std::collections::VecDeque;

/// Height gained per cell inwards from the edge, in levels.
const PITCH: f32 = 0.45;
/// Highest a roof gets above the wall tops, in levels.
const PEAK: f32 = 1.3;
/// How far the eaves reach out past the walls (cells), and how far they drop (levels).
const EAVE_OUT: f32 = 0.3;
const EAVE_DROP: f32 = 0.12;

const TILES: Rgb = Rgb(0.6, 0.29, 0.21);
const UNDERSIDE: Rgb = Rgb(0.3, 0.21, 0.14);

/// The roof mesh (there's at most one).
#[derive(Component)]
pub struct RoofMesh;

#[derive(Resource)]
struct Roofs {
    /// World generation, `Map::roof_changes`, layer view level and roof cut
    /// version the mesh was built for.
    key: Option<(u64, u64, usize, u64)>,
    entity: Option<Entity>,
    material: Handle<StandardMaterial>,
}

pub struct RoofsPlugin;

impl Plugin for RoofsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup).add_systems(Update, update_roofs);
    }
}

fn setup(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    // Like the terrain's: colours come from the vertices.
    let material = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.9, ..default() });
    commands.insert_resource(Roofs { key: None, entity: None, material });
}

fn update_roofs(
    mut commands: Commands,
    sim: Res<Sim>,
    ui: Res<UiState>,
    roofs: ResMut<Roofs>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let roofs = roofs.into_inner();
    let m = &sim.world.map;
    let key = (sim.generation, m.roof_changes, ui.top_level(m), ui.roof_version);
    if roofs.key == Some(key) {
        return;
    }
    roofs.key = Some(key);
    match (build_roofs(m, &|x, y| ui.top_at(m, x, y)), roofs.entity) {
        (Some(mesh), Some(e)) => {
            commands.entity(e).insert(Mesh3d(meshes.add(mesh)));
        }
        (Some(mesh), None) => {
            let bundle = (RoofMesh, Mesh3d(meshes.add(mesh)), MeshMaterial3d(roofs.material.clone()), Transform::default());
            roofs.entity = Some(commands.spawn(bundle).id());
        }
        (None, Some(e)) => {
            commands.entity(e).despawn();
            roofs.entity = None;
        }
        (None, None) => {}
    }
}

/// Steps (of half a cell, sideways or diagonally) from each point of the
/// half-cell grid over a `w`×`h` area to the nearest point touching a cell
/// that isn't `inside`. Point (i, j) is at (i / 2, j / 2) cells; the grid is
/// (2w + 1) × (2h + 1), row by row.
fn half_steps(w: usize, h: usize, inside: &dyn Fn(usize, usize) -> bool) -> Vec<u32> {
    let (gw, gh) = (2 * w + 1, 2 * h + 1);
    // Cells a grid coordinate touches: both sides of a cell border, or the cell it's inside.
    let touched = |i: usize, n: usize| -> [Option<usize>; 2] {
        if i % 2 == 1 {
            [Some(i / 2), None]
        } else {
            [(i > 0).then(|| i / 2 - 1), (i / 2 < n).then_some(i / 2)]
        }
    };
    let mut steps = vec![u32::MAX; gw * gh];
    let mut queue = VecDeque::new();
    for j in 0..gh {
        for i in 0..gw {
            let (xs, ys) = (touched(i, w), touched(j, h));
            let edge = xs.iter().any(|x| x.is_none() && i % 2 == 0)
                || ys.iter().any(|y| y.is_none() && j % 2 == 0)
                || xs.iter().flatten().any(|&x| ys.iter().flatten().any(|&y| !inside(x, y)));
            if edge {
                steps[j * gw + i] = 0;
                queue.push_back((i, j));
            }
        }
    }
    while let Some((i, j)) = queue.pop_front() {
        let next = steps[j * gw + i] + 1;
        for dj in -1i32..=1 {
            for di in -1i32..=1 {
                let (ni, nj) = (i as i32 + di, j as i32 + dj);
                if ni < 0 || nj < 0 || ni >= gw as i32 || nj >= gh as i32 {
                    continue;
                }
                let k = nj as usize * gw + ni as usize;
                if steps[k] > next {
                    steps[k] = next;
                    queue.push_back((ni as usize, nj as usize));
                }
            }
        }
    }
    steps
}

/// Height above the wall tops, in levels, `steps` half cells in from the edge.
fn rise(steps: u32) -> f32 {
    (steps as f32 * PITCH / 2.0).min(PEAK) * LEVEL_H
}

/// Triangles with flat normals and vertex colours, each also drawn from below.
#[derive(Default)]
struct RoofBuf {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
}

impl RoofBuf {
    /// The top in `color` (turned to face up), and the underside darker.
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, color: Rgb) {
        let n = (b - a).cross(c - a);
        let (b, c, n) = if n.y < 0.0 { (c, b, -n) } else { (b, c, n) };
        let n = n.normalize_or_zero();
        self.push([a, b, c], n, color);
        self.push([a, c, b], -n, UNDERSIDE);
    }

    fn push(&mut self, corners: [Vec3; 3], n: Vec3, color: Rgb) {
        for p in corners {
            self.pos.push(p.to_array());
            self.nrm.push(n.to_array());
            self.col.push(color.vertex());
        }
    }

    fn into_mesh(self) -> Option<Mesh> {
        if self.pos.is_empty() {
            return None;
        }
        let idx = (0..self.pos.len() as u32).collect();
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        mesh.insert_indices(Indices::U32(idx));
        Some(mesh)
    }
}

/// Free space on top of a wall, door or window at level `z - 1`, which the
/// roof beside it can reach over.
fn wall_top(m: &Map, x: usize, y: usize, z: usize) -> bool {
    let i = m.idx((x, y, z));
    let below = m.idx((x, y, z - 1));
    m.block[i] == Block::Air
        && m.feature[i] == Feature::None
        && !m.floor[i]
        && !m.roof[i]
        && m.feature[below].blocks_room()
}

/// Every shown roof as one mesh (`None` if there are none). `top(x, y)` is
/// the highest level shown in a column.
fn build_roofs(m: &Map, top: &dyn Fn(usize, usize) -> usize) -> Option<Mesh> {
    let (w, h) = (m.w, m.h);
    let mut buf = RoofBuf::default();
    for z in 1..m.d {
        let mut cover = vec![false; w * h];
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                if m.roof[m.idx((x, y, z))] && top(x, y) >= z {
                    cover[y * w + x] = true;
                    any = true;
                }
            }
        }
        if !any {
            continue;
        }
        // Over the walls around, too.
        let roofed = cover.clone();
        for y in 0..h {
            for x in 0..w {
                let near = (-1i32..=1).any(|dy| {
                    (-1i32..=1).any(|dx| {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < h && roofed[ny as usize * w + nx as usize]
                    })
                });
                if !roofed[y * w + x] && near && top(x, y) >= z && wall_top(m, x, y, z) {
                    cover[y * w + x] = true;
                }
            }
        }
        level_roof(&mut buf, w, h, z, &cover);
    }
    buf.into_mesh()
}

/// The roof over the cells `cover` marks on level `z` (its base is the floor of that level).
fn level_roof(buf: &mut RoofBuf, w: usize, h: usize, z: usize, cover: &[bool]) {
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && cover[y as usize * w + x as usize];
    let steps = half_steps(w, h, &|x, y| cover[y * w + x]);
    let gw = 2 * w + 1;
    let base = z as f32 * LEVEL_H;
    let point = |i: usize, j: usize| Vec3::new(i as f32 / 2.0, base + rise(steps[j * gw + i]), j as f32 / 2.0);
    for y in 0..h {
        for x in 0..w {
            if !cover[y * w + x] {
                continue;
            }
            let (xi, yi) = (x as i32, y as i32);
            let color = TILES.shade(0.86 + hash(xi, yi + 61 * z as i32, 71) * 0.14);
            // Four quarters, each two triangles split where the slope bends:
            // along the diagonal through the odd corner out.
            for (qi, qj) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (i, j) = (2 * x + qi, 2 * y + qj);
                let [a, b, c, d] = [point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1)];
                if b.y == d.y || a.y != c.y {
                    buf.tri(a, b, c, color);
                    buf.tri(a, c, d, color);
                } else {
                    buf.tri(a, b, d, color);
                    buf.tri(b, c, d, color);
                }
            }
            // Eaves: a strip out and down from each outer edge, and a corner
            // piece where two of them meet.
            let (fx, fy) = (x as f32, y as f32);
            let low = base - EAVE_DROP * LEVEL_H;
            let eave = color.shade(0.9);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                if inside(xi + dx, yi + dy) {
                    continue;
                }
                // The edge's ends, then the same pushed out.
                let (p, q) = match (dx, dy) {
                    (1, 0) => ((fx + 1.0, fy), (fx + 1.0, fy + 1.0)),
                    (-1, 0) => ((fx, fy), (fx, fy + 1.0)),
                    (0, 1) => ((fx, fy + 1.0), (fx + 1.0, fy + 1.0)),
                    _ => ((fx, fy), (fx + 1.0, fy)),
                };
                let out = Vec3::new(dx as f32, 0.0, dy as f32) * EAVE_OUT;
                let (p, q) = (Vec3::new(p.0, base, p.1), Vec3::new(q.0, base, q.1));
                let (po, qo) = (Vec3::new(p.x, low, p.z) + out, Vec3::new(q.x, low, q.z) + out);
                buf.tri(p, q, qo, eave);
                buf.tri(p, qo, po, eave);
            }
            for (dx, dy) in [(1, 1), (1, -1), (-1, 1), (-1, -1)] {
                if inside(xi + dx, yi) || inside(xi, yi + dy) {
                    continue;
                }
                let corner = Vec3::new(fx + (dx > 0) as i32 as f32, base, fy + (dy > 0) as i32 as f32);
                let (ox, oy) = (Vec3::new(dx as f32, 0.0, 0.0) * EAVE_OUT, Vec3::new(0.0, 0.0, dy as f32) * EAVE_OUT);
                let drop = Vec3::new(0.0, low - base, 0.0);
                buf.tri(corner, corner + ox + drop, corner + ox + oy + drop, eave);
                buf.tri(corner, corner + ox + oy + drop, corner + oy + drop, eave);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps along row `j` of the half grid.
    fn row(steps: &[u32], w: usize, j: usize) -> Vec<u32> {
        steps[j * (2 * w + 1)..(j + 1) * (2 * w + 1)].to_vec()
    }

    /// Steps along column `i` of the half grid.
    fn column(steps: &[u32], w: usize, h: usize, i: usize) -> Vec<u32> {
        (0..2 * h + 1).map(|j| steps[j * (2 * w + 1) + i]).collect()
    }

    #[test]
    fn strips_get_a_ridge_down_the_middle() {
        // Two cells wide: the ridge is on the cell border.
        let s = half_steps(6, 2, &|_, _| true);
        assert_eq!(column(&s, 6, 2, 6), vec![0, 1, 2, 1, 0]);
        // Three wide: in the middle of the middle cell, not a flat strip.
        let s = half_steps(8, 3, &|_, _| true);
        assert_eq!(column(&s, 8, 3, 8), vec![0, 1, 2, 3, 2, 1, 0]);
        // Along the ridge it's level, until the hips at the ends.
        assert_eq!(row(&s, 8, 3), vec![0, 1, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 2, 1, 0]);
    }

    #[test]
    fn squares_get_a_pyramid() {
        let s = half_steps(4, 4, &|_, _| true);
        let peak = s[4 * 9 + 4];
        assert_eq!(peak, 4, "the centre is highest");
        assert_eq!(s.iter().filter(|&&v| v == peak).count(), 1, "and only the centre");
        assert_eq!(row(&s, 4, 2), vec![0, 1, 2, 2, 2, 2, 2, 1, 0]);
    }

    #[test]
    fn l_shapes_get_a_valley() {
        // A 4×4 square with its top-right 2×2 missing.
        let inside = |x: usize, y: usize| !(x >= 2 && y >= 2);
        let s = half_steps(4, 4, &inside);
        let at = |i: usize, j: usize| s[j * 9 + i];
        assert_eq!(at(4, 4), 0, "the inner corner is on the edge");
        // Inwards from the inner corner the roof rises: the valley.
        assert_eq!([at(3, 3), at(2, 2), at(1, 1)], [1, 2, 1]);
        // Both wings get the same ridge height.
        assert_eq!(at(2, 6), 2);
        assert_eq!(at(6, 2), 2);
        // Nothing over the missing corner.
        assert_eq!(at(6, 6), 0);
    }

    #[test]
    fn big_halls_are_flat_on_top() {
        assert_eq!(rise(100), PEAK * LEVEL_H);
        assert!(rise(2) < rise(3));
    }
}
