//! Player input: picking tiles and pawns, tools, keyboard shortcuts, and the
//! in-world markers (designations, selection, drag box, gunfire) as gizmos.

use super::camera::{colony_center, CameraRig, DragTracker, MainCamera};
use super::sim::{seed, Sim};
use super::terrain::{surface_height, ROCK_H, WATER_Y};
use super::{tile_center, tile_point};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{egui_wants_any_keyboard_input, EguiWantsInput};
use my_world::map::{BuildKind, Desig, Map, Pos, Zone};
use my_world::pawn::Faction;
use my_world::world::{Order, World as SimWorld};
use std::f32::consts::FRAC_PI_2;

pub const SAVE_PATH: &str = "savegame.json";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Paint(Order),
}

pub const TOOLS: [(Tool, &str, KeyCode); 11] = [
    (Tool::Select, "Esc Select", KeyCode::Escape),
    (Tool::Paint(Order::Desig(Desig::Chop)), "1 Chop", KeyCode::Digit1),
    (Tool::Paint(Order::Desig(Desig::Mine)), "2 Mine", KeyCode::Digit2),
    (Tool::Paint(Order::Desig(Desig::Forage)), "3 Forage", KeyCode::Digit3),
    (Tool::Paint(Order::Zone(Zone::Stockpile)), "4 Stockpile", KeyCode::Digit4),
    (Tool::Paint(Order::Zone(Zone::Grow)), "5 Grow zone", KeyCode::Digit5),
    (Tool::Paint(Order::Build(BuildKind::Wall)), "6 Wall", KeyCode::Digit6),
    (Tool::Paint(Order::Build(BuildKind::Door)), "7 Door", KeyCode::Digit7),
    (Tool::Paint(Order::Build(BuildKind::Bed)), "8 Bed", KeyCode::Digit8),
    (Tool::Paint(Order::Zone(Zone::None)), "9 Clear zone", KeyCode::Digit9),
    (Tool::Paint(Order::Cancel), "0 Cancel", KeyCode::Digit0),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Pawn(u32),
    Tile(Pos),
}

#[derive(Resource)]
pub struct UiState {
    pub tool: Tool,
    pub selected: Option<Selection>,
    pub drag_start: Option<Pos>,
    /// Tile under the cursor (last known, so drags can end over the UI).
    pub hover: Option<Pos>,
    pub show_work: bool,
    pub show_help: bool,
    pub inspector_health: bool,
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiState {
            tool: Tool::Select,
            selected: None,
            drag_start: None,
            hover: None,
            show_work: false,
            show_help: false,
            inspector_health: false,
        })
        .add_systems(
            Update,
            (
                update_hover,
                keyboard.run_if(not(egui_wants_any_keyboard_input)),
                mouse,
                forget_missing_selection,
                draw_markers,
            )
                .chain(),
        );
    }
}

// ---------- picking ----------

/// March along the camera ray until it dips below the surface.
fn pick_tile(m: &Map, ray: Ray3d) -> Option<Pos> {
    let (o, d) = (ray.origin, *ray.direction);
    if d.y > -1e-4 {
        return None;
    }
    let t0 = ((ROCK_H + 0.1 - o.y) / d.y).max(0.0);
    let t1 = (WATER_Y - 0.1 - o.y) / d.y;
    let step = 0.04;
    let steps = (((t1 - t0) / step).ceil() as usize).min(20_000);
    for k in 0..=steps {
        let p = o + d * (t0 + k as f32 * step);
        let (x, z) = (p.x.floor(), p.z.floor());
        if x < 0.0 || z < 0.0 || x >= m.w as f32 || z >= m.h as f32 {
            continue;
        }
        let (x, z) = (x as usize, z as usize);
        if p.y <= surface_height(m, x, z) {
            return Some((x, z));
        }
    }
    None
}

fn update_hover(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    sim: Res<Sim>,
    mut ui: ResMut<UiState>,
) {
    let (cam, gt) = *camera;
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(ray) = cam.viewport_to_world(gt, cursor) else { return };
    if let Some(t) = pick_tile(&sim.world.map, ray) {
        ui.hover = Some(t);
    }
}

/// Pawn nearest to a tile (living ones preferred), if close enough.
pub fn pawn_at(w: &SimWorld, t: Pos) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for p in w.pawns.iter().filter(|p| !p.left) {
        let d = ((p.x - t.0 as f32).powi(2) + (p.y - t.1 as f32).powi(2)).sqrt() + if p.dead { 0.5 } else { 0.0 };
        if d < 0.9 && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, p.id));
        }
    }
    best.map(|(_, id)| id)
}

// ---------- actions shared by keys and buttons ----------

pub fn toggle_draft(sim: &mut Sim, ui: &UiState) {
    if let Some(Selection::Pawn(id)) = ui.selected {
        if let Some(p) = sim.world.pawn(id) {
            if p.is_colonist() {
                let d = !p.drafted;
                sim.world.set_drafted(id, d);
                return;
            }
        }
    }
    let colonists: Vec<(u32, bool)> =
        sim.world.pawns.iter().filter(|p| p.is_colonist() && p.active()).map(|p| (p.id, p.drafted)).collect();
    let draft = colonists.iter().any(|(_, d)| !d);
    for (id, _) in colonists {
        sim.world.set_drafted(id, draft);
    }
}

pub fn save(sim: &mut Sim) {
    let msg = match sim.world.save_json().map_err(|e| e.to_string()).and_then(|json| {
        std::fs::write(SAVE_PATH, json).map_err(|e| e.to_string())
    }) {
        Ok(()) => format!("Game saved to {}.", SAVE_PATH),
        Err(e) => format!("Saving failed: {}", e),
    };
    sim.world.push_log(msg);
}

pub fn load(sim: &mut Sim, ui: &mut UiState) {
    let loaded = std::fs::read_to_string(SAVE_PATH)
        .map_err(|e| e.to_string())
        .and_then(|s| SimWorld::load_json(&s).map_err(|e| e.to_string()));
    match loaded {
        Ok(w) => {
            sim.replace(w);
            ui.selected = None;
            sim.world.push_log("Game loaded.".into());
        }
        Err(e) => sim.world.push_log(format!("Loading failed: {}", e)),
    }
}

pub fn new_colony(sim: &mut Sim, ui: &mut UiState, rig: &mut CameraRig) {
    sim.replace(SimWorld::new(seed()));
    ui.selected = None;
    rig.focus = colony_center(sim);
}

pub fn center_on_selection(sim: &Sim, ui: &UiState, rig: &mut CameraRig) {
    rig.focus = match ui.selected {
        Some(Selection::Pawn(id)) => match sim.world.pawn(id) {
            Some(p) => tile_point(p.x, p.y),
            None => return,
        },
        Some(Selection::Tile(t)) => tile_center(t),
        None => colony_center(sim),
    };
}

// ---------- systems ----------

fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>, mut ui: ResMut<UiState>, mut rig: ResMut<CameraRig>) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    if keys.just_pressed(KeyCode::Tab) {
        sim.cycle_speed();
    }
    if keys.just_pressed(KeyCode::Escape) {
        ui.show_help = false;
        ui.show_work = false;
    }
    for (tool, _, key) in TOOLS {
        if keys.just_pressed(key) {
            ui.tool = tool;
            ui.drag_start = None;
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        ui.show_work = !ui.show_work;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        ui.show_help = !ui.show_help;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        toggle_draft(&mut sim, &ui);
    }
    if keys.just_pressed(KeyCode::KeyC) {
        center_on_selection(&sim, &ui, &mut rig);
    }
    if keys.just_pressed(KeyCode::KeyK) {
        save(&mut sim);
    }
    if keys.just_pressed(KeyCode::KeyL) {
        load(&mut sim, &mut ui);
    }
    if keys.just_pressed(KeyCode::KeyN) && (shift || sim.world.colony_lost()) {
        new_colony(&mut sim, &mut ui, &mut rig);
    }
}

fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    egui_input: Res<EguiWantsInput>,
    drag: Res<DragTracker>,
    mut sim: ResMut<Sim>,
    mut ui: ResMut<UiState>,
) {
    let over_ui = egui_input.wants_any_pointer_input();
    let hover = ui.hover;
    match ui.tool {
        Tool::Select => {
            if buttons.just_pressed(MouseButton::Left) && !over_ui {
                if let Some(t) = hover {
                    ui.selected = Some(match pawn_at(&sim.world, t) {
                        Some(id) => Selection::Pawn(id),
                        None => Selection::Tile(t),
                    });
                }
            }
        }
        Tool::Paint(order) => {
            if buttons.just_pressed(MouseButton::Left) && !over_ui {
                ui.drag_start = hover;
            }
            if buttons.just_released(MouseButton::Left) {
                if let (Some(a), Some(b)) = (ui.drag_start, hover) {
                    for y in a.1.min(b.1)..=a.1.max(b.1) {
                        for x in a.0.min(b.0)..=a.0.max(b.0) {
                            sim.world.apply_order((x, y), order);
                        }
                    }
                }
                ui.drag_start = None;
            }
        }
    }

    // A right *click* (not a camera drag): back to Select, or order a drafted colonist.
    if buttons.just_released(MouseButton::Right) && drag.right_moved <= 4.0 && !over_ui {
        if let Tool::Paint(_) = ui.tool {
            ui.tool = Tool::Select;
            ui.drag_start = None;
            return;
        }
        let (Some(Selection::Pawn(id)), Some(t)) = (ui.selected, hover) else { return };
        if !sim.world.pawn(id).is_some_and(|p| p.drafted) {
            return;
        }
        let enemy = pawn_at(&sim.world, t)
            .filter(|&o| sim.world.pawn(o).is_some_and(|o| o.faction == Faction::Raider && o.active()));
        match enemy {
            Some(target) => sim.world.order_attack(id, target),
            None => sim.world.order_move(id, t),
        }
    }
}

fn forget_missing_selection(sim: Res<Sim>, mut ui: ResMut<UiState>) {
    if let Some(Selection::Pawn(id)) = ui.selected {
        if sim.world.pawn(id).is_none() {
            ui.selected = None;
        }
    }
}

/// Flat square outline lying on top of a tile's surface.
fn tile_outline(gizmos: &mut Gizmos, m: &Map, t: Pos, inset: f32, color: Color) {
    let c = tile_center(t) + Vec3::Y * (surface_height(m, t.0, t.1) + 0.03);
    gizmos.rect(Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2)), Vec2::splat(1.0 - inset), color);
}

fn draw_markers(mut gizmos: Gizmos, sim: Res<Sim>, ui: Res<UiState>) {
    let w = &sim.world;
    let m = &w.map;

    for i in 0..m.w * m.h {
        if let Some(d) = m.desig[i] {
            let color = match d {
                Desig::Chop => Color::srgb(1.0, 0.6, 0.1),
                Desig::Mine => Color::srgb(0.45, 0.8, 1.0),
                Desig::Forage => Color::srgb(1.0, 0.5, 0.75),
            };
            tile_outline(&mut gizmos, m, m.pos(i), 0.12, color);
        }
    }

    if let Some(t) = ui.hover {
        tile_outline(&mut gizmos, m, t, 0.02, Color::srgba(1.0, 1.0, 1.0, 0.6));
    }
    if let Some(Selection::Tile(t)) = ui.selected {
        tile_outline(&mut gizmos, m, t, 0.0, Color::srgb(1.0, 0.9, 0.2));
    }
    if let (Some(a), Some(b), Tool::Paint(_)) = (ui.drag_start, ui.hover, ui.tool) {
        let (lx, ly, hx, hy) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
        let c = Vec3::new((lx + hx + 1) as f32 / 2.0, 0.05, (ly + hy + 1) as f32 / 2.0);
        let size = Vec2::new((hx - lx + 1) as f32, (hy - ly + 1) as f32);
        gizmos.rect(Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2)), size, Color::WHITE);
    }

    for p in w.pawns.iter().filter(|p| !p.left && !p.dead) {
        let feet = Isometry3d::new(tile_point(p.x, p.y) + Vec3::Y * 0.03, Quat::from_rotation_x(FRAC_PI_2));
        if ui.selected == Some(Selection::Pawn(p.id)) {
            gizmos.circle(feet, 0.48, Color::WHITE);
        }
        if p.drafted {
            gizmos.circle(feet, 0.4, Color::srgb(1.0, 0.8, 0.1));
        } else if p.faction == Faction::Raider {
            gizmos.circle(feet, 0.4, Color::srgb(0.9, 0.15, 0.1));
        }
    }

    for e in &w.effects {
        let from = tile_point(e.from.0, e.from.1) + Vec3::Y * 0.45;
        let to = tile_point(e.to.0, e.to.1) + Vec3::Y * 0.5;
        let color = if e.hit { Color::srgb(1.0, 0.92, 0.5) } else { Color::srgba(0.9, 0.9, 0.9, 0.6) };
        gizmos.line(from, to, color);
    }
}
