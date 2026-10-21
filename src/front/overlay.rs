//! Labels drawn over the 3D view: names, health and work bars, sleep and
//! bleeding markers, item counts and blueprint progress.

use super::input::UiState;
use super::sim::Sim;
use super::{tile_center, tile_point};
use super::theme;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2};
use colcom::map::{Feature, Pos};
use colcom::pawn::{Faction, Job, PawnId};
use colcom::world::World as SimWorld;

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

#[derive(Default)]
pub struct MapLabels {
    key: Option<(u64, u64)>,
    labels: Vec<(Pos, f32, String, Color32)>,
}

impl MapLabels {
    fn update(&mut self, w: &SimWorld, generation: u64) {
        let m = &w.map;
        let key = (generation, m.changes());
        if self.key == Some(key) { return; }
        self.labels.clear();
        for i in 0..m.len() {
            let pos = m.pos(i);
            if m.has_items(i) {
                let total: u32 = m.stacks(i).map(|s| s.count).sum();
                self.labels.push((pos, 0.45, total.to_string(), Color32::WHITE));
            }
            if let Feature::Blueprint { kind, delivered, .. } = m.feature[i] {
                if !m.roof_blueprint(i) {
                    self.labels.push((pos, 1.35, format!("{}/{}", delivered, kind.def().total_cost()), theme::TEAL));
                }
            }
        }
        self.key = Some(key);
    }
}

/// Anything hidden by the layer view or hidden roofs is skipped (`ui`).
/// `view` is the part of the screen showing the world (not covered by docked panels).
pub fn draw(
    ctx: &egui::Context,
    sim: &Sim,
    camera: (&Camera, &GlobalTransform),
    distance: f32,
    ui: &UiState,
    view: egui::Rect,
    labels: &mut MapLabels,
) {
    let w = &sim.world;
    let (cam, gt) = camera;
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
        for (st, kind) in w.derived.room_stats.iter().filter_map(|s| Some((s, s.kind?))) {
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
        labels.update(w, sim.generation);
        for (pos, lift, label, color) in &labels.labels {
            if pos.2 > ui.top_at(m, pos.0, pos.1) { continue; }
            if let Some(sp) = to_screen(tile_center(*pos) + Vec3::Y * *lift) {
                shadow_text(&painter, sp, Align2::CENTER_BOTTOM, label, 12.0, *color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colcom::defs::{BuildId, ItemId};
    use colcom::map::{Map, Stack, Dir, FLAT_Z};
    use colcom::rng::Rng;

    #[test]
    fn labels_update_after_pickup_delivery_and_world_replacement() {
        let mut w = SimWorld::with_map(Map::flat(5, 5), Rng::new(1));
        let at = (2, 2, FLAT_Z);
        let i = w.map.idx(at);
        let item = ItemId::named("wood");
        w.map.drop_item(at, Stack { kind: item, count: 10 });
        let mut labels = MapLabels::default();
        labels.update(&w, 1);
        assert_eq!(labels.labels[0].2, "10");
        w.map.take_item(i, item, 4);
        labels.update(&w, 1);
        assert_eq!(labels.labels[0].2, "6");
        let kind = BuildId::named("wall");
        w.map.set_feature(i, Feature::blueprint(kind, 0, Dir::PlusX));
        labels.update(&w, 1);
        assert_eq!(labels.labels.len(), 2);
        w.map.set_feature(i, Feature::blueprint(kind, 1, Dir::PlusX));
        labels.update(&w, 1);
        assert!(labels.labels[1].2.starts_with("1/"));
        let replacement = SimWorld::with_map(Map::flat(5, 5), Rng::new(2));
        labels.update(&replacement, 2);
        assert!(labels.labels.is_empty());
    }
}
