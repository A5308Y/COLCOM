//! Shared palette, stable per-tile randomness, and how each pawn looks.

use bevy::prelude::*;
use bevy_egui::egui;
use my_world::pawn::{Faction, Pawn};

/// An sRGB colour as plain floats, convertible to Bevy, vertex and egui colours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);


impl Rgb {
    pub fn color(self) -> Color {
        Color::srgb(self.0, self.1, self.2)
    }

    /// Linear RGBA, as mesh vertex colours expect.
    pub fn vertex(self) -> [f32; 4] {
        self.color().to_linear().to_f32_array()
    }

    pub fn egui(self) -> egui::Color32 {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        egui::Color32::from_rgb(c(self.0), c(self.1), c(self.2))
    }

    pub fn mix(self, o: Rgb, t: f32) -> Rgb {
        Rgb(self.0 + (o.0 - self.0) * t, self.1 + (o.1 - self.1) * t, self.2 + (o.2 - self.2) * t)
    }

    pub fn shade(self, f: f32) -> Rgb {
        Rgb((self.0 * f).min(1.0), (self.1 * f).min(1.0), (self.2 * f).min(1.0))
    }
}

pub const GRASS_A: Rgb = Rgb(0.35, 0.49, 0.24);
pub const GRASS_B: Rgb = Rgb(0.45, 0.57, 0.29);
/// Autumn grass and winter snow.
pub const GRASS_AUTUMN: Rgb = Rgb(0.62, 0.54, 0.27);
pub const SNOW: Rgb = Rgb(0.9, 0.92, 0.95);
pub const SOIL: Rgb = Rgb(0.42, 0.3, 0.2);
pub const WATER: Rgb = Rgb(0.2, 0.42, 0.62);
pub const FLOOR: Rgb = Rgb(0.62, 0.47, 0.31);
pub const ROCK_TOP: Rgb = Rgb(0.5, 0.48, 0.46);
pub const ROCK_FACE: Rgb = Rgb(0.36, 0.34, 0.33);
pub const ORE: Rgb = Rgb(0.62, 0.36, 0.22);
/// Copper ore: weathered green with a coppery glint.
pub const COPPER: Rgb = Rgb(0.33, 0.6, 0.5);
pub const WALL_TOP: Rgb = Rgb(0.76, 0.73, 0.68);
pub const WALL_FACE: Rgb = Rgb(0.6, 0.57, 0.53);
pub const STOCKPILE: Rgb = Rgb(0.75, 0.75, 0.95);
/// Tint of safe zones (where civilians shelter in battles).
pub const SAFE: Rgb = Rgb(0.45, 0.9, 0.55);
/// Quarry zones (on the ground still to be dug).
pub const QUARRY: Rgb = Rgb(0.95, 0.7, 0.35);
pub const BLOOD: Rgb = Rgb(0.45, 0.03, 0.04);

pub const WOOD: Rgb = Rgb(0.55, 0.37, 0.2);
pub const METAL: Rgb = Rgb(0.25, 0.26, 0.28);

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

pub struct Look {
    pub skin: Rgb,
    pub hair: Rgb,
    pub clothes: Rgb,
}

const SKINS: [Rgb; 5] = [
    Rgb(0.98, 0.84, 0.7),
    Rgb(0.93, 0.74, 0.58),
    Rgb(0.8, 0.58, 0.42),
    Rgb(0.6, 0.42, 0.3),
    Rgb(0.42, 0.29, 0.21),
];
const HAIRS: [Rgb; 6] = [
    Rgb(0.15, 0.11, 0.09),
    Rgb(0.35, 0.22, 0.12),
    Rgb(0.62, 0.4, 0.2),
    Rgb(0.85, 0.7, 0.4),
    Rgb(0.7, 0.28, 0.14),
    Rgb(0.75, 0.75, 0.75),
];

pub fn pawn_look(p: &Pawn) -> Look {
    let h = |salt: u32| hash(p.id as i32, 17, salt);
    let skin = SKINS[(h(1) * SKINS.len() as f32) as usize % SKINS.len()];
    let look = match p.faction {
        Faction::Colony => Look {
            skin,
            hair: HAIRS[(h(2) * HAIRS.len() as f32) as usize % HAIRS.len()],
            clothes: Rgb(p.color[0], p.color[1], p.color[2]).shade(0.85),
        },
        // Raiders: dark leathers and a steel helmet.
        Faction::Raider => Look { skin, hair: Rgb(0.42, 0.43, 0.46), clothes: Rgb(0.38, 0.17, 0.14) },
    };
    if p.dead {
        let grey = Rgb(0.5, 0.5, 0.5);
        return Look { skin: look.skin.mix(grey, 0.6), hair: look.hair.mix(grey, 0.6), clothes: look.clothes.mix(grey, 0.6) };
    }
    look
}
