//! egui interface: HUD and toolbar, colonist bar, alerts, inspector (with the
//! Health tab), work priorities, help and game over.

use super::camera::{CameraRig, MainCamera};
use super::input::{self, Selection, UiState, TOOLS};
use super::look::{pawn_look, Look};
use super::overlay;
use super::sim::Sim;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, ProgressBar, RichText};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use my_world::health::HediffKind;
use my_world::map::{Feature, ItemKind, Pos, Terrain, Zone};
use my_world::pawn::{Pawn, Skill, WorkType};
use my_world::DAY_LEN;

const HELP: [&str; 19] = [
    "You don't control colonists directly - you give orders and they pick up the work.",
    "",
    "1-3   Designate trees to chop, rock/structures to mine, berry bushes to forage",
    "4/5   Paint stockpile zones (items get hauled there) and grow zones (crops)",
    "6-8   Place walls (4 stone), doors (4 wood), beds (8 wood); colonists haul materials",
    "9/0   Clear zones / cancel designations and blueprints",
    "      Left-drag to paint a rectangle. Right-click goes back to Select.",
    "",
    "Enclosed rooms get a roof (wooden floor). Sleeping in a bed indoors makes colonists happier.",
    "Wounds bleed and can get infected. Doctors (see P) rescue downed colonists to a bed",
    "and tend them - better with medicine and skill. Click a colonist, then Health for details.",
    "Raids come every few days: press F to draft the selected colonist (or everyone).",
    "Drafted colonists shoot enemies in range; right-click to move, right-click an enemy to attack.",
    "",
    "Camera: WASD/arrows pan, Q/E rotate, wheel zooms, right-drag orbits, middle-drag pans.",
    "Space: pause   Tab: speed   P: work priorities   C: center camera",
    "K: save   L: load   Shift+N: new colony",
    "",
    "H or Esc closes this help.",
];

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_ui);
    }
}

fn draw_ui(
    mut contexts: EguiContexts,
    mut sim: ResMut<Sim>,
    mut st: ResMut<UiState>,
    mut rig: ResMut<CameraRig>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let (cam, gt) = *camera;
    overlay::draw(&ctx, &sim.world, cam, gt, rig.distance);

    hud(&ctx, &mut sim, &mut st, &mut rig);
    colonist_bar(&ctx, &sim, &mut st);
    alerts(&ctx, &sim);
    match st.selected {
        Some(Selection::Pawn(id)) => pawn_inspector(&ctx, &sim, &mut st, id),
        Some(Selection::Tile(t)) => tile_inspector(&ctx, &sim, t),
        None => {}
    }
    if st.show_work {
        work_panel(&ctx, &mut sim, &mut st);
    }
    if st.show_help {
        egui::Window::new("Help").open(&mut st.show_help).collapsible(false).resizable(false).anchor(
            Align2::CENTER_CENTER,
            [0.0, 0.0],
        ).show(&ctx, |ui| {
            for line in HELP {
                ui.label(line);
            }
        });
    }
    if sim.world.colony_lost() {
        egui::Area::new(egui::Id::new("game_over")).anchor(Align2::CENTER_CENTER, [0.0, -60.0]).show(&ctx, |ui| {
            ui.label(RichText::new("Your colony has perished. Press N to start over.").size(30.0).color(Color32::RED));
        });
    }
    Ok(())
}

fn mood_color(mood: f32) -> Color32 {
    if mood < 25.0 {
        Color32::from_rgb(220, 60, 50)
    } else if mood < 45.0 {
        Color32::from_rgb(230, 150, 40)
    } else {
        Color32::from_rgb(80, 180, 80)
    }
}

fn capacity_color(v: f32) -> Color32 {
    if v < 0.5 {
        Color32::from_rgb(220, 60, 50)
    } else if v < 0.85 {
        Color32::from_rgb(230, 150, 40)
    } else {
        Color32::from_rgb(80, 180, 80)
    }
}

fn portrait(ui: &mut egui::Ui, look: &Look, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let c = rect.center();
    let p = ui.painter();
    p.circle_filled(c, size * 0.45, look.hair.egui());
    p.circle_filled(c + egui::vec2(0.0, size * 0.14), size * 0.33, look.skin.egui());
}

fn hud(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, rig: &mut CameraRig) {
    egui::Area::new(egui::Id::new("hud")).anchor(Align2::LEFT_BOTTOM, [8.0, -8.0]).show(ctx, |ui| {
        egui::Frame::window(&ctx.global_style()).show(ui, |ui| {
            let w = &sim.world;
            let hour = w.hour();
            let clock = format!("Day {}  {:02}:{:02}", w.day(), hour as u32, ((hour - hour.floor()) * 60.0) as u32);
            let stock: Vec<String> =
                ItemKind::ALL.iter().map(|k| format!("{} {}", k.name(), w.map.count_items(*k))).collect();
            let colonists = format!("Colonists {}", w.colonists_alive());
            ui.horizontal(|ui| {
                ui.label(RichText::new(clock).strong());
                ui.separator();
                for s in stock {
                    ui.label(s);
                }
                ui.separator();
                ui.label(colonists);
                ui.separator();
                let pause = if sim.paused { "Resume (Space)" } else { "Pause (Space)" };
                if ui.button(pause).clicked() {
                    sim.paused = !sim.paused;
                }
                if ui.button(format!("Speed {}x (Tab)", sim.speed)).clicked() {
                    sim.cycle_speed();
                }
            });
            ui.horizontal(|ui| {
                for (tool, label, _) in TOOLS {
                    if ui.add(egui::Button::new(label).selected(st.tool == tool)).clicked() {
                        st.tool = tool;
                        st.drag_start = None;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new("Work (P)").selected(st.show_work)).clicked() {
                    st.show_work = !st.show_work;
                }
                if ui.add(egui::Button::new("Help (H)").selected(st.show_help)).clicked() {
                    st.show_help = !st.show_help;
                }
                if ui.button("Draft (F)").clicked() {
                    input::toggle_draft(sim, st);
                }
                if ui.button("Center (C)").clicked() {
                    input::center_on_selection(sim, st, rig);
                }
                if ui.button("Save (K)").clicked() {
                    input::save(sim);
                }
                if ui.button("Load (L)").clicked() {
                    input::load(sim, st);
                }
            });
            ui.add_space(2.0);
            for (k, msg) in sim.world.log.iter().rev().take(3).enumerate() {
                let alpha = 255 - k as u8 * 60;
                ui.label(RichText::new(msg).small().color(Color32::from_rgba_unmultiplied(225, 225, 170, alpha)));
            }
        });
    });
}

fn colonist_bar(ctx: &egui::Context, sim: &Sim, st: &mut UiState) {
    egui::Area::new(egui::Id::new("colonists")).anchor(Align2::LEFT_TOP, [8.0, 8.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for p in sim.world.pawns.iter().filter(|p| p.is_colonist()) {
                let selected = st.selected == Some(Selection::Pawn(p.id));
                let trouble = p.downed || p.hunger <= 0.0 || p.break_timer > 0.0 || p.health.bleed_rate() > 0.0;
                let stroke = if selected {
                    egui::Stroke::new(2.0, Color32::WHITE)
                } else if p.drafted {
                    egui::Stroke::new(2.0, Color32::GOLD)
                } else if trouble && !p.dead {
                    egui::Stroke::new(2.0, Color32::RED)
                } else {
                    egui::Stroke::new(1.0, Color32::from_gray(90))
                };
                let resp = egui::Frame::window(&ctx.global_style())
                    .stroke(stroke)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.set_width(118.0);
                        ui.horizontal(|ui| {
                            portrait(ui, &pawn_look(p), 18.0);
                            ui.label(RichText::new(&p.name).strong());
                        });
                        ui.add(ProgressBar::new(p.mood / 100.0).desired_width(112.0).desired_height(6.0).fill(mood_color(p.mood)));
                        ui.label(RichText::new(p.status()).small());
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.clicked() {
                    st.selected = Some(Selection::Pawn(p.id));
                }
            }
        });
    });
}

fn alerts(ctx: &egui::Context, sim: &Sim) {
    let alerts = sim.world.alerts();
    if alerts.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("alerts")).anchor(Align2::RIGHT_TOP, [-8.0, 8.0]).show(ctx, |ui| {
        egui::Frame::window(&ctx.global_style()).show(ui, |ui| {
            for (text, urgent) in alerts {
                let color = if urgent { Color32::from_rgb(255, 90, 80) } else { Color32::from_rgb(240, 210, 90) };
                ui.label(RichText::new(text).color(color));
            }
        });
    });
}

fn pawn_inspector(ctx: &egui::Context, sim: &Sim, st: &mut UiState, id: u32) {
    let Some(p) = sim.world.pawn(id) else { return };
    let title = if p.is_colonist() { p.name.clone() } else { format!("{} (raider)", p.name) };
    egui::Window::new(RichText::new(title).strong())
        .id(egui::Id::new("inspector"))
        .anchor(Align2::LEFT_TOP, [8.0, 92.0])
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut st.inspector_health, false, "Overview");
                ui.selectable_value(&mut st.inspector_health, true, "Health");
            });
            ui.label(RichText::new(format!("{} - {}", p.status(), p.weapon.name())).weak());
            ui.separator();
            if st.inspector_health {
                health_tab(ui, p);
            } else {
                overview_tab(ui, p);
            }
        });
}

fn overview_tab(ui: &mut egui::Ui, p: &Pawn) {
    let need = |ui: &mut egui::Ui, label: &str, v: f32, fill: Color32| {
        ui.horizontal(|ui| {
            ui.add_sized([60.0, 16.0], egui::Label::new(label));
            ui.add(ProgressBar::new(v / 100.0).desired_width(220.0).fill(fill));
        });
    };
    need(ui, "Health", p.health.summary() * 100.0, Color32::from_rgb(210, 60, 60));
    if p.is_colonist() {
        need(ui, "Mood", p.mood, mood_color(p.mood));
        need(ui, "Food", p.hunger, Color32::from_rgb(230, 150, 40));
        need(ui, "Rest", p.rest, Color32::from_rgb(110, 170, 230));
        ui.separator();
        egui::Grid::new("skills").num_columns(4).spacing([12.0, 2.0]).show(ui, |ui| {
            for (k, s) in Skill::ALL.iter().enumerate() {
                ui.label(s.name());
                ui.label(RichText::new(p.skill(*s).to_string()).strong());
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.separator();
    }
    for (label, value) in p.thoughts() {
        let color = if value >= 0.0 { Color32::from_rgb(110, 200, 110) } else { Color32::from_rgb(255, 120, 100) };
        ui.label(RichText::new(format!("{:+}  {}", value as i32, label)).color(color));
    }
    if p.is_colonist() && !p.dead {
        let hint = if p.drafted { "F: undraft   right-click: move / attack" } else { "F: draft" };
        ui.label(RichText::new(hint).small().weak());
    }
}

/// RimWorld-style health tab: overall state, capacities, and every condition.
fn health_tab(ui: &mut egui::Ui, p: &Pawn) {
    let h = &p.health;
    let pain = match h.pain {
        v if v >= 0.6 => "mind-shattering",
        v if v >= 0.4 => "intense",
        v if v >= 0.2 => "serious",
        v if v >= 0.05 => "minor",
        _ => "none",
    };
    ui.label(RichText::new(format!("Overall {:.0}%     Pain {:.0}% ({})", h.summary() * 100.0, h.pain * 100.0, pain)).strong());
    let bleed = h.bleed_rate();
    match h.time_to_bleed_out() {
        Some(secs) => {
            let text = format!("Bleeding {:.0}%/day - bleeds out in ~{:.0}h", bleed * 100.0, secs / DAY_LEN * 24.0);
            ui.label(RichText::new(text).color(Color32::from_rgb(255, 80, 70)));
        }
        None if bleed > 0.0 => {
            let text = format!("Bleeding {:.0}%/day (body keeps up)", bleed * 100.0);
            ui.label(RichText::new(text).color(Color32::from_rgb(240, 160, 60)));
        }
        None => {
            ui.label(RichText::new("Not bleeding").weak());
        }
    }
    if h.blood_loss > 0.01 || h.malnutrition > 0.01 {
        let text = format!("Blood loss {:.0}%     Malnutrition {:.0}%", h.blood_loss * 100.0, h.malnutrition * 100.0);
        ui.label(RichText::new(text).color(Color32::from_rgb(240, 160, 60)));
    }
    ui.separator();
    egui::Grid::new("capacities").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
        for (name, v) in h.caps.list() {
            ui.label(name);
            ui.add(
                ProgressBar::new(v.clamp(0.0, 1.0))
                    .desired_width(180.0)
                    .fill(capacity_color(v))
                    .text(format!("{:.0}%", v * 100.0)),
            );
            ui.end_row();
        }
    });
    ui.separator();
    if h.hediffs.is_empty() {
        ui.label(RichText::new("No injuries or illnesses").color(Color32::from_rgb(110, 200, 110)));
        return;
    }
    for d in &h.hediffs {
        let part = d.part.map_or("Whole body", |p| p.name());
        let (detail, color) = match d.kind {
            HediffKind::Injury { permanent: true, .. } => (String::new(), Color32::GRAY),
            HediffKind::Injury { .. } => (
                format!(" {:.0}", d.severity),
                if d.bleed() > 0.0 { Color32::from_rgb(255, 80, 70) } else { Color32::from_rgb(240, 160, 60) },
            ),
            HediffKind::Missing => {
                (String::new(), if d.bleed() > 0.0 { Color32::from_rgb(255, 80, 70) } else { Color32::GRAY })
            }
            HediffKind::Infection { immunity } | HediffKind::Flu { immunity } => (
                format!(" {:.0}% (immunity {:.0}%)", d.severity * 100.0, immunity * 100.0),
                Color32::from_rgb(240, 220, 90),
            ),
        };
        let mut text = format!("{}: {}{}", part, d.label(), detail);
        if d.bleed() > 0.0 {
            text += ", bleeding";
        }
        match d.tend {
            Some(q) => text += &format!(", tended {:.0}%", q * 100.0),
            None if d.needs_tending() => text += ", needs tending",
            None => {}
        }
        ui.label(RichText::new(text).color(color));
    }
}

fn tile_inspector(ctx: &egui::Context, sim: &Sim, t: Pos) {
    let m = &sim.world.map;
    let i = m.idx(t);
    egui::Window::new(RichText::new(format!("Tile {}, {}", t.0, t.1)).strong())
        .id(egui::Id::new("inspector"))
        .anchor(Align2::LEFT_TOP, [8.0, 92.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let what = match m.feature[i] {
                Feature::Crop { growth } => format!("Crop, {:.0}% grown", growth * 100.0),
                Feature::Blueprint { kind, delivered } => {
                    let c = kind.cost();
                    format!("{} blueprint ({}/{} {})", kind.name(), delivered, c.count, c.kind.name().to_lowercase())
                }
                f => format!("{}{}", f.name(), if m.terrain[i] == Terrain::Water { " (water)" } else { "" }),
            };
            ui.label(what);
            if let Some(s) = m.items[i] {
                ui.label(format!("{} x{}", s.kind.name(), s.count));
            }
            match m.zone[i] {
                Zone::Stockpile => {
                    ui.label("Stockpile zone");
                }
                Zone::Grow => {
                    ui.label("Grow zone");
                }
                Zone::None => {}
            }
            if let Some(d) = m.desig[i] {
                ui.label(format!("Designated: {:?}", d));
            }
            let room = m.room.get(i).copied().unwrap_or(0);
            if room != 0 {
                ui.label(format!("Indoors, roofed room of {} tiles", m.room_sizes[room as usize]));
            } else {
                ui.label("Outdoors");
            }
        });
}

fn work_panel(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState) {
    let rows: Vec<(u32, String, [u8; 6], [u8; 7])> = sim
        .world
        .pawns
        .iter()
        .filter(|p| p.is_colonist() && !p.dead)
        .map(|p| (p.id, p.name.clone(), p.priorities, p.skills))
        .collect();
    egui::Window::new("Work priorities")
        .open(&mut st.show_work)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_TOP, [0.0, 80.0])
        .show(ctx, |ui| {
            ui.label(RichText::new("Click to cycle: 1 = first ... 4 = last, - = never").weak());
            egui::Grid::new("work_grid").striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label("");
                for wt in WorkType::ALL {
                    ui.label(RichText::new(wt.name()).strong());
                }
                ui.end_row();
                for (id, name, prios, skills) in &rows {
                    ui.label(name);
                    for wt in WorkType::ALL {
                        let prio = prios[wt as usize];
                        let label = match (prio, wt.skill()) {
                            (0, _) => "-".to_string(),
                            (p, Some(s)) => format!("{}  (lv {})", p, skills[s as usize]),
                            (p, None) => p.to_string(),
                        };
                        let button = egui::Button::new(label).selected(prio == 1).min_size(egui::vec2(84.0, 22.0));
                        if ui.add(button).clicked() {
                            let next = match prio {
                                0 => 1,
                                4 => 0,
                                p => p + 1,
                            };
                            sim.world.set_priority(*id, wt, next);
                        }
                    }
                    ui.end_row();
                }
            });
        });
}
