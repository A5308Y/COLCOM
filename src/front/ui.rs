//! egui interface: HUD and toolbar, colonist bar, alerts, inspector (with the
//! Health tab), work priorities, help and game over.

use super::camera::{CameraRig, MainCamera};
use super::audio::{self, AudioSettings, SoundQueue};
use super::build_menu;
use super::input::{self, InspectorTab, Selection, UiState};
use super::squads_ui;
use super::look::{pawn_look, Look, Rgb};
use super::overlay;
use super::sim::Sim;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, ProgressBar, RichText};
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use my_world::health::HediffKind;
use my_world::defs::{defs, BuildId, ItemId, PlantId, RoomDef, Special};
use my_world::map::{Block, Feature, Pos, Zone};
use my_world::pawn::{Pawn, Role, Skill, WorkType};
use my_world::production::{BillMode, PRIORITY_NAMES};
use my_world::rooms::{Progress, RoomStats, TIERS};
use my_world::world::{Order, World as SimWorld};
use my_world::DAY_LEN;

const HELP_ORDERS: [&str; 7] = [
    "You don't control colonists directly - you give orders and they pick up the work.",
    "",
    "Orders (build menu, left): mark trees to chop, rock and soil to mine, berry bushes to forage;",
    "      Backspace: deconstruct. Cancel takes designations and blueprints away.",
    "Zones: paint stockpile zones (items get hauled there) and grow zones (crops); Clear zone removes them.",
    "      Left-drag to paint a rectangle. Right-click (anywhere) goes back to Select.",
    "",
];

const HELP_REST: [&str; 43] = [
    "Colonists haul the materials to blueprints, then build them.",
    "Workbenches (stove, sawing bench, stonecutter, smelter, smithy, crafting bench): click one to add bills -",
    "      make N, make until you have N, or forever. Haulers bring the ingredients; cooks and crafters make it.",
    "The build menu on the left lists everything (` hides it); hover an entry for details.",
    "Click a stockpile to set its priority and which items it takes; click a grow zone to pick its crop.",
    "Shelves are storage by themselves and hold four stacks; click one to set what goes on it (shelves",
    "      side by side share settings). Colonists eat at a table if there's a chair beside it, and hate eating without.",
    "Themed rooms (Rooms button): furnish a room with what a theme needs - bedroom, barracks, kitchen,",
    "      smithy, workshop - and its floor and name show it. Click a cell in a room for a checklist.",
    "Room quality (size, beauty of furniture, windows and doors, cleanliness, light) affects mood when",
    "      colonists sleep or eat there. Click a cell inside a room to see why it's good or bad.",
    "Raw food can cause food poisoning - cook meals at a stove. Mine iron ore deep in the rock,",
    "      smelt it into steel and make weapons and armour at a smithy. Colonists pick up better gear;",
    "      select one and right-click a weapon or clothes to have them use it (and keep it).",
    "",
    "Floors are for upper storeys; they need a wall, stairs or ground below within 3 cells.",
    "      Use the layer view to place floors in mid-air: PageDown/PageUp (or [ and ]).",
    "      Shift+R hides roofs, to look into the top room of every building at once.",
    "Stairs rise away from where you walk onto them; you can't walk across them.",
    "Lamps light up rooms and caves - nights are dark.",
    "",
    "A room needs walls and a roof. Enclose an area with walls and doors and it gets a roof by itself:",
    "      builders put it up with wood. A roof needs a wall within 3 cells, like floors: red crosses mark",
    "      where it can't reach. Sleeping in a roofed bedroom makes colonists happier.",
    "Mining also digs soil. Dig straight down to sink a shaft; put stairs in it.",
    "Anything left with nothing underneath falls - more than one level hurts.",
    "Dig graves (Structure menu, on soil) for the dead: haulers bury them. Unburied dead upset everyone.",
    "Wounds bleed and can get infected. Doctors (see P) rescue downed colonists to a bed",
    "and tend them - better with medicine and skill. Click a colonist, then Health for details.",
    "Raids come every few days. Pay what the raiders demand, or fight a turn-based battle (the colony stops).",
    "      Squads: in a colonist's Combat tab make them a leader, put others in their squad, set loadouts;",
    "      mark civilians and paint a safe zone. Far-away squad members join their leader a few rounds in.",
    "      Leaders want a decent bedroom of their own: click a bed to choose who sleeps in it.",
    "      A squad is all ranged or all melee (by the loadout weapon, or the one they carry).",
    "      In battle: click your unit (or Tab), click a cell to move, an enemy to attack; Space ends its turn.",
    "      Over each unit: health per member, then morale and suppression (the tick is where it breaks or",
    "      gets pinned). Hover the bars in the battle panel to see what they do.",
    "",
    "Camera: WASD/arrows pan, Q/E rotate, wheel zooms, right-drag orbits, middle-drag pans.",
    "Space: pause   1-4: speed (1x, 2x, 5x, 10x)   P: work priorities   I: resources   C: center camera",
    "Esc: menu (save, load, new game, sound)   K: save   L: load   Shift+N: new colony",
    "",
    "H or Esc closes this help.",
];

/// One line per buildable the player can place: shortcut, name and cost.
fn help_builds() -> Vec<String> {
    BuildId::all()
        .filter(|kind| kind.def().special != Special::Roof)
        .map(|kind| {
            let def = kind.def();
            let key = def.key.as_deref().unwrap_or("").to_uppercase();
            format!("{:<5} {} ({})", key, def.name, def.cost_text())
        })
        .collect()
}

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
    overlay::draw(&ctx, &sim.world, cam, gt, rig.distance, &st, view);
    // The selection box while dragging with the Select tool.
    if let (Some(a), Some(b)) = (st.box_start, ctx.pointer_latest_pos()) {
        let a = egui::pos2(a.x, a.y);
        if a.distance(b) > 6.0 {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("select_box")));
            let rect = egui::Rect::from_two_pos(a, b);
            painter.rect_filled(rect, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18));
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Inside);
        }
    }

    tool_at_cursor(&ctx, &st);
    job_menu(&ctx, &mut sim, &mut st, &mut sounds);
    hud(&ctx, &mut sim, &mut st, &mut rig, left);
    colonist_bar(&ctx, &sim, &mut st, left);
    alerts(&ctx, &sim);
    match st.selected {
        Some(Selection::Pawn(id)) => pawn_inspector(&ctx, &mut sim, &mut st, id, left),
        Some(Selection::Tile(t)) => {
            let mut move_from = None;
            let (changed, open) = tile_inspector(&ctx, &mut sim, t, left, &mut move_from);
            if changed {
                sounds.play("ui_click", None);
            }
            if let Some(from) = move_from {
                st.tool = input::Tool::Move(Some(from));
                st.drag_start = None;
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
    if st.show_rooms {
        rooms_window(&ctx, &sim, &mut st, &mut rig);
    }
    if st.show_spawn {
        spawn_window(&ctx, &mut st);
    }
    if st.show_menu {
        game_menu(&ctx, &mut sim, &mut st, &mut rig, &mut audio);
    }
    if st.show_help {
        egui::Window::new("Help").open(&mut st.show_help).collapsible(false).resizable(false).anchor(
            Align2::CENTER_CENTER,
            [0.0, 0.0],
        ).show(&ctx, |ui| {
            for line in HELP_ORDERS {
                ui.label(line);
            }
            for line in help_builds() {
                ui.label(line);
            }
            for line in HELP_REST {
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

/// The right-click menu: what the selected colonist could do there. Picking
/// one has them do it now; clicking elsewhere or Esc (`input::keyboard`) closes it.
fn job_menu(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, sounds: &mut SoundQueue) {
    let Some(menu) = st.job_menu.as_mut() else { return };
    let at = *menu.at.get_or_insert_with(|| ctx.pointer_latest_pos().unwrap_or_default() + egui::vec2(6.0, 6.0));
    let name = sim.world.pawn(menu.pawn).map_or("", |p| p.name.as_str()).to_string();
    let mut chosen = None;
    let area = egui::Area::new(egui::Id::new("job_menu")).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(&ctx.global_style()).show(ui, |ui| {
            ui.label(RichText::new(name).small().weak());
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
fn tool_at_cursor(ctx: &egui::Context, st: &UiState) {
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
    let turn = matches!(st.tool, input::Tool::Paint(my_world::world::Order::Build(k)) if k.def().turnable());
    let text = if turn { format!("{}  (, . turn   right-click: stop)", name) } else { format!("{}  (right-click: stop)", name) };
    let galley = painter.layout_no_wrap(text, font, Color32::from_rgb(255, 225, 140));
    let rect = egui::Rect::from_min_size(pos, galley.size() + egui::vec2(10.0, 4.0));
    painter.rect_filled(rect, 3.0, Color32::from_black_alpha(190));
    painter.galley(pos + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
}

fn room_color(room: &RoomDef) -> Color32 {
    let (r, g, b) = room.floor;
    Rgb(r, g, b).egui().lerp_to_gamma(Color32::WHITE, 0.45)
}

const MET: Color32 = Color32::from_rgb(120, 220, 120);
const UNMET: Color32 = Color32::from_rgb(255, 140, 100);
const PERK: Color32 = Color32::from_rgb(150, 200, 150);

/// What a themed room does, one muted line each.
fn room_perks(ui: &mut egui::Ui, room: &RoomDef, indent: &str) {
    for line in room.effects.lines() {
        ui.label(RichText::new(format!("{}+ {}", indent, line)).color(PERK));
    }
}

/// The room's theme if it has one, and a checklist towards each kind of
/// themed room, closest first (the closest unfinished one open).
fn room_themes(ui: &mut egui::Ui, r: &RoomStats) {
    let rooms = &defs().rooms;
    if let Some(k) = r.kind {
        ui.label(RichText::new(&rooms[k].name).strong().size(16.0).color(room_color(&rooms[k])));
        ui.label(RichText::new(&rooms[k].about).weak());
        room_perks(ui, &rooms[k], "");
    }
    let mut order: Vec<(usize, Progress)> = rooms.iter().enumerate().map(|(k, room)| (k, r.progress(room))).collect();
    // Closest first: the most needs met, as a share.
    order.sort_by(|a, b| {
        let share = |p: &Progress| {
            let (m, n) = p.score();
            m as f32 / n.max(1) as f32
        };
        share(&b.1).total_cmp(&share(&a.1))
    });
    let open = order.iter().find(|(k, p)| Some(*k) != r.kind && !p.done()).map(|(k, _)| *k);
    ui.label(RichText::new("Room themes").strong());
    for (k, p) in order {
        let room = &rooms[k];
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
    egui::Window::new("Rooms").open(&mut open).anchor(Align2::RIGHT_TOP, [-8.0, 92.0]).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.set_max_width(360.0);
        ui.label("Enclose an area with walls, doors and a roof to make a room. Furnish it with everything a theme \
                  needs and it becomes that kind of room: its floor changes and its name shows over it.");
        ui.label(RichText::new("Doors and windows in the walls count. Beds and workbenches the theme doesn't ask for spoil it.").small().weak());
        for (k, room) in defs().rooms.iter().enumerate() {
            ui.separator();
            let mine: Vec<&RoomStats> = w.room_stats.iter().filter(|r| r.kind == Some(k)).collect();
            ui.horizontal(|ui| {
                ui.label(RichText::new(&room.name).strong().color(room_color(room)));
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
    st.show_rooms = open;
    if let Some(t) = go {
        st.select(Some(Selection::Tile(t)));
        input::center_on_selection(sim, st, rig);
    }
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

fn hud(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, rig: &mut CameraRig, left: f32) {
    egui::Area::new(egui::Id::new("hud")).anchor(Align2::LEFT_BOTTOM, [left + 8.0, -8.0]).show(ctx, |ui| {
        egui::Frame::window(&ctx.global_style()).show(ui, |ui| {
            let w = &sim.world;
            let hour = w.hour();
            let clock = format!("Day {}  {:02}:{:02}", w.day(), hour as u32, ((hour - hour.floor()) * 60.0) as u32);
            let colonists = format!("Colonists {}", w.colonists_alive());
            ui.horizontal(|ui| {
                ui.label(RichText::new(clock).strong());
                ui.separator();
                ui.label(colonists);
                ui.separator();
                let pause = if sim.paused { "Resume (Space)" } else { "Pause (Space)" };
                if ui.button(pause).clicked() {
                    sim.paused = !sim.paused;
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
                    ui.label(RichText::new(format!("Scenario: {} (Shift+N restarts)", name)).color(Color32::LIGHT_BLUE));
                }
            });
            ui.horizontal(|ui| {
                let view = match st.view_level {
                    Some(l) => format!("Viewing level {} of {}", l, sim.world.map.d - 1),
                    None => "Viewing all levels".to_string(),
                };
                ui.label(RichText::new(view).strong());
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
            ui.add_space(2.0);
            for (k, msg) in sim.world.log.iter().rev().take(3).enumerate() {
                let alpha = 255 - k as u8 * 60;
                ui.label(RichText::new(msg).small().color(Color32::from_rgba_unmultiplied(225, 225, 170, alpha)));
            }
        });
    });
}

/// What the colony has, by category, leaving out what it has none of.
/// The game menu: save, load, a new game (asked twice) and the sound settings.
fn game_menu(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, rig: &mut CameraRig, audio: &mut AudioSettings) {
    let mut open = true;
    egui::Window::new(RichText::new("Menu").strong())
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
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
                ui.label(RichText::new("Start over? The current game is lost unless you saved it.").color(Color32::from_rgb(255, 180, 110)));
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
            ui.label(RichText::new("Save as").strong());
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
            ui.label(RichText::new("Sound").strong());
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
    egui::Window::new("Spawn items (testing)").open(&mut open).anchor(Align2::RIGHT_TOP, [-8.0, 92.0]).collapsible(false).resizable(false).show(ctx, |ui| {
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
                egui::CollapsingHeader::new(cat).id_salt(("spawn", cat)).default_open(cat == "Materials" || cat == "Components").show(ui, |ui| {
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
    ui.label(RichText::new("Difficulty").strong());
    ui.checkbox(&mut w.difficulty.raids, "Raids").on_hover_text("Off: no raiders come (the rest of the game goes on).");
    ui.add_enabled_ui(w.difficulty.raids, |ui| {
        ui.horizontal(|ui| {
            ui.label("Raid size");
            ui.add(egui::Slider::new(&mut w.difficulty.raid_size, my_world::story::RAID_SIZES).step_by(0.25).suffix("×"));
        });
        let hours = ((w.story.next_raid - w.time).max(0.0) / DAY_LEN * 24.0).round();
        let text = format!("The next raid would be {} raiders, in about {} h.", w.raid_size(), hours);
        ui.label(RichText::new(text).small().weak());
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
fn item_categories(keep: impl Fn(ItemId) -> bool) -> Vec<&'static str> {
    let mut categories: Vec<&'static str> = Vec::new();
    for k in ItemId::all().filter(|&k| keep(k)) {
        if !categories.contains(&k.def().category.as_str()) {
            categories.push(&k.def().category);
        }
    }
    categories
}

fn resources(ctx: &egui::Context, sim: &Sim, st: &mut UiState) {
    let totals = sim.world.item_totals();
    let categories = item_categories(|k| totals[k.index()] > 0);
    egui::Window::new("Resources")
        .open(&mut st.show_resources)
        .anchor(Align2::RIGHT_BOTTOM, [-8.0, -8.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            // What the colony's solar panels and batteries could do.
            let (output, storage) = sim.world.map.feature.iter().filter_map(|f| f.structure()?.power).fold((0.0, 0.0), |a, p| (a.0 + p.output, a.1 + p.storage));
            if output > 0.0 || storage > 0.0 {
                ui.label(RichText::new("Power").strong());
                ui.label(format!("{:.1} kW solar, {:.0} kWh storage", output / 1000.0, storage / 1000.0));
                ui.label(RichText::new("Nothing uses power yet.").small().weak());
                ui.add_space(4.0);
            }
            if categories.is_empty() {
                ui.label(RichText::new("Nothing yet").weak());
            }
            for cat in categories {
                ui.label(RichText::new(cat).strong());
                egui::Grid::new(("resources", cat)).num_columns(2).spacing([24.0, 2.0]).show(ui, |ui| {
                    for k in ItemId::all().filter(|k| k.def().category == cat && totals[k.index()] > 0) {
                        ui.label(k.name());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(totals[k.index()].to_string());
                        });
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
            }
        });
}

/// Inside size of a card in the colonist list: the same for everyone, whatever they're doing.
const CARD: egui::Vec2 = egui::vec2(150.0, 62.0);
const CARD_MARGIN: f32 = 6.0;
/// Width the colonist list takes from the left edge (after the build menu);
/// the inspectors open to its right.
const COLONIST_LIST_W: f32 = 8.0 + CARD.x + 2.0 * CARD_MARGIN + 4.0;

fn colonist_bar(ctx: &egui::Context, sim: &Sim, st: &mut UiState, left: f32) {
    // Leave room for the HUD at the bottom; scroll if there are more.
    let room = (ctx.viewport_rect().height() - 90.0).max(CARD.y * 2.0);
    egui::Area::new(egui::Id::new("colonists")).anchor(Align2::LEFT_TOP, [left + 8.0, 8.0]).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            // The buried are at rest in their graves (click one to see whose it is).
            for p in sim.world.pawns.iter().filter(|p| p.is_colonist() && !p.buried) {
                let selected = st.is_selected(p.id);
                let trouble = p.downed || p.hunger <= 0.0 || p.break_timer > 0.0 || p.health.bleed_rate() > 0.0;
                let stroke = if selected {
                    egui::Stroke::new(2.0, Color32::WHITE)
                } else if sim.world.battle.as_ref().is_some_and(|b| b.unit_of(p.id).is_some_and(|u| b.units[u].can_act())) {
                    egui::Stroke::new(2.0, Color32::GOLD)
                } else if trouble && !p.dead {
                    egui::Stroke::new(2.0, Color32::RED)
                } else {
                    egui::Stroke::new(1.0, Color32::from_gray(90))
                };
                let resp = egui::Frame::window(&ctx.global_style())
                    .stroke(stroke)
                    .inner_margin(CARD_MARGIN)
                    .show(ui, |ui| {
                        ui.set_min_size(CARD);
                        ui.set_max_size(CARD);
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.horizontal(|ui| {
                            portrait(ui, &pawn_look(p), 18.0);
                            ui.add(egui::Label::new(RichText::new(&p.name).strong()).truncate());
                        });
                        ui.add(ProgressBar::new(p.mood / 100.0).desired_width(CARD.x).desired_height(6.0).fill(mood_color(p.mood)));
                        // One line, cut short with an ellipsis; the whole text on hover.
                        let status = p.status();
                        ui.add(egui::Label::new(RichText::new(&status).small()).truncate()).on_hover_text(&status);
                        let weapon = p.weapon.map_or("Unarmed", |w| w.name());
                        let color = if p.weapon.is_some() { Color32::from_rgb(230, 205, 140) } else { Color32::GRAY };
                        ui.add(egui::Label::new(RichText::new(weapon).small().color(color)).truncate());
                    })
                    .response
                    .interact(egui::Sense::click());
                if resp.clicked() {
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

fn pawn_inspector(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState, id: u32, left: f32) {
    let Some(p) = sim.world.pawn(id) else { return };
    let combat = p.is_living_colonist();
    if !combat && st.inspector_tab == InspectorTab::Combat {
        st.inspector_tab = InspectorTab::Overview;
    }
    let mut actions = Vec::new();
    let mut world_actions: Vec<Action> = Vec::new();
    let title = if p.is_colonist() { p.name.clone() } else { format!("{} (raider)", p.name) };
    let mut open = true;
    egui::Window::new(RichText::new(title).strong())
        .id(egui::Id::new("inspector"))
        .open(&mut open)
        .anchor(Align2::LEFT_TOP, [left + COLONIST_LIST_W + 8.0, 8.0])
        .collapsible(false)
        .resizable(false)
        .default_width(340.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut st.inspector_tab, InspectorTab::Overview, "Overview");
                ui.selectable_value(&mut st.inspector_tab, InspectorTab::Health, "Health");
                if combat {
                    ui.selectable_value(&mut st.inspector_tab, InspectorTab::Combat, "Combat");
                }
            });
            if st.group.len() > 1 {
                let others = st.group.len() - 1;
                let text = format!("and {} more selected (orders go to all; Shift+click to add or remove)", others);
                ui.label(RichText::new(text).small());
            }
            ui.label(RichText::new(format!("{} - {}", p.status(), p.weapon_name())).weak());
            if !p.apparel.is_empty() {
                let worn: Vec<&str> = p.apparel.iter().map(|a| a.name()).collect();
                ui.label(RichText::new(format!("Wearing {}", worn.join(", ").to_lowercase())).weak());
            }
            ui.separator();
            match st.inspector_tab {
                InspectorTab::Overview => overview_tab(ui, p),
                InspectorTab::Health => {
                    health_tab(ui, p);
                    operations(ui, &sim.world, p, &mut world_actions);
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
    if !open {
        st.select(None);
    }
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
    if p.is_living_colonist() {
        ui.label(RichText::new("Right-click a cell: prioritize a job there, or equip gear   Combat tab: squad and loadout").small().weak());
    }
}

/// Lost limbs and the operations ordered for them: fit a prosthetic, or call it off.
fn operations(ui: &mut egui::Ui, w: &SimWorld, p: &Pawn, actions: &mut Vec<Action>) {
    let lost = p.health.lost_parts();
    if !p.is_colonist() || p.dead || lost.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Operations").strong());
    let id = p.id;
    let tables = (0..w.map.len()).any(|i| w.medical_bed(w.map.pos(i)));
    for part in lost {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{}: lost", part.name()));
            match p.operations.iter().find(|o| o.part == part) {
                Some(op) => {
                    let have = w.map.count_items(op.item);
                    ui.label(RichText::new(format!("fitting a {}", op.item.name().to_lowercase())).color(Color32::from_rgb(150, 190, 230)));
                    if ui.small_button("Cancel").clicked() {
                        actions.push(Box::new(move |w| {
                            w.cancel_operation(id, part);
                        }));
                    }
                    let waiting = if have == 0 {
                        Some(format!("Waiting for a {} (a smithy makes them).", op.item.name().to_lowercase()))
                    } else if !tables {
                        Some("Waiting for an operating table.".to_string())
                    } else {
                        None
                    };
                    if let Some(text) = waiting {
                        ui.label(RichText::new(text).small().color(Color32::from_rgb(240, 210, 90)));
                    }
                }
                None => {
                    for item in SimWorld::prosthetics_for(part) {
                        let have = w.map.count_items(item);
                        let hint = format!(
                            "A doctor fits it at an operating table ({} in the colony). It can go wrong: better doctors and a surgery room help.",
                            have
                        );
                        if ui.small_button(format!("Fit {}", item.name().to_lowercase())).on_hover_text(hint).clicked() {
                            actions.push(Box::new(move |w| {
                                w.order_operation(id, part, item);
                            }));
                        }
                    }
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
            HediffKind::FoodPoisoning => (format!(" {:.0}%", d.severity * 100.0), Color32::from_rgb(240, 220, 90)),
            HediffKind::Prosthetic { .. } => (String::new(), Color32::from_rgb(150, 190, 230)),
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

/// A change to the world chosen in the inspector, applied after drawing it.
type Action = Box<dyn FnOnce(&mut SimWorld)>;

/// Returns whether anything was changed in it.
/// The cell inspector; returns whether something was changed, and whether it's still open.
/// `move_from` is set if the player chose to move the furniture here.
fn tile_inspector(ctx: &egui::Context, sim: &mut Sim, t: Pos, left: f32, move_from: &mut Option<Pos>) -> (bool, bool) {
    let mut actions: Vec<Action> = Vec::new();
    let w = &sim.world;
    let m = &w.map;
    let i = m.idx(t);
    let mut open = true;
    egui::Window::new(RichText::new(format!("Cell {}, {}  level {}", t.0, t.1, t.2)).strong())
        .id(egui::Id::new("inspector"))
        .open(&mut open)
        .anchor(Align2::LEFT_TOP, [left + COLONIST_LIST_W + 8.0, 8.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let what = match m.feature[i] {
                f @ Feature::Plant { growth, .. } if f.is_crop() => format!("{}, {:.0}% grown", f.name(), growth * 100.0),
                Feature::Blueprint { kind, delivered, .. } => {
                    let d = kind.def();
                    match d.missing(delivered) {
                        Some((item, n)) => format!(
                            "{} blueprint ({}/{} delivered; needs {} {} next)",
                            kind.name(),
                            delivered,
                            d.total_cost(),
                            n,
                            item.name().to_lowercase()
                        ),
                        None => format!("{} blueprint (all {} delivered)", kind.name(), d.total_cost()),
                    }
                }
                Feature::None if m.block[i] != Block::Air => m.block[i].name().to_string(),
                f => {
                    let ground = m.below(t).map_or(Block::Air, |b| m.block[m.idx(b)]);
                    format!("{} on {}", f.name(), ground.name().to_lowercase())
                }
            };
            ui.label(what);
            if m.floor[i] {
                ui.label("Built floor");
            }
            if m.roof[i] {
                ui.label("Roof");
            }
            // Here or overhead: a room wants a roof, but nothing would hold it up.
            let above = m.above(t).map(|a| m.idx(a));
            if m.roof_gaps.contains(&i) || above.is_some_and(|a| m.roof_gaps.contains(&a)) {
                let why = "No roof can reach here: it needs a wall within 3 cells, so the room isn't closed.";
                ui.label(RichText::new(why).color(Color32::from_rgb(255, 140, 100)));
            }
            if m.covered(t) && !m.roofed(t) {
                ui.label("Covered overhead");
            }
            for s in m.stacks(i) {
                ui.label(format!("{} x{}", s.kind.name(), s.count));
                if let Some(about) = &s.kind.def().about {
                    ui.label(RichText::new(about).weak());
                }
            }
            if m.slots(i) > 1 {
                let used = m.stacks(i).count();
                ui.label(RichText::new(format!("{} of {} shelf slots used (50 each)", used, m.slots(i))).weak());
            }
            if let Some(power) = m.feature[i].structure().and_then(|d| d.power_text()) {
                ui.label(RichText::new(format!("{} (nothing uses power yet)", power)).weak());
            }
            if let Some(d) = m.desig[i] {
                ui.label(format!("Designated: {:?}", d));
            }
            match w.room_at(t) {
                Some(r) => {
                    let tier = TIERS[r.tier() as usize];
                    ui.label(format!("Indoors: {} room, {} tiles (quality {:+.1})", tier.to_lowercase(), r.size, r.score()));
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
                    ui.label("Outdoors");
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

/// Moving the furniture here, or how its move is going.
fn furniture_move(ui: &mut egui::Ui, w: &SimWorld, t: Pos, move_from: &mut Option<Pos>, actions: &mut Vec<Action>) {
    let cell = |p: Pos| format!("cell {}, {}", p.0, p.1);
    let (text, call_off) = if let Some(mv) = w.move_from(t) {
        let how = if mv.lifted { "Packed up" } else { "Waiting to be packed up" };
        (format!("{}: moving {} to {}.", how, mv.kind.name().to_lowercase(), cell(mv.to)), "Leave it here")
    } else if let Some(mv) = w.move_to(t) {
        let how = if mv.lifted { "packed up, waiting to be set down here" } else { "waiting to be packed up" };
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
    ui.label(RichText::new(text).color(Color32::from_rgb(120, 210, 255)));
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
            ui.label(RichText::new(why).color(Color32::from_rgb(255, 140, 100)));
        }
    }
}

fn stockpile_settings(ui: &mut egui::Ui, w: &SimWorld, id: u16, actions: &mut Vec<Action>) {
    let Some(store) = w.stockpiles.get(id as usize) else { return };
    ui.separator();
    ui.label(RichText::new(&store.name).strong());
    ui.horizontal(|ui| {
        ui.label("Priority");
        for prio in 1..=4u8 {
            if ui.add(egui::Button::new(PRIORITY_NAMES[prio as usize]).selected(store.priority == prio)).clicked() {
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
    ui.label(RichText::new("Bills").strong());
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
                if ui.small_button("-").clicked() && n > 1 {
                    let mode = with(n - 1);
                    actions.push(Box::new(move |w| w.set_bill(t, k, mode, b.suspended)));
                }
                if ui.small_button("+").clicked() {
                    let mode = with(n + 1);
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

fn work_panel(ctx: &egui::Context, sim: &mut Sim, st: &mut UiState) {
    let rows: Vec<(u32, String, [u8; WorkType::ALL.len()], [u8; Skill::ALL.len()])> = sim
        .world
        .pawns
        .iter()
        .filter(|p| p.is_living_colonist())
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

