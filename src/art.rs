//! Palette and drawing primitives for the flat, outlined top-down style.
//! Everything is built from shapes, so it stays crisp at any zoom level.

use macroquad::prelude::*;
use my_world::pawn::{Faction, Pawn, Weapon};
use std::f32::consts::TAU;

pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color { r, g, b, a: 1.0 }
}

pub const OUTLINE: Color = rgb(0.11, 0.09, 0.08);
pub const SHADOW: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.25 };

pub const GRASS_A: Color = rgb(0.35, 0.49, 0.24);
pub const GRASS_B: Color = rgb(0.45, 0.57, 0.29);
pub const GRASS_DARK: Color = rgb(0.26, 0.39, 0.19);
pub const SOIL: Color = rgb(0.42, 0.3, 0.2);
pub const SOIL_DARK: Color = rgb(0.32, 0.22, 0.14);
pub const WATER: Color = rgb(0.22, 0.45, 0.65);
pub const WATER_DEEP: Color = rgb(0.15, 0.33, 0.53);
pub const FOAM: Color = rgb(0.75, 0.88, 0.94);
pub const FLOOR: Color = rgb(0.62, 0.47, 0.31);
pub const FLOOR_SEAM: Color = rgb(0.46, 0.33, 0.21);

pub const ROCK_TOP: Color = rgb(0.49, 0.47, 0.45);
pub const ROCK_FACE: Color = rgb(0.31, 0.29, 0.28);
pub const WALL_TOP: Color = rgb(0.74, 0.71, 0.66);
pub const WALL_FACE: Color = rgb(0.5, 0.48, 0.45);

pub const WOOD: Color = rgb(0.55, 0.37, 0.2);
pub const WOOD_LIGHT: Color = rgb(0.76, 0.57, 0.35);
pub const LEAF: Color = rgb(0.2, 0.42, 0.18);
pub const LEAF_LIGHT: Color = rgb(0.36, 0.6, 0.27);
pub const PINE: Color = rgb(0.13, 0.33, 0.21);
pub const BERRY: Color = rgb(0.84, 0.16, 0.24);
pub const VEG: Color = rgb(0.93, 0.52, 0.16);
pub const WHEAT: Color = rgb(0.91, 0.77, 0.32);
pub const SHEET: Color = rgb(0.93, 0.91, 0.86);
pub const BLUEPRINT: Color = rgb(0.38, 0.68, 1.0);
pub const METAL: Color = rgb(0.25, 0.26, 0.28);

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

pub fn shade(c: Color, f: f32) -> Color {
    Color { r: (c.r * f).min(1.0), g: (c.g * f).min(1.0), b: (c.b * f).min(1.0), a: c.a }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

pub fn color3(c: [f32; 3]) -> Color {
    rgb(c[0], c[1], c[2])
}

/// Stable pseudo-random value in [0, 1) for a tile, so decoration doesn't flicker.
pub fn hash(x: i32, y: i32, salt: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ salt.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Smooth value noise, for large soft colour patches.
pub fn noise(x: f32, y: f32, salt: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (x0 as i32, y0 as i32);
    let a = hash(xi, yi, salt);
    let b = hash(xi + 1, yi, salt);
    let c = hash(xi, yi + 1, salt);
    let d = hash(xi + 1, yi + 1, salt);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Outline thickness for a given on-screen tile size.
pub fn line_w(ts: f32) -> f32 {
    (ts / 16.0).clamp(0.8, 2.5)
}

pub fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32, color: Color) {
    const N: usize = 18;
    for k in 0..N {
        let a0 = k as f32 / N as f32 * TAU;
        let a1 = (k + 1) as f32 / N as f32 * TAU;
        draw_triangle(
            vec2(cx, cy),
            vec2(cx + rx * a0.cos(), cy + ry * a0.sin()),
            vec2(cx + rx * a1.cos(), cy + ry * a1.sin()),
            color,
        );
    }
}

/// Filled circle with a dark outline.
pub fn circle_o(x: f32, y: f32, r: f32, fill: Color, o: f32) {
    draw_circle(x, y, r + o, OUTLINE);
    draw_circle(x, y, r, fill);
}

/// Rounded rectangle. Only use opaque colours (the pieces overlap).
pub fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32, color: Color) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    draw_rectangle(x + r, y, w - 2.0 * r, h, color);
    draw_rectangle(x, y + r, w, h - 2.0 * r, color);
    for (cx, cy) in [(x + r, y + r), (x + w - r, y + r), (x + r, y + h - r), (x + w - r, y + h - r)] {
        draw_circle(cx, cy, r, color);
    }
}

/// Rounded rectangle with a dark outline.
pub fn rrect_o(x: f32, y: f32, w: f32, h: f32, r: f32, fill: Color, o: f32) {
    rrect(x - o, y - o, w + 2.0 * o, h + 2.0 * o, r + o, OUTLINE);
    rrect(x, y, w, h, r, fill);
}

/// Thick line with a dark outline.
pub fn line_o(x1: f32, y1: f32, x2: f32, y2: f32, w: f32, color: Color, o: f32) {
    draw_line(x1, y1, x2, y2, w + 2.0 * o, OUTLINE);
    draw_line(x1, y1, x2, y2, w, color);
}

/// RimWorld-style selection corners.
pub fn brackets(x: f32, y: f32, w: f32, h: f32, thick: f32, color: Color) {
    let l = (w.min(h) * 0.3).max(4.0);
    for (cx, cy, dx, dy) in [(x, y, 1.0, 1.0), (x + w, y, -1.0, 1.0), (x, y + h, 1.0, -1.0), (x + w, y + h, -1.0, -1.0)] {
        draw_line(cx, cy, cx + dx * l, cy, thick, color);
        draw_line(cx, cy, cx, cy + dy * l, thick, color);
    }
}

pub fn dashed_rect(x: f32, y: f32, w: f32, h: f32, thick: f32, color: Color) {
    let dash = (w / 4.0).max(2.0);
    let mut t = 0.0;
    while t < w {
        let e = (t + dash * 0.6).min(w);
        draw_line(x + t, y, x + e, y, thick, color);
        draw_line(x + t, y + h, x + e, y + h, thick, color);
        t += dash;
    }
    let mut t = 0.0;
    while t < h {
        let e = (t + dash * 0.6).min(h);
        draw_line(x, y + t, x, y + e, thick, color);
        draw_line(x + w, y + t, x + w, y + e, thick, color);
        t += dash;
    }
}

pub fn text_shadow(text: &str, x: f32, y: f32, size: f32, color: Color) {
    draw_text(text, x + 1.0, y + 1.0, size, Color { r: 0.0, g: 0.0, b: 0.0, a: 0.75 });
    draw_text(text, x, y, size, color);
}

// ---------- pawns ----------

pub struct Look {
    pub skin: Color,
    pub hair: Color,
    pub clothes: Color,
}

const SKINS: [Color; 5] = [
    rgb(0.98, 0.84, 0.7),
    rgb(0.93, 0.74, 0.58),
    rgb(0.8, 0.58, 0.42),
    rgb(0.6, 0.42, 0.3),
    rgb(0.42, 0.29, 0.21),
];
const HAIRS: [Color; 6] = [
    rgb(0.15, 0.11, 0.09),
    rgb(0.35, 0.22, 0.12),
    rgb(0.62, 0.4, 0.2),
    rgb(0.85, 0.7, 0.4),
    rgb(0.7, 0.28, 0.14),
    rgb(0.75, 0.75, 0.75),
];

pub fn pawn_look(p: &Pawn) -> Look {
    let h = |salt: u32| hash(p.id as i32, 17, salt);
    let skin = SKINS[(h(1) * SKINS.len() as f32) as usize % SKINS.len()];
    let mut look = match p.faction {
        Faction::Colony => Look {
            skin,
            hair: HAIRS[(h(2) * HAIRS.len() as f32) as usize % HAIRS.len()],
            clothes: shade(color3(p.color), 0.85),
        },
        // Raiders: dark leathers and a steel helmet.
        Faction::Raider => Look { skin, hair: rgb(0.42, 0.43, 0.46), clothes: rgb(0.38, 0.17, 0.14) },
    };
    if p.dead {
        let grey = rgb(0.5, 0.5, 0.5);
        look = Look { skin: mix(look.skin, grey, 0.6), hair: mix(look.hair, grey, 0.6), clothes: mix(look.clothes, grey, 0.6) };
    }
    look
}

/// Head seen from above and slightly in front: hair cap with the face below it.
pub fn draw_head(cx: f32, cy: f32, s: f32, look: &Look, facing: f32, o: f32) {
    let r = s * 0.19;
    draw_circle(cx, cy, r + o, OUTLINE);
    draw_circle(cx, cy, r, look.hair);
    draw_circle(cx + facing * r * 0.15, cy + r * 0.3, r * 0.78, look.skin);
}

/// Standing pawn centred on (cx, cy), `s` = tile size on screen.
pub fn draw_standing(cx: f32, cy: f32, s: f32, look: &Look, facing: f32, weapon: Option<Weapon>, o: f32) {
    ellipse(cx, cy + s * 0.3, s * 0.26, s * 0.09, SHADOW);
    rrect_o(cx - s * 0.22, cy - s * 0.08, s * 0.44, s * 0.36, s * 0.14, look.clothes, o);
    if let Some(w) = weapon {
        draw_weapon(cx, cy, s, w, facing, o);
    }
    draw_head(cx, cy - s * 0.2, s, look, facing, o);
}

/// Pawn lying on its side (downed, dead, or asleep on the ground).
pub fn draw_lying(cx: f32, cy: f32, s: f32, look: &Look, o: f32) {
    ellipse(cx + s * 0.04, cy + s * 0.16, s * 0.36, s * 0.1, SHADOW);
    rrect_o(cx - s * 0.16, cy - s * 0.12, s * 0.46, s * 0.28, s * 0.12, look.clothes, o);
    draw_head(cx - s * 0.24, cy - s * 0.01, s * 0.95, look, -1.0, o);
}

pub fn draw_weapon(cx: f32, cy: f32, s: f32, w: Weapon, facing: f32, o: f32) {
    let (hx, hy) = (cx + facing * s * 0.12, cy + s * 0.1);
    match w {
        Weapon::Rifle => line_o(hx, hy, hx + facing * s * 0.42, hy - s * 0.08, s * 0.07, METAL, o),
        Weapon::Pistol => line_o(hx, hy, hx + facing * s * 0.24, hy - s * 0.04, s * 0.08, METAL, o),
        Weapon::Club => line_o(hx, hy, hx + facing * s * 0.2, hy - s * 0.3, s * 0.09, WOOD, o),
        Weapon::Knife => {
            // Short wooden grip, then a pale blade.
            let (gx, gy) = (hx + facing * s * 0.08, hy - s * 0.04);
            line_o(hx, hy, gx, gy, s * 0.07, WOOD, o);
            line_o(gx, gy, gx + facing * s * 0.2, gy - s * 0.1, s * 0.05, rgb(0.82, 0.84, 0.88), o);
        }
    }
}
