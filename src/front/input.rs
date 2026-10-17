//! Player input: picking tiles and pawns, tools, keyboard shortcuts, and the
//! in-world markers (designations, selection, drag box, gunfire) as gizmos.

use super::audio::SoundQueue;
use super::camera::{colony_center, CameraRig, DragTracker, MainCamera};
use super::sim::{Sim, SPEEDS};
use super::terrain::{FLOOR_T, LEVEL_H};
use super::{tile_center, tile_point};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{egui_wants_any_keyboard_input, EguiWantsInput};
use my_world::defs::{defs, BuildId, Special};
use my_world::map::{Block, Desig, Map, Pos, Zone};
use my_world::pawn::Faction;
use my_world::world::{Order, World as SimWorld};
use std::f32::consts::FRAC_PI_2;
use std::sync::LazyLock;

pub const SAVE_PATH: &str = "savegame.json";
/// Written when the game is closed (the manual save above stays as it was).
pub const AUTOSAVE_PATH: &str = "autosave.json";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Paint(Order),
}

/// One tool in the build menu.
pub struct ToolEntry {
    pub tool: Tool,
    pub name: String,
    /// Shortcut as shown, and as pressed.
    pub key: Option<(String, KeyCode)>,
    /// Menu group: "Orders", "Zones", or a buildable's category.
    pub group: String,
    /// What it does, for tooltips (buildables describe themselves from their definition).
    pub hint: &'static str,
}

/// Every tool: orders and zones, then one per buildable in the definitions.
pub static TOOLS: LazyLock<Vec<ToolEntry>> = LazyLock::new(|| {
    let fixed = |tool, name: &str, key: Option<(&str, KeyCode)>, group: &str, hint| ToolEntry {
        tool,
        name: name.to_string(),
        key: key.map(|(k, code)| (k.to_string(), code)),
        group: group.to_string(),
        hint,
    };
    // (The number keys set the game speed.)
    let mut tools = vec![
        fixed(Tool::Select, "Select", Some(("Esc", KeyCode::Escape)), "Orders", "Click colonists and cells to inspect them."),
        fixed(Tool::Paint(Order::Desig(Desig::Chop)), "Chop trees", None, "Orders", "Mark trees to be cut down for wood."),
        fixed(Tool::Paint(Order::Desig(Desig::Mine)), "Mine", None, "Orders", "Dig out rock, ore and soil."),
        fixed(
            Tool::Paint(Order::Desig(Desig::Deconstruct)),
            "Deconstruct",
            Some(("Bksp", KeyCode::Backspace)),
            "Orders",
            "Take down walls, furniture, floors and roofs. Builders do it and get back half the cost.",
        ),
        fixed(Tool::Paint(Order::Desig(Desig::Forage)), "Forage", None, "Orders", "Mark berry bushes to be picked."),
        fixed(
            Tool::Paint(Order::Cancel),
            "Cancel",
            None,
            "Orders",
            "Remove designations and blueprints (delivered materials are dropped).",
        ),
        fixed(
            Tool::Paint(Order::Stockpile),
            "Stockpile",
            None,
            "Zones",
            "Loose items get hauled here. Click a stockpile to set its priority and what it takes.",
        ),
    ];
    if let Some(crop) = defs().sown() {
        let hint = "Colonists plant and harvest crops here. Click it to choose the crop.";
        tools.push(fixed(Tool::Paint(Order::Zone(Zone::Grow(crop))), "Grow zone", None, "Zones", hint));
    }
    let hint = "Civilians shelter here when a battle starts. Raiders can still reach it.";
    tools.push(fixed(Tool::Paint(Order::Zone(Zone::Safe)), "Safe zone", None, "Zones", hint));
    tools.push(fixed(Tool::Paint(Order::Zone(Zone::None)), "Clear zone", None, "Zones", "Remove stockpile, grow and safe zones."));
    // Roofs aren't placed by the player: rooms get them by themselves.
    for kind in BuildId::all().filter(|k| k.def().special != Special::Roof) {
        let def = kind.def();
        let key = def.key.as_deref().and_then(|k| match parse_key(k) {
            Some(code) if !RESERVED.contains(&code) => Some((k.to_uppercase(), code)),
            _ => {
                warn!("{}: key \"{}\" can't be used as a shortcut", def.id, k);
                None
            }
        });
        tools.push(ToolEntry { tool: Tool::Paint(Order::Build(kind)), name: def.name.clone(), key, group: def.category.clone(), hint: "" });
    }
    tools
});

/// Keys for the game speeds in `SPEEDS`, slowest first.
const SPEED_KEYS: [KeyCode; 4] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];

/// Keys with fixed meanings, which buildables can't use as shortcuts.
const RESERVED: [KeyCode; 19] = [
    KeyCode::Escape,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::KeyW,
    KeyCode::KeyA,
    KeyCode::KeyS,
    KeyCode::KeyD,
    KeyCode::KeyQ,
    KeyCode::KeyE,
    KeyCode::KeyP,
    KeyCode::KeyI,
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

/// Tabs of the colonist inspector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InspectorTab {
    Overview,
    Health,
    /// Role, squad and loadout (colonists only).
    Combat,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// What the inspector shows; with several colonists selected, the first.
    pub selected: Option<Selection>,
    /// Every selected colonist when there's more than one (else empty).
    pub group: Vec<u32>,
    /// Screen point where a box selection started (left drag with Select).
    pub box_start: Option<Vec2>,
    /// Cell where a paint drag started (its level is used for the whole box).
    pub drag_start: Option<Pos>,
    /// Under the cursor (last known, so drags can end over the UI).
    pub hover: Option<Hover>,
    pub show_work: bool,
    pub show_help: bool,
    pub inspector_tab: InspectorTab,
    /// The Squads overview window.
    pub show_squads: bool,
    /// Layer view: hide everything above this level (None = show all).
    pub view_level: Option<usize>,
    /// Hide built roofs: each column over a roofed room is cut off above
    /// the topmost such room (`roof_cut`, one level per column, `u8::MAX` = no
    /// cut), so you can look into buildings without the layer view.
    pub hide_roofs: bool,
    pub roof_cut: Vec<u8>,
    /// Bumped whenever `roof_cut` changes, for things that cache visibility.
    pub roof_version: u64,
    /// The build menu docked on the left.
    pub show_build: bool,
    /// Search text in the build menu.
    pub build_filter: String,
    /// The window listing what the colony has.
    pub show_resources: bool,
    /// The sound settings window.
    /// The game menu: save, load, new game, sound settings.
    pub show_menu: bool,
    /// Asking whether to really start a new game.
    pub confirm_new: bool,
    /// Width of the docked build menu last frame (in logical pixels).
    pub dock_width: f32,
}

impl UiState {
    /// Whether the cursor is over the docked build menu (egui doesn't count
    /// docked panels as "over the UI" by itself).
    pub fn over_dock(&self, window: &Window) -> bool {
        window.cursor_position().is_some_and(|c| c.x < self.dock_width)
    }
}

impl UiState {
    pub fn new() -> Self {
        UiState {
            tool: Tool::Select,
            selected: None,
            group: Vec::new(),
            box_start: None,
            drag_start: None,
            hover: None,
            show_work: false,
            show_help: false,
            inspector_tab: InspectorTab::Overview,
            show_squads: false,
            view_level: None,
            hide_roofs: false,
            roof_cut: Vec::new(),
            roof_version: 0,
            show_build: true,
            build_filter: String::new(),
            dock_width: 0.0,
            show_resources: false,
            show_menu: false,
            confirm_new: false,
        }
    }

    /// Every selected pawn: the group, or the one selected pawn.
    pub fn selected_pawns(&self) -> Vec<u32> {
        match self.selected {
            _ if !self.group.is_empty() => self.group.clone(),
            Some(Selection::Pawn(id)) => vec![id],
            _ => Vec::new(),
        }
    }

    pub fn is_selected(&self, id: u32) -> bool {
        self.selected == Some(Selection::Pawn(id)) || self.group.contains(&id)
    }

    /// Select exactly these pawns (the first is shown in the inspector).
    pub fn select_pawns(&mut self, ids: Vec<u32>) {
        self.selected = ids.first().map(|&id| Selection::Pawn(id));
        self.group = if ids.len() > 1 { ids } else { Vec::new() };
    }

    /// Select something else: a single pawn or a cell.
    pub fn select(&mut self, s: Option<Selection>) {
        self.selected = s;
        self.group.clear();
    }

    /// Shift+click: add a pawn to the selection, or take it out.
    pub fn toggle_pawn(&mut self, id: u32) {
        let mut ids = self.selected_pawns();
        match ids.iter().position(|&i| i == id) {
            Some(k) => {
                ids.remove(k);
            }
            None => ids.push(id),
        }
        self.select_pawns(ids);
    }

    /// Highest level being shown.
    pub fn top_level(&self, m: &Map) -> usize {
        self.view_level.unwrap_or(m.d - 1).min(m.d - 1)
    }

    /// Highest level shown in column (x, y): the layer view, lowered over
    /// roofed rooms while roofs are hidden.
    pub fn top_at(&self, m: &Map, x: usize, y: usize) -> usize {
        let top = self.top_level(m);
        match self.roof_cut.get(y * m.w + x) {
            Some(&c) if c != u8::MAX => top.min(c as usize),
            _ => top,
        }
    }

    /// Whether something at (fractional) cell coordinates is visible.
    pub fn shows_at(&self, m: &Map, x: f32, y: f32, z: f32) -> bool {
        let (cx, cy) = ((x.round().max(0.0) as usize).min(m.w - 1), (y.round().max(0.0) as usize).min(m.h - 1));
        z < self.top_at(m, cx, cy) as f32 + 0.5
    }
}

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiState::new())
            .add_systems(PreUpdate, update_roof_cut)
            .add_systems(Last, autosave_on_exit)
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
/// whatever is hidden (layer view, hidden roofs). With a layer view, empty
/// air on the viewed level can be picked too, so floors can be placed there.
fn pick(m: &Map, ray: Ray3d, ui: &UiState) -> Option<Hover> {
    let cut = ui.top_level(m);
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
        if cell.2 > ui.top_at(m, cell.0, cell.1) {
            continue;
        }
        let i = m.idx(cell);
        let in_slab = p.y - cell.2 as f32 * LEVEL_H < FLOOR_T * 2.0;
        // Roofs rise from the bottom of their cell, so they stop the ray anywhere in it.
        if m.floor[i] && in_slab || m.roof[i] {
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
    if let Some(h) = pick(&sim.world.map, ray, &ui) {
        ui.hover = Some(h);
    }
}

/// Pawn nearest to a cell (living ones preferred), if close enough.
pub fn pawn_at(w: &SimWorld, t: Pos) -> Option<u32> {
    let mut best: Option<(f32, u32)> = None;
    for p in w.pawns.iter().filter(|p| !p.left && !p.buried) {
        let (dx, dy, dz) = (p.x - t.0 as f32, p.y - t.1 as f32, p.z - t.2 as f32);
        let d = (dx * dx + dy * dy + dz * dz).sqrt() + if p.dead { 0.5 } else { 0.0 };
        if d < 0.9 && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, p.id));
        }
    }
    best.map(|(_, id)| id)
}

// ---------- actions shared by keys and buttons ----------

/// Colonists whose figures are inside a screen rectangle (box selection).
fn colonists_in_box(sim: &Sim, ui: &UiState, cam: &Camera, gt: &GlobalTransform, a: Vec2, b: Vec2) -> Vec<u32> {
    let rect = Rect::from_corners(a, b);
    let m = &sim.world.map;
    sim.world
        .pawns
        .iter()
        .filter(|p| p.is_colonist() && !p.dead && !p.left && ui.shows_at(m, p.x, p.y, p.z))
        .filter(|p| {
            let at = tile_point(p.x, p.y, p.z) + Vec3::Y * 0.5;
            cam.world_to_viewport(gt, at).is_ok_and(|v| rect.contains(v))
        })
        .map(|p| p.id)
        .collect()
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

/// The newer of the manual save and the autosave from exiting, loaded, with
/// a description of which it was. `Ok(None)` if there's no save at all.
pub fn read_newest_save() -> Result<Option<(SimWorld, String)>, String> {
    let Some(path) = newer_file(std::path::Path::new(SAVE_PATH), std::path::Path::new(AUTOSAVE_PATH)) else {
        return Ok(None);
    };
    let world = std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| SimWorld::load_json(&s))?;
    let which = if path.as_path() == std::path::Path::new(AUTOSAVE_PATH) { "the autosave from exiting" } else { "the save" };
    Ok(Some((world, format!("{} ({})", which, path.display()))))
}

/// Load the newer of the manual save and the autosave from exiting.
pub fn load(sim: &mut Sim, ui: &mut UiState) {
    match read_newest_save() {
        Ok(Some((w, which))) => {
            sim.replace(w);
            ui.select(None);
            sim.world.push_log(format!("Loaded {}.", which));
        }
        Ok(None) => sim.world.push_log("There's no saved game yet.".into()),
        Err(e) => sim.world.push_log(format!("Loading failed: {}", e)),
    }
}

/// Whichever of the two files exists and was written last.
fn newer_file(a: &std::path::Path, b: &std::path::Path) -> Option<std::path::PathBuf> {
    let modified = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (modified(a), modified(b)) {
        (Some(x), Some(y)) => Some(if y > x { b } else { a }.to_path_buf()),
        (Some(_), None) => Some(a.to_path_buf()),
        (None, Some(_)) => Some(b.to_path_buf()),
        (None, None) => None,
    }
}

/// Save to `AUTOSAVE_PATH` when the window is closed or the app quits (once).
fn autosave_on_exit(
    sim: Res<Sim>,
    mut closing: MessageReader<bevy::window::WindowCloseRequested>,
    mut exiting: MessageReader<AppExit>,
    mut done: Local<bool>,
) {
    let leaving = closing.read().count() + exiting.read().count() > 0;
    if !leaving || *done {
        return;
    }
    *done = true;
    match sim.world.save_json().and_then(|json| std::fs::write(AUTOSAVE_PATH, json).map_err(|e| e.to_string())) {
        Ok(()) => info!("Saved the game to {} on exit.", AUTOSAVE_PATH),
        Err(e) => error!("Saving on exit failed: {}", e),
    }
}

/// A new colony, or the scenario again if the game started in one.
pub fn new_colony(sim: &mut Sim, ui: &mut UiState, rig: &mut CameraRig) {
    let world = sim.fresh_world();
    sim.replace(world);
    ui.select(None);
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

/// Where each column is cut off while roofs are hidden: just under the
/// topmost roof (built or a blueprint, whether or not the room under it is
/// finished), or above the topmost room whose ceiling is a built floor.
/// Rock over caves and tunnels stays. One level per column, `u8::MAX` = none.
fn roof_cut(m: &Map) -> Vec<u8> {
    let mut cut = vec![u8::MAX; m.w * m.h];
    for y in 0..m.h {
        for x in 0..m.w {
            let ceiling = |z: usize| {
                let i = m.idx((x, y, z + 1));
                m.roof[i] || m.roof_blueprint(i) || m.roofed((x, y, z)) && m.floor[i]
            };
            if let Some(z) = (0..m.d - 1).rev().find(|&z| ceiling(z)) {
                cut[y * m.w + x] = z as u8;
            }
        }
    }
    cut
}

fn update_roof_cut(sim: Res<Sim>, mut ui: ResMut<UiState>) {
    let m = &sim.world.map;
    let cut = if ui.hide_roofs { roof_cut(m) } else { vec![u8::MAX; m.w * m.h] };
    if cut != ui.roof_cut {
        ui.roof_cut = cut;
        ui.roof_version += 1;
    }
}

fn keyboard(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>, mut ui: ResMut<UiState>, mut rig: ResMut<CameraRig>) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // In a battle, Space and Tab belong to the battle (`battle_ui.rs`), it
    // always plays at 1x, and there's nothing to build.
    let battle = sim.world.battle.is_some();
    if keys.just_pressed(KeyCode::Space) && !battle {
        sim.paused = !sim.paused;
    }
    // 1-4 pick the speed (and carry on if paused).
    for (k, key) in SPEED_KEYS.into_iter().enumerate() {
        if keys.just_pressed(key) && !battle {
            sim.speed = SPEEDS[k];
            sim.paused = false;
        }
    }
    // Esc, one step at a time: close a window, put the tool down (below),
    // let go of the selection, open the menu.
    if keys.just_pressed(KeyCode::Escape) {
        if ui.show_help || ui.show_work || ui.show_menu {
            ui.show_help = false;
            ui.show_work = false;
            ui.show_menu = false;
            ui.confirm_new = false;
        } else if ui.tool == Tool::Select {
            if ui.selected.is_some() {
                ui.select(None);
            } else {
                ui.show_menu = true;
            }
        }
    }
    if shift && keys.just_pressed(KeyCode::KeyR) {
        ui.hide_roofs = !ui.hide_roofs;
    }
    // Shift+key is for other things (Shift+N, Shift+R).
    for e in TOOLS.iter().filter(|_| !shift && !battle) {
        if e.key.as_ref().is_some_and(|(_, k)| keys.just_pressed(*k)) {
            ui.tool = e.tool;
            ui.drag_start = None;
        }
    }
    if keys.just_pressed(KeyCode::Backquote) {
        ui.show_build = !ui.show_build;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        ui.show_work = !ui.show_work;
    }
    if keys.just_pressed(KeyCode::KeyI) {
        ui.show_resources = !ui.show_resources;
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

#[allow(clippy::too_many_arguments)]
fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    drag: Res<DragTracker>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut sim: ResMut<Sim>,
    mut ui: ResMut<UiState>,
    mut sounds: ResMut<SoundQueue>,
) {
    // Battles have their own clicks (`battle_ui.rs`).
    if sim.world.battle.is_some() || sim.world.parley.is_some() {
        ui.box_start = None;
        ui.drag_start = None;
        ui.tool = Tool::Select;
        return;
    }
    let over_ui = egui_input.wants_any_pointer_input() || ui.over_dock(&window);
    let hover = ui.hover;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let cursor = window.cursor_position();
    match ui.tool {
        Tool::Select => {
            if buttons.just_pressed(MouseButton::Left) && !over_ui {
                ui.box_start = cursor;
            }
            if buttons.just_released(MouseButton::Left) {
                let start = ui.box_start.take();
                let dragged = start.zip(cursor).filter(|(a, b)| a.distance(*b) > 6.0);
                if let Some((a, b)) = dragged {
                    // Box selection: the colonists inside it (added to the selection with Shift).
                    let (cam, gt) = *camera;
                    let found = colonists_in_box(&sim, &ui, cam, gt, a, b);
                    if shift {
                        let mut ids = ui.selected_pawns();
                        ids.extend(found.into_iter().filter(|id| !ids.contains(id)).collect::<Vec<_>>());
                        ui.select_pawns(ids);
                    } else {
                        ui.select_pawns(found);
                    }
                } else if let (Some(_), Some(h)) = (start, hover) {
                    let wall = sim.world.map.feature[sim.world.map.idx(h.solid)].is_block();
                    match pawn_at(&sim.world, h.stand) {
                        Some(id) if shift => ui.toggle_pawn(id),
                        Some(id) => ui.select(Some(Selection::Pawn(id))),
                        None if shift => {}
                        None => ui.select(Some(Selection::Tile(if wall { h.solid } else { h.stand }))),
                    }
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
                    if paint_box(&mut sim.world, a, b, order) > 0 {
                        sounds.play("ui_place", None);
                    }
                }
                ui.drag_start = None;
            }
        }
    }

    // A right *click* (not a camera drag): back to Select, or equip something.
    if buttons.just_released(MouseButton::Right) && drag.right_moved <= 4.0 && !over_ui {
        if let Tool::Paint(_) = ui.tool {
            ui.tool = Tool::Select;
            ui.drag_start = None;
            return;
        }
        let ids = ui.selected_pawns();
        let (Some(&first), Some(Hover { stand: t, .. })) = (ids.first(), hover) else {
            ui.select(None);
            return;
        };
        // A weapon or piece of clothing: the first selected colonist equips it.
        let m = &sim.world.map;
        let gear = m.items[m.idx(t)].is_some_and(|s| s.kind.def().weapon.is_some() || s.kind.def().apparel.is_some());
        if gear && sim.world.pawn(first).is_some_and(|p| p.is_colonist()) {
            if sim.world.order_equip(first, t) {
                sounds.play("ui_place", None);
            }
        } else {
            // Right-click anywhere else lets go of the selection.
            ui.select(None);
        }
    }
}

/// Apply an order over a rectangle at the level where the drag started,
/// following the ground one level up or down where it slopes. Returns how
/// many cells it applied to.
fn paint_box(w: &mut SimWorld, a: Pos, b: Pos, order: Order) -> usize {
    let z0 = a.2 as i32;
    let mut done = 0;
    for y in a.1.min(b.1)..=a.1.max(b.1) {
        for x in a.0.min(b.0)..=a.0.max(b.0) {
            for dz in [0, 1, -1] {
                if let Some(p) = w.map.at(x as i32, y as i32, z0 + dz) {
                    if w.apply_order(p, order) {
                        done += 1;
                        break;
                    }
                }
            }
        }
    }
    done
}

fn forget_missing_selection(sim: Res<Sim>, mut ui: ResMut<UiState>) {
    let gone = |id: &u32| sim.world.pawn(*id).is_none_or(|p| p.left);
    if ui.group.iter().any(gone) {
        let ids: Vec<u32> = ui.group.iter().copied().filter(|id| !gone(id)).collect();
        ui.select_pawns(ids);
    }
    if let Some(Selection::Pawn(id)) = ui.selected {
        if gone(&id) {
            ui.select(None);
        }
    }
}

/// Flat square outline on the floor of a cell.
pub fn floor_outline(gizmos: &mut Gizmos, t: Pos, inset: f32, color: Color) {
    let c = tile_center(t) + Vec3::Y * 0.03;
    gizmos.rect(Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2)), Vec2::splat(1.0 - inset), color);
}

/// Box outline around a whole (solid) cell.
pub fn cell_outline(gizmos: &mut Gizmos, t: Pos, color: Color) {
    let c = tile_center(t) + Vec3::Y * (LEVEL_H / 2.0);
    gizmos.cube(Transform::from_translation(c).with_scale(Vec3::new(1.03, LEVEL_H * 1.03, 1.03)), color);
}

fn draw_markers(mut gizmos: Gizmos, sim: Res<Sim>, ui: Res<UiState>) {
    let w = &sim.world;
    let m = &w.map;

    for i in 0..m.len() {
        let Some(d) = m.desig[i] else { continue };
        let p = m.pos(i);
        if p.2 > ui.top_at(m, p.0, p.1) {
            continue;
        }
        match d {
            Desig::Mine if m.block[i].solid() => cell_outline(&mut gizmos, p, Color::srgb(0.45, 0.8, 1.0)),
            Desig::Mine => floor_outline(&mut gizmos, p, 0.12, Color::srgb(0.45, 0.8, 1.0)),
            Desig::Chop => floor_outline(&mut gizmos, p, 0.12, Color::srgb(1.0, 0.6, 0.1)),
            Desig::Forage => floor_outline(&mut gizmos, p, 0.12, Color::srgb(1.0, 0.5, 0.75)),
            Desig::Deconstruct if m.feature[i].is_block() => cell_outline(&mut gizmos, p, Color::srgb(1.0, 0.3, 0.25)),
            Desig::Deconstruct => floor_outline(&mut gizmos, p, 0.12, Color::srgb(1.0, 0.3, 0.25)),
        }
    }

    // Where a room's roof can't reach (no wall near enough): a red cross at the top of the walls.
    for &i in &m.roof_gaps {
        let p = m.pos(i);
        if p.2 > ui.top_at(m, p.0, p.1) {
            continue;
        }
        let red = Color::srgb(1.0, 0.25, 0.2);
        floor_outline(&mut gizmos, p, 0.03, red);
        let (x, y, z) = (p.0 as f32, p.2 as f32 * LEVEL_H + 0.03, p.1 as f32);
        gizmos.line(Vec3::new(x + 0.2, y, z + 0.2), Vec3::new(x + 0.8, y, z + 0.8), red);
        gizmos.line(Vec3::new(x + 0.8, y, z + 0.2), Vec3::new(x + 0.2, y, z + 0.8), red);
    }

    // A faint grid around the cursor on the viewed level, to aim at cells in the air.
    if let (Some(level), Some(h)) = (ui.view_level, ui.hover) {
        let c = Vec3::new(h.stand.0 as f32 + 0.5, level as f32 * LEVEL_H + 0.02, h.stand.1 as f32 + 0.5);
        let iso = Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2));
        gizmos.grid(iso, UVec2::splat(9), Vec2::ONE, Color::srgba(1.0, 1.0, 1.0, 0.18));
    }

    if let Some(h) = ui.hover {
        match ui.tool {
            Tool::Paint(order @ Order::Desig(Desig::Mine | Desig::Deconstruct)) if order_target(w, h, order) == h.solid => {
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

    for p in w.pawns.iter().filter(|p| !p.left && !p.dead && ui.shows_at(m, p.x, p.y, p.z)) {
        let feet = Isometry3d::new(tile_point(p.x, p.y, p.z) + Vec3::Y * 0.03, Quat::from_rotation_x(FRAC_PI_2));
        if ui.is_selected(p.id) {
            gizmos.circle(feet, 0.48, Color::WHITE);
        }
        if p.faction == Faction::Raider {
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
    fn loading_picks_the_newer_save() {
        let dir = std::env::temp_dir().join(format!("my-world-saves-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("save.json"), dir.join("auto.json"));
        assert_eq!(newer_file(&a, &b), None);
        std::fs::write(&a, "a").unwrap();
        assert_eq!(newer_file(&a, &b), Some(a.clone()));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&b, "b").unwrap();
        assert_eq!(newer_file(&a, &b), Some(b.clone()), "the autosave is newer");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hiding_roofs_hides_unfinished_ones_too() {
        use my_world::map::{Feature, FLAT_Z as Z};
        let mut m = Map::flat(12, 12);
        // A walled room with a hole in its roof (so not a room yet) and a roof blueprint.
        for y in 2..=6 {
            for x in 2..=6 {
                let (i, above) = (m.idx((x, y, Z)), m.idx((x, y, Z + 1)));
                if x == 2 || y == 2 || x == 6 || y == 6 {
                    m.set_feature(i, Feature::built("wall"));
                } else if (x, y) == (4, 4) {
                    m.feature[above] = Feature::Blueprint { kind: defs().roof().unwrap(), delivered: 0 };
                } else if (x, y) != (3, 3) {
                    m.set_roof(above, true);
                }
            }
        }
        // A floor out in the open: a platform, not a ceiling.
        let platform = m.idx((9, 9, Z + 1));
        m.set_floor(platform, true);
        m.recompute_rooms();
        assert!(!m.roofed((4, 4, Z)));
        let cut = roof_cut(&m);
        let at = |x: usize, y: usize| cut[y * m.w + x];
        assert_eq!(at(5, 5), Z as u8, "under a built roof");
        assert_eq!(at(4, 4), Z as u8, "under a roof blueprint");
        assert_eq!(at(3, 3), u8::MAX, "nothing over the hole");
        assert_eq!(at(9, 9), u8::MAX, "platforms stay");
        assert_eq!(at(2, 2), u8::MAX, "walls stay");
    }

    #[test]
    fn selecting_several_colonists() {
        let mut ui = UiState::new();
        ui.select(Some(Selection::Pawn(1)));
        assert_eq!(ui.selected_pawns(), vec![1]);
        ui.toggle_pawn(2);
        ui.toggle_pawn(3);
        assert_eq!(ui.selected_pawns(), vec![1, 2, 3]);
        assert!(ui.is_selected(2) && ui.is_selected(3));
        assert_eq!(ui.selected, Some(Selection::Pawn(1)), "the first stays in the inspector");
        ui.toggle_pawn(1);
        ui.toggle_pawn(3);
        assert_eq!(ui.selected_pawns(), vec![2]);
        assert!(ui.group.is_empty(), "one left: a plain selection again");
        ui.toggle_pawn(4);
        ui.select(Some(Selection::Tile((1, 1, 1))));
        assert!(ui.selected_pawns().is_empty() && !ui.is_selected(4), "selecting a cell drops the group");
    }

    #[test]
    fn every_buildable_has_a_usable_distinct_shortcut() {
        let keys: Vec<KeyCode> = TOOLS.iter().filter_map(|t| t.key.as_ref().map(|k| k.1)).collect();
        for (k, key) in keys.iter().enumerate() {
            assert!(!keys[k + 1..].contains(key), "{:?} used twice", key);
        }
        assert!(!keys.iter().any(|k| SPEED_KEYS.contains(k)), "the number keys set the speed");
        for kind in BuildId::all().filter(|k| k.def().special != Special::Roof) {
            let tool = TOOLS.iter().find(|t| t.tool == Tool::Paint(Order::Build(kind))).expect("in the build menu");
            assert_eq!(tool.key.is_some(), kind.def().key.is_some(), "{} shortcut", kind.def().id);
        }
    }
}
