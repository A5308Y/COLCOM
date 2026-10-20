//! Labels drawn over the 3D view: names, health and work bars, sleep and
//! bleeding markers, item counts and blueprint progress.

use super::input::UiState;
use super::{tile_center, tile_point};
use super::theme;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2};
use my_world::map::Feature;
use my_world::pawn::{Faction, Job, PawnId};
use my_world::world::World as SimWorld;

fn shadow_text(painter: &egui::Painter, pos: Pos2, align: Align2, text: &str, size: f32, color: Color32) {
    let font = FontId::proportional(size);
    painter.text(pos + egui::vec2(1.0, 1.0), align, text, font.clone(), Color32::from_black_alpha(190));
    painter.text(pos, align, text, font, color);
}

fn bar(painter: &egui::Painter, center: Pos2, width: f32, frac: f32, fill: Color32) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(width, 4.0));
    painter.rect_filled(rect, 1.0, Color32::from_black_alpha(170));
    let mut inner = rect;
    inner.set_width(width * frac.clamp(0.0, 1.0));
    painter.rect_filled(inner, 1.0, fill);
}

/// Anything hidden by the layer view or hidden roofs is skipped (`ui`).
/// `view` is the part of the screen showing the world (not covered by docked panels).
pub fn draw(
    ctx: &egui::Context,
    w: &SimWorld,
    cam: &Camera,
    gt: &GlobalTransform,
    distance: f32,
    ui: &UiState,
    view: egui::Rect,
) {
    let painter = ctx
        .layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("world_overlay")))
        .with_clip_rect(view);
    let screen = view;
    let to_screen = |p: Vec3| {
        cam.world_to_viewport(gt, p).ok().map(|v| Pos2::new(v.x, v.y)).filter(|p| screen.contains(*p))
    };
    let m = &w.map;

    // Themed rooms: their name over the middle of the floor, in the floor's colour.
    if distance < 70.0 {
        for (st, kind) in w.room_stats.iter().filter_map(|s| Some((s, s.kind?))) {
            let (cx, cy) = (st.centre.0.round() as usize, st.centre.1.round() as usize);
            if st.level > ui.top_at(m, cx.min(m.w - 1), cy.min(m.h - 1)) {
                continue;
            }
            let Some(at) = to_screen(tile_point(st.centre.0, st.centre.1, st.level as f32) + Vec3::Y * 0.15) else { continue };
            let room = kind.def();
            let (r, g, b) = room.floor;
            let c = Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);
            let light = c.lerp_to_gamma(Color32::WHITE, 0.55);
            shadow_text(&painter, at, Align2::CENTER_CENTER, &room.name, 16.0, light);
        }
    }

    // In a battle, squads show their members in one status row (`battle_ui.rs`)
    // instead of labels stacked on one square.
    let in_squad = |id: PawnId| {
        w.battle.as_ref().is_some_and(|b| b.units.iter().any(|u| u.roster.len() > 1 && u.roster.contains(&id)))
    };
    for p in w.pawns.iter().filter(|p| !p.has_left() && !p.is_buried() && ui.shows_at(m, p.x, p.y, p.z) && !in_squad(p.id)) {
        let base = tile_point(p.x, p.y, p.z);
        let lying = p.is_dead() || p.lying();
        let Some(top) = to_screen(base + Vec3::Y * if lying { 0.7 } else { 1.15 }) else { continue };
        if !p.is_dead() {
            let mut y = top.y;
            let health = p.health.summary();
            if health < 0.99 {
                bar(&painter, Pos2::new(top.x, y), 30.0, health, theme::RED);
                y -= 6.0;
            }
            if let Some(Job::Work { .. } | Job::Tend { .. }) = p.job {
                if p.job_progress > 0.0 {
                    bar(&painter, Pos2::new(top.x, y), 30.0, p.job_progress, theme::GREEN);
                }
            }
            if p.health.bleed_rate() > 0.0 {
                let pulse = 0.7 + 0.3 * (ctx.input(|i| i.time) as f32 * 5.0).sin();
                painter.circle_filled(Pos2::new(top.x - 20.0, top.y), 4.0 * pulse, theme::RED);
            }
            if p.sleeping {
                shadow_text(&painter, Pos2::new(top.x + 14.0, top.y - 4.0), Align2::LEFT_BOTTOM, "z", 16.0, Color32::WHITE);
            }
        }
        if distance < 45.0 {
            if let Some(feet) = to_screen(base - Vec3::Y * 0.05) {
                let color = if p.is_dead() {
                    Color32::GRAY
                } else if p.faction == Faction::Raider {
                    theme::RED
                } else {
                    Color32::WHITE
                };
                shadow_text(&painter, feet + egui::vec2(0.0, 4.0), Align2::CENTER_TOP, &p.name, 13.0, color);
            }
        }
    }

    if distance < 24.0 {
        for i in 0..m.len() {
            let pos = m.pos(i);
            if pos.2 > ui.top_at(m, pos.0, pos.1) {
                continue;
            }
            let c = tile_center(pos) + Vec3::Y * 0.45;
            if m.has_items(i) {
                // All of it (a shelf's stacks together).
                let total: u32 = m.stacks(i).map(|s| s.count).sum();
                if let Some(sp) = to_screen(c) {
                    shadow_text(&painter, sp, Align2::CENTER_BOTTOM, &total.to_string(), 12.0, Color32::WHITE);
                }
            }
            // Roofs are many cheap cells: their counts would only clutter the view.
            if let Feature::Blueprint { kind, delivered, .. } = m.feature[i] {
                if m.roof_blueprint(i) {
                    continue;
                }
                if let Some(sp) = to_screen(c + Vec3::Y * 0.9) {
                    let label = format!("{}/{}", delivered, kind.def().total_cost());
                    shadow_text(&painter, sp, Align2::CENTER_BOTTOM, &label, 12.0, theme::TEAL);
                }
            }
        }
    }
}
