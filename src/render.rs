//! Draws the map, pawns, lighting and effects.
//!
//! Layers, back to front: ground (terrain, floors, zones, cast shadows) →
//! features (trees, rock, walls, furniture) → items → designations → pawns →
//! lighting → labels, bars and effects (kept readable at night).

use crate::art::*;
use crate::camera::{View, TS};
use macroquad::prelude::*;
use my_world::map::{Desig, Feature, ItemKind, Map, Pos, Stack, Terrain, Zone};
use my_world::pawn::{Faction, Job, Pawn};
use my_world::world::World;
use std::collections::HashMap;

/// Tile range currently on screen.
struct Visible {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Visible {
    fn tiles(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (self.y0..self.y1).flat_map(move |y| (self.x0..self.x1).map(move |x| (x, y)))
    }
}

fn feat(m: &Map, x: i32, y: i32) -> Option<Feature> {
    if x < 0 || y < 0 || x >= m.w as i32 || y >= m.h as i32 {
        None
    } else {
        Some(m.feature[m.idx((x as usize, y as usize))])
    }
}

fn is_rock(f: Feature) -> bool {
    f == Feature::Rock
}

fn is_wallish(f: Feature) -> bool {
    matches!(f, Feature::Wall | Feature::Door)
}

fn casts_shadow(f: Feature) -> bool {
    matches!(f, Feature::Wall | Feature::Rock)
}

pub fn draw_world(w: &World, v: &View, view_h: f32, selected: Option<u32>, drag: Option<(Pos, Pos)>) {
    let m = &w.map;
    let s = v.ts();
    let o = line_w(s);
    let t = get_time() as f32;
    let vis = Visible {
        x0: (v.x / TS).floor().max(0.0) as usize,
        y0: (v.y / TS).floor().max(0.0) as usize,
        x1: ((((v.x + screen_width() / v.zoom) / TS).ceil().max(0.0)) as usize + 1).min(m.w),
        y1: ((((v.y + view_h / v.zoom) / TS).ceil().max(0.0)) as usize + 1).min(m.h),
    };

    let mut bed_owner: HashMap<Pos, Color> = HashMap::new();
    for p in w.pawns.iter().filter(|p| !p.dead) {
        if let Some(b) = p.bed {
            bed_owner.insert(b, shade(color3(p.color), 0.8));
        }
    }

    for (x, y) in vis.tiles() {
        draw_ground(m, x, y, v, s, o, t);
    }
    for (x, y) in vis.tiles() {
        draw_feature(w, x, y, v, s, o, t, &bed_owner);
    }
    for (x, y) in vis.tiles() {
        let i = m.idx((x, y));
        let (sx, sy) = v.tile_to_screen(x as f32, y as f32);
        if let Some(stack) = m.items[i] {
            draw_item(sx, sy, s, stack, o);
        }
        if let Some(d) = m.desig[i] {
            let color = match d {
                Desig::Chop => ORANGE,
                Desig::Mine => SKYBLUE,
                Desig::Forage => PINK,
            };
            brackets(sx + 1.5, sy + 1.5, s - 3.0, s - 3.0, o * 1.5, color);
        }
    }

    // Pawns, back to front.
    let mut pawns: Vec<&Pawn> = w.pawns.iter().filter(|p| !p.left).collect();
    pawns.sort_by(|a, b| (a.dead as u8).cmp(&(b.dead as u8)).reverse().then(a.y.total_cmp(&b.y)));
    for p in &pawns {
        draw_pawn(m, p, v, s, o);
    }

    draw_lighting(w, &vis, v, s);

    for e in &w.effects {
        let (ax, ay) = v.tile_to_screen(e.from.0 + 0.5, e.from.1 + 0.3);
        let (bx, by) = v.tile_to_screen(e.to.0 + 0.5, e.to.1 + 0.4);
        let fade = (e.ttl / 0.15).clamp(0.0, 1.0);
        draw_line(ax, ay, bx, by, o * 1.2, Color::new(1.0, 0.92, 0.55, 0.9 * fade));
        draw_circle(ax, ay, s * 0.12 * fade, Color::new(1.0, 0.85, 0.4, 0.8 * fade));
    }
    for p in &pawns {
        draw_pawn_overlay(p, v, s, o, selected == Some(p.id));
    }

    if let Some((a, b)) = drag {
        let (lx, ly) = (a.0.min(b.0), a.1.min(b.1));
        let (hx, hy) = (a.0.max(b.0), a.1.max(b.1));
        let (sx, sy) = v.tile_to_screen(lx as f32, ly as f32);
        let (rw, rh) = ((hx - lx + 1) as f32 * s, (hy - ly + 1) as f32 * s);
        draw_rectangle(sx, sy, rw, rh, Color::new(1.0, 1.0, 1.0, 0.12));
        draw_rectangle_lines(sx, sy, rw, rh, 2.0, WHITE);
        text_shadow(&format!("{}x{}", hx - lx + 1, hy - ly + 1), sx + 4.0, sy - 6.0, 18.0, WHITE);
    }
}

// ---------- ground ----------

fn draw_ground(m: &Map, x: usize, y: usize, v: &View, s: f32, o: f32, t: f32) {
    let i = m.idx((x, y));
    let (sx, sy) = v.tile_to_screen(x as f32, y as f32);
    let (xi, yi) = (x as i32, y as i32);
    let fill = s + 0.6; // slight overlap hides seams between tiles

    match m.terrain[i] {
        Terrain::Water => {
            let n = noise(x as f32 / 4.0, y as f32 / 4.0, 3);
            draw_rectangle(sx, sy, fill, fill, mix(WATER_DEEP, WATER, n));
            // Foam where the water meets land.
            let land = |dx: i32, dy: i32| {
                let (nx, ny) = (xi + dx, yi + dy);
                nx >= 0 && ny >= 0 && nx < m.w as i32 && ny < m.h as i32 && m.terrain[m.idx((nx as usize, ny as usize))] != Terrain::Water
            };
            let foam = alpha(FOAM, 0.75);
            let fw = (s * 0.09).max(1.0);
            if land(0, -1) {
                draw_rectangle(sx, sy, s, fw, foam);
            }
            if land(0, 1) {
                draw_rectangle(sx, sy + s - fw, s, fw, foam);
            }
            if land(-1, 0) {
                draw_rectangle(sx, sy, fw, s, foam);
            }
            if land(1, 0) {
                draw_rectangle(sx + s - fw, sy, fw, s, foam);
            }
            // Drifting glints.
            let h = hash(xi, yi, 4);
            if h > 0.5 {
                let off = (t * 1.1 + h * 12.0).sin() * s * 0.15;
                let gy = sy + s * (0.25 + hash(xi, yi, 5) * 0.5);
                draw_line(sx + s * 0.25 + off, gy, sx + s * 0.6 + off, gy, o, alpha(FOAM, 0.35));
            }
        }
        Terrain::Grass if m.zone[i] == Zone::Grow => {
            draw_rectangle(sx, sy, fill, fill, mix(SOIL, SOIL_DARK, hash(xi, yi, 6) * 0.3));
            for k in 0..3 {
                let fy = sy + s * (0.2 + 0.3 * k as f32);
                draw_line(sx + s * 0.08, fy, sx + s * 0.92, fy, o, SOIL_DARK);
            }
        }
        Terrain::Grass if m.room.get(i).map_or(false, |&r| r != 0) => {
            // Wooden plank floor; joints are staggered per plank row.
            draw_rectangle(sx, sy, fill, fill, shade(FLOOR, 0.95 + hash(xi, yi, 7) * 0.1));
            if s >= 10.0 {
                for k in 0..3 {
                    let py = sy + s * k as f32 / 3.0;
                    draw_line(sx, py, sx + s, py, o * 0.7, FLOOR_SEAM);
                    let jx = sx + s * hash(xi, yi * 3 + k, 8);
                    draw_line(jx, py, jx, py + s / 3.0, o * 0.7, FLOOR_SEAM);
                }
            }
        }
        Terrain::Grass => {
            let n = noise(x as f32 / 6.0, y as f32 / 6.0, 1) * 0.75 + hash(xi, yi, 2) * 0.25;
            draw_rectangle(sx, sy, fill, fill, mix(GRASS_A, GRASS_B, n));
            if s >= 14.0 && m.feature[i] == Feature::None {
                if hash(xi, yi, 9) > 0.55 {
                    let (bx, by) = (sx + s * (0.2 + hash(xi, yi, 10) * 0.6), sy + s * (0.3 + hash(xi, yi, 11) * 0.5));
                    for dx in [-0.08, 0.0, 0.08] {
                        draw_line(bx, by, bx + dx * s, by - s * 0.14, o * 0.8, GRASS_DARK);
                    }
                }
                if hash(xi, yi, 12) > 0.965 {
                    let petal = [WHITE, rgb(1.0, 0.85, 0.3), rgb(0.75, 0.55, 0.95)][(hash(xi, yi, 13) * 3.0) as usize % 3];
                    let (fx, fy) = (sx + s * 0.6, sy + s * 0.35);
                    draw_circle(fx, fy, s * 0.06, petal);
                    draw_circle(fx, fy, s * 0.025, rgb(0.95, 0.7, 0.2));
                }
            }
        }
    }

    // Shadows cast by walls and cliffs (light comes from the top left).
    if !casts_shadow(m.feature[i]) {
        if feat(m, xi, yi - 1).map_or(false, casts_shadow) {
            draw_rectangle(sx, sy, s, s * 0.24, alpha(SHADOW, 0.22));
        }
        if feat(m, xi - 1, yi).map_or(false, casts_shadow) {
            draw_rectangle(sx, sy, s * 0.14, s, alpha(SHADOW, 0.16));
        }
    }

    let blood = m.blood.get(i).copied().unwrap_or(0.0);
    if blood > 0.02 {
        let c = Color::new(0.45, 0.02, 0.04, (blood * 0.8).min(0.8));
        for k in 0..3 {
            let (bx, by) = (sx + s * (0.25 + 0.5 * hash(xi, yi, 50 + k)), sy + s * (0.25 + 0.5 * hash(xi, yi, 60 + k)));
            draw_circle(bx, by, s * (0.06 + 0.08 * hash(xi, yi, 70 + k)), c);
        }
    }

    if m.zone[i] == Zone::Stockpile {
        draw_rectangle(sx, sy, s, s, Color::new(0.8, 0.8, 1.0, 0.1));
        let edge = Color::new(0.85, 0.85, 1.0, 0.55);
        let other = |dx: i32, dy: i32| {
            let (nx, ny) = (xi + dx, yi + dy);
            nx < 0 || ny < 0 || nx >= m.w as i32 || ny >= m.h as i32 || m.zone[m.idx((nx as usize, ny as usize))] != Zone::Stockpile
        };
        if other(0, -1) {
            draw_line(sx, sy, sx + s, sy, o, edge);
        }
        if other(0, 1) {
            draw_line(sx, sy + s, sx + s, sy + s, o, edge);
        }
        if other(-1, 0) {
            draw_line(sx, sy, sx, sy + s, o, edge);
        }
        if other(1, 0) {
            draw_line(sx + s, sy, sx + s, sy + s, o, edge);
        }
    }
}

// ---------- features ----------

#[allow(clippy::too_many_arguments)]
fn draw_feature(w: &World, x: usize, y: usize, v: &View, s: f32, o: f32, t: f32, bed_owner: &HashMap<Pos, Color>) {
    let m = &w.map;
    let i = m.idx((x, y));
    let (sx, sy) = v.tile_to_screen(x as f32, y as f32);
    let (cx, cy) = (sx + s / 2.0, sy + s / 2.0);
    let (xi, yi) = (x as i32, y as i32);
    match m.feature[i] {
        Feature::None => {}
        Feature::Tree => draw_tree(cx, cy, s, hash(xi, yi, 20), hash(xi, yi, 21), t, o),
        Feature::Bush => draw_bush(cx, cy, s, o),
        Feature::Rock => {
            draw_block(m, xi, yi, sx, sy, s, o, ROCK_TOP, ROCK_FACE, is_rock);
            if s >= 12.0 {
                for k in 0..3 {
                    let h = hash(xi, yi, 30 + k);
                    if h > 0.4 {
                        let (px, py) = (sx + s * (0.15 + 0.7 * hash(xi, yi, 40 + k)), sy + s * (0.15 + 0.5 * h));
                        draw_circle(px, py, s * 0.05, shade(ROCK_TOP, 0.82));
                    }
                }
            }
        }
        Feature::Wall => {
            draw_block(m, xi, yi, sx, sy, s, o, WALL_TOP, WALL_FACE, is_wallish);
            if s >= 12.0 {
                // Brick courses on the top face.
                let seam = shade(WALL_TOP, 0.85);
                draw_line(sx, sy + s * 0.36, sx + s, sy + s * 0.36, o * 0.7, seam);
                let off = if y % 2 == 0 { 0.3 } else { 0.7 };
                draw_line(sx + s * off, sy, sx + s * off, sy + s * 0.36, o * 0.7, seam);
            }
        }
        Feature::Door => {
            let open = w.pawns.iter().any(|p| !p.dead && (p.x - x as f32).abs() < 0.9 && (p.y - y as f32).abs() < 0.9);
            let horizontal = feat(m, xi - 1, yi).map_or(false, is_wallish) || feat(m, xi + 1, yi).map_or(false, is_wallish);
            draw_door(sx, sy, s, o, horizontal, open);
        }
        Feature::Bed => {
            let blanket = bed_owner.get(&(x, y)).copied().unwrap_or(rgb(0.55, 0.58, 0.62));
            draw_bed(sx, sy, s, o, blanket);
        }
        Feature::Crop { growth } => draw_crop(sx, sy, s, o, growth),
        Feature::Blueprint { kind, delivered } => {
            draw_rectangle(sx + 2.0, sy + 2.0, s - 4.0, s - 4.0, alpha(BLUEPRINT, 0.2));
            dashed_rect(sx + 2.0, sy + 2.0, s - 4.0, s - 4.0, o, BLUEPRINT);
            if s >= 22.0 {
                let label = format!("{}/{}", delivered, kind.cost().count);
                text_shadow(&label, sx + 4.0, sy + s - 5.0, s * 0.42, WHITE);
            }
        }
    }
}

/// Walls and rock: a top face, a darker front face where nothing continues
/// below, and outlines only on edges that don't join a neighbour.
#[allow(clippy::too_many_arguments)]
fn draw_block(m: &Map, x: i32, y: i32, sx: f32, sy: f32, s: f32, o: f32, top: Color, face: Color, joins: fn(Feature) -> bool) {
    let joined = |dx: i32, dy: i32| feat(m, x + dx, y + dy).map_or(true, joins);
    let tone = 0.96 + hash(x, y, 31) * 0.08;
    draw_rectangle(sx, sy, s + 0.6, s + 0.6, shade(top, tone));
    let lw = o * 1.4;
    if !joined(0, 1) {
        let fy = sy + s * 0.7;
        draw_rectangle(sx, fy, s + 0.6, s * 0.3, shade(face, tone));
        draw_line(sx, fy, sx + s, fy, o, shade(face, 0.75));
        draw_line(sx, sy + s, sx + s, sy + s, lw, OUTLINE);
    }
    if !joined(0, -1) {
        draw_line(sx, sy + lw, sx + s, sy + lw, o, shade(top, 1.18));
        draw_line(sx, sy, sx + s, sy, lw, OUTLINE);
    }
    if !joined(-1, 0) {
        draw_line(sx, sy, sx, sy + s, lw, OUTLINE);
    }
    if !joined(1, 0) {
        draw_line(sx + s, sy, sx + s, sy + s, lw, OUTLINE);
    }
}

fn draw_tree(cx: f32, cy: f32, s: f32, h1: f32, h2: f32, t: f32, o: f32) {
    let size = 0.85 + h1 * 0.3;
    let sway = (t * 0.9 + h2 * 20.0).sin() * s * 0.02;
    ellipse(cx + s * 0.12, cy + s * 0.32, s * 0.38 * size, s * 0.13 * size, SHADOW);
    rrect_o(cx - s * 0.06, cy, s * 0.12, s * 0.34, s * 0.03, WOOD, o);

    if h2 < 0.35 {
        // Conifer: stacked triangles.
        let tiers = [(0.08, 0.42), (-0.1, 0.33), (-0.27, 0.23)];
        let color = mix(PINE, LEAF, h1 * 0.3);
        for (k, &(base, half)) in tiers.iter().enumerate() {
            let (b, hw) = (cy + base * s * size, half * s * size);
            let top = b - s * 0.36 * size;
            let c = shade(color, 1.0 + k as f32 * 0.08);
            draw_triangle(vec2(cx - hw + sway, b), vec2(cx + hw + sway, b), vec2(cx + sway * 1.5, top), c);
            draw_triangle_lines(vec2(cx - hw + sway, b), vec2(cx + hw + sway, b), vec2(cx + sway * 1.5, top), o, OUTLINE);
        }
    } else {
        // Broadleaf: overlapping blobs, outlined as one shape.
        let leaf = mix(LEAF, LEAF_LIGHT, h1 * 0.45);
        let blobs = [(-0.16, -0.12, 0.23), (0.15, -0.1, 0.22), (0.0, -0.3, 0.25), (0.0, -0.08, 0.24)];
        for (dx, dy, r) in blobs {
            draw_circle(cx + dx * s * size + sway, cy + dy * s * size, r * s * size + o, OUTLINE);
        }
        for (dx, dy, r) in blobs {
            draw_circle(cx + dx * s * size + sway, cy + dy * s * size, r * s * size, leaf);
        }
        draw_circle(cx - s * 0.07 * size + sway, cy - s * 0.32 * size, s * 0.1 * size, shade(leaf, 1.22));
    }
}

fn draw_bush(cx: f32, cy: f32, s: f32, o: f32) {
    ellipse(cx + s * 0.06, cy + s * 0.22, s * 0.3, s * 0.1, SHADOW);
    let blobs = [(-0.13, 0.04, 0.17), (0.13, 0.05, 0.16), (0.0, -0.08, 0.18)];
    for (dx, dy, r) in blobs {
        draw_circle(cx + dx * s, cy + dy * s, r * s + o, OUTLINE);
    }
    for (dx, dy, r) in blobs {
        draw_circle(cx + dx * s, cy + dy * s, r * s, LEAF);
    }
    for (dx, dy) in [(-0.14, -0.02), (0.1, -0.1), (0.04, 0.1), (0.17, 0.06)] {
        circle_o(cx + dx * s, cy + dy * s, s * 0.055, BERRY, o * 0.6);
        if s >= 16.0 {
            draw_circle(cx + dx * s - s * 0.015, cy + dy * s - s * 0.015, s * 0.018, WHITE);
        }
    }
}

fn draw_door(sx: f32, sy: f32, s: f32, o: f32, horizontal: bool, open: bool) {
    let post = s * 0.14;
    let leaf_len = if open { (s - 2.0 * post) * 0.25 } else { s - 2.0 * post };
    if horizontal {
        draw_rectangle(sx, sy + s * 0.3, post, s * 0.4, WALL_FACE);
        draw_rectangle(sx + s - post, sy + s * 0.3, post, s * 0.4, WALL_FACE);
        rrect_o(sx + post, sy + s * 0.4, leaf_len, s * 0.2, s * 0.04, WOOD, o);
    } else {
        draw_rectangle(sx + s * 0.3, sy, s * 0.4, post, WALL_FACE);
        draw_rectangle(sx + s * 0.3, sy + s - post, s * 0.4, post, WALL_FACE);
        rrect_o(sx + s * 0.4, sy + post, s * 0.2, leaf_len, s * 0.04, WOOD, o);
    }
}

fn draw_bed(sx: f32, sy: f32, s: f32, o: f32, blanket: Color) {
    ellipse(sx + s * 0.55, sy + s * 0.9, s * 0.4, s * 0.08, SHADOW);
    rrect_o(sx + s * 0.12, sy + s * 0.06, s * 0.76, s * 0.88, s * 0.08, WOOD, o);
    draw_rectangle(sx + s * 0.17, sy + s * 0.11, s * 0.66, s * 0.78, SHEET);
    rrect(sx + s * 0.24, sy + s * 0.14, s * 0.52, s * 0.18, s * 0.06, WHITE);
    draw_rectangle(sx + s * 0.17, sy + s * 0.42, s * 0.66, s * 0.47, blanket);
    draw_rectangle(sx + s * 0.17, sy + s * 0.42, s * 0.66, s * 0.07, shade(blanket, 1.3));
}

fn draw_crop(sx: f32, sy: f32, s: f32, o: f32, growth: f32) {
    let ripe = growth >= 1.0;
    for row in 0..2 {
        for col in 0..3 {
            let px = sx + s * (0.2 + 0.3 * col as f32);
            let py = sy + s * (0.38 + 0.36 * row as f32);
            if growth < 0.3 {
                draw_line(px, py, px - s * 0.04, py - s * 0.12, o, LEAF_LIGHT);
                draw_line(px, py, px + s * 0.04, py - s * 0.1, o, LEAF_LIGHT);
            } else {
                let r = s * (0.05 + 0.07 * growth);
                let leaf = if ripe { mix(LEAF_LIGHT, WHEAT, 0.35) } else { LEAF_LIGHT };
                circle_o(px, py - s * 0.06, r, leaf, o * 0.7);
                if ripe {
                    circle_o(px + s * 0.02, py - s * 0.15, s * 0.05, WHEAT, o * 0.6);
                }
            }
        }
    }
}

// ---------- items ----------

/// Small icon for an item kind, centred on (cx, cy) and sized for tile size `s`.
pub fn draw_item_icon(cx: f32, cy: f32, s: f32, kind: ItemKind, o: f32) {
    match kind {
        ItemKind::Wood => {
            for (dx, dy) in [(-0.04, 0.1), (0.04, -0.05), (0.0, -0.2)] {
                let (lx, ly) = (cx + (dx - 0.26) * s, cy + dy * s);
                rrect_o(lx, ly, s * 0.52, s * 0.14, s * 0.07, WOOD, o);
                draw_circle(lx + s * 0.45, ly + s * 0.07, s * 0.05, WOOD_LIGHT);
            }
        }
        ItemKind::Stone => {
            for (dx, dy, w, h) in [(-0.26, -0.02, 0.24, 0.2), (0.02, 0.0, 0.24, 0.18), (-0.13, -0.2, 0.24, 0.18)] {
                let (bx, by) = (cx + dx * s, cy + dy * s);
                rrect_o(bx, by, w * s, h * s, s * 0.04, ROCK_TOP, o);
                draw_line(bx + s * 0.03, by + s * 0.04, bx + w * s - s * 0.05, by + s * 0.04, o, shade(ROCK_TOP, 1.25));
            }
        }
        ItemKind::Food => {
            for (k, &(dx, dy)) in [(-0.14f32, 0.08f32), (0.12, 0.09), (0.0, 0.1), (-0.07, -0.06), (0.08, -0.05)].iter().enumerate() {
                let c = if k % 2 == 0 { BERRY } else { VEG };
                circle_o(cx + dx * s, cy + dy * s, s * 0.1, c, o * 0.7);
            }
            draw_line(cx, cy - s * 0.12, cx + s * 0.08, cy - s * 0.22, o * 1.5, LEAF_LIGHT);
        }
        ItemKind::Medicine => {
            // A white medkit with a red cross.
            rrect_o(cx - s * 0.24, cy - s * 0.17, s * 0.48, s * 0.36, s * 0.06, SHEET, o);
            draw_rectangle(cx - s * 0.05, cy - s * 0.12, s * 0.1, s * 0.26, BERRY);
            draw_rectangle(cx - s * 0.13, cy - s * 0.04, s * 0.26, s * 0.1, BERRY);
        }
    }
}

fn draw_item(sx: f32, sy: f32, s: f32, stack: Stack, o: f32) {
    draw_item_icon(sx + s / 2.0, sy + s / 2.0, s, stack.kind, o);
    if s >= 18.0 {
        let label = stack.count.to_string();
        let size = (s * 0.42).max(11.0);
        let d = measure_text(&label, None, size as u16, 1.0);
        text_shadow(&label, sx + s - d.width - 2.0, sy + s - 2.0, size, WHITE);
    }
}

// ---------- pawns ----------

fn facing(p: &Pawn) -> f32 {
    if let Some(&(nx, _)) = p.path.last() {
        let d = nx as f32 - p.x;
        if d.abs() > 0.01 {
            return d.signum();
        }
    }
    if let Some(Job::Work { target, .. }) = p.job {
        let d = target.0 as f32 - p.x;
        if d.abs() > 0.01 {
            return d.signum();
        }
    }
    1.0
}

fn draw_pawn(m: &Map, p: &Pawn, v: &View, s: f32, o: f32) {
    let (cx, cy) = v.tile_to_screen(p.x + 0.5, p.y + 0.5);
    let look = pawn_look(p);
    let in_bed = p.lying() && !p.dead && m.feature[m.idx(p.pos)] == Feature::Bed;
    if in_bed {
        // Tucked in: just the head on the pillow.
        draw_head(cx, cy - s * 0.22, s * 0.9, &look, 1.0, o);
        return;
    }
    if p.dead || p.lying() {
        draw_lying(cx, cy, s, &look, o);
        return;
    }
    let bob = if p.path.is_empty() { 0.0 } else { ((p.x + p.y) * std::f32::consts::TAU).sin().abs() * s * 0.06 };
    let armed = p.drafted || p.faction == Faction::Raider || matches!(p.job, Some(Job::Attack { .. }));
    draw_standing(cx, cy - bob, s, &look, facing(p), armed.then_some(p.weapon), o);
}

/// Name, bars and markers, drawn after lighting so they stay readable.
fn draw_pawn_overlay(p: &Pawn, v: &View, s: f32, o: f32, selected: bool) {
    let (cx, cy) = v.tile_to_screen(p.x + 0.5, p.y + 0.5);
    if selected {
        brackets(cx - s * 0.5, cy - s * 0.55, s, s * 1.05, o * 1.5, WHITE);
    }
    if p.dead {
        return;
    }
    if p.drafted {
        let (dx, dy) = (cx, cy - s * 0.62);
        draw_triangle(vec2(dx - s * 0.1, dy - s * 0.08), vec2(dx + s * 0.1, dy - s * 0.08), vec2(dx, dy + s * 0.05), GOLD);
    }
    if p.sleeping {
        let bounce = (get_time() as f32 * 2.0 + p.id as f32).sin() * s * 0.05;
        text_shadow("z", cx + s * 0.3, cy - s * 0.35 + bounce, (s * 0.6).max(12.0), WHITE);
    }
    let bar_w = s * 0.7;
    let mut by = cy - s * 0.5 - 6.0;
    let health = p.health.summary();
    if health < 0.99 {
        draw_rectangle(cx - bar_w / 2.0, by, bar_w, 3.0, Color::new(0.25, 0.0, 0.0, 0.9));
        draw_rectangle(cx - bar_w / 2.0, by, bar_w * health, 3.0, RED);
        by -= 5.0;
    }
    if p.health.bleed_rate() > 0.0 {
        // Pulsing blood drop.
        let pulse = 0.7 + 0.3 * (get_time() as f32 * 5.0).sin();
        let (dx, dy) = (cx - s * 0.42, cy - s * 0.3);
        let r = s * 0.08 * pulse;
        draw_triangle(vec2(dx - r, dy), vec2(dx + r, dy), vec2(dx, dy - r * 2.2), BERRY);
        draw_circle(dx, dy, r, BERRY);
    }
    if let Some(Job::Work { .. }) = p.job {
        if p.job_progress > 0.0 {
            draw_rectangle(cx - bar_w / 2.0, by, bar_w, 3.0, Color::new(0.1, 0.1, 0.1, 0.9));
            draw_rectangle(cx - bar_w / 2.0, by, bar_w * p.job_progress.min(1.0), 3.0, GREEN);
        }
    }
    if let Some(c) = p.carrying {
        draw_item_icon(cx + s * 0.28, cy + s * 0.1, s * 0.55, c.kind, o * 0.7);
    }
    if s >= 14.0 {
        let size = 14.0;
        let d = measure_text(&p.name, None, size as u16, 1.0);
        let color = if p.faction == Faction::Raider { Color::new(1.0, 0.55, 0.5, 1.0) } else { WHITE };
        text_shadow(&p.name, cx - d.width / 2.0, cy + s * 0.5 + 13.0, size, color);
    }
}

// ---------- lighting ----------

/// Night darkness per tile, so roofed rooms stay warmly lit; plus a dusk/dawn glow.
fn draw_lighting(w: &World, vis: &Visible, v: &View, s: f32) {
    let day = w.daylight();
    let dark = (1.0 - day) * 0.6;
    if dark > 0.01 {
        let m = &w.map;
        for (x, y) in vis.tiles() {
            let (sx, sy) = v.tile_to_screen(x as f32, y as f32);
            let indoors = m.room.get(m.idx((x, y))).map_or(false, |&r| r != 0);
            let c = if indoors { Color::new(0.25, 0.12, 0.0, dark * 0.25) } else { Color::new(0.02, 0.03, 0.14, dark) };
            draw_rectangle(sx, sy, s + 0.6, s + 0.6, c);
        }
    }
    let glow = (1.0 - (day - 0.4).abs() / 0.15).max(0.0) * 0.12;
    if glow > 0.0 {
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(1.0, 0.5, 0.2, glow));
    }
}
