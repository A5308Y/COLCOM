//! Player input: picking tiles and pawns, tools, keyboard shortcuts, and the
//! in-world markers (designations, selection, drag box, gunfire) as gizmos.

use super::audio::SoundQueue;
use super::info::InfoTarget;
use super::camera::{colony_center, CameraRig, DragTracker, MainCamera};
use super::sim::{Sim, SPEEDS};
use super::terrain::{FLOOR_T, LEVEL_H};
use super::{tile_center, tile_point};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{egui_wants_any_keyboard_input, EguiWantsInput};
use my_world::defs::{defs, BuildId, Special};
use my_world::map::{Block, Desig, Map, Pos, Zone, DIRS};
use my_world::pawn::{Faction, PawnId};
use my_world::world::{Order, World as SimWorld};
use std::f32::consts::FRAC_PI_2;
use std::sync::LazyLock;

pub const SAVE_PATH: &str = "savegame.json";
/// Written when the game is closed (the manual save above stays as it was).
pub const AUTOSAVE_PATH: &str = "autosave.json";
/// Where games saved under a name go ("Save as").
pub const SAVES_DIR: &str = "saves";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Paint(Order),
    /// Move furniture: first which (`None`), then where it goes.
    Move(Option<Pos>),
    /// Testing: drop `UiState::spawn_count` of this item where clicked.
    Spawn(my_world::defs::ItemId),
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
            Tool::Paint(Order::Desig(Desig::Fill)),
            "Fill",
            None,
            "Orders",
            "Fill holes and tunnels back in with soil (free), from the bottom up. On open ground it raises the ground a level.",
        ),
        fixed(
            Tool::Move(None),
            "Move furniture",
            None,
            "Orders",
            "Click furniture, then where it should go. A builder packs it up and sets it down there, at no cost.",
        ),
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
    let hint = "Miners dig the area out level by level from the top, with a ladder down at its edge (3 wood a level). Click it to set how deep.";
    tools.push(fixed(Tool::Paint(Order::Quarry), "Quarry zone", None, "Zones", hint));
    tools.push(fixed(Tool::Paint(Order::Zone(Zone::None)), "Clear zone", None, "Zones", "Remove stockpile, grow, safe and quarry zones."));
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
        tools.push(ToolEntry { tool: Tool::Paint(Order::Build(kind)), name: def.name.clone(), key, group: def.category.to_string(), hint: "" });
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
    Pawn(PawnId),
    Tile(Pos),
}

/// What the cursor points at: the first solid thing along the view ray
/// (ground, rock, a wall, water) and the open cell on top of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hover {
    pub solid: Pos,
    pub stand: Pos,
}

/// What a colonist could do where the player right-clicked (see `World::job_options`).
pub struct JobMenu {
    pub pawn: PawnId,
    /// Label, cell and job of each choice.
    pub options: Vec<(String, Pos, my_world::pawn::Job)>,
    /// Where it opens (taken from the pointer when it's first drawn).
    pub at: Option<bevy_egui::egui::Pos2>,
}

#[derive(Resource)]
pub struct UiState {
    pub tool: Tool,
    /// What the inspector shows; with several colonists selected, the first.
    pub selected: Option<Selection>,
    /// Every selected colonist when there's more than one (else empty).
    pub group: Vec<PawnId>,
    /// Screen point where a box selection started (left drag with Select).
    pub box_start: Option<Vec2>,
    /// Cell where a paint drag started (its level is used for the whole box).
    pub drag_start: Option<Pos>,
    /// Under the cursor: None over the UI, except during a paint drag (the
    /// last cell over the map, so the drag can end over a panel).
    pub hover: Option<Hover>,
    pub show_work: bool,
    /// The Work window's tab: priorities (false) or schedules (true), and the block being painted.
    pub work_schedule: bool,
    pub schedule_brush: my_world::recreation::Block,
    pub show_help: bool,
    /// The handbook's open topic (index into `help::TOPICS`) and search text.
    pub help_topic: usize,
    pub help_filter: String,
    pub inspector_tab: InspectorTab,
    /// The Squads overview window.
    pub show_squads: bool,
    /// The themed rooms window.
    pub show_rooms: bool,
    /// The Expeditions window and what's being planned or traded in it.
    pub expeditions: super::expeditions_ui::ExpeditionsUi,
    /// The testing window to drop items into the world.
    pub show_spawn: bool,
    /// How many to drop at a time with the spawn tool.
    pub spawn_count: u32,
    /// Name typed in for "Save as".
    pub save_name: String,
    /// Layer view: hide everything above this level (None = show all).
    pub view_level: Option<usize>,
    /// Hide built roofs: each column over a roofed room is cut off above
    /// the topmost such room (`roof_cut`, one level per column, `u8::MAX` = no
    /// cut), so you can look into buildings without the layer view.
    pub hide_roofs: bool,
    pub roof_cut: Vec<u8>,
    /// Bumped whenever `roof_cut` changes, for things that cache visibility.
    pub roof_version: u64,
    /// The last right press put a tool down (so its release doesn't also deselect).
    pub dropped_tool: bool,
    /// The right-click menu of jobs to prioritize, while it's open.
    pub job_menu: Option<JobMenu>,
    /// How what's being placed is turned (index into `DIRS`; `,` and `.` turn it).
    pub place_dir: u8,
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
    /// The wall face and slot under the cursor (wall cell, face, slot), when it points at a wall.
    pub wall_slot: Option<(Pos, u8, u8)>,
    /// The info window, what it shows, what it showed before (for Back),
    /// and its search text.
    pub show_info: bool,
    pub info: Option<InfoTarget>,
    pub info_back: Vec<InfoTarget>,
    pub info_filter: String,
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
            work_schedule: false,
            schedule_brush: my_world::recreation::Block::Work,
            show_help: false,
            help_topic: 0,
            help_filter: String::new(),
            inspector_tab: InspectorTab::Overview,
            show_squads: false,
            show_rooms: false,
            expeditions: Default::default(),
            show_spawn: false,
            spawn_count: 50,
            save_name: String::new(),
            view_level: None,
            hide_roofs: false,
            roof_cut: Vec::new(),
            roof_version: 0,
            dropped_tool: false,
            job_menu: None,
            place_dir: 0,
            show_build: true,
            build_filter: String::new(),
            dock_width: 0.0,
            wall_slot: None,
            show_info: false,
            info: None,
            info_back: Vec::new(),
            info_filter: String::new(),
            show_resources: false,
            show_menu: false,
            confirm_new: false,
        }
    }

    /// Every selected pawn: the group, or the one selected pawn.
    pub fn selected_pawns(&self) -> Vec<PawnId> {
        match self.selected {
            _ if !self.group.is_empty() => self.group.clone(),
            Some(Selection::Pawn(id)) => vec![id],
            _ => Vec::new(),
        }
    }

    pub fn is_selected(&self, id: PawnId) -> bool {
        self.selected == Some(Selection::Pawn(id)) || self.group.contains(&id)
    }

    /// Select exactly these pawns (the first is shown in the inspector).
    pub fn select_pawns(&mut self, ids: Vec<PawnId>) {
        self.selected = ids.first().map(|&id| Selection::Pawn(id));
        self.group = if ids.len() > 1 { ids } else { Vec::new() };
    }

    /// Pick the furniture at `from` to move, starting out turned as it stands.
    pub fn start_move(&mut self, from: Pos, w: &SimWorld) {
        self.tool = Tool::Move(Some(from));
        self.place_dir = w.facing_of(from);
        self.drag_start = None;
    }

    /// What's being placed or moved, if `,` and `.` turn it.
    pub fn turning(&self, w: &SimWorld) -> Option<BuildId> {
        let kind = match self.tool {
            Tool::Paint(Order::Build(kind)) => kind,
            Tool::Move(Some(from)) => w.movable(from)?,
            _ => return None,
        };
        kind.def().turnable().then_some(kind)
    }

    /// Show `t` in the info window (what it showed before is kept for Back).
    pub fn open_info(&mut self, t: InfoTarget) {
        if let Some(old) = self.info.filter(|&old| old != t) {
            self.info_back.push(old);
        }
        self.info = Some(t);
        self.show_info = true;
    }

    /// Select something else: a single pawn or a cell.
    pub fn select(&mut self, s: Option<Selection>) {
        self.selected = s;
        self.group.clear();
    }

    /// Shift+click: add a pawn to the selection, or take it out.
    pub fn toggle_pawn(&mut self, id: PawnId) {
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
/// Where the furniture at `from` would go with the cursor at `h`, if it can.
pub fn move_target(w: &SimWorld, from: Pos, h: Hover) -> Option<Pos> {
    let kind = w.movable(from)?;
    let to = order_target(w, h, Order::Build(kind));
    w.move_target_ok(from, to).then_some(to)
}

pub fn order_target(w: &SimWorld, h: Hover, order: Order) -> Pos {
    let first = match order {
        Order::Desig(Desig::Mine) => [h.solid, h.stand],
        // Doors and windows go into the clicked wall (replacing it) rather than on top.
        Order::Build(kind) if kind.def().along_walls => [h.solid, h.stand],
        _ => [h.stand, h.solid],
    };
    first.into_iter().find(|&p| w.order_applies(p, order)).unwrap_or(first[0])
}

fn update_hover(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    sim: Res<Sim>,
    egui_input: Res<EguiWantsInput>,
    mut ui: ResMut<UiState>,
) {
    // Over a panel or button the cursor points at no tile (unless a paint
    // drag is under way: it ends where the cursor last was over the map).
    if (egui_input.wants_any_pointer_input() || ui.over_dock(&window)) && ui.drag_start.is_none() {
        ui.hover = None;
        ui.wall_slot = None;
        return;
    }
    let (cam, gt) = *camera;
    let Some(cursor) = window.cursor_position() else { return };
    let Ok(ray) = cam.viewport_to_world(gt, cursor) else { return };
    if let Some(h) = pick(&sim.world.map, ray, &ui) {
        ui.hover = Some(h);
        let m = &sim.world.map;
        ui.wall_slot = m.feature[m.idx(h.solid)].is_block().then(|| wall_slot_at(ray, h.solid)).flatten().map(|(f, s)| (h.solid, f, s));
    }
}

/// The face of the wall at `wall` the ray meets first, and the slot on it
/// (faces turned away from the camera don't count).
fn wall_slot_at(ray: Ray3d, wall: Pos) -> Option<(u8, u8)> {
    use my_world::decor::{SLOTS, WALL_A, WALL_B};
    let (o, d) = (ray.origin, *ray.direction);
    let (x, y) = (wall.0 as f32, wall.1 as f32);
    let (lo, hi) = (wall.2 as f32 * LEVEL_H, (wall.2 + 1) as f32 * LEVEL_H);
    let mut best: Option<(f32, u8, u8)> = None;
    for face in 0..4u8 {
        let (nx, ny) = DIRS[face as usize];
        if nx as f32 * d.x + ny as f32 * d.z >= 0.0 {
            continue;
        }
        let off = if nx + ny > 0 { WALL_B } else { WALL_A };
        // Faces looking along x sit at a fixed x and run along z; the others the other way.
        let (plane, o_n, d_n) = if face < 2 { (x + off, o.x, d.x) } else { (y + off, o.z, d.z) };
        let t = (plane - o_n) / d_n;
        let hit = o + d * t;
        let along = if face < 2 { hit.z - y } else { hit.x - x };
        if t <= 0.0 || !(0.0..1.0).contains(&along) || !(lo..hi).contains(&hit.y) {
            continue;
        }
        let slot = ((along * SLOTS as f32) as u8).min(SLOTS - 1);
        if best.is_none_or(|b| t < b.0) {
            best = Some((t, face, slot));
        }
    }
    best.map(|(_, f, s)| (f, s))
}

/// Pawn nearest to a cell (living ones preferred), if close enough.
pub fn pawn_at(w: &SimWorld, t: Pos) -> Option<PawnId> {
    let mut best: Option<(f32, PawnId)> = None;
    for p in w.pawns.iter().filter(|p| !p.has_left() && !p.is_buried()) {
        let (dx, dy, dz) = (p.x - t.0 as f32, p.y - t.1 as f32, p.z - t.2 as f32);
        let d = (dx * dx + dy * dy + dz * dz).sqrt() + if p.is_dead() { 0.5 } else { 0.0 };
        if d < 0.9 && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, p.id));
        }
    }
    best.map(|(_, id)| id)
}

// ---------- actions shared by keys and buttons ----------

/// Colonists whose figures are inside a screen rectangle (box selection).
fn colonists_in_box(sim: &Sim, ui: &UiState, cam: &Camera, gt: &GlobalTransform, a: Vec2, b: Vec2) -> Vec<PawnId> {
    let rect = Rect::from_corners(a, b);
    let m = &sim.world.map;
    sim.world
        .pawns
        .iter()
        .filter(|p| p.is_living_colonist() && !p.has_left() && ui.shows_at(m, p.x, p.y, p.z))
        .filter(|p| {
            let at = tile_point(p.x, p.y, p.z) + Vec3::Y * 0.5;
            cam.world_to_viewport(gt, at).is_ok_and(|v| rect.contains(v))
        })
        .map(|p| p.id)
        .collect()
}

/// Replace a save without risking it: the new game goes to `<file>.tmp` first and
/// then takes the old one's place in a single rename, so a crash or a full disk
/// mid-write leaves the previous save intact. The previous save is kept as
/// `<file>.bak`.
fn write_save(path: &std::path::Path, json: &str) -> Result<(), String> {
    use std::io::Write;
    let with_suffix = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        std::path::PathBuf::from(name)
    };
    let tmp = with_suffix(".tmp");
    let write = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        if path.exists() {
            std::fs::copy(path, with_suffix(".bak"))?;
        }
        std::fs::rename(&tmp, path)
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Read a save; if it's damaged, fall back to its backup. The text says which
/// was read.
fn read_save(path: &std::path::Path) -> Result<(SimWorld, String), String> {
    let read = |p: &std::path::Path| std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|s| SimWorld::load_json(&s));
    match read(path) {
        Ok(w) => Ok((w, path.display().to_string())),
        Err(e) => {
            let mut bak = path.as_os_str().to_owned();
            bak.push(".bak");
            let bak = std::path::PathBuf::from(bak);
            match read(&bak) {
                Ok(w) => Ok((w, format!("the backup {} ({} could not be read: {})", bak.display(), path.display(), e))),
                Err(_) => Err(e),
            }
        }
    }
}

pub fn save(sim: &mut Sim) {
    let msg = match sim.saveable().save_json().and_then(|json| write_save(std::path::Path::new(SAVE_PATH), &json)) {
        Ok(()) => format!("Game saved to {}.", SAVE_PATH),
        Err(e) => format!("Saving failed: {}", e),
    };
    sim.world.push_log(msg);
}

/// A save name as a file in `SAVES_DIR`: letters, digits, spaces, - and _ (the rest dropped).
pub fn save_file(name: &str) -> Option<std::path::PathBuf> {
    save_file_in(std::path::Path::new(SAVES_DIR), name)
}

fn save_file_in(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let clean: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect();
    let clean = clean.trim();
    (!clean.is_empty()).then(|| dir.join(format!("{}.json", clean)))
}

/// Save the game under `name` (in `SAVES_DIR`), replacing a save of that name.
pub fn save_as(sim: &mut Sim, name: &str) -> bool {
    let Some(path) = save_file(name) else {
        sim.world.push_log("A save needs a name (letters, digits, spaces).".into());
        return false;
    };
    let result = std::fs::create_dir_all(SAVES_DIR)
        .map_err(|e| e.to_string())
        .and_then(|_| sim.saveable().save_json())
        .and_then(|json| write_save(&path, &json));
    let msg = match &result {
        Ok(()) => format!("Game saved to {}.", path.display()),
        Err(e) => format!("Saving failed: {}", e),
    };
    sim.world.push_log(msg);
    result.is_ok()
}

/// The games saved under a name, newest first: name and file.
pub fn named_saves() -> Vec<(String, std::path::PathBuf)> {
    saves_in(std::path::Path::new(SAVES_DIR))
}

fn saves_in(dir: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let Ok(dir) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut saves: Vec<(std::time::SystemTime, String, std::path::PathBuf)> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| {
            let when = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            Some((when, p.file_stem()?.to_string_lossy().into_owned(), p))
        })
        .collect();
    saves.sort_by_key(|s| std::cmp::Reverse(s.0));
    saves.into_iter().map(|(_, name, path)| (name, path)).collect()
}

/// Load a game saved under a name.
pub fn load_file(sim: &mut Sim, ui: &mut UiState, path: &std::path::Path) {
    match read_save(path) {
        Ok((w, which)) => {
            sim.replace(w);
            ui.select(None);
            sim.world.push_log(format!("Loaded {}.", which));
        }
        Err(e) => sim.world.push_log(format!("Loading {} failed: {}", path.display(), e)),
    }
}

/// The newer of the manual save and the autosave from exiting, loaded, with
/// a description of which it was. `Ok(None)` if there's no save at all.
pub fn read_newest_save() -> Result<Option<(SimWorld, String)>, String> {
    let Some(path) = newer_file(std::path::Path::new(SAVE_PATH), std::path::Path::new(AUTOSAVE_PATH)) else {
        return Ok(None);
    };
    let (world, file) = read_save(&path)?;
    let which = if path.as_path() == std::path::Path::new(AUTOSAVE_PATH) { "the autosave from exiting" } else { "the save" };
    Ok(Some((world, format!("{} ({})", which, file))))
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

/// Save to `AUTOSAVE_PATH` when the window is closed or the app quits (once);
/// never after taking screenshots, which mustn't replace the player's game.
fn autosave_on_exit(
    sim: Res<Sim>,
    screenshots: Option<Res<super::screenshot::ScreenshotMode>>,
    mut closing: MessageReader<bevy::window::WindowCloseRequested>,
    mut exiting: MessageReader<AppExit>,
    mut done: Local<bool>,
) {
    let leaving = closing.read().count() + exiting.read().count() > 0;
    if !leaving || *done || screenshots.is_some() {
        return;
    }
    *done = true;
    match sim.saveable().save_json().and_then(|json| write_save(std::path::Path::new(AUTOSAVE_PATH), &json)) {
        Ok(()) => info!("Saved the game to {} on exit.", AUTOSAVE_PATH),
        Err(e) => error!("Saving on exit failed: {}", e),
    }
}

/// A new colony, or the scenario again if the game started in one.
pub fn new_colony(sim: &mut Sim, ui: &mut UiState, rig: &mut CameraRig) {
    let mut world = sim.fresh_world();
    // The difficulty carries over to the new game.
    world.difficulty = sim.world.difficulty.clone();
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
        if ui.job_menu.is_some() {
            ui.job_menu = None;
        } else if ui.show_help || ui.show_work || ui.show_menu {
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
    // `,` and `.` turn what's being placed or moved, a quarter turn each way.
    if ui.turning(&sim.world).is_some() {
        let turn = [KeyCode::Comma, KeyCode::Period].iter().position(|&k| keys.just_pressed(k));
        if let Some(t) = turn {
            ui.place_dir = turn_dir(ui.place_dir, t == 1);
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
    if keys.just_pressed(KeyCode::KeyN) && (shift || sim.lost()) {
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
        // Things for walls go into the slot under the cursor, one per click.
        Tool::Paint(Order::Build(kind)) if kind.def().on_wall => {
            if buttons.just_released(MouseButton::Left) && !over_ui {
                if let Some((wall, face, slot)) = ui.wall_slot {
                    if sim.world.order_decor(wall, face, slot, kind) {
                        sounds.play("ui_place", None);
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
                    if paint_box(&mut sim.world, a, b, order, hollow(order, shift), ui.place_dir) > 0 {
                        sounds.play("ui_place", None);
                    }
                }
                ui.drag_start = None;
            }
        }
        Tool::Spawn(kind) => {
            if buttons.just_released(MouseButton::Left) && !over_ui {
                if let Some(h) = hover {
                    sim.world.spawn_items(h.stand, kind, ui.spawn_count);
                    sounds.play("ui_place", None);
                }
            }
        }
        Tool::Move(from) => {
            if buttons.just_released(MouseButton::Left) && !over_ui {
                if let Some(h) = hover {
                    match from {
                        None => {
                            if sim.world.movable(h.stand).is_some() {
                                ui.start_move(h.stand, &sim.world);
                                sounds.play("ui_click", None);
                            }
                        }
                        Some(from) => {
                            if let Some(to) = move_target(&sim.world, from, h) {
                                if sim.world.order_move_facing(from, to, ui.place_dir) {
                                    sounds.play("ui_place", None);
                                    ui.tool = Tool::Select;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Any right press puts a tool down (back to Select), over the map or a
    // panel, click or camera drag alike.
    if buttons.just_pressed(MouseButton::Right) {
        if ui.tool != Tool::Select {
            ui.tool = Tool::Select;
            ui.drag_start = None;
            ui.dropped_tool = true;
            return;
        }
        ui.dropped_tool = false;
    }
    // A right *click* (not a camera drag) in Select: a menu of what the first
    // selected colonist could do there (equip, prioritize a job), or let go of the selection.
    if buttons.just_released(MouseButton::Right) && drag.right_moved <= 4.0 && !over_ui && !ui.dropped_tool {
        ui.job_menu = None;
        let ids = ui.selected_pawns();
        let (Some(&first), Some(h)) = (ids.first(), hover) else {
            ui.select(None);
            return;
        };
        // The floor cell, and a wall or rock face behind it (to mine or take down).
        let mut options = Vec::new();
        for cell in [h.stand, h.solid] {
            for (label, job) in sim.world.job_options(first, cell) {
                if !options.iter().any(|o: &(String, Pos, _)| o.2 == job) {
                    options.push((label, cell, job));
                }
            }
        }
        if options.is_empty() {
            ui.select(None);
        } else {
            sounds.play("ui_click", None);
            ui.job_menu = Some(JobMenu { pawn: first, options, at: None });
        }
    }
}

/// Apply an order over a rectangle at the level where the drag started,
/// following the ground one level up or down where it slopes. Returns how
/// many cells it applied to.
/// Walls go around the dragged rectangle, not over all of it (Shift fills it).
pub fn hollow(order: Order, shift: bool) -> bool {
    matches!(order, Order::Build(kind) if kind.def().is_block()) && !shift
}

/// The next direction (index into `DIRS`) a quarter turn from `dir`, one way or the other.
pub fn turn_dir(dir: u8, clockwise: bool) -> u8 {
    // +x, +y, -x, -y: round the compass.
    const ROUND: [u8; 4] = [0, 2, 1, 3];
    let k = ROUND.iter().position(|&d| d == dir % 4).unwrap_or(0);
    ROUND[(k + if clockwise { 1 } else { 3 }) % 4]
}

/// The cells `order` would go to over the rectangle from `a` to `b` (only
/// its edge if `hollow`): in each column the first of `a`'s level, one up and
/// one down where it applies (a quarry: the column's ground).
pub fn box_cells(w: &SimWorld, a: Pos, b: Pos, order: Order, hollow: bool) -> Vec<Pos> {
    let z0 = a.2 as i32;
    let (lx, ly, hx, hy) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
    let mut cells = Vec::new();
    for y in ly..=hy {
        for x in lx..=hx {
            if hollow && x > lx && x < hx && y > ly && y < hy {
                continue;
            }
            // A quarry takes whole columns, wherever their ground is.
            let surface = (order == Order::Quarry).then(|| w.map.ground_level(x, y)).flatten();
            let levels: Vec<i32> = match surface {
                Some(z) => vec![z as i32],
                None => vec![z0, z0 + 1, z0 - 1],
            };
            let at = levels.into_iter().filter_map(|z| w.map.at(x as i32, y as i32, z)).find(|&p| w.order_applies(p, order));
            cells.extend(at);
        }
    }
    cells
}

/// Apply `order` over the rectangle from `a` to `b` (see `box_cells`);
/// blueprints are turned to face `dir`.
fn paint_box(w: &mut SimWorld, a: Pos, b: Pos, order: Order, hollow: bool, dir: u8) -> usize {
    let turnable = matches!(order, Order::Build(kind) if kind.def().turnable());
    let mut done = 0;
    for p in box_cells(w, a, b, order, hollow) {
        if w.apply_order(p, order) {
            if turnable {
                w.turn_blueprint(p, dir);
            }
            done += 1;
        }
    }
    done
}

fn forget_missing_selection(sim: Res<Sim>, mut ui: ResMut<UiState>) {
    let gone = |id: &PawnId| sim.world.pawn(*id).is_none_or(|p| p.has_left());
    if ui.group.iter().any(gone) {
        let ids: Vec<PawnId> = ui.group.iter().copied().filter(|id| !gone(id)).collect();
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
/// An arrow from cell `at` towards the front of `kind` turned to `dir`:
/// where it's used from (stairs: the way up).
/// Stairs that couldn't be climbed that way (yet) get an orange arrow.
fn front_arrow(gizmos: &mut Gizmos, m: &Map, at: Pos, kind: BuildId, dir: u8) {
    let (dx, dy) = DIRS[super::props::turned(kind.def().front, dir) as usize];
    let c = tile_center(at) + Vec3::Y * 0.1;
    let blocked = kind.def().special == Special::Stairs && !m.stairs_fit(at, dir);
    let color = if blocked { Color::srgb(1.0, 0.45, 0.15) } else { Color::srgb(1.0, 0.85, 0.3) };
    gizmos.arrow(c, c + Vec3::new(dx as f32, 0.0, dy as f32) * 0.6, color);
}

/// A line a little above the floor from cell `a` to cell `b`, with an arrowhead at `b`.
fn move_arrow(gizmos: &mut Gizmos, a: Pos, b: Pos, color: Color) {
    let lift = Vec3::Y * 0.25;
    gizmos.arrow(tile_center(a) + lift, tile_center(b) + lift, color);
}

pub fn cell_outline(gizmos: &mut Gizmos, t: Pos, color: Color) {
    let c = tile_center(t) + Vec3::Y * (LEVEL_H / 2.0);
    gizmos.cube(Transform::from_translation(c).with_scale(Vec3::new(1.03, LEVEL_H * 1.03, 1.03)), color);
}

fn draw_markers(mut gizmos: Gizmos, sim: Res<Sim>, ui: Res<UiState>, keys: Res<ButtonInput<KeyCode>>) {
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
            Desig::Fill => cell_outline(&mut gizmos, p, Color::srgb(0.75, 0.55, 0.3)),
            Desig::Deconstruct if m.feature[i].is_block() => cell_outline(&mut gizmos, p, Color::srgb(1.0, 0.3, 0.25)),
            Desig::Deconstruct => floor_outline(&mut gizmos, p, 0.12, Color::srgb(1.0, 0.3, 0.25)),
        }
    }

    // Furniture on the move: where it stands (until packed up), an arrow to where it goes.
    let moving = Color::srgb(0.4, 0.85, 1.0);
    for mv in &w.moves {
        if !mv.is_lifted() && mv.from.2 <= ui.top_at(m, mv.from.0, mv.from.1) {
            floor_outline(&mut gizmos, mv.from, 0.12, moving);
        }
        move_arrow(&mut gizmos, mv.from, mv.to, moving);
    }
    // Choosing where furniture goes: green where it can, red where it can't.
    if let (Tool::Move(Some(from)), Some(h)) = (ui.tool, ui.hover) {
        floor_outline(&mut gizmos, from, 0.06, Color::WHITE);
        match move_target(w, from, h) {
            Some(to) => {
                floor_outline(&mut gizmos, to, 0.06, Color::srgb(0.4, 1.0, 0.45));
                move_arrow(&mut gizmos, from, to, Color::srgb(0.4, 1.0, 0.45));
                if let Some(kind) = ui.turning(w) {
                    front_arrow(&mut gizmos, m, to, kind, ui.place_dir);
                }
            }
            None => floor_outline(&mut gizmos, h.stand, 0.06, Color::srgb(1.0, 0.3, 0.25)),
        }
    }

    // Quarries: where the stairs go down, from the ground outside to the bottom.
    let quarry_shown = matches!(ui.tool, Tool::Paint(Order::Quarry))
        || matches!(ui.selected, Some(Selection::Tile(t)) if m.quarry_at(t.0, t.1).is_some());
    if quarry_shown {
        let amber = Color::srgb(0.95, 0.7, 0.35);
        for q in &w.quarries {
            let Some((x, y)) = q.shaft else { continue };
            let top = (x as usize, y as usize, q.ground);
            if top.2 > ui.top_at(m, top.0, top.1) {
                continue;
            }
            floor_outline(&mut gizmos, top, 0.06, amber);
            let c = tile_center(top) + Vec3::Y * 0.1;
            let (dx, dy) = DIRS[q.out as usize];
            gizmos.arrow(c, c + Vec3::new(dx as f32, 0.0, dy as f32) * 0.7, amber);
            gizmos.line(c, Vec3::new(c.x, q.bottom() as f32 * LEVEL_H + 0.1, c.z), amber);
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

    // Something for a wall: the slot under the cursor, green if it fits there.
    if let (Some((wall, face, slot)), Tool::Paint(Order::Build(kind))) = (ui.wall_slot, ui.tool) {
        if kind.def().on_wall {
            let ok = w.decor_ok(wall, face, slot, kind);
            let lift = if m.floor[m.idx(wall)] { FLOOR_T } else { 0.0 };
            let t = super::props::decor_transform(wall, face, slot, lift);
            let c = [(-0.1, 0.3), (0.1, 0.3), (0.1, 0.9), (-0.1, 0.9)].map(|(x, y)| t.transform_point(Vec3::new(x * 0.95, y, 0.01)));
            let color = if ok { Color::srgb(0.4, 1.0, 0.45) } else { Color::srgb(1.0, 0.3, 0.25) };
            gizmos.linestrip([c[0], c[1], c[2], c[3], c[0]], color);
        }
    }
    // What's being placed: an arrow to its front.
    if let (Some(h), Tool::Paint(order @ Order::Build(kind))) = (ui.hover, ui.tool) {
        if kind.def().turnable() {
            front_arrow(&mut gizmos, m, order_target(w, h, order), kind, ui.place_dir);
        }
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
        // Walls: the inside stays empty.
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        if hollow(order, shift) && size.x >= 3.0 && size.y >= 3.0 {
            gizmos.rect(Isometry3d::new(c, Quat::from_rotation_x(FRAC_PI_2)), size - Vec2::splat(2.0), Color::WHITE);
        }
    }

    for p in w.pawns.iter().filter(|p| p.present() && ui.shows_at(m, p.x, p.y, p.z)) {
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
    fn saving_keeps_a_backup_and_loading_falls_back_to_it() {
        let dir = std::env::temp_dir().join(format!("my-world-safe-saves-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("game.json");
        let first = SimWorld::with_map(Map::flat(8, 8), my_world::rng::Rng::new(1));
        let second = SimWorld::with_map(Map::flat(9, 9), my_world::rng::Rng::new(2));
        write_save(&path, &first.save_json().unwrap()).unwrap();
        write_save(&path, &second.save_json().unwrap()).unwrap();
        assert!(!dir.join("game.json.tmp").exists(), "no leftovers");
        assert_eq!(read_save(&path).unwrap().0.map.len(), second.map.len());
        // A save cut off mid-write: the backup of the previous one is loaded instead.
        std::fs::write(&path, "{\"version\":").unwrap();
        let (loaded, which) = read_save(&path).unwrap();
        assert_eq!(loaded.map.len(), first.map.len());
        assert!(which.contains("backup"), "{which}");
        assert_eq!(saves_in(&dir).len(), 1, "backups aren't listed as saves");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saves_have_names() {
        let dir = std::env::temp_dir().join(format!("my-world-named-saves-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(save_file_in(&dir, "  "), None, "a name is needed");
        assert_eq!(save_file_in(&dir, "Test ground/../x?"), Some(dir.join("Test groundx.json")), "only safe characters");
        let w = SimWorld::with_map(Map::flat(8, 8), my_world::rng::Rng::new(1));
        for name in ["older", "Test ground"] {
            std::fs::write(save_file_in(&dir, name).unwrap(), w.save_json().unwrap()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let saves = saves_in(&dir);
        let names: Vec<&str> = saves.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(names, ["Test ground", "older"], "newest first");
        let loaded = SimWorld::load_json(&std::fs::read_to_string(&saves[0].1).unwrap()).unwrap();
        assert_eq!(loaded.map.len(), w.map.len());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_preview_shows_the_cells_a_click_would_take() {
        let mut w = SimWorld::with_map(Map::flat(10, 10), my_world::rng::Rng::new(1));
        let z = (0..w.map.d).find(|&z| w.map.standable((3, 3, z))).unwrap();
        let wall = Order::Build(my_world::defs::BuildId::named("wall"));
        // Walls: the outline of the box (Shift: all of it).
        assert_eq!(box_cells(&w, (2, 2, z), (5, 4, z), wall, hollow(wall, false)).len(), 4 * 3 - 2);
        assert_eq!(box_cells(&w, (2, 2, z), (5, 4, z), wall, hollow(wall, true)).len(), 4 * 3);
        // Only where it can go: not where a chair stands.
        let i = w.map.idx((2, 2, z));
        w.map.set_feature(i, my_world::map::Feature::built("chair"));
        let cells = box_cells(&w, (2, 2, z), (5, 4, z), wall, true);
        assert!(!cells.iter().any(|c| (c.0, c.1) == (2, 2)) && cells.len() == 4 * 3 - 2 - 1, "the outline but the chair's cell: {:?}", cells);
        // And painting takes just those.
        assert_eq!(paint_box(&mut w, (2, 2, z), (5, 4, z), wall, true, 0), cells.len());
    }

    #[test]
    fn windows_go_into_the_clicked_wall_and_walls_on_top() {
        let mut w = SimWorld::with_map(Map::flat(8, 8), my_world::rng::Rng::new(1));
        let z = (0..w.map.d).find(|&z| w.map.standable((3, 3, z))).unwrap();
        let wall = (3, 3, z);
        let i = w.map.idx(wall);
        w.map.set_feature(i, my_world::map::Feature::built("wall"));
        let h = Hover { solid: wall, stand: (3, 3, z + 1) };
        let build = |id: &str| Order::Build(my_world::defs::BuildId::named(id));
        assert_eq!(order_target(&w, h, build("window")), wall, "replacing the wall");
        assert_eq!(order_target(&w, h, build("door")), wall);
        assert_eq!(order_target(&w, h, build("wall")), (3, 3, z + 1), "stacked on top");
    }

    #[test]
    fn the_cursor_picks_a_wall_face_and_slot() {
        // Looking at the wall at (3, 3) from +z (map +y), down a little, aimed at x = 3.9.
        let wall = (3, 3, 1);
        let ray = |x: f32| Ray3d::new(Vec3::new(x, 1.8, 8.0), Dir3::new(Vec3::new(0.0, -0.3, -5.0)).unwrap());
        assert_eq!(wall_slot_at(ray(3.9), wall), Some((2, 4)), "the +y face, last slot");
        assert_eq!(wall_slot_at(ray(3.05), wall), Some((2, 0)));
        assert_eq!(wall_slot_at(ray(5.5), wall), None, "missed");
    }

    #[test]
    fn turning_goes_round_the_compass() {
        let mut d = 0;
        let mut seen = Vec::new();
        for _ in 0..4 {
            d = turn_dir(d, true);
            seen.push(DIRS[d as usize]);
        }
        assert_eq!(seen, vec![(0, 1), (-1, 0), (0, -1), (1, 0)], "a quarter turn at a time, back to the start");
        assert_eq!(turn_dir(turn_dir(2, true), false), 2, "and back");
    }

    #[test]
    fn walls_go_around_the_dragged_rectangle() {
        use my_world::map::{Feature, FLAT_Z as Z};
        use my_world::rng::Rng;
        let mut w = SimWorld::with_map(Map::flat(12, 12), Rng::new(1));
        let wall = Order::Build(BuildId::named("wall"));
        assert_eq!(paint_box(&mut w, (2, 2, Z), (6, 5, Z), wall, hollow(wall, false), 0), 14, "5×4 edge");
        let at = |w: &SimWorld, x, y| matches!(w.map.feature[w.map.idx((x, y, Z))], Feature::Blueprint { .. });
        assert!(at(&w, 2, 2) && at(&w, 6, 5) && at(&w, 4, 2) && at(&w, 2, 4));
        assert!(!at(&w, 3, 3) && !at(&w, 5, 4), "the inside stays empty");
        assert!(hollow(wall, false) && !hollow(wall, true), "Shift fills");
        let chair = Order::Build(BuildId::named("chair"));
        assert!(!hollow(chair, false), "furniture fills the rectangle");
        assert_eq!(paint_box(&mut w, (8, 8, Z), (9, 9, Z), chair, false, 2), 4);
        assert!(matches!(w.map.feature[w.map.idx((8, 8, Z))], Feature::Blueprint { dir: 2, .. }), "turned as placed");
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
                    m.feature[above] = Feature::blueprint(defs().roof().unwrap(), 0, 0);
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
        let id = PawnId::new;
        ui.select(Some(Selection::Pawn(id(1))));
        assert_eq!(ui.selected_pawns(), vec![id(1)]);
        ui.toggle_pawn(id(2));
        ui.toggle_pawn(id(3));
        assert_eq!(ui.selected_pawns(), vec![id(1), id(2), id(3)]);
        assert!(ui.is_selected(id(2)) && ui.is_selected(id(3)));
        assert_eq!(ui.selected, Some(Selection::Pawn(id(1))), "the first stays in the inspector");
        ui.toggle_pawn(id(1));
        ui.toggle_pawn(id(3));
        assert_eq!(ui.selected_pawns(), vec![id(2)]);
        assert!(ui.group.is_empty(), "one left: a plain selection again");
        ui.toggle_pawn(id(4));
        ui.select(Some(Selection::Tile((1, 1, 1))));
        assert!(ui.selected_pawns().is_empty() && !ui.is_selected(id(4)), "selecting a cell drops the group");
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
