//! egui interface: HUD and toolbar, colonist bar, alerts, inspector (with the
//! Health tab), work priorities and game over (the handbook is in `help`).

use super::camera::{CameraRig, MainCamera};
use super::audio::{self, AudioSettings, SoundQueue};
use super::build_menu;
use super::help;
use super::info::{self, InfoTarget};
use super::input::{self, InspectorTab, Selection, UiState};
use super::expeditions_ui;
use super::squads_ui;
use super::theme;
use super::look::{pawn_look, Look, Rgb};
use super::overlay;
use super::sim::{Clock, Sim};
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use my_world::health::{CyberSlot, HediffKind};
use my_world::decor::DecorState;
use my_world::defs::{defs, ItemCategory, ItemId, PlantId, RoomDef, RoomId, Slot, Special};
use my_world::map::{Block, Feature, Pos, StockpileId, Zone};
use my_world::pawn::{Pawn, PawnId, PerSkill, PerWork, Role, Skill, WorkPriority, WorkType};
use my_world::production::{BillMode, StorePriority};
use my_world::rooms::{Progress, RoomStats, TIERS};
use my_world::world::{Order, World as SimWorld};
use my_world::DAY_LEN;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PreUpdate,
            install_theme.after(bevy_egui::EguiPreUpdateSet::InitContexts).before(bevy_egui::EguiPreUpdateSet::BeginPass),
        )
        .add_systems(EguiPrimaryContextPass, draw_ui);
    }
}

/// Fonts and style before egui's first pass (fonts set in a pass apply only from the next one).
fn install_theme(mut contexts: EguiContexts) -> Result {
    theme::install(contexts.ctx_mut()?);
    Ok(())
}

#[expect(clippy::too_many_arguments, reason = "Bevy injects the UI contexts, resources and camera separately")]
fn draw_ui(
    mut contexts: EguiContexts,
    mut labels: Local<overlay::MapLabels>,
    mut notices: Local<NotificationState>,
    mut sim: ResMut<Sim>,
    mut st: ResMut<UiState>,
    mut rig: ResMut<CameraRig>,
    mut audio: ResMut<AudioSettings>,
    mut sounds: ResMut<SoundQueue>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let (cam, gt) = *camera;
    // The build menu first: everything else keeps to the right of it.
    // Nothing to build during a raid: the build menu makes way for the battle.
    let raid = sim.world.battle.is_some() || sim.world.parley.is_some();
    let left = if raid { 0.0 } else { build_menu::draw(&ctx, &sim, &mut st) };
    st.dock_width = left;
    let mut view = ctx.viewport_rect();
    view.min.x += left;
    overlay::draw(&ctx, &sim, (cam, gt), rig.distance, &st, view, &mut labels);
    // The selection box while dragging with the Select tool.
    if let (Some(a), Some(b)) = (st.box_start, ctx.pointer_latest_pos()) {
        let a = egui::pos2(a.x, a.y);
        if a.distance(b) > 6.0 {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("select_box")));
            let rect = egui::Rect::from_two_pos(a, b);
            painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(228, 164, 62, 18));
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, theme::AMBER), egui::StrokeKind::Inside);
        }
    }

    tool_at_cursor(&ctx, &st, &sim.world);
    job_menu(&ctx, &mut sim, &mut st, &mut sounds);
    hud(&ctx, &mut sim, &mut st, &mut rig, left);
    colonist_bar(&ctx, &sim, &mut st, &mut rig, left);
    // The battle panel takes the top right, with its own events.
    if sim.world.battle.is_none() {
        notifications(&ctx, &sim, &mut notices);
    }
    match st.selected {
        Some(Selection::Pawn(id)) => pawn_inspector(&ctx, &mut sim, &mut st, id, left),
        Some(Selection::Tile(t)) => {
            let mut move_from = None;
            let mut look_up = None;
            let (changed, open) = tile_inspector(&ctx, &mut sim, t, left, &mut move_from, &mut look_up);
            if let Some(it) = look_up {
                st.open_info(it);
            }
            if changed {
                sounds.play("ui_click", None);
            }
            if let Some(from) = move_from {
                st.start_move(from, &sim.world);
                sounds.play("ui_click", None);
            }
            if !open {
                st.select(None);
            }
        }
        None => {}
    }
    if st.show_work {
        work_panel(&ctx, &mut sim, &mut st);
    }
    if st.show_resources {
        resources(&ctx, &sim, &mut st);
    }
    if st.show_squads {
        squads_ui::squads_window(&ctx, &sim, &mut st);
    }
    if st.expeditions.open {
        expeditions_ui::window(&ctx, &mut sim, &mut st.expeditions);
    }
    if st.show_rooms {
        rooms_window(&ctx, &sim, &mut st, &mut rig);
    }
    if st.show_spawn {
        spawn_window(&ctx, &mut st);
    }
    info::window(&ctx, &mut st);
    if st.show_menu {
        game_menu(&ctx, &mut sim, &mut st, &mut rig, &mut audio);
    }
    help::window(&ctx, &mut st);
    if sim.lost() {
        egui::Area::new(egui::Id::new("game_over")).anchor(Align2::CENTER_CENTER, [0.0, -60.0]).show(&ctx, |ui| {
            theme::plate(ui, |ui| {
                ui.label(theme::caps("Your colony has perished", 34.0).color(theme::RED));
                ui.label(RichText::new("Press N to start over.").color(theme::CREAM));
            });
        });
    }
    Ok(())
}

/// The right-click menu: what the selected colonist could do there. Picking
/// one has them do it now; clicking elsewhere or Esc (`input::keyboard`) closes it.
fn job_menu(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, sounds: &mut SoundQueue) {
    let Some(menu) = st.job_menu.as_mut() else { return };
    let at = *menu.at.get_or_insert_with(|| ctx.pointer_latest_pos().unwrap_or_default() + egui::vec2(6.0, 6.0));
    let name = sim.world.pawn(menu.pawn).map_or("", |p| p.name.as_str()).to_string();
    let mut chosen = None;
    let area = egui::Area::new(egui::Id::new("job_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        theme::Plate::panel().margin(6).show(ui, |ui| {
            ui.label(theme::caps(name, 13.0).color(theme::CREAM_DIM));
            for (k, (label, _, _)) in menu.options.iter().enumerate() {
                if ui.button(label).clicked() {
                    chosen = Some(k);
                }
            }
        });
    });
    let clicked_away = ctx.input(|i| i.pointer.any_pressed())
        && ctx.pointer_latest_pos().is_some_and(|p| !area.response.rect.contains(p));
    if let Some(k) = chosen {
        let (_, cell, job) = menu.options[k];
        if sim.world.prioritize(menu.pawn, cell, job) {
            sounds.play("ui_place", None);
        }
        st.job_menu = None;
    } else if clicked_away {
        st.job_menu = None;
    }
}

/// With a tool in hand, its name by the cursor, so it's clear clicks give
/// orders rather than inspect.
fn tool_at_cursor(ctx: &egui::Context, st: &UiState, w: &SimWorld) {
    let name = match st.tool {
        input::Tool::Select => return,
        input::Tool::Move(None) => "Move furniture: which?",
        input::Tool::Move(Some(_)) => "Move furniture: where to?",
        input::Tool::Spawn(_) => "Drop items (testing)",
        tool => input::TOOLS.iter().find(|e| e.tool == tool).map_or("Tool", |e| e.name.as_str()),
    };
    let Some(at) = ctx.pointer_hover_pos() else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tool_at_cursor")));
    let pos = at + egui::vec2(16.0, 14.0);
    let font = egui::FontId::proportional(13.0);
    let turn = st.turning(w).is_some();
    let text = if turn { format!("{}  (, . turn   right-click: stop)", name) } else { format!("{}  (right-click: stop)", name) };
    let galley = painter.layout_no_wrap(text, font, theme::AMBER);
    let rect = egui::Rect::from_min_size(pos, galley.size() + egui::vec2(10.0, 4.0));
    painter.rect_filled(rect, 0.0, theme::PANEL_DEEP.gamma_multiply(0.92));
    painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, theme::OLIVE), egui::StrokeKind::Inside);
    painter.galley(pos + egui::vec2(5.0, 2.0), galley, theme::CREAM_BRIGHT);
}

fn room_color(room: &RoomDef) -> Color32 {
    let (r, g, b) = room.floor;
    Rgb(r, g, b).egui().lerp_to_gamma(Color32::WHITE, 0.45)
}

const MET: Color32 = theme::GREEN;
const UNMET: Color32 = theme::RED;
const PERK: Color32 = theme::GREEN;

/// What a themed room does, one muted line each.
fn room_perks(ui: &mut egui::Ui, room: &RoomDef, indent: &str) {
    for line in room.effects.lines() {
        ui.label(RichText::new(format!("{}+ {}", indent, line)).color(PERK));
    }
}

/// The room's theme if it has one, and a checklist towards each kind of
/// themed room, closest first (the closest unfinished one open).
fn room_themes(ui: &mut egui::Ui, r: &RoomStats) {
    if let Some(room) = r.kind.map(RoomId::def) {
        ui.label(theme::caps(&room.name, 19.0).color(room_color(room)));
        ui.label(RichText::new(&room.about).weak());
        room_perks(ui, room, "");
    }
    let mut order: Vec<(RoomId, Progress)> = RoomId::all().map(|k| (k, r.progress(k.def()))).collect();
    // Closest first: the most needs met, as a share.
    order.sort_by(|a, b| {
        let share = |p: &Progress| {
            let (m, n) = p.score();
            m as f32 / n.max(1) as f32
        };
        share(&b.1).total_cmp(&share(&a.1))
    });
    let open = order.iter().find(|(k, p)| Some(*k) != r.kind && !p.done()).map(|(k, _)| *k);
    theme::section(ui, "Room themes");
    for (k, p) in order {
        let room = k.def();
        let (met, of) = p.score();
        let status = if p.done() { "all there".to_string() } else { format!("{}/{}", met, of) };
        let header = RichText::new(format!("{} ({})", room.name, status)).color(room_color(room));
        egui::CollapsingHeader::new(header).id_salt(("theme", k)).default_open(Some(k) == open).show(ui, |ui| {
            for (need, &(have, ok)) in room.needs.iter().zip(&p.needs) {
                let line = format!("• {}: {} (needs {})", need.things_text(), have, need.count_text());
                ui.label(RichText::new(line).color(if ok { MET } else { UNMET }));
            }
            for &u in &p.unwanted {
                ui.label(RichText::new(format!("• Not in a {}: {}", room.name.to_lowercase(), u.name())).color(UNMET));
            }
        });
    }
    ui.label(RichText::new("Doors and windows in its walls count. See Rooms for all themes.").small().weak());
}

/// Every kind of themed room with what it needs, and the colony's rooms of each kind.
fn rooms_window(ctx: &egui::Context, sim: &Sim, st: &mut UiState, rig: &mut CameraRig) {
    let w = &sim.world;
    let mut open = st.show_rooms;
    let mut go = None;
    theme::FieldWindow::new("Rooms").open(&mut open).anchor(Align2::RIGHT_TOP, [-8.0, 92.0]).show(ctx, |ui| {
        ui.set_max_width(360.0);
        ui.label("Enclose an area with walls, doors and a roof to make a room. Furnish it with everything a theme \
                  needs and it becomes that kind of room: its floor changes and its name shows over it.");
        ui.label(RichText::new("Doors and windows in the walls count. Beds and workbenches the theme doesn't ask for spoil it.").small().weak());
        let room_h = (ctx.viewport_rect().height() - 260.0).max(200.0);
        egui::ScrollArea::vertical().max_height(room_h).show(ui, |ui| {
            for k in RoomId::all() {
                let room = k.def();
                ui.separator();
                let mine: Vec<&RoomStats> = w.derived.room_stats.iter().filter(|r| r.kind == Some(k)).collect();
                ui.horizontal(|ui| {
                    ui.label(theme::caps(&room.name, 15.0).color(room_color(room)));
                    ui.label(RichText::new(&room.about).weak());
                });
                for need in &room.needs {
                    ui.label(format!("  - {}: {}", need.things_text(), need.count_text()));
                }
                room_perks(ui, room, "  ");
                ui.horizontal_wrapped(|ui| {
                    if mine.is_empty() {
                        ui.label(RichText::new("You have none yet.").small().weak());
                    }
                    for (n, r) in mine.iter().enumerate() {
                        if ui.small_button(format!("{} {}", room.name, n + 1)).on_hover_text("Show it").clicked() {
                            go = Some((r.centre.0.round() as usize, r.centre.1.round() as usize, r.level));
                        }
                    }
                });
            }
        });
    });
    st.show_rooms = open;
    if let Some(t) = go {
        st.select(Some(Selection::Tile(t)));
        input::center_on_selection(sim, st, rig);
    }
}

fn mood_color(mood: f32) -> Color32 {
    if mood < 25.0 {
        theme::RED
    } else if mood < 45.0 {
        theme::AMBER
    } else {
        theme::GREEN
    }
}

fn capacity_color(v: f32) -> Color32 {
    if v < 0.5 {
        theme::RED
    } else if v < 0.85 {
        theme::AMBER
    } else {
        theme::GREEN
    }
}

fn portrait(ui: &mut egui::Ui, look: &Look, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let c = rect.center();
    let p = ui.painter();
    p.circle_filled(c, size * 0.45, look.hair.egui());
    p.circle_filled(c + egui::vec2(0.0, size * 0.14), size * 0.33, look.skin.egui());
}

fn hud(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, rig: &mut CameraRig, left: f32) {
    egui::Area::new(egui::Id::new("hud")).anchor(Align2::LEFT_BOTTOM, [left + 8.0, -8.0]).show(ctx, |ui| {
        theme::plate(ui, |ui| {
            let w = &sim.world;
            let hour = w.hour();
            let clock = format!(
                "Day {}  {} {}/{}  {:02}:{:02}",
                w.day(),
                w.season().name(),
                w.day_of_season(),
                my_world::seasons::SEASON_DAYS,
                hour as u32,
                ((hour - hour.floor()) * 60.0) as u32
            );
            let colonists = format!("Colonists {}   Outside {:.0}°C", w.colonists_alive(), w.outdoor_temperature());
            ui.horizontal(|ui| {
                ui.label(theme::caps(clock, 16.0).color(theme::CREAM_BRIGHT));
                ui.separator();
                ui.label(colonists);
                ui.separator();
                // Pausing holds up the colony, never a battle.
                let battle = sim.world.battle.is_some();
                let pause = match sim.clock {
                    Clock::Running | Clock::Frozen => "Pause (Space)",
                    Clock::Paused => "Resume (Space)",
                };
                if ui.add_enabled(!battle, egui::Button::new(pause)).clicked() {
                    sim.toggle_pause();
                }
                let speed = if sim.world.battle.is_some() {
                    "Speed 1x (battle)".to_string()
                } else {
                    format!("Speed {}x (1-4)", sim.speed)
                };
                if ui.button(speed).clicked() {
                    sim.cycle_speed();
                }
                if let Some(name) = &sim.scenario {
                    ui.separator();
                    ui.label(RichText::new(format!("Scenario: {} (Shift+N restarts)", name)).color(theme::TEAL));
                }
            });
            ui.horizontal(|ui| {
                let view = match st.view_level {
                    Some(l) => format!("Viewing level {} of {}", l, sim.world.map.d - 1),
                    None => "Viewing all levels".to_string(),
                };
                ui.label(theme::caps(view, 14.5).color(theme::CREAM_DIM));
                if ui.button("Down (PgDn / [)").clicked() {
                    input::level_down(sim, st, rig);
                }
                if ui.button("Up (PgUp / ])").clicked() {
                    input::level_up(sim, st);
                }
                if st.view_level.is_some() && ui.button("Show all").clicked() {
                    st.view_level = None;
                }
                let roofs = egui::Button::new("Hide roofs (Shift+R)").selected(st.hide_roofs);
                if ui.add(roofs).on_hover_text("Look into buildings: hides built roofs over rooms").clicked() {
                    st.hide_roofs = !st.hide_roofs;
                }
                ui.separator();
                if ui.add(egui::Button::new("Squads").selected(st.show_squads)).clicked() {
                    st.show_squads = !st.show_squads;
                }
                let wait = expeditions_ui::waiting(sim.home_world());
                let label = if wait > 0 { format!("Expeditions ({}!)", wait) } else { "Expeditions".to_string() };
                let exp = egui::Button::new(label).selected(st.expeditions.open);
                if ui.add(exp).on_hover_text("Send squads on missions: trade, diplomacy, salvage, fights").clicked() {
                    st.expeditions.open = !st.expeditions.open;
                }
                let rooms = egui::Button::new("Rooms").selected(st.show_rooms);
                if ui.add(rooms).on_hover_text("Themed rooms: what each needs, and yours").clicked() {
                    st.show_rooms = !st.show_rooms;
                }
                if ui.add(egui::Button::new("Resources (I)").selected(st.show_resources)).clicked() {
                    st.show_resources = !st.show_resources;
                }
                if ui.add(egui::Button::new("Work (P)").selected(st.show_work)).clicked() {
                    st.show_work = !st.show_work;
                }
                let lookup = egui::Button::new("Info").selected(st.show_info);
                if ui.add(lookup).on_hover_text("Look up any item, building or plant: stats, costs, what it's for").clicked() {
                    st.show_info = !st.show_info;
                }
                if ui.add(egui::Button::new("Help (H)").selected(st.show_help)).clicked() {
                    st.show_help = !st.show_help;
                }
                if ui.button("Center (C)").clicked() {
                    input::center_on_selection(sim, st, rig);
                }
                if ui.add(egui::Button::new("Menu (Esc)").selected(st.show_menu)).clicked() {
                    st.show_menu = !st.show_menu;
                    st.confirm_new = false;
                }
            });
        });
    });
}

/// What the colony has, by category, leaving out what it has none of.
/// The game menu: save, load, a new game (asked twice) and the sound settings.
fn game_menu(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, rig: &mut CameraRig, audio: &mut AudioSettings) {
    let mut open = true;
    theme::FieldWindow::new("Menu")
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save (K)").clicked() {
                    input::save(sim);
                }
                if ui.button("Load (L)").clicked() {
                    input::load(sim, st);
                    st.show_menu = false;
                }
                let new = if sim.scenario.is_some() { "Restart scenario" } else { "New game" };
                if ui.add(egui::Button::new(new).selected(st.confirm_new)).clicked() {
                    st.confirm_new = !st.confirm_new;
                }
            });
            ui.label(RichText::new("Load picks the newer of your save and the autosave from quitting.").small().weak());
            if st.confirm_new {
                ui.separator();
                ui.label(RichText::new("Start over? The current game is lost unless you saved it.").color(theme::AMBER));
                ui.horizontal(|ui| {
                    if ui.button("Yes, start over").clicked() {
                        input::new_colony(sim, st, rig);
                        st.confirm_new = false;
                        st.show_menu = false;
                    }
                    if ui.button("Cancel").clicked() {
                        st.confirm_new = false;
                    }
                });
            }
            ui.separator();
            ui.separator();
            theme::section(ui, "Save as");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.save_name).hint_text("name").desired_width(160.0));
                let exists = input::save_file(&st.save_name).is_some_and(|p| p.exists());
                let label = if exists { "Overwrite" } else { "Save" };
                if ui.add_enabled(input::save_file(&st.save_name).is_some(), egui::Button::new(label)).clicked() {
                    input::save_as(sim, &st.save_name.clone());
                }
            });
            let saves = input::named_saves();
            if !saves.is_empty() {
                egui::ScrollArea::vertical().max_height(140.0).id_salt("saves").show(ui, |ui| {
                    for (name, path) in saves {
                        ui.horizontal(|ui| {
                            if ui.small_button("Load").clicked() {
                                input::load_file(sim, st, &path);
                                st.show_menu = false;
                            }
                            // Click the name to save over it.
                            if ui.selectable_label(st.save_name == name, &name).on_hover_text("Use this name").clicked() {
                                st.save_name = name.clone();
                            }
                        });
                    }
                });
            }
            ui.label(RichText::new(format!("Saved games are kept in the {} folder.", input::SAVES_DIR)).small().weak());
            ui.separator();
            if ui.button("Spawn items (testing)").clicked() {
                st.show_spawn = true;
                st.show_menu = false;
            }
            if difficulty_ui(ui, sim) {
                // Out of the way of the raiders' demand.
                st.show_menu = false;
            }
            ui.separator();
            theme::section(ui, "Sound");
            audio::settings_ui(ui, audio);
        });
    if !open {
        st.show_menu = false;
        st.confirm_new = false;
    }
}

/// Testing: pick an item and an amount, then click on the map to drop it.
fn spawn_window(ctx: &egui::Context, st: &mut UiState) {
    let mut open = true;
    theme::FieldWindow::new("Spawn items (testing)").open(&mut open).anchor(Align2::RIGHT_TOP, [-8.0, 92.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Amount");
            ui.add(egui::DragValue::new(&mut st.spawn_count).range(1..=500));
            for n in [1, 10, 50, 200] {
                if ui.small_button(n.to_string()).clicked() {
                    st.spawn_count = n;
                }
            }
        });
        ui.label(RichText::new("Pick an item, then click on the map to drop it (again and again). Right-click: stop.").small().weak());
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for cat in item_categories(|_| true) {
                egui::CollapsingHeader::new(cat.to_string()).id_salt(("spawn", cat)).default_open(matches!(cat, ItemCategory::Materials | ItemCategory::Components)).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for k in ItemId::all().filter(|k| k.def().category == cat) {
                            let chosen = st.tool == input::Tool::Spawn(k);
                            let button = ui.add(egui::Button::new(k.name()).selected(chosen));
                            let button = match &k.def().about {
                                Some(about) => button.on_hover_text(about),
                                None => button,
                            };
                            if button.clicked() {
                                st.tool = input::Tool::Spawn(k);
                                st.drag_start = None;
                            }
                        }
                    });
                });
            }
        });
    });
    if !open {
        st.show_spawn = false;
        if matches!(st.tool, input::Tool::Spawn(_)) {
            st.tool = input::Tool::Select;
        }
    }
}

/// Raids on or off, how big, and a raid on demand (for testing). Returns
/// whether a raid was called in.
fn difficulty_ui(ui: &mut egui::Ui, sim: &mut Sim) -> bool {
    let w = &mut sim.world;
    theme::section(ui, "Difficulty");
    ui.checkbox(&mut w.difficulty.raids, "Raids").on_hover_text("Off: no raiders come (the rest of the game goes on).");
    ui.add_enabled_ui(w.difficulty.raids, |ui| {
        ui.horizontal(|ui| {
            ui.label("Raid size");
            ui.add(egui::Slider::new(&mut w.difficulty.raid_size, my_world::story::RAID_SIZES).step_by(0.25).suffix("×"));
        });
        let hours = ((w.story.next_raid - w.time()).max(0.0) / DAY_LEN * 24.0).round();
        let tiers = &defs().raider_tiers;
        let tier = &tiers[w.raid_tier()].name;
        let text = format!("The next raid would be {} raiders ({}, threat {:.0}), in about {} h.", w.raid_size(), tier.to_lowercase(), w.threat(), hours);
        ui.label(RichText::new(text).small().weak());
        ui.horizontal(|ui| {
            ui.label("Raider gear");
            let shown = w.difficulty.raid_gear.map_or("By threat".to_string(), |t| tiers[t.min(tiers.len() - 1)].name.clone());
            egui::ComboBox::from_id_salt("raid_gear").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut w.difficulty.raid_gear, None, "By threat");
                for (k, t) in tiers.iter().enumerate() {
                    ui.selectable_value(&mut w.difficulty.raid_gear, Some(k), &t.name);
                }
            })
            .response
            .on_hover_text("How well raiders come equipped: rising with the colony's days and wealth, or always one tier (for testing).");
        });
    });
    let busy = w.parley.is_some() || w.battle.is_some();
    let raided = ui
        .add_enabled(!busy, egui::Button::new("Raid now"))
        .on_hover_text("Raiders arrive straight away (for testing).")
        .clicked()
        && w.raid_now();
    ui.label(RichText::new("Saved with the game; a new game keeps them.").small().weak());
    raided
}

/// Item categories in the order they first appear in the definitions, of
/// the items that pass `keep`.
fn item_categories(keep: impl Fn(ItemId) -> bool) -> Vec<ItemCategory> {
    let mut categories: Vec<ItemCategory> = Vec::new();
    for k in ItemId::all().filter(|&k| keep(k)) {
        if !categories.contains(&k.def().category) {
            categories.push(k.def().category);
        }
    }
    categories
}

fn resources(ctx: &egui::Context, sim: &Sim, st: &mut UiState) {
    let totals = sim.world.item_totals();
    let categories = item_categories(|k| totals[k.index()] > 0);
    let mut look_up = None;
    theme::FieldWindow::new("Resources")
        .open(&mut st.show_resources)
        .anchor(Align2::RIGHT_BOTTOM, [-8.0, -8.0])
        .show(ctx, |ui| {
            // The colony's grid: what the panels make now, what's used, what's stored.
            let g = sim.world.derived.power;
            if g.peak > 0.0 || g.capacity > 0.0 || g.used > 0.0 || g.unpowered > 0 {
                theme::section(ui, "Power");
                ui.label(format!("Solar {:.0} W now (up to {:.1} kW), using {:.0} W", g.made, g.peak / 1000.0, g.used));
                if g.capacity > 0.0 {
                    ui.label(format!("Batteries {:.1} of {:.0} kWh", g.stored / 1000.0, g.capacity / 1000.0));
                }
                if g.unpowered > 0 {
                    let text = format!("{} heater(s)/cooler(s) without power", g.unpowered);
                    ui.label(RichText::new(text).color(theme::RED));
                }
                ui.add_space(4.0);
            }
            if categories.is_empty() {
                ui.label(RichText::new("Nothing yet").weak());
            }
            // A long list scrolls rather than running off the top of the screen.
            let list_h = (ctx.viewport_rect().height() - 140.0).max(200.0);
            egui::ScrollArea::vertical().max_height(list_h).show(ui, |ui| {
                for cat in categories {
                    theme::section(ui, cat.to_string());
                    egui::Grid::new(("resources", cat)).num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
                        for k in ItemId::all().filter(|k| k.def().category == cat && totals[k.index()] > 0) {
                            let name = ui.add(egui::Label::new(k.name()).sense(egui::Sense::click()));
                            if name.on_hover_text("Click for details").clicked() {
                                look_up = Some(InfoTarget::Item(k));
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(totals[k.index()].to_string());
                            });
                            ui.end_row();
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        });
    if let Some(it) = look_up {
        st.open_info(it);
    }
}

/// Inside size of a card in the colonist list: the same for everyone, whatever they're doing.
const CARD: egui::Vec2 = egui::vec2(150.0, 62.0);
const CARD_MARGIN: f32 = 6.0;
/// Width the colonist list takes from the left edge (after the build menu);
/// the inspectors open to its right.
const COLONIST_LIST_W: f32 = 8.0 + CARD.x + 2.0 * CARD_MARGIN + 4.0;

fn colonist_bar(ctx: &egui::Context, sim: &Sim, st: &mut UiState, rig: &mut CameraRig, left: f32) {
    // Leave room for the HUD at the bottom; scroll if there are more.
    let room = (ctx.viewport_rect().height() - 90.0).max(CARD.y * 2.0);
    egui::Area::new(egui::Id::new("colonists")).anchor(Align2::LEFT_TOP, [left + 8.0, 8.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // The buried are at rest in their graves (click one to see whose it is).
            for p in sim.world.pawns.iter().filter(|p| p.is_colonist() && !p.is_buried()) {
                let selected = st.is_selected(p.id);
                let trouble = p.is_downed() || p.hunger <= 0.0 || p.break_timer > 0.0 || p.health.bleed_rate() > 0.0;
                let stroke = if selected {
                    egui::Stroke::new(2.0, theme::CREAM_BRIGHT)
                } else if sim.world.battle.as_ref().is_some_and(|b| b.unit_of(p.id).is_some_and(|u| b.units[u].can_act())) {
                    egui::Stroke::new(2.0, theme::AMBER)
                } else if trouble && !p.is_dead() {
                    egui::Stroke::new(2.0, theme::RED)
                } else {
                    egui::Stroke::new(1.0, theme::OLIVE)
                };
                let resp = theme::Plate::inset()
                    .fill(theme::PANEL)
                    .stroke(stroke)
                    .margin(CARD_MARGIN as i8)
                    .show(ui, |ui| {
                        ui.set_min_size(CARD);
                        ui.set_max_size(CARD);
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.horizontal(|ui| {
                            portrait(ui, &pawn_look(p), 18.0);
                            ui.add(egui::Label::new(theme::caps(&p.name, 15.0).color(theme::CREAM_BRIGHT)).truncate());
                        });
                        theme::meter(ui, p.mood / 100.0, CARD.x, 5.0, mood_color(p.mood));
                        // One line, cut short with an ellipsis; the whole text on hover.
                        let status = p.status();
                        ui.add(egui::Label::new(RichText::new(&status).small()).truncate()).on_hover_text(&status);
                        let weapon = p.weapon.map_or("Unarmed", |w| w.name());
                        let color = if p.weapon.is_some() { theme::WARN } else { theme::CREAM_DIM };
                        ui.add(egui::Label::new(RichText::new(weapon).small().color(color)).truncate());
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.double_clicked() {
                    // Double click: select them and bring them into view.
                    st.select(Some(Selection::Pawn(p.id)));
                    input::center_on_selection(sim, st, rig);
                } else if resp.clicked() {
                    if ui.input(|i| i.modifiers.shift) {
                        st.toggle_pawn(p.id);
                    } else {
                        st.select(Some(Selection::Pawn(p.id)));
                    }
                }
            }
        });
    });
}

/// Idle counts fluctuate between jobs. Keep their presentation stable without
/// delaying urgent alerts or changing the simulation or save data.
#[derive(Default)]
enum IdleNotice {
    #[default]
    Hidden,
    Visible(usize),
    Pending { shown: usize, next: usize, since: f64 },
}

impl IdleNotice {
    fn update(&mut self, count: usize, now: f64) -> usize {
        let (shown, pending) = match *self {
            Self::Hidden => (0, None),
            Self::Visible(shown) => (shown, None),
            Self::Pending { shown, next, since } => (shown, Some((next, since))),
        };
        if count == shown {
            *self = if shown == 0 { Self::Hidden } else { Self::Visible(shown) };
            return shown;
        }
        let since = match pending {
            Some((next, since)) if next == count => since,
            Some(_) | None => now,
        };
        // Real seconds: fast simulation speeds must not make the notice flash.
        let delay = if count == 0 { 2.0 } else { 3.0 };
        if now - since >= delay {
            *self = if count == 0 { Self::Hidden } else { Self::Visible(count) };
            count
        } else {
            *self = Self::Pending { shown, next: count, since };
            shown
        }
    }
}

/// How many of the latest log entries the notification panel lists.
const RECENT_EVENTS: usize = 15;

#[derive(Default)]
struct NotificationState {
    generation: u64,
    idle: IdleNotice,
}

impl NotificationState {
    fn idle_count(&mut self, generation: u64, count: usize, now: f64) -> usize {
        if self.generation != generation {
            *self = Self { generation, ..Default::default() };
        }
        self.idle.update(count, now)
    }
}

fn notifications(ctx: &egui::Context, sim: &Sim, state: &mut NotificationState) {
    let mut alerts = sim.world.alerts();
    let idle = state.idle_count(sim.generation, sim.world.idle_colonists(), ctx.input(|i| i.time));
    if idle > 0 {
        alerts.push((format!("{} idle colonist(s)", idle), false));
    }
    if alerts.is_empty() && sim.world.log.is_empty() {
        return;
    }
    let width = (ctx.viewport_rect().width() * 0.3).clamp(220.0, 350.0);
    let max_height = ctx.viewport_rect().height() * 0.55;
    egui::Area::new(egui::Id::new("notifications")).anchor(Align2::RIGHT_TOP, [-12.0, 12.0]).show(ctx, |ui| {
        theme::Plate::panel().stroke(egui::Stroke::new(2.0, theme::AMBER)).show(ui, |ui| {
            ui.set_width(width);
            {
                let urgent = alerts.iter().any(|(_, urgent)| *urgent);
                let accent = if urgent { theme::RED } else { theme::AMBER };
                ui.label(theme::caps(format!("Colony alerts ({})", alerts.len()), 21.0).color(accent));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt("colony_alerts").max_height(max_height * 0.55)
                    .auto_shrink([false, false]).show(ui, |ui| {
                    if alerts.is_empty() {
                        ui.label(RichText::new("No active alerts").color(theme::CREAM_DIM));
                    }
                    // Put urgent problems first so routine warnings can't push them out of view.
                    for (text, urgent) in alerts.iter().filter(|(_, urgent)| *urgent)
                        .chain(alerts.iter().filter(|(_, urgent)| !*urgent)) {
                        let accent = if *urgent { theme::RED } else { theme::AMBER };
                        let fill = if *urgent { Color32::from_rgb(65, 30, 25) } else { theme::RAISED };
                        theme::Plate::inset().fill(fill).stroke(egui::Stroke::new(2.0, accent)).margin(9).show(ui, |ui| {
                            ui.set_width((width - 20.0).max(0.0));
                            ui.label(theme::caps(if *urgent { "! Urgent" } else { "! Attention" }, 14.0).color(accent));
                            ui.label(RichText::new(text).size(16.0).strong().color(theme::CREAM_BRIGHT));
                        });
                        ui.add_space(5.0);
                    }
                });
                ui.add_space(8.0);
            }
            if !sim.world.log.is_empty() {
                let shown = sim.world.log.len().min(RECENT_EVENTS);
                let header = theme::caps(format!("Recent events ({})", shown), 19.0).color(theme::AMBER);
                // Events keep the panel's full width rather than indenting under the header.
                ui.spacing_mut().indent = 0.0;
                egui::CollapsingHeader::new(header).id_salt("recent_events_header").default_open(true).show(ui, |ui| {
                    // Reserve the same alert space even when a condition clears, so
                    // persistent events (including save confirmations) don't jump.
                    let events_height = max_height * 0.45;
                    egui::ScrollArea::vertical().id_salt("recent_events").max_height(events_height)
                        .min_scrolled_height(events_height).show(ui, |ui| {
                        for (k, text) in sim.world.log.iter().rev().take(RECENT_EVENTS).enumerate() {
                            let latest = k == 0;
                            let accent = if latest { theme::AMBER } else { theme::STEEL };
                            // Older events are more compact, but every card spans the panel.
                            let margin: i8 = if latest { 9 } else { 6 };
                            theme::Plate::inset().fill(if latest { theme::RAISED } else { theme::PANEL_DEEP })
                                .stroke(egui::Stroke::new(if latest { 2.0 } else { 1.0 }, accent))
                                .margin(margin).show(ui, |ui| {
                                    ui.set_width((width - 2.0 * f32::from(margin) - 2.0).max(0.0));
                                    if latest {
                                        ui.label(theme::caps("Latest", 13.0).color(theme::AMBER));
                                    }
                                    ui.label(RichText::new(text).size(if latest { 17.0 } else { 15.0 })
                                        .color(theme::CREAM_BRIGHT));
                                });
                            ui.add_space(if latest { 5.0 } else { 3.0 });
                        }
                    });
                });
            }
        });
    });
}

fn pawn_inspector(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, id: PawnId, left: f32) {
    let Some(p) = sim.world.pawn(id) else { return };
    let combat = p.is_living_colonist();
    if !combat && st.inspector_tab == InspectorTab::Combat {
        st.inspector_tab = InspectorTab::Overview;
    }
    let mut actions = Vec::new();
    let mut world_actions: Vec<Action> = Vec::new();
    let title = if p.is_colonist() { p.name.clone() } else { format!("{} (raider)", p.name) };
    let mut look_up = None;
    let mut open = true;
    theme::FieldWindow::new(title)
        .id(egui::Id::new("inspector"))
        .open(&mut open)
        .anchor(Align2::LEFT_TOP, [left + COLONIST_LIST_W + 8.0, 8.0])
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                theme::tab_value(ui, &mut st.inspector_tab, InspectorTab::Overview, "Overview");
                theme::tab_value(ui, &mut st.inspector_tab, InspectorTab::Health, "Health");
                if combat {
                    theme::tab_value(ui, &mut st.inspector_tab, InspectorTab::Combat, "Combat");
                }
            });
            if st.group.len() > 1 {
                let others = st.group.len() - 1;
                let text = format!("and {} more selected (orders go to all; Shift+click to add or remove)", others);
                ui.label(RichText::new(text).small());
            }
            ui.label(RichText::new(format!("{} - {}", p.status(), p.weapon_name())).weak());
            gear_list(ui, p);
            if p.is_living_colonist() {
                let text = format!("Feels {:.0}°C (clothes +{:.0}°C in the cold)", p.felt, p.warmth());
                ui.label(RichText::new(text).color(temp_color(p.felt)));
            }
            // Their gear and cyberware, to look up.
            let implants = p.health.implants().map(|(_, k)| k);
            let gear: Vec<ItemId> =
                p.weapon.into_iter().chain(p.sidearm).chain(p.utility.map(|s| s.kind)).chain(p.apparel.iter().copied()).chain(implants).collect();
            if !gear.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Info:").small().weak());
                    for k in gear {
                        if ui.small_button(k.name()).on_hover_text("Stats of this gear or cyberware").clicked() {
                            look_up = Some(InfoTarget::Item(k));
                        }
                    }
                });
            }
            ui.separator();
            match st.inspector_tab {
                InspectorTab::Overview => overview_tab(ui, p),
                InspectorTab::Health => {
                    health_tab(ui, p);
                    cyberware(ui, &sim.world, p, &mut world_actions);
                }
                InspectorTab::Combat => actions = squads_ui::combat_tab(ui, &sim.world, p),
            }
        });
    if !actions.is_empty() {
        squads_ui::apply(&mut sim.world, id, actions);
    }
    for act in world_actions {
        act(&mut sim.world);
    }
    if let Some(it) = look_up {
        st.open_info(it);
    }
    if !open {
        st.select(None);
    }
}

fn overview_tab(ui: &mut egui::Ui, p: &Pawn) {
    let need = |ui: &mut egui::Ui, label: &str, v: f32, fill: Color32| {
        ui.horizontal(|ui| {
            ui.add_sized([60.0, 16.0], egui::Label::new(label));
            ui.add(theme::bar(v / 100.0).desired_width(220.0).fill(fill));
        });
    };
    need(ui, "Health", p.health.summary() * 100.0, theme::RED);
    if p.is_colonist() {
        need(ui, "Mood", p.mood, mood_color(p.mood));
        need(ui, "Food", p.hunger, theme::AMBER);
        need(ui, "Rest", p.rest, theme::TEAL);
        need(ui, "Fun", p.joy, Color32::from_rgb(170, 120, 220));
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
        let color = if value >= 0.0 { theme::GREEN } else { theme::RED };
        ui.label(RichText::new(format!("{:+}  {}", value as i32, label)).color(color));
    }
    if p.is_living_colonist() {
        ui.label(RichText::new("Right-click a cell: prioritize a job there, or equip gear   Combat tab: squad and loadout").small().weak());
    }
}

/// What they carry and wear, slot by slot (only the slots in use).
fn gear_list(ui: &mut egui::Ui, p: &Pawn) {
    let mut lines: Vec<String> = Vec::new();
    if let Some(k) = p.sidearm {
        let drawn = if p.sidearm_drawn { ", drawn" } else { "" };
        lines.push(format!("Sidearm: {}{}", k.name(), drawn));
    }
    if let Some(s) = p.utility {
        match p.shield() {
            Some((full, _)) => lines.push(format!("Utility: {} ({:.0}/{:.0} energy; no ranged weapons)", s.kind.name(), p.shield_energy(), full)),
            None => lines.push(format!("Utility: {} ×{}", s.kind.name(), s.count)),
        }
    }
    for &k in &p.apparel {
        let Some(a) = k.def().apparel.as_ref() else { continue };
        let slots: Vec<&str> = a.slots().map(Slot::name).collect();
        lines.push(format!("{}: {}", slots.join(", "), k.name()));
    }
    if !lines.is_empty() {
        ui.label(RichText::new(lines.join("   ")).weak());
    }
}

/// Cyberware slots: what's in each, and the operations ordered for them
/// (install, swap or take out; call it off).
fn cyberware(ui: &mut egui::Ui, w: &SimWorld, p: &Pawn, actions: &mut Vec<Action>) {
    if !p.is_colonist() || p.is_dead() {
        return;
    }
    ui.separator();
    theme::section(ui, "Cyberware");
    let about = "A doctor installs it at an operating table, in place of a natural or lost part (the brain and skin get add-ons). \
                 Cyberware it replaces is left at the table. It can go wrong: better doctors and a surgery room help.";
    ui.label(RichText::new(about).small().weak());
    let id = p.id;
    let tables = (0..w.map.len()).any(|i| w.medical_bed(w.map.pos(i)));
    let blue = theme::TEAL;
    for slot in CyberSlot::ALL {
        ui.horizontal_wrapped(|ui| {
            ui.add_sized([90.0, 18.0], egui::Label::new(slot.name()));
            let installed = p.health.implant(slot);
            let (state, color) = match installed {
                Some((item, eff)) => {
                    let name = item.map_or("Prosthetic", |k| k.name());
                    (if eff == 1.0 { name.to_string() } else { format!("{} ({:.0}%)", name, eff * 100.0) }, blue)
                }
                None if p.health.slot_lost(slot) => ("Lost".to_string(), theme::RED),
                None => ("Natural".to_string(), theme::CREAM_DIM),
            };
            match p.surgeries.iter().find(|o| o.slot == slot) {
                Some(op) => {
                    let doing = op.item.map_or("taking it out".to_string(), |k| format!("installing a {}", k.name().to_lowercase()));
                    ui.label(RichText::new(format!("{} - {}", state, doing)).color(blue));
                    if ui.small_button("Cancel").clicked() {
                        actions.push(Box::new(move |w| {
                            w.cancel_operation(id, slot);
                        }));
                    }
                    let waiting = match op.item {
                        Some(k) if w.map.count_items(k) == 0 => Some(format!("Waiting for a {} (see its Info for where it's made).", k.name().to_lowercase())),
                        _ if !tables => Some("Waiting for an operating table.".to_string()),
                        _ => None,
                    };
                    if let Some(text) = waiting {
                        ui.label(RichText::new(text).small().color(theme::AMBER));
                    }
                }
                None => {
                    egui::ComboBox::from_id_salt(("cyberware", id, slot.name())).selected_text(RichText::new(state).color(color)).show_ui(ui, |ui| {
                        for item in SimWorld::cyberware_for(slot) {
                            if installed.is_some_and(|(k, _)| k == Some(item)) {
                                continue;
                            }
                            let text = format!("Install {} ({} in the colony)", item.name().to_lowercase(), w.map.count_items(item));
                            if ui.selectable_label(false, text).clicked() {
                                actions.push(Box::new(move |w| {
                                    w.order_operation(id, slot, item);
                                }));
                            }
                        }
                        if installed.is_some() && slot.removable() && ui.selectable_label(false, "Take it out").clicked() {
                            actions.push(Box::new(move |w| {
                                w.order_removal(id, slot);
                            }));
                        }
                    });
                }
            }
        });
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
            ui.label(RichText::new(text).color(theme::RED));
        }
        None if bleed > 0.0 => {
            let text = format!("Bleeding {:.0}%/day (body keeps up)", bleed * 100.0);
            ui.label(RichText::new(text).color(theme::AMBER));
        }
        None => {
            ui.label(RichText::new("Not bleeding").weak());
        }
    }
    if h.hypothermia > 0.01 {
        let text = format!("Hypothermia {:.0}% (collapses at 70%, fatal at 100%): get them somewhere warm", h.hypothermia * 100.0);
        ui.label(RichText::new(text).color(theme::TEAL));
    }
    if h.blood_loss > 0.01 || h.malnutrition > 0.01 {
        let text = format!("Blood loss {:.0}%     Malnutrition {:.0}%", h.blood_loss * 100.0, h.malnutrition * 100.0);
        ui.label(RichText::new(text).color(theme::AMBER));
    }
    ui.separator();
    egui::Grid::new("capacities").num_columns(2).spacing([10.0, 2.0]).show(ui, |ui| {
        for (name, v) in h.caps.list() {
            ui.label(name);
            ui.add(
                theme::bar(v.clamp(0.0, 1.0))
                    .desired_width(180.0)
                    .fill(capacity_color(v).linear_multiply(0.7))
                    .text(format!("{:.0}%", v * 100.0)),
            );
            ui.end_row();
        }
    });
    ui.separator();
    if h.hediffs.is_empty() {
        ui.label(RichText::new("No injuries or illnesses").color(theme::GREEN));
        return;
    }
    for d in &h.hediffs {
        let part = d.part.map_or("Whole body", |p| p.name());
        let (detail, color) = match d.kind {
            HediffKind::Injury { permanent: true, .. } => (String::new(), theme::CREAM_DIM),
            HediffKind::Injury { .. } => (
                format!(" {:.0}", d.severity),
                if d.bleed() > 0.0 { theme::RED } else { theme::AMBER },
            ),
            HediffKind::Missing => {
                (String::new(), if d.bleed() > 0.0 { theme::RED } else { theme::CREAM_DIM })
            }
            HediffKind::FoodPoisoning => (format!(" {:.0}%", d.severity * 100.0), theme::AMBER),
            HediffKind::Prosthetic { .. } => (String::new(), theme::TEAL),
            HediffKind::Infection { immunity } | HediffKind::Flu { immunity } => (
                format!(" {:.0}% (immunity {:.0}%)", d.severity * 100.0, immunity * 100.0),
                theme::AMBER,
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

/// A change to the world chosen in the inspector, applied after drawing it.
type Action = Box<dyn FnOnce(&mut SimWorld)>;

/// Returns whether anything was changed in it.
/// The cell inspector; returns whether something was changed, and whether it's still open.
/// `move_from` is set if the player chose to move the furniture here.
fn tile_inspector(
    ctx: &egui::Context,
    sim: &mut Sim,
    t: Pos,
    left: f32,
    move_from: &mut Option<Pos>,
    look_up: &mut Option<InfoTarget>,
) -> (bool, bool) {
    let mut actions: Vec<Action> = Vec::new();
    let w = &sim.world;
    let m = &w.map;
    let i = m.idx(t);
    let mut open = true;
    theme::FieldWindow::new(format!("Cell {}, {}  level {}", t.0, t.1, t.2))
        .id(egui::Id::new("inspector"))
        .open(&mut open)
        .anchor(Align2::LEFT_TOP, [left + COLONIST_LIST_W + 8.0, 8.0])
        .show(ctx, |ui| {
            let what = match m.feature[i] {
                f @ Feature::Plant { growth, .. } if f.is_crop() => format!("{}, {:.0}% grown", f.name(), growth * 100.0),
                Feature::Blueprint { kind, delivered, over, .. } => {
                    let d = kind.def();
                    let instead = over.map_or(String::new(), |(old, _)| format!(", to replace the {}", old.name().to_lowercase()));
                    let state = match d.missing(delivered) {
                        Some((item, n)) => format!(
                            "{} blueprint ({}/{} delivered; needs {} {} next)",
                            kind.name(),
                            delivered,
                            d.total_cost(),
                            n,
                            item.name().to_lowercase()
                        ),
                        None => format!("{} blueprint (all {} delivered)", kind.name(), d.total_cost()),
                    };
                    state + &instead
                }
                Feature::None if m.block[i] != Block::Air => m.block[i].name().to_string(),
                f => {
                    let ground = m.below(t).map_or(Block::Air, |b| m.block[m.idx(b)]);
                    format!("{} on {}", f.name(), ground.name().to_lowercase())
                }
            };
            let thing = match m.feature[i] {
                Feature::Built { kind, .. } | Feature::Blueprint { kind, .. } => Some(InfoTarget::Build(kind)),
                Feature::Plant { kind, .. } => Some(InfoTarget::Plant(kind)),
                Feature::None => None,
            };
            ui.horizontal(|ui| {
                ui.label(what);
                if let Some(it) = thing {
                    info::button(ui, it, look_up);
                }
            });
            if m.floor[i] {
                ui.label("Built floor");
            }
            if m.roof[i] {
                ui.label("Roof");
            }
            // Here or overhead: a room wants a roof, but nothing would hold it up.
            let above = m.above(t).map(|a| m.idx(a));
            if m.no_roof.contains(&(i as u32)) {
                ui.label("No roof area (automatic roofing disabled above this tile)");
            }
            if m.roof_gaps.contains(&i) || above.is_some_and(|a| m.roof_gaps.contains(&a)) {
                let why = "No roof can reach here: it needs a wall within 3 cells, so the room isn't closed.";
                ui.label(RichText::new(why).color(theme::RED));
            }
            if m.covered(t) && !m.roofed(t) {
                ui.label("Covered overhead");
            }
            for s in m.stacks(i) {
                ui.horizontal(|ui| {
                    ui.label(format!("{} x{}", s.kind.name(), s.count));
                    info::button(ui, InfoTarget::Item(s.kind), look_up);
                });
                if let Some(about) = &s.kind.def().about {
                    ui.label(RichText::new(about).weak());
                }
            }
            if m.slots(i) > 1 {
                let used = m.stacks(i).count();
                ui.label(RichText::new(format!("{} of {} shelf slots used (50 each)", used, m.slots(i))).weak());
            }
            wall_decor(ui, w, t, look_up, &mut actions);
            if let Some(power) = m.feature[i].structure().and_then(|d| d.power_text()) {
                ui.label(RichText::new(power).weak());
            }
            if let Some(state) = w.device_state(i) {
                use my_world::power::DeviceState;
                let (text, color) = match state {
                    DeviceState::Running => ("Running", theme::GREEN),
                    DeviceState::Idle => ("Idle: its room needs nothing", Color32::LIGHT_GRAY),
                    DeviceState::NoPower => ("No power: build more PV systems or a battery", theme::RED),
                };
                ui.label(RichText::new(text).color(color));
            }
            if let Some(d) = m.desig[i] {
                ui.label(format!("Designated: {:?}", d));
            }
            match w.room_at(t) {
                Some(r) => {
                    let tier = TIERS[r.tier() as usize];
                    ui.label(format!("Indoors: {} room, {} tiles (quality {:+.1})", tier.to_lowercase(), r.size, r.score()));
                    let heat = if r.heat > 0.0 { ", warmed by what burns in it" } else { "" };
                    let text = format!("{:.0}°C (outside {:.0}°C){}", r.temp, w.outdoor_temperature(), heat);
                    ui.label(RichText::new(text).color(temp_color(r.temp)));
                    let parts = format!(
                        "space {:+.1}, beauty {:+.1}, cleanliness {:+.1}, light {:+.1}",
                        r.space_points(),
                        r.beauty_points(),
                        r.dirt_points(),
                        r.light_points()
                    );
                    ui.label(RichText::new(parts).weak());
                    room_themes(ui, r);
                }
                None if m.roofed(t) => {
                    ui.label("Indoors");
                }
                None => {
                    let t = w.outdoor_temperature();
                    ui.label(RichText::new(format!("Outdoors, {:.0}°C", t)).color(temp_color(t)));
                }
            }
            match m.zone[i] {
                Zone::Stockpile(id) => stockpile_settings(ui, w, id, &mut actions),
                Zone::Grow(crop) => crop_choice(ui, t, crop, &mut actions),
                Zone::Safe => {
                    ui.label("Safe zone: civilians shelter here during battles.");
                }
                Zone::None => {}
            }
            if let Some(k) = m.quarry_at(t.0, t.1) {
                quarry_settings(ui, w, k, &mut actions);
            }
            if let Some(kind) = m.feature[i].structure_id().filter(|k| !k.def().recipes.is_empty()) {
                bills(ui, w, t, kind, &mut actions);
            }
            if w.care_bed(t) {
                ui.separator();
                let text = if w.medical_bed(t) {
                    "For operations and patients: nobody sleeps here."
                } else {
                    "A hospital bed: kept for patients, nobody's own."
                };
                ui.label(RichText::new(text).weak());
            } else if m.feature[i].is_bed() {
                bed_owner(ui, w, t, &mut actions);
            }
            furniture_move(ui, w, t, move_from, &mut actions);
            if m.feature[i].is_special(Special::Grave) {
                ui.separator();
                match w.grave_occupant(t).and_then(|id| w.pawn(id)) {
                    Some(p) => ui.label(RichText::new(format!("Here lies {}.", p.name)).italics()),
                    None => ui.label(RichText::new("An open grave: haulers bury a dead colonist here.").weak()),
                };
            }
        });
    let changed = !actions.is_empty();
    for act in actions {
        act(&mut sim.world);
    }
    (changed, open)
}

/// A quarry: how deep it goes, and how far the digging has got.
fn quarry_settings(ui: &mut egui::Ui, w: &SimWorld, k: usize, actions: &mut Vec<Action>) {
    let q = &w.quarries[k];
    ui.separator();
    theme::section(ui, format!("Quarry ({} columns)", q.cols.len()));
    ui.horizontal(|ui| {
        ui.label("Dig down");
        let depth = q.depth;
        if ui.add_enabled(depth > 1, egui::Button::new("-").small()).clicked() {
            actions.push(Box::new(move |w| w.set_quarry_depth(k, depth - 1)));
        }
        ui.label(format!("{} levels", depth));
        if ui.add_enabled(depth < my_world::quarry::MAX_DEPTH, egui::Button::new("+").small()).clicked() {
            actions.push(Box::new(move |w| w.set_quarry_depth(k, depth + 1)));
        }
    });
    use my_world::quarry::QuarryState;
    let bottom = q.bottom();
    let state = match w.quarry_state(k) {
        QuarryState::Digging { level, left } => format!("Digging level {}: {} to go there (down to level {}).", level, left, bottom),
        QuarryState::Stairs { level } => format!("Waiting for the ladder down to level {} (3 wood; builders put it up).", level),
        QuarryState::Done => format!("Dug out down to level {}.", bottom),
    };
    ui.label(state);
    let stuck = w.derived.quarry_unreachable.get(k).copied().unwrap_or(0);
    if stuck > 0 {
        let why = format!(
            "{} cell(s) nobody can get to (a ledge, or a pillar too tall to climb) are left standing; it digs on around and under them. Build a way up to have them dug.",
            stuck
        );
        ui.label(RichText::new(why).color(theme::AMBER));
    }
    match q.shaft {
        Some((x, y)) => {
            let text = format!("Ladder down at {}, {} from the ground at level {} (marked).", x, y, q.ground);
            ui.label(RichText::new(text).weak());
        }
        None => {
            let why = "No way in: no ground to walk on beside it, so only what's above the ground is dug.";
            ui.label(RichText::new(why).color(theme::AMBER));
        }
    }
}

/// Blue when cold, orange when hot, plain in between (see `temperature.rs`).
fn temp_color(t: f32) -> Color32 {
    use my_world::temperature::{COLD, HOT};
    if t < COLD {
        theme::TEAL
    } else if t > HOT {
        theme::AMBER
    } else {
        Color32::LIGHT_GRAY
    }
}

/// What hangs on the wall at `t`, side by side, with what's planned:
/// calling plans off, taking things down.
fn wall_decor(ui: &mut egui::Ui, w: &SimWorld, t: Pos, look_up: &mut Option<InfoTarget>, actions: &mut Vec<Action>) {
    let m = &w.map;
    let list = m.decor(m.idx(t));
    if !m.feature[m.idx(t)].is_block() {
        return;
    }
    ui.separator();
    if list.is_empty() {
        ui.label(RichText::new("Nothing on this wall. Wall decor (build menu) hangs in five places on each side.").weak());
        return;
    }
    for d in list {
        let (face, slot) = (d.face, d.slot);
        let side = match m.decor_front(t, face, slot).and_then(|f| w.room_at(f)) {
            Some(r) => r.kind_name().map_or("indoors".to_string(), |k| k.to_lowercase()),
            None => "outside".to_string(),
        };
        ui.horizontal(|ui| {
            let state = match (d.state, d.still_needs()) {
                (DecorState::Planned { .. }, Some((item, n))) => format!(" (planned: needs {} {})", n, item.name().to_lowercase()),
                (DecorState::Planned { .. }, None) => " (planned: ready to put up)".to_string(),
                (DecorState::ComingDown, _) => " (to be taken down)".to_string(),
                (DecorState::Up, _) => String::new(),
            };
            ui.label(format!("{}{} - place {} facing {}", d.kind.name(), state, slot + 1, side));
            info::button(ui, InfoTarget::Build(d.kind), look_up);
            let (label, hint) = match d.state {
                DecorState::Planned { .. } => ("Cancel", "Call it off; what was brought is put down"),
                DecorState::Up => ("Take down", "A builder takes it down and gets half its materials back"),
                DecorState::ComingDown => ("Keep", "Leave it up after all"),
            };
            if ui.small_button(label).on_hover_text(hint).clicked() {
                actions.push(Box::new(move |w| {
                    w.cancel_decor(t, face, slot);
                }));
            }
        });
        if d.is_up() && d.kind.def().shows_date {
            let hour = w.hour();
            let date = format!("Day {}, {:02}:{:02}", w.day(), hour as u32, ((hour - hour.floor()) * 60.0) as u32);
            ui.label(RichText::new(format!("    It says: {}", date)).weak());
        }
    }
}

/// Moving the furniture here, or how its move is going.
fn furniture_move(ui: &mut egui::Ui, w: &SimWorld, t: Pos, move_from: &mut Option<Pos>, actions: &mut Vec<Action>) {
    let cell = |p: Pos| format!("cell {}, {}", p.0, p.1);
    let (text, call_off) = if let Some(mv) = w.move_from(t) {
        let how = if mv.is_lifted() { "Packed up" } else { "Waiting to be packed up" };
        (format!("{}: moving {} to {}.", how, mv.kind.name().to_lowercase(), cell(mv.to)), "Leave it here")
    } else if let Some(mv) = w.move_to(t) {
        let how = if mv.is_lifted() { "packed up, waiting to be set down here" } else { "waiting to be packed up" };
        (format!("{} from {}: {}.", mv.kind.name(), cell(mv.from), how), "Call off the move")
    } else {
        if w.movable(t).is_some() {
            ui.separator();
            let hint = "Pick a new spot for it. A builder packs it up and sets it down there, at no cost.";
            if ui.button("Move").on_hover_text(hint).clicked() {
                *move_from = Some(t);
            }
        }
        return;
    };
    ui.separator();
    ui.label(RichText::new(text).color(theme::TEAL));
    if ui.button(call_off).clicked() {
        actions.push(Box::new(move |w| {
            w.apply_order(t, Order::Cancel);
        }));
    }
}

/// Who sleeps in the bed at `t`, to choose from the colonists. A leader
/// is told whether it makes proper quarters for them.
fn bed_owner(ui: &mut egui::Ui, w: &SimWorld, t: Pos, actions: &mut Vec<Action>) {
    ui.separator();
    let label = |p: &Pawn| if p.role == Role::Leader { format!("{} (leader)", p.name) } else { p.name.clone() };
    let owner = w.bed_owner(t).and_then(|id| w.pawn(id));
    ui.horizontal(|ui| {
        ui.label("Sleeps here");
        let current = owner.map_or("Nobody yet".to_string(), label);
        egui::ComboBox::from_id_salt(("bed", t)).selected_text(current).show_ui(ui, |ui| {
            if ui.selectable_label(owner.is_none(), "Nobody (first come)").clicked() {
                actions.push(Box::new(move |w| {
                    w.assign_bed(t, None);
                }));
            }
            for p in w.pawns.iter().filter(|p| p.is_living_colonist()) {
                let id = p.id;
                if ui.selectable_label(owner.is_some_and(|o| o.id == id), label(p)).clicked() {
                    actions.push(Box::new(move |w| {
                        w.assign_bed(t, Some(id));
                    }));
                }
            }
        });
    });
    if let Some(p) = owner.filter(|p| p.role == Role::Leader) {
        if w.has_proper_quarters(p) {
            ui.label(RichText::new("Proper quarters for a leader.").weak());
        } else {
            let why = "Not proper quarters: a leader wants a room of at least decent quality that nobody else sleeps in.";
            ui.label(RichText::new(why).color(theme::RED));
        }
    }
}

fn stockpile_settings(ui: &mut egui::Ui, w: &SimWorld, id: StockpileId, actions: &mut Vec<Action>) {
    let Some(store) = w.stockpile(id) else { return };
    ui.separator();
    theme::section(ui, &store.name);
    ui.horizontal(|ui| {
        ui.label("Priority");
        for prio in StorePriority::ALL {
            if ui.add(egui::Button::new(prio.name()).selected(store.priority == prio)).clicked() {
                actions.push(Box::new(move |w| w.set_stockpile_priority(id, prio)));
            }
        }
    });
    for cat in item_categories(|_| true) {
        let items: Vec<ItemId> = ItemId::all().filter(|k| k.def().category == cat).collect();
        let allowed = items.iter().filter(|&&k| store.accepts(k)).count();
        let header = format!("{} ({}/{})", cat, allowed, items.len());
        egui::CollapsingHeader::new(header).id_salt(("stockpile", id, cat)).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (label, allow) in [("All", true), ("None", false)] {
                    if ui.small_button(label).clicked() {
                        let items = items.clone();
                        actions.push(Box::new(move |w| items.into_iter().for_each(|k| w.set_stockpile_allows(id, k, allow))));
                    }
                }
            });
            for k in items.iter().copied() {
                let mut on = store.accepts(k);
                let check = ui.checkbox(&mut on, k.name());
                let check = match &k.def().about {
                    Some(about) => check.on_hover_text(about),
                    None => check,
                };
                if check.changed() {
                    actions.push(Box::new(move |w| w.set_stockpile_allows(id, k, on)));
                }
            }
        });
    }
}

fn crop_choice(ui: &mut egui::Ui, t: Pos, crop: PlantId, actions: &mut Vec<Action>) {
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        ui.label("Grow zone:");
        for k in PlantId::all().filter(|k| k.def().sow.is_some()) {
            if ui.add(egui::Button::new(k.name()).selected(k == crop)).clicked() {
                actions.push(Box::new(move |w| w.set_grow_crop(t, k)));
            }
        }
    });
}

fn mode_label(mode: BillMode) -> String {
    match mode {
        BillMode::Count(n) => format!("Make {}", n),
        BillMode::Until(n) => format!("Until {}", n),
        BillMode::Forever => "Forever".into(),
    }
}

fn bills(ui: &mut egui::Ui, w: &SimWorld, t: Pos, kind: my_world::defs::BuildId, actions: &mut Vec<Action>) {
    ui.separator();
    theme::section(ui, "Bills");
    let bench = w.bench_at(t);
    let status = match bench.and_then(|b| b.current.map(|r| (b, r))) {
        Some((b, r)) => match b.need() {
            Some(n) => format!("Gathering for {}: needs {} {}", r.name().to_lowercase(), n.count, n.kind.name().to_lowercase()),
            None => format!("Ready: {}", r.name().to_lowercase()),
        },
        None => "Idle".into(),
    };
    ui.label(RichText::new(status).weak());
    let list = bench.map_or(&[][..], |b| &b.bills[..]);
    for (k, bill) in list.iter().enumerate() {
        let b = *bill;
        ui.horizontal(|ui| {
            let name = RichText::new(b.recipe.name());
            ui.label(if b.suspended { name.strikethrough() } else { name });
            // Click the mode to cycle it; - and + change the number.
            if ui.button(mode_label(b.mode)).clicked() {
                let next = match b.mode {
                    BillMode::Count(n) => BillMode::Until(n.max(1)),
                    BillMode::Until(_) => BillMode::Forever,
                    BillMode::Forever => BillMode::Count(1),
                };
                actions.push(Box::new(move |w| w.set_bill(t, k, next, b.suspended)));
            }
            let n = match b.mode {
                BillMode::Count(n) | BillMode::Until(n) => Some(n),
                BillMode::Forever => None,
            };
            if let Some(n) = n {
                let with = |n: u32| match b.mode {
                    BillMode::Count(_) => BillMode::Count(n),
                    _ => BillMode::Until(n),
                };
                // Shift+click steps by 5.
                let step = if ui.input(|i| i.modifiers.shift) { 5 } else { 1 };
                if ui.small_button("-").on_hover_text("Shift+click: 5 fewer").clicked() && n > 1 {
                    let mode = with(n.saturating_sub(step).max(1));
                    actions.push(Box::new(move |w| w.set_bill(t, k, mode, b.suspended)));
                }
                if ui.small_button("+").on_hover_text("Shift+click: 5 more").clicked() {
                    let mode = with(n + step);
                    actions.push(Box::new(move |w| w.set_bill(t, k, mode, b.suspended)));
                }
            }
            if ui.small_button(if b.suspended { "Resume" } else { "Pause" }).clicked() {
                actions.push(Box::new(move |w| w.set_bill(t, k, b.mode, !b.suspended)));
            }
            if k > 0 && ui.small_button("Up").clicked() {
                actions.push(Box::new(move |w| w.raise_bill(t, k)));
            }
            if ui.small_button("x").clicked() {
                actions.push(Box::new(move |w| w.remove_bill(t, k)));
            }
        });
    }
    ui.label(RichText::new("Add:").weak());
    ui.horizontal_wrapped(|ui| {
        for &r in &kind.def().recipes {
            let d = r.def();
            let needs: Vec<String> = d.ingredients.iter().map(|a| format!("{} {}", a.count, a.item.name().to_lowercase())).collect();
            if ui.button(&d.name).on_hover_text(format!("From {}", needs.join(", "))).clicked() {
                actions.push(Box::new(move |w| w.add_bill(t, r, BillMode::Count(1))));
            }
        }
    });
    if let Some(b) = bench.filter(|b| !b.stock.is_empty()) {
        let stock: Vec<String> = b.stock.iter().map(|s| format!("{} {}", s.count, s.kind.name().to_lowercase())).collect();
        ui.label(RichText::new(format!("Delivered: {}", stock.join(", "))).weak());
    }
}

/// The colour of a schedule block.
fn block_color(b: my_world::recreation::Block) -> Color32 {
    use my_world::recreation::Block;
    match b {
        // Deeper than the accents, so names on them stay readable.
        Block::Anything => Color32::from_rgb(72, 68, 60),
        Block::Work => Color32::from_rgb(160, 112, 40),
        Block::Fun => Color32::from_rgb(46, 122, 112),
        Block::Sleep => Color32::from_rgb(58, 56, 108),
    }
}

/// Every colonist's day, an hour a cell: pick a block, then click or drag over hours to paint it.
fn schedules(ui: &mut egui::Ui, sim: &mut Sim, st: &mut UiState) {
    use my_world::recreation::Block;
    ui.horizontal(|ui| {
        ui.label("Paint:");
        for b in Block::ALL {
            let chosen = st.schedule_brush == b;
            let text = RichText::new(b.name()).color(if chosen { theme::CREAM_BRIGHT } else { theme::CREAM });
            let button = egui::Button::new(text).fill(block_color(b)).stroke(if chosen {
                egui::Stroke::new(2.0, theme::CREAM_BRIGHT)
            } else {
                egui::Stroke::NONE
            });
            if ui.add(button).clicked() {
                st.schedule_brush = b;
            }
        }
    });
    let how = "Sleep: to bed even a little tired. Fun: fun until full, no work. Work: no stopping for fun. \
               Anything: work, and fun when bored. Hunger and exhaustion come first.";
    ui.label(RichText::new(how).small().weak());
    let now = (sim.world.hour() as usize).min(23);
    let rows: Vec<(PawnId, String, [Block; 24])> =
        sim.world.pawns.iter().filter(|p| p.is_living_colonist()).map(|p| (p.id, p.name.clone(), p.schedule)).collect();
    let painting = ui.input(|i| i.pointer.primary_down());
    egui::Grid::new("schedule_grid").spacing([1.0, 3.0]).show(ui, |ui| {
        ui.label("");
        for h in 0..24 {
            let text = RichText::new(format!("{:>2}", h)).small().monospace();
            ui.label(if h == now { text.strong().color(theme::AMBER) } else { text.weak() });
        }
        ui.end_row();
        for (id, name, day) in &rows {
            ui.label(name);
            for (h, &b) in day.iter().enumerate() {
                let cell = egui::Button::new("").fill(block_color(b)).min_size(egui::vec2(18.0, 18.0)).stroke(if h == now {
                    egui::Stroke::new(1.0, theme::AMBER)
                } else {
                    egui::Stroke::NONE
                });
                let r = ui.add(cell).on_hover_text(format!("{}:00 {}", h, b.name()));
                // Click, or drag across: paint the hour.
                if (r.clicked() || r.hovered() && painting) && b != st.schedule_brush {
                    sim.world.set_schedule(*id, h, st.schedule_brush);
                }
            }
            ui.end_row();
        }
    });
}

/// A work priority's background: the more urgent, the stronger the green; never, plain.
fn priority_color(prio: WorkPriority) -> Color32 {
    match prio {
        WorkPriority::First => Color32::from_rgb(64, 132, 72),
        WorkPriority::Second => Color32::from_rgb(60, 108, 68),
        WorkPriority::Third => Color32::from_rgb(56, 88, 64),
        WorkPriority::Fourth => Color32::from_rgb(52, 70, 60),
        WorkPriority::Never => Color32::from_rgb(42, 42, 42),
    }
}

fn work_panel(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState) {
    let rows: Vec<(PawnId, String, PerWork<WorkPriority>, PerSkill<u8>)> = sim
        .world
        .pawns
        .iter()
        .filter(|p| p.is_living_colonist())
        .map(|p| (p.id, p.name.clone(), p.priorities, p.skills))
        .collect();
    let mut open = st.show_work;
    theme::FieldWindow::new("Work")
        .open(&mut open)
        .anchor(Align2::CENTER_TOP, [0.0, 80.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                theme::tab_value(ui, &mut st.work_schedule, false, "Priorities");
                theme::tab_value(ui, &mut st.work_schedule, true, "Schedule");
            });
            ui.separator();
            if st.work_schedule {
                schedules(ui, sim, st);
                return;
            }
            ui.label(RichText::new("Left click: higher priority, right click: lower. 1 = first ... 4 = last, - = never").weak());
            egui::Grid::new("work_grid").striped(true).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label("");
                for wt in WorkType::ALL {
                    ui.label(RichText::new(wt.name()).strong());
                }
                ui.end_row();
                for (id, name, prios, skills) in &rows {
                    ui.label(name);
                    for wt in WorkType::ALL {
                        let prio = prios[wt];
                        let label = match (prio.rank(), wt.skill()) {
                            (None, _) => "-".to_string(),
                            (Some(p), Some(s)) => format!("{}  (lv {})", p, skills[s]),
                            (Some(p), None) => p.to_string(),
                        };
                        let button = egui::Button::new(label).fill(priority_color(prio)).min_size(egui::vec2(84.0, 22.0));
                        let response = ui.add(button);
                        if response.clicked() {
                            sim.world.set_priority(*id, wt, prio.raised());
                        } else if response.secondary_clicked() {
                            sim.world.set_priority(*id, wt, prio.lowered());
                        }
                    }
                    ui.end_row();
                }
            });
        });
    st.show_work = open;
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_notice_ignores_job_gaps_and_brief_returns_to_work() {
        let mut notice = IdleNotice::default();
        assert_eq!(notice.update(1, 0.0), 0);
        assert_eq!(notice.update(0, 0.5), 0, "a brief gap between jobs isn't an alert");
        assert_eq!(notice.update(1, 1.0), 0);
        assert_eq!(notice.update(1, 3.9), 0);
        assert_eq!(notice.update(1, 4.0), 1, "sustained idleness is shown");
        assert_eq!(notice.update(0, 4.1), 1);
        assert_eq!(notice.update(1, 5.0), 1, "a brief job doesn't make the warning disappear");
        assert_eq!(notice.update(0, 6.0), 1);
        assert_eq!(notice.update(0, 7.9), 1);
        assert_eq!(notice.update(0, 8.0), 0, "sustained work clears the warning");
    }

    #[test]
    fn idle_notice_keeps_the_last_stable_count_during_changes() {
        let mut notice = IdleNotice::default();
        notice.update(1, 0.0);
        assert_eq!(notice.update(1, 3.0), 1);
        assert_eq!(notice.update(2, 4.0), 1);
        assert_eq!(notice.update(1, 5.0), 1);
        assert_eq!(notice.update(2, 6.0), 1);
        assert_eq!(notice.update(2, 9.0), 2);
    }

    #[test]
    fn loading_another_world_resets_pending_and_visible_idle_notices() {
        let mut state = NotificationState::default();
        state.idle_count(0, 1, 0.0);
        assert_eq!(state.idle_count(0, 1, 3.0), 1);
        assert_eq!(state.idle_count(1, 0, 3.1), 0, "don't carry a previous world's warnings");
        state.idle_count(1, 2, 4.0);
        assert_eq!(state.idle_count(2, 2, 6.0), 0, "pending timers reset too");
        assert_eq!(state.idle_count(2, 2, 9.0), 2);
    }

    #[test]
    fn save_confirmation_stays_in_place_when_idle_alert_appears_and_clears() {
        fn text_y(shape: &egui::Shape, text: &str) -> Option<f32> {
            match shape {
                egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos.y),
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| text_y(s, text)),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut sim = Sim::new(None);
        sim.world = SimWorld::with_map(my_world::map::Map::flat(8, 8), my_world::rng::Rng::new(1));
        let id = sim.world.spawn_colonist((3, 3, my_world::map::FLAT_Z));
        sim.world.push_log("Game saved.".into());
        let mut state = NotificationState::default();
        let mut frame = |sim: &Sim, now| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
                time: Some(now),
                ..Default::default()
            });
            notifications(&ctx, sim, &mut state);
            let output = ctx.end_pass();
            let y = output.shapes.iter().find_map(|s| text_y(&s.shape, "Game saved."));
            output.drop_without_applying_deltas();
            y
        };
        // Warm egui's area measurement and fonts before recording positions.
        frame(&sim, 0.0);
        frame(&sim, 0.1);
        let before = frame(&sim, 0.2).expect("save confirmation visible");
        let during = frame(&sim, 3.2).expect("still visible when idle alert appears");
        let i = sim.world.pawn_index(id).unwrap();
        sim.world.pawns[i].job = Some(my_world::pawn::Job::Goto { to: (4, 3, my_world::map::FLAT_Z) });
        frame(&sim, 4.0);
        let after = frame(&sim, 6.1).expect("still visible when idle alert clears");
        assert!((before - during).abs() < 0.5, "event moved from {before} to {during}");
        assert!((before - after).abs() < 0.5, "event moved from {before} to {after}");
    }

    #[test]
    fn clicks_step_priorities_up_and_down() {
        // Left: never, 4, 3, 2, 1, and no further.
        use WorkPriority::*;
        let up: Vec<WorkPriority> = std::iter::successors(Some(Never), |p| Some(p.raised())).take(6).collect();
        assert_eq!(up, [Never, Fourth, Third, Second, First, First]);
        // Right: back down to never, and no further.
        let down: Vec<WorkPriority> = std::iter::successors(Some(First), |p| Some(p.lowered())).take(6).collect();
        assert_eq!(down, [First, Second, Third, Fourth, Never, Never]);
        // Every step looks a little different.
        let shades: std::collections::HashSet<_> = [Never, First, Second, Third, Fourth].map(|p| priority_color(p).to_array()).into_iter().collect();
        assert_eq!(shades.len(), 5);
    }
}
