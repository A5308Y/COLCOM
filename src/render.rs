//! Drawing the map, pawns and effects.

use crate::camera::{View, TS};
use macroquad::prelude::*;
use my_world::map::{Desig, Feature, ItemKind, Map, Pos, Terrain, Zone};
use my_world::pawn::{Faction, Job, Pawn};
use my_world::world::World;

pub fn item_color(k: ItemKind) -> Color {
    match k {
        ItemKind::Wood => Color::new(0.6, 0.4, 0.2, 1.0),
        ItemKind::Stone => Color::new(0.68, 0.68, 0.72, 1.0),
        ItemKind::Food => Color::new(0.88, 0.32, 0.24, 1.0),
    }
}

pub fn rgb(c: [f32; 3]) -> Color {
    Color::new(c[0], c[1], c[2], 1.0)
}

pub fn draw_world(w: &World, v: &View, view_h: f32, selected: Option<u32>, drag: Option<(Pos, Pos)>) {
    let m = &w.map;
    let ts = v.ts();
    let x0 = (v.x / TS).floor().max(0.0) as usize;
    let y0 = (v.y / TS).floor().max(0.0) as usize;
    let x1 = ((((v.x + screen_width() / v.zoom) / TS).ceil().max(0.0)) as usize + 1).min(m.w);
    let y1 = ((((v.y + view_h / v.zoom) / TS).ceil().max(0.0)) as usize + 1).min(m.h);
    for y in y0..y1 {
        for x in x0..x1 {
            draw_tile(m, v, x, y, ts);
        }
    }

    for p in w.pawns.iter().filter(|p| p.dead) {
        draw_pawn(p, v, ts, selected == Some(p.id));
    }
    for p in w.pawns.iter().filter(|p| !p.dead && !p.left) {
        draw_pawn(p, v, ts, selected == Some(p.id));
    }

    let dark = (1.0 - w.daylight()) * 0.55;
    draw_rectangle(0.0, 0.0, screen_width(), view_h, Color::new(0.02, 0.02, 0.12, dark));

    for e in &w.effects {
        let (ax, ay) = v.tile_to_screen(e.from.0 + 0.5, e.from.1 + 0.5);
        let (bx, by) = v.tile_to_screen(e.to.0 + 0.5, e.to.1 + 0.5);
        let color = if e.hit { Color::new(1.0, 0.9, 0.4, 0.95) } else { Color::new(0.85, 0.85, 0.85, 0.6) };
        draw_line(ax, ay, bx, by, 1.5, color);
    }

    if let Some((a, b)) = drag {
        let (lx, ly) = (a.0.min(b.0), a.1.min(b.1));
        let (hx, hy) = (a.0.max(b.0), a.1.max(b.1));
        let (sx, sy) = v.tile_to_screen(lx as f32, ly as f32);
        let (rw, rh) = ((hx - lx + 1) as f32 * ts, (hy - ly + 1) as f32 * ts);
        draw_rectangle(sx, sy, rw, rh, Color::new(1.0, 1.0, 1.0, 0.12));
        draw_rectangle_lines(sx, sy, rw, rh, 2.0, WHITE);
        let label = format!("{}x{}", hx - lx + 1, hy - ly + 1);
        draw_text(&label, sx + 4.0, sy - 4.0, 18.0, WHITE);
    }
}

fn draw_tile(m: &Map, v: &View, x: usize, y: usize, ts: f32) {
    let i = m.idx((x, y));
    let (sx, sy) = v.tile_to_screen(x as f32, y as f32);
    let var = ((x * 31 + y * 17) % 5) as f32 * 0.012;
    let indoors = m.room.get(i).map_or(false, |&r| r != 0);
    let base = match m.terrain[i] {
        Terrain::Water => Color::new(0.16, 0.36 + var, 0.62 + var, 1.0),
        Terrain::Grass if indoors => Color::new(0.46 + var, 0.37, 0.26, 1.0),
        Terrain::Grass => Color::new(0.31 + var, 0.46 + var, 0.24, 1.0),
    };
    draw_rectangle(sx, sy, ts + 0.5, ts + 0.5, base);

    match m.zone[i] {
        Zone::Stockpile => draw_rectangle(sx, sy, ts, ts, Color::new(0.75, 0.75, 0.95, 0.18)),
        Zone::Grow => draw_rectangle(sx, sy, ts, ts, Color::new(0.9, 0.85, 0.3, 0.16)),
        Zone::None => {}
    }

    let (cx, cy) = (sx + ts / 2.0, sy + ts / 2.0);
    match m.feature[i] {
        Feature::None => {}
        Feature::Tree => {
            draw_rectangle(cx - ts * 0.06, cy, ts * 0.12, ts * 0.45, Color::new(0.4, 0.27, 0.15, 1.0));
            draw_circle(cx, cy - ts * 0.08, ts * 0.38, Color::new(0.12, 0.33 + var, 0.14, 1.0));
        }
        Feature::Bush => {
            draw_circle(cx, cy, ts * 0.32, Color::new(0.22, 0.43, 0.2, 1.0));
            for (ox, oy) in [(-0.14, -0.08), (0.12, -0.04), (0.0, 0.14)] {
                draw_circle(cx + ts * ox, cy + ts * oy, ts * 0.07, Color::new(0.8, 0.15, 0.25, 1.0));
            }
        }
        Feature::Rock => {
            let g = 0.4 + var * 2.0;
            draw_rectangle(sx, sy, ts + 0.5, ts + 0.5, Color::new(g, g, g + 0.03, 1.0));
            draw_rectangle(sx + ts * 0.15, sy + ts * 0.15, ts * 0.35, ts * 0.25, Color::new(g + 0.08, g + 0.08, g + 0.1, 1.0));
        }
        Feature::Wall => {
            draw_rectangle(sx, sy, ts + 0.5, ts + 0.5, Color::new(0.55, 0.55, 0.58, 1.0));
            draw_rectangle_lines(sx, sy, ts, ts, 1.5, Color::new(0.3, 0.3, 0.33, 1.0));
        }
        Feature::Door => {
            draw_rectangle(sx + ts * 0.1, sy + ts * 0.1, ts * 0.8, ts * 0.8, Color::new(0.55, 0.38, 0.2, 1.0));
            draw_rectangle_lines(sx, sy, ts, ts, 2.0, Color::new(0.3, 0.3, 0.33, 1.0));
        }
        Feature::Bed => {
            draw_rectangle(sx + ts * 0.12, sy + ts * 0.08, ts * 0.76, ts * 0.84, Color::new(0.25, 0.4, 0.75, 1.0));
            draw_rectangle(sx + ts * 0.2, sy + ts * 0.12, ts * 0.6, ts * 0.22, Color::new(0.92, 0.92, 0.92, 1.0));
        }
        Feature::Crop { growth } => {
            let color = if growth >= 1.0 { Color::new(0.9, 0.78, 0.25, 1.0) } else { Color::new(0.35, 0.75, 0.3, 1.0) };
            let r = ts * (0.08 + 0.12 * growth);
            for (ox, oy) in [(-0.22, -0.22), (0.22, -0.22), (-0.22, 0.22), (0.22, 0.22)] {
                draw_circle(cx + ts * ox, cy + ts * oy, r, color);
            }
        }
        Feature::Blueprint { kind, delivered } => {
            let blue = Color::new(0.35, 0.65, 1.0, 1.0);
            draw_rectangle(sx + 1.0, sy + 1.0, ts - 2.0, ts - 2.0, Color::new(0.35, 0.65, 1.0, 0.25));
            draw_rectangle_lines(sx + 1.0, sy + 1.0, ts - 2.0, ts - 2.0, 1.0, blue);
            if ts >= 20.0 {
                let label = format!("{}/{}", delivered, kind.cost().count);
                draw_text(&label, sx + 2.0, sy + ts - 3.0, ts * 0.5, WHITE);
            }
        }
    }

    if let Some(d) = m.desig[i] {
        let color = match d {
            Desig::Chop => ORANGE,
            Desig::Mine => SKYBLUE,
            Desig::Forage => PINK,
        };
        draw_rectangle_lines(sx + 1.0, sy + 1.0, ts - 2.0, ts - 2.0, 2.0, color);
    }

    if let Some(s) = m.items[i] {
        draw_rectangle(sx + ts * 0.2, sy + ts * 0.2, ts * 0.6, ts * 0.6, item_color(s.kind));
        draw_rectangle_lines(sx + ts * 0.2, sy + ts * 0.2, ts * 0.6, ts * 0.6, 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
        if ts >= 20.0 {
            draw_text(&s.count.to_string(), sx + 2.0, sy + ts - 2.0, ts * 0.55, WHITE);
        }
    }
}

fn draw_pawn(p: &Pawn, v: &View, ts: f32, selected: bool) {
    let (cx, cy) = v.tile_to_screen(p.x + 0.5, p.y + 0.5);
    let r = ts * 0.38;
    if p.dead {
        let c = Color::new(0.35, 0.35, 0.35, 1.0);
        draw_line(cx - r, cy - r, cx + r, cy + r, 2.0, c);
        draw_line(cx - r, cy + r, cx + r, cy - r, 2.0, c);
        if selected {
            draw_circle_lines(cx, cy, r + 5.0, 1.5, WHITE);
        }
        return;
    }
    let body = if p.faction == Faction::Raider { Color::new(0.85, 0.2, 0.2, 1.0) } else { rgb(p.color) };
    if p.downed || p.sleeping {
        draw_rectangle(cx - r, cy - r * 0.5, r * 2.0, r, body);
        draw_rectangle_lines(cx - r, cy - r * 0.5, r * 2.0, r, 1.5, BLACK);
    } else {
        draw_circle(cx, cy, r, body);
        draw_circle_lines(cx, cy, r, 1.5, BLACK);
        if p.faction == Faction::Raider {
            draw_line(cx - r * 0.5, cy - r * 0.2, cx + r * 0.5, cy - r * 0.2, 2.0, BLACK);
        }
    }
    if p.drafted {
        draw_circle_lines(cx, cy, r + 3.0, 2.0, GOLD);
    }
    if selected {
        draw_circle_lines(cx, cy, r + 6.0, 1.5, WHITE);
    }
    if let Some(s) = p.carrying {
        draw_rectangle(cx + r * 0.2, cy, r * 0.8, r * 0.8, item_color(s.kind));
    }
    if p.sleeping {
        draw_text("z", cx + r, cy - r, ts * 0.9, WHITE);
    }
    if p.health < 99.0 {
        draw_rectangle(cx - r, cy - r - 6.0, r * 2.0, 3.0, Color::new(0.2, 0.0, 0.0, 1.0));
        draw_rectangle(cx - r, cy - r - 6.0, r * 2.0 * (p.health / 100.0).max(0.0), 3.0, RED);
    }
    if let Some(Job::Work { .. }) = p.job {
        if p.job_progress > 0.0 {
            draw_rectangle(cx - r, cy + r + 3.0, r * 2.0, 3.0, DARKGRAY);
            draw_rectangle(cx - r, cy + r + 3.0, r * 2.0 * p.job_progress.min(1.0), 3.0, GREEN);
        }
    }
    if ts >= 14.0 {
        let d = measure_text(&p.name, None, 14, 1.0);
        draw_text(&p.name, cx - d.width / 2.0, cy + r + 16.0, 14.0, WHITE);
    }
}
