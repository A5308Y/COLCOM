//! Player input: picking tiles and pawns, tools, keyboard shortcuts, and the
//! in-world markers (designations, selection, drag box, gunfire) as gizmos.

use super::camera::{colony_center, CameraRig, DragTracker, MainCamera};
use super::sim::{seed, Sim};
use super::terrain::{FLOOR_T, LEVEL_H};
use super::{tile_center, tile_point};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{egui_wants_any_keyboard_input, EguiWantsInput};
use my_world::defs::{defs, BuildId};
use my_world::map::{Block, Desig, Map, Pos, Zone};
use my_world::pawn::Faction;
use my_world::world::{Order, World as SimWorld};
use std::f32::consts::FRAC_PI_2;
use std::sync::LazyLock;

pub const SAVE_PATH: &str = "savegame.json";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Paint(Order),
}

/// Toolbar entries (tool, label, shortcut): fixed tools around one per
/// buildable in the definitions.
pub static TOOLS: LazyLock<Vec<(Tool, String, Option<KeyCode>)>> = LazyLock::new(|| {
    let fixed = |tool, label: &str, key| (tool, label.to_string(), Some(key));
    let mut tools = vec![
        fixed(Tool::Select, "Esc Select", KeyCode::Escape),
        fixed(Tool::Paint(Order::Desig(Desig::Chop)), "1 Chop", KeyCode::Digit1),
        fixed(Tool::Paint(Order::Desig(Desig::Mine)), "2 Mine", KeyCode::Digit2),
        fixed(Tool::Paint(Order::Desig(Desig::Forage)), "3 Forage", KeyCode::Digit3),
        fixed(Tool::Paint(Order::Stockpile), "4 Stockpile", KeyCode::Digit4),
    ];
    if let Some(crop) = defs().sown() {
        tools.push(fixed(Tool::Paint(Order::Zone(Zone::Grow(crop))), "5 Grow zone", KeyCode::Digit5));
    }
    for kind in BuildId::all() {
        let def = kind.def();
        let key = def.key.as_deref().and_then(|k| match parse_key(k) {
            Some(code) if !RESERVED.contains(&code) => Some((k, code)),
            _ => {
                warn!("{}: key \"{}\" can't be used as a shortcut", def.id, k);
                None
            }
        });
        let label = match key {
            Some((k, _)) => format!("{} {}", k.to_uppercase(), def.name),
            None => def.name.clone(),
        };
        tools.push((Tool::Paint(Order::Build(kind)), label, key.map(|k| k.1)));
    }
    tools.push(fixed(Tool::Paint(Order::Zone(Zone::None)), "9 Clear zone", KeyCode::Digit9));
    tools.push(fixed(Tool::Paint(Order::Cancel), "0 Cancel", KeyCode::Digit0));
    tools
});

/// Keys with fixed meanings, which buildables can't use as shortcuts.
const RESERVED: [KeyCode; 21] = [
    KeyCode::Escape,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit9,
    KeyCode::Digit0,
    KeyCode::KeyW,
    KeyCode::KeyA,
    KeyCode::KeyS,
    KeyCode::KeyD,
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::KeyP,
    KeyCode::KeyH,
    KeyCode::KeyF,
    KeyCode::KeyC,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyN,
];

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// A shortcut written in the definitions: one letter, digit, `-` or `=`.
fn parse_key(key: &str) -> Option<KeyCode> {
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return None };
    match c.to_ascii_uppercase() {
        c @ 'A'..='Z' => Some(LETTERS[(c as u8 - b'A') as usize]),
        c @ '0'..='9' => Some(DIGITS[(c as u8 - b'0') as usize]),
        '-' => Some(KeyCode::Minus),
        '=' => Some(KeyCode::Equal),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Pawn(u32),
    Tile(Pos),
}

/// What the cursor points at: the first solid thing along the view ray
/// (ground, rock, a wall, water) and the open cell on top of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hover {
    pub solid: Pos,
    pub stand: Pos,
}

#[derive(Resource)]
pub struct UiState {
    pub tool: Tool,
    pub selected: Option<Selection>,
    /// Cell where a paint drag started (its level is used for the whole box).
    pub drag_start: Option<Pos>,
    /// Under the cursor (last known, so drags can end over the UI).
    pub hover: Option<Hover>,
    pub show_work: bool,
    pub show_help: bool,
    pub inspector_health: bool,
    /// Layer view: hide everything above this level (None = show all).
    pub view_level: Option<usize>,
}

impl UiState {
    /// Highest level being shown.
    pub fn top_level(&self, m: &Map) -> usize {
        self.view_level.unwrap_or(m.d - 1).min(m.d - 1)
    }

    /// Whether something at (fractional) level `z` is visible in the layer view.
    pub fn shows(&self, m: &Map, z: f32) -> bool {
        z < self.top_level(m) as f32 + 0.5
    }
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
            view_level: None,
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

/// Solid for picking: ground, rock, water, or a wall.
fn stops_ray(m: &Map, p: Pos) -> bool {
    let i = m.idx(p);
    m.block[i] != Block::Air || m.feature[i].is_block()
}

/// Step along the camera ray until it enters something solid, ignoring
/// levels above `cut` (layer view). With a layer view, empty air on the
/// viewed level can be picked too, so floors can be placed there.
fn pick(m: &Map, ray: Ray3d, cut: usize) -> Option<Hover> {
    let layered = cut + 1 < m.d;
    let (o, d) = (ray.origin, *ray.direction);
    let top = m.d as f32 * LEVEL_H;
    // Start where the ray comes down to the top of the world, end below the bottom.
    let t0 = if o.y > top {
        if d.y >= 0.0 {
            return None;
        }
        (top - o.y) / d.y
    } else {
        0.0
    };
    let t1 = if d.y < 0.0 { (-0.1 - o.y) / d.y } else { t0 + 300.0 };
    // In the layer view, the floor of the viewed level is a pickable plane
    // (exactly where outlines are drawn), so empty air there can be targeted.
    let plane = (layered && d.y < 0.0).then(|| (cut as f32 * LEVEL_H - o.y) / d.y);
    let step = 0.04;
    let steps = (((t1 - t0) / step).ceil().max(0.0) as usize).min(20_000);
    for k in 0..=steps {
        let t = t0 + k as f32 * step;
        if let Some(tp) = plane.filter(|&tp| t >= tp) {
            let p = o + d * tp;
            let cell = m.at(p.x.floor() as i32, p.z.floor() as i32, cut as i32)?;
            return Some(Hover { solid: m.below(cell).unwrap_or(cell), stand: cell });
        }
        let p = o + d * t;
        let Some(cell) = m.at(p.x.floor() as i32, p.z.floor() as i32, (p.y / LEVEL_H).floor() as i32) else { continue };
        if cell.2 > cut {
            continue;
        }
        let i = m.idx(cell);
        let in_slab = p.y - cell.2 as f32 * LEVEL_H < FLOOR_T * 2.0;
        if m.floor[i] && in_slab {
            return Some(Hover { solid: m.below(cell).unwrap_or(cell), stand: cell });
        }
        if stops_ray(m, cell) {
            let stand = m.above(cell).unwrap_or(cell);
            return Some(Hover { solid: cell, stand });
        }
    }
    None
}

/// The cell an order should go to: rock and walls are mined where they are,
/// everything else happens in the open cell on top of what was clicked.
pub fn order_target(w: &SimWorld, h: Hover, order: Order) -> Pos {
    let first = match order {
        Order::Desig(Desig::Mine) => [h.solid, h.stand],
        _ => [h.stand, h.solid],
    };
    first.into_iter().find(|&p| w.order_applies(p, order)).unwrap_or(first[0])
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
    let cut = ui.top_level(&sim.world.map);
    if let Some(h) = pick(&sim.world.map, ray, cut) {
        ui.hover = Some(h);
    }
}

/// Pawn nearest to a cell (living ones preferred), if close enough.
pub fn pawn_at(w: &SimWorld, t: Pos) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for p in w.pawns.iter().filter(|p| !p.left) {
        let (dx, dy, dz) = (p.x - t.0 as f32, p.y - t.1 as f32, p.z - t.2 as f32);
        let d = (dx * dx + dy * dy + dz * dz).sqrt() + if p.dead { 0.5 } else { 0.0 };
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
    let msg = match sim.world.save_json().and_then(|json| {
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
        .and_then(|s| SimWorld::load_json(&s));
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

/// Layer view one level lower; the first step starts at the ground under the camera.
pub fn level_down(sim: &Sim, ui: &mut UiState, rig: &CameraRig) {
    let m = &sim.world.map;
    ui.view_level = Some(match ui.view_level {
        None => ((rig.focus.y / LEVEL_H).round() as usize).clamp(1, m.d - 1),
        Some(l) => l.saturating_sub(1).max(1),
    });
}

/// Layer view one level higher; past the top it shows everything again.
pub fn level_up(sim: &Sim, ui: &mut UiState) {
    let top = sim.world.map.d - 1;
    ui.view_level = match ui.view_level {
        Some(l) if l + 1 < top => Some(l + 1),
        _ => None,
    };
}

pub fn center_on_selection(sim: &Sim, ui: &UiState, rig: &mut CameraRig) {
    rig.focus = match ui.selected {
        Some(Selection::Pawn(id)) => match sim.world.pawn(id) {
            Some(p) => tile_point(p.x, p.y, p.z),
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
    for (tool, _, key) in TOOLS.iter() {
        if key.is_some_and(|k| keys.just_pressed(k)) {
            ui.tool = *tool;
            ui.drag_start = None;
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        ui.show_work = !ui.show_work;
    }
    if keys.any_just_pressed([KeyCode::PageDown, KeyCode::BracketLeft]) {
        level_down(&sim, &mut ui, &rig);
    }
    if keys.any_just_pressed([KeyCode::PageUp, KeyCode::BracketRight]) {
        level_up(&sim, &mut ui);
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
                if let Some(h) = hover {
                    let wall = sim.world.map.feature[sim.world.map.idx(h.solid)].is_block();
                    ui.selected = Some(match pawn_at(&sim.world, h.stand) {
                        Some(id) => Selection::Pawn(id),
                        None => Selection::Tile(if wall { h.solid } else { h.stand }),
                    });
                }
            }
        }
        Tool::Paint(order) => {
            if buttons.just_pressed(MouseButton::Left) && !over_ui {
                ui.drag_start = hover.map(|h| order_target(&sim.world, h, order));
            }
            if buttons.just_released(MouseButton::Left) {
                if let (Some(a), Some(h)) = (ui.drag_start, hover) {
                    let b = order_target(&sim.world, h, order);
                    paint_box(&mut sim.world, a, b, order);
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
        let (Some(Selection::Pawn(id)), Some(Hover { stand: t, .. })) = (ui.selected, hover) else { return };
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

/// Apply an order over a rectangle at the level where the drag started,
/// following the ground one level up or down where it slopes.
fn paint_box(w: &mut SimWorld, a: Pos, b: Pos, order: Order) {
    let z0 = a.2 as i32;
    for y in a.1.min(b.1)..=a.1.max(b.1) {
        for x in a.0.min(b.0)..=a.0.max(b.0) {
            for dz in [0, 1, -1] {
                if let Some(p) = w.map.at(x as i32, y as i32, z0 + dz) {
                    if w.apply_order(p, order) {
                        break;
                    }
                }
            }
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

/// Flat square outline on the floor of a cell.
fn floor_outline(gizmos: &mut Gizmos, t: Pos, inset: f32, color: Color) {
    let c = tile_center(t) + Vec3::Y * 0.03;
    gizmos.rect(Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2)), Vec2::splat(1.0 - inset), color);
}

/// Box outline around a whole (solid) cell.
fn cell_outline(gizmos: &mut Gizmos, t: Pos, color: Color) {
    let c = tile_center(t) + Vec3::Y * (LEVEL_H / 2.0);
    gizmos.cube(Transform::from_translation(c).with_scale(Vec3::new(1.03, LEVEL_H * 1.03, 1.03)), color);
}

fn draw_markers(mut gizmos: Gizmos, sim: Res<Sim>, ui: Res<UiState>) {
    let w = &sim.world;
    let m = &w.map;

    let cut = ui.top_level(m);
    for i in 0..m.len() {
        let Some(d) = m.desig[i] else { continue };
        let p = m.pos(i);
        if p.2 > cut {
            continue;
        }
        match d {
            Desig::Mine if m.block[i].solid() => cell_outline(&mut gizmos, p, Color::srgb(0.45, 0.8, 1.0)),
            Desig::Mine => floor_outline(&mut gizmos, p, 0.12, Color::srgb(0.45, 0.8, 1.0)),
            Desig::Chop => floor_outline(&mut gizmos, p, 0.12, Color::srgb(1.0, 0.6, 0.1)),
            Desig::Forage => floor_outline(&mut gizmos, p, 0.12, Color::srgb(1.0, 0.5, 0.75)),
        }
    }

    // A faint grid around the cursor on the viewed level, to aim at cells in the air.
    if let (Some(level), Some(h)) = (ui.view_level, ui.hover) {
        let c = Vec3::new(h.stand.0 as f32 + 0.5, level as f32 * LEVEL_H + 0.02, h.stand.1 as f32 + 0.5);
        let iso = Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2));
        gizmos.grid(iso, UVec2::splat(9), Vec2::ONE, Color::srgba(1.0, 1.0, 1.0, 0.18));
    }

    if let Some(h) = ui.hover {
        match ui.tool {
            Tool::Paint(order @ Order::Desig(Desig::Mine)) if order_target(w, h, order) == h.solid => {
                cell_outline(&mut gizmos, h.solid, Color::srgba(1.0, 1.0, 1.0, 0.6))
            }
            _ => floor_outline(&mut gizmos, h.stand, 0.02, Color::srgba(1.0, 1.0, 1.0, 0.6)),
        }
    }
    if let Some(Selection::Tile(t)) = ui.selected {
        if m.feature[m.idx(t)].is_block() {
            cell_outline(&mut gizmos, t, Color::srgb(1.0, 0.9, 0.2));
        } else {
            floor_outline(&mut gizmos, t, 0.0, Color::srgb(1.0, 0.9, 0.2));
        }
    }
    if let (Some(a), Some(h), Tool::Paint(order)) = (ui.drag_start, ui.hover, ui.tool) {
        let b = order_target(w, h, order);
        let (lx, ly, hx, hy) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
        let floor = a.2 as f32 * LEVEL_H + 0.05;
        let c = Vec3::new((lx + hx + 1) as f32 / 2.0, floor, (ly + hy + 1) as f32 / 2.0);
        let size = Vec2::new((hx - lx + 1) as f32, (hy - ly + 1) as f32);
        gizmos.rect(Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2)), size, Color::WHITE);
    }

    for p in w.pawns.iter().filter(|p| !p.left && !p.dead && ui.shows(m, p.z)) {
        let feet = Isometry3d::new(tile_point(p.x, p.y, p.z) + Vec3::Y * 0.03, Quat::from_rotation_x(FRAC_PI_2));
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
        let from = tile_point(e.from.0, e.from.1, e.from.2) + Vec3::Y * 0.55;
        let to = tile_point(e.to.0, e.to.1, e.to.2) + Vec3::Y * 0.5;
        // Fades over the effect's short life; colours above 1.0 glow (bloom).
        let fade = (e.ttl / 0.15).clamp(0.0, 1.0);
        let tracer = if e.hit { LinearRgba::rgb(3.0, 2.4, 0.9) } else { LinearRgba::rgb(1.2, 1.2, 1.2) };
        gizmos.line(from, to, Color::LinearRgba(tracer * fade));
        let flash = Color::LinearRgba(LinearRgba::rgb(8.0, 5.0, 1.5) * fade);
        gizmos.sphere(Isometry3d::from_translation(from), 0.05 + 0.1 * fade, flash);
        let puff = if e.hit { LinearRgba::rgb(1.6, 0.15, 0.1) } else { LinearRgba::rgb(0.7, 0.62, 0.5) };
        gizmos.sphere(Isometry3d::from_translation(to), 0.25 * (1.0 - fade) + 0.05, Color::LinearRgba(puff * fade));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_buildable_has_a_usable_distinct_shortcut() {
        let keys: Vec<KeyCode> = TOOLS.iter().filter_map(|t| t.2).collect();
        for (k, key) in keys.iter().enumerate() {
            assert!(!keys[k + 1..].contains(key), "{:?} used twice", key);
        }
        for kind in BuildId::all() {
            let tool = TOOLS.iter().find(|t| t.0 == Tool::Paint(Order::Build(kind))).expect("on the toolbar");
            assert_eq!(tool.2.is_some(), kind.def().key.is_some(), "{} shortcut", kind.def().id);
        }
    }
}
