//! Input handling and the immediate-mode interface around the map.

use crate::art::{draw_head, pawn_look, Look};
use crate::camera::View;
use crate::render::draw_world;
use macroquad::prelude::*;
use my_world::health::HediffKind;
use my_world::map::{BuildKind, Desig, Feature, ItemKind, Pos, Zone};
use my_world::pawn::{Faction, Pawn, Skill, WorkType};
use my_world::world::{Order, World};
use my_world::{DAY_LEN, TICK};

const HUD_H: f32 = 112.0;
const SAVE_PATH: &str = "savegame.json";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Paint(Order),
}

const TOOLS: [(Tool, &str, KeyCode); 11] = [
    (Tool::Select, "Esc Select", KeyCode::Escape),
    (Tool::Paint(Order::Desig(Desig::Chop)), "1 Chop", KeyCode::Key1),
    (Tool::Paint(Order::Desig(Desig::Mine)), "2 Mine", KeyCode::Key2),
    (Tool::Paint(Order::Desig(Desig::Forage)), "3 Forage", KeyCode::Key3),
    (Tool::Paint(Order::Zone(Zone::Stockpile)), "4 Stockpile", KeyCode::Key4),
    (Tool::Paint(Order::Zone(Zone::Grow)), "5 Grow zone", KeyCode::Key5),
    (Tool::Paint(Order::Build(BuildKind::Wall)), "6 Wall", KeyCode::Key6),
    (Tool::Paint(Order::Build(BuildKind::Door)), "7 Door", KeyCode::Key7),
    (Tool::Paint(Order::Build(BuildKind::Bed)), "8 Bed", KeyCode::Key8),
    (Tool::Paint(Order::Zone(Zone::None)), "9 Clear zone", KeyCode::Key9),
    (Tool::Paint(Order::Cancel), "0 Cancel", KeyCode::Key0),
];

const HELP: [&str; 18] = [
    "You don't control colonists directly - you give orders and they pick up the work.",
    "",
    "1-3   Designate trees to chop, rock/structures to mine, berry bushes to forage",
    "4/5   Paint stockpile zones (items get hauled there) and grow zones (crops)",
    "6-8   Place walls (4 stone), doors (4 wood), beds (8 wood); colonists haul materials",
    "9/0   Clear zones / cancel designations and blueprints",
    "      Drag with the left mouse button to paint a rectangle. Right click = back to Select.",
    "",
    "Enclosed rooms get a roof. Colonists sleeping in a bed indoors are happier.",
    "Hungry, tired or upset colonists have low mood and may have mental breaks.",
    "Wounds bleed and can get infected. Doctors (see P) rescue downed colonists to a bed",
    "and tend them - better with medicine and skill. Click a colonist, then Health for details.",
    "Raids come every few days: select a colonist and press F to draft (F with nothing",
    "selected drafts everyone). Drafted colonists shoot enemies in range; right-click to move,",
    "right-click an enemy to attack.",
    "",
    "WASD/arrows/middle-drag: pan   Wheel: zoom   Space: pause   Tab: speed   P: work priorities",
    "C: center camera   K: save   L: load   Shift+N: new colony   H: close this help",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Pawn(u32),
    Tile(Pos),
}

pub struct App {
    world: World,
    view: View,
    tool: Tool,
    speed: u32,
    paused: bool,
    acc: f32,
    selected: Option<Selection>,
    drag_start: Option<Pos>,
    last_tile: Option<Pos>,
    show_work: bool,
    show_help: bool,
    inspector_health: bool,
    ui_hover: bool,
}

fn mouse_in(r: Rect) -> bool {
    let (mx, my) = mouse_position();
    r.contains(vec2(mx, my))
}

fn bar(x: f32, y: f32, w: f32, frac: f32, color: Color) {
    draw_rectangle(x, y, w, 8.0, Color::new(0.2, 0.2, 0.22, 1.0));
    draw_rectangle(x, y, w * frac.clamp(0.0, 1.0), 8.0, color);
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(42)
}

impl App {
    pub fn new() -> Self {
        let mut app = App {
            world: World::new(seed()),
            view: View::new(),
            tool: Tool::Select,
            speed: 1,
            paused: false,
            acc: 0.0,
            selected: None,
            drag_start: None,
            last_tile: None,
            show_work: false,
            show_help: false,
            inspector_health: false,
            ui_hover: false,
        };
        app.center_on_colony();
        app
    }

    fn view_h() -> f32 {
        screen_height() - HUD_H
    }

    pub fn frame(&mut self) {
        let view_h = Self::view_h();
        self.handle_keys();

        if !self.paused && !self.world.colony_lost() {
            self.acc += get_frame_time().min(0.25) * self.speed as f32;
            let mut steps = 0;
            while self.acc >= TICK && steps < 40 {
                self.world.tick(TICK);
                self.acc -= TICK;
                steps += 1;
            }
            if steps == 40 {
                self.acc = 0.0;
            }
        }
        if let Some(Selection::Pawn(id)) = self.selected {
            if self.world.pawn(id).is_none() {
                self.selected = None;
            }
        }

        let (mx, my) = mouse_position();
        let tile = if my < view_h { self.view.screen_to_tile(mx, my, self.world.map.w, self.world.map.h) } else { None };
        if tile.is_some() {
            self.last_tile = tile;
        }

        clear_background(BLACK);
        let sel_pawn = match self.selected {
            Some(Selection::Pawn(id)) => Some(id),
            _ => None,
        };
        let drag = match (self.drag_start, self.last_tile) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        };
        draw_world(&self.world, &self.view, view_h, sel_pawn, drag);
        let ts = self.view.ts();
        if let Some(Selection::Tile(p)) = self.selected {
            let (sx, sy) = self.view.tile_to_screen(p.0 as f32, p.1 as f32);
            draw_rectangle_lines(sx, sy, ts, ts, 2.0, YELLOW);
        }
        if let Some(p) = tile {
            let (sx, sy) = self.view.tile_to_screen(p.0 as f32, p.1 as f32);
            draw_rectangle_lines(sx, sy, ts, ts, 1.0, Color::new(1.0, 1.0, 1.0, 0.6));
        }

        self.ui_hover = false;
        self.draw_ui(view_h);

        let mouse_on_map = tile.is_some() && !self.ui_hover;
        let drafted_selected = sel_pawn.and_then(|id| self.world.pawn(id)).map_or(false, |p| p.drafted);
        let right_drag = self.tool == Tool::Select && !drafted_selected;
        self.view.update(view_h, self.world.map.w, self.world.map.h, mouse_on_map, right_drag);
        self.handle_map_mouse(if self.ui_hover { None } else { tile });
    }

    // ---------- input ----------

    fn handle_keys(&mut self) {
        let shift = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
        if is_key_pressed(KeyCode::Space) {
            self.paused = !self.paused;
        }
        if is_key_pressed(KeyCode::Tab) {
            self.cycle_speed();
        }
        if is_key_pressed(KeyCode::Escape) {
            self.show_help = false;
            self.show_work = false;
        }
        for (tool, _, key) in TOOLS {
            if is_key_pressed(key) {
                self.tool = tool;
                self.drag_start = None;
            }
        }
        if is_key_pressed(KeyCode::P) {
            self.show_work = !self.show_work;
        }
        if is_key_pressed(KeyCode::H) {
            self.show_help = !self.show_help;
        }
        if is_key_pressed(KeyCode::F) {
            self.toggle_draft();
        }
        if is_key_pressed(KeyCode::C) {
            self.center_on_selection();
        }
        if is_key_pressed(KeyCode::K) {
            self.save();
        }
        if is_key_pressed(KeyCode::L) {
            self.load();
        }
        if is_key_pressed(KeyCode::N) && (shift || self.world.colony_lost()) {
            self.world = World::new(seed());
            self.selected = None;
            self.center_on_colony();
        }
    }

    fn cycle_speed(&mut self) {
        self.speed = match self.speed {
            1 => 2,
            2 => 3,
            _ => 1,
        };
    }

    fn handle_map_mouse(&mut self, tile: Option<Pos>) {
        match self.tool {
            Tool::Select => {
                if let Some(t) = tile {
                    if is_mouse_button_pressed(MouseButton::Left) {
                        self.select_at(t);
                    }
                    if is_mouse_button_pressed(MouseButton::Right) {
                        self.command_at(t);
                    }
                }
            }
            Tool::Paint(order) => {
                if let Some(t) = tile {
                    if is_mouse_button_pressed(MouseButton::Left) {
                        self.drag_start = Some(t);
                    }
                    if is_mouse_button_pressed(MouseButton::Right) {
                        self.tool = Tool::Select;
                        self.drag_start = None;
                        return;
                    }
                }
                if is_mouse_button_released(MouseButton::Left) {
                    if let (Some(a), Some(b)) = (self.drag_start, self.last_tile) {
                        for y in a.1.min(b.1)..=a.1.max(b.1) {
                            for x in a.0.min(b.0)..=a.0.max(b.0) {
                                self.world.apply_order((x, y), order);
                            }
                        }
                    }
                    self.drag_start = None;
                }
            }
        }
    }

    /// Pawn closest to the clicked tile (living ones first), within reach.
    fn pawn_at(&self, t: Pos) -> Option<u32> {
        let mut best: Option<(f32, u32)> = None;
        for p in self.world.pawns.iter().filter(|p| !p.left) {
            let d = ((p.x - t.0 as f32).powi(2) + (p.y - t.1 as f32).powi(2)).sqrt() + if p.dead { 0.5 } else { 0.0 };
            if d < 0.9 && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, p.id));
            }
        }
        best.map(|(_, id)| id)
    }

    fn select_at(&mut self, t: Pos) {
        self.selected = Some(match self.pawn_at(t) {
            Some(id) => Selection::Pawn(id),
            None => Selection::Tile(t),
        });
    }

    /// Right click with a drafted colonist selected: move there or attack.
    fn command_at(&mut self, t: Pos) {
        let Some(Selection::Pawn(id)) = self.selected else { return };
        let Some(p) = self.world.pawn(id) else { return };
        if !p.drafted {
            return;
        }
        let enemy = self
            .pawn_at(t)
            .filter(|&o| self.world.pawn(o).map_or(false, |o| o.faction == Faction::Raider && o.active()));
        match enemy {
            Some(target) => self.world.order_attack(id, target),
            None => self.world.order_move(id, t),
        }
    }

    fn toggle_draft(&mut self) {
        if let Some(Selection::Pawn(id)) = self.selected {
            if let Some(p) = self.world.pawn(id) {
                if p.is_colonist() {
                    let d = !p.drafted;
                    self.world.set_drafted(id, d);
                    return;
                }
            }
        }
        let colonists: Vec<(u32, bool)> =
            self.world.pawns.iter().filter(|p| p.is_colonist() && p.active()).map(|p| (p.id, p.drafted)).collect();
        let draft = colonists.iter().any(|(_, d)| !d);
        for (id, _) in colonists {
            self.world.set_drafted(id, draft);
        }
    }

    fn center_on_colony(&mut self) {
        let living: Vec<(f32, f32)> =
            self.world.pawns.iter().filter(|p| p.is_colonist() && !p.dead).map(|p| (p.x, p.y)).collect();
        if living.is_empty() {
            return;
        }
        let n = living.len() as f32;
        let cx = living.iter().map(|p| p.0).sum::<f32>() / n;
        let cy = living.iter().map(|p| p.1).sum::<f32>() / n;
        self.view.center_on(cx, cy, Self::view_h());
    }

    fn center_on_selection(&mut self) {
        match self.selected {
            Some(Selection::Pawn(id)) => {
                if let Some(p) = self.world.pawn(id) {
                    let (x, y) = (p.x, p.y);
                    self.view.center_on(x, y, Self::view_h());
                }
            }
            Some(Selection::Tile(t)) => self.view.center_on(t.0 as f32, t.1 as f32, Self::view_h()),
            None => self.center_on_colony(),
        }
    }

    fn save(&mut self) {
        let msg = match self.world.save_json() {
            Ok(json) => match std::fs::write(SAVE_PATH, json) {
                Ok(()) => format!("Game saved to {}.", SAVE_PATH),
                Err(e) => format!("Saving failed: {}", e),
            },
            Err(e) => format!("Saving failed: {}", e),
        };
        self.world.push_log(msg);
    }

    fn load(&mut self) {
        match std::fs::read_to_string(SAVE_PATH).map_err(|e| e.to_string()).and_then(|s| World::load_json(&s).map_err(|e| e.to_string())) {
            Ok(w) => {
                self.world = w;
                self.selected = None;
                self.world.push_log("Game loaded.".into());
            }
            Err(e) => self.world.push_log(format!("Loading failed: {}", e)),
        }
    }

    // ---------- widgets ----------

    fn panel(&mut self, r: Rect) {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.08, 0.08, 0.1, 0.92));
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, Color::new(0.4, 0.4, 0.45, 1.0));
        if mouse_in(r) {
            self.ui_hover = true;
        }
    }

    fn button(&mut self, r: Rect, label: &str, active: bool) -> bool {
        let over = mouse_in(r);
        if over {
            self.ui_hover = true;
        }
        let bg = if active {
            Color::new(0.55, 0.45, 0.15, 1.0)
        } else if over {
            Color::new(0.3, 0.3, 0.36, 1.0)
        } else {
            Color::new(0.18, 0.18, 0.22, 1.0)
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, Color::new(0.45, 0.45, 0.5, 1.0));
        let d = measure_text(label, None, 16, 1.0);
        draw_text(label, r.x + (r.w - d.width) / 2.0, r.y + r.h / 2.0 + 5.0, 16.0, WHITE);
        over && is_mouse_button_pressed(MouseButton::Left)
    }

    // ---------- panels ----------

    fn draw_ui(&mut self, view_h: f32) {
        self.draw_hud(view_h);
        self.draw_colonist_bar();
        self.draw_alerts();
        match self.selected {
            Some(Selection::Pawn(id)) => self.draw_pawn_inspector(id),
            Some(Selection::Tile(t)) => self.draw_tile_inspector(t),
            None => {}
        }
        if self.show_work {
            self.draw_work_panel();
        }
        if self.show_help {
            self.draw_help();
        }
        if self.world.colony_lost() {
            let msg = "Your colony has perished. Press N to start over.";
            let d = measure_text(msg, None, 36, 1.0);
            draw_text(msg, (screen_width() - d.width) / 2.0, view_h / 2.0, 36.0, RED);
        }
    }

    fn draw_hud(&mut self, view_h: f32) {
        let sw = screen_width();
        let top = view_h;
        self.panel(Rect::new(0.0, top, sw, HUD_H));

        let w = &self.world;
        let hour = w.hour();
        let status = format!(
            "Day {}  {:02}:{:02}    Wood {}   Stone {}   Food {}   Medicine {}    Colonists {}    Speed {}x{}",
            w.day(),
            hour as u32,
            ((hour - hour.floor()) * 60.0) as u32,
            w.map.count_items(ItemKind::Wood),
            w.map.count_items(ItemKind::Stone),
            w.map.count_items(ItemKind::Food),
            w.map.count_items(ItemKind::Medicine),
            w.colonists_alive(),
            self.speed,
            if self.paused { "  [PAUSED]" } else { "" }
        );
        draw_text(&status, 10.0, top + 20.0, 20.0, WHITE);

        // Tool buttons.
        let mut x = 8.0;
        for (tool, label, _) in TOOLS {
            let bw = measure_text(label, None, 16, 1.0).width + 16.0;
            if self.button(Rect::new(x, top + 30.0, bw, 26.0), label, self.tool == tool) {
                self.tool = tool;
                self.drag_start = None;
            }
            x += bw + 4.0;
        }

        // Toggles on the right of the tool row.
        let pause_label = if self.paused { "Resume" } else { "Pause" };
        let buttons = [(pause_label, 0), ("Speed", 1), ("Work (P)", 2), ("Help (H)", 3), ("Save", 4), ("Load", 5)];
        let mut bx = x + 16.0;
        for (label, action) in buttons {
            let bw = measure_text(label, None, 16, 1.0).width + 16.0;
            if self.button(Rect::new(bx, top + 30.0, bw, 26.0), label, false) {
                match action {
                    0 => self.paused = !self.paused,
                    1 => self.cycle_speed(),
                    2 => self.show_work = !self.show_work,
                    3 => self.show_help = !self.show_help,
                    4 => self.save(),
                    _ => self.load(),
                }
            }
            bx += bw + 4.0;
        }

        // Most recent log lines.
        let log = &self.world.log;
        for (k, msg) in log.iter().rev().take(3).enumerate() {
            let alpha = 1.0 - k as f32 * 0.25;
            draw_text(msg, 10.0, top + 74.0 + k as f32 * 15.0, 16.0, Color::new(0.85, 0.85, 0.65, alpha));
        }
    }

    fn draw_colonist_bar(&mut self) {
        let cards: Vec<(u32, String, f32, String, bool, bool, Look)> = self
            .world
            .pawns
            .iter()
            .filter(|p| p.is_colonist())
            .map(|p| {
                let trouble = p.downed || p.hunger <= 0.0 || p.break_timer > 0.0 || p.health.bleed_rate() > 0.0;
                (p.id, p.name.clone(), p.mood, p.status(), p.drafted, trouble || p.dead, pawn_look(p))
            })
            .collect();
        let mut x = 8.0;
        for (id, name, mood, status, drafted, trouble, look) in cards {
            let r = Rect::new(x, 8.0, 124.0, 40.0);
            let selected = self.selected == Some(Selection::Pawn(id));
            self.panel(r);
            if selected {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, WHITE);
            } else if drafted {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, GOLD);
            } else if trouble {
                draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, RED);
            }
            draw_head(r.x + 12.0, r.y + 11.0, 36.0, &look, 1.0, 1.0);
            draw_text(&name, r.x + 24.0, r.y + 16.0, 16.0, WHITE);
            bar(r.x + 6.0, r.y + 21.0, r.w - 12.0, mood / 100.0, mood_color(mood));
            draw_text(&status, r.x + 6.0, r.y + 37.0, 13.0, LIGHTGRAY);
            if mouse_in(r) && is_mouse_button_pressed(MouseButton::Left) {
                self.selected = Some(Selection::Pawn(id));
            }
            x += 128.0;
        }
    }

    fn draw_alerts(&mut self) {
        let alerts = self.world.alerts();
        let sw = screen_width();
        for (k, (text, urgent)) in alerts.iter().enumerate() {
            let r = Rect::new(sw - 230.0, 8.0 + k as f32 * 26.0, 222.0, 22.0);
            self.panel(r);
            draw_text(text, r.x + 8.0, r.y + 16.0, 16.0, if *urgent { RED } else { YELLOW });
        }
    }

    fn draw_pawn_inspector(&mut self, id: u32) {
        let Some(p) = self.world.pawn(id).cloned() else { return };
        let health_tab = self.inspector_health;
        let rows = if health_tab {
            3 + 9 + p.health.hediffs.len().max(1)
        } else {
            4 + p.thoughts().len() + if p.is_colonist() { 7 } else { 0 }
        };
        let r = Rect::new(8.0, 56.0, 360.0, 64.0 + rows as f32 * 18.0);
        self.panel(r);
        if self.button(Rect::new(r.x + r.w - 150.0, r.y + 6.0, 74.0, 22.0), "Overview", !health_tab) {
            self.inspector_health = false;
        }
        if self.button(Rect::new(r.x + r.w - 72.0, r.y + 6.0, 64.0, 22.0), "Health", health_tab) {
            self.inspector_health = true;
        }
        let (x, mut y) = (r.x + 10.0, r.y + 22.0);
        let title = if p.is_colonist() { p.name.clone() } else { format!("{} (raider)", p.name) };
        draw_text(&title, x, y, 22.0, WHITE);
        y += 20.0;
        draw_text(&format!("{} - {}", p.status(), p.weapon.name()), x, y, 16.0, LIGHTGRAY);
        y += 22.0;

        if health_tab {
            draw_health_tab(&p, x, y);
            return;
        }

        let mut need = |label: &str, value: f32, color: Color| {
            draw_text(label, x, y, 16.0, WHITE);
            bar(x + 70.0, y - 9.0, 200.0, value / 100.0, color);
            y += 18.0;
        };
        need("Health", p.health.summary() * 100.0, RED);
        if p.is_colonist() {
            need("Mood", p.mood, mood_color(p.mood));
            need("Food", p.hunger, ORANGE);
            need("Rest", p.rest, SKYBLUE);
        }

        if p.is_colonist() {
            y += 4.0;
            for (k, s) in Skill::ALL.iter().enumerate() {
                let col = (k % 2) as f32 * 150.0;
                let row = (k / 2) as f32 * 18.0;
                draw_text(&format!("{} {}", s.name(), p.skill(*s)), x + col, y + row, 16.0, LIGHTGRAY);
            }
            y += ((Skill::ALL.len() + 1) / 2) as f32 * 18.0 + 6.0;
        }

        for (label, value) in p.thoughts() {
            let color = if value >= 0.0 { GREEN } else { Color::new(1.0, 0.45, 0.4, 1.0) };
            draw_text(&format!("{:+}  {}", value as i32, label), x, y, 16.0, color);
            y += 18.0;
        }
        if p.is_colonist() && !p.dead {
            let hint = if p.drafted { "F: undraft   right-click: move/attack" } else { "F: draft" };
            draw_text(hint, x, y + 4.0, 14.0, GRAY);
        }
    }

    fn draw_tile_inspector(&mut self, t: Pos) {
        let m = &self.world.map;
        let i = m.idx(t);
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("Tile {}, {}", t.0, t.1));
        lines.push(match m.feature[i] {
            Feature::Crop { growth } => format!("Crop, {:.0}% grown", growth * 100.0),
            Feature::Blueprint { kind, delivered } => {
                let c = kind.cost();
                format!("{} blueprint ({}/{} {})", kind.name(), delivered, c.count, c.kind.name().to_lowercase())
            }
            f => format!("{}{}", f.name(), if m.terrain[i] == my_world::map::Terrain::Water { " (water)" } else { "" }),
        });
        if let Some(s) = m.items[i] {
            lines.push(format!("{} x{}", s.kind.name(), s.count));
        }
        match m.zone[i] {
            Zone::Stockpile => lines.push("Stockpile zone".into()),
            Zone::Grow => lines.push("Grow zone".into()),
            Zone::None => {}
        }
        if let Some(d) = m.desig[i] {
            lines.push(format!("Designated: {:?}", d));
        }
        let room = m.room.get(i).copied().unwrap_or(0);
        if room != 0 {
            lines.push(format!("Indoors, roofed room of {} tiles", m.room_sizes[room as usize]));
        } else {
            lines.push("Outdoors".into());
        }
        let r = Rect::new(8.0, 56.0, 300.0, 16.0 + lines.len() as f32 * 20.0);
        self.panel(r);
        for (k, line) in lines.iter().enumerate() {
            draw_text(line, r.x + 10.0, r.y + 22.0 + k as f32 * 20.0, 17.0, if k == 0 { WHITE } else { LIGHTGRAY });
        }
    }

    fn draw_work_panel(&mut self) {
        let rows: Vec<(u32, String, [u8; 6], [u8; 7])> = self
            .world
            .pawns
            .iter()
            .filter(|p| p.is_colonist() && !p.dead)
            .map(|p| (p.id, p.name.clone(), p.priorities, p.skills))
            .collect();
        let (name_w, cell_w, row_h) = (90.0, 92.0, 30.0);
        let w = name_w + cell_w * WorkType::ALL.len() as f32 + 20.0;
        let h = 80.0 + rows.len() as f32 * row_h;
        let r = Rect::new((screen_width() - w) / 2.0, 70.0, w, h);
        self.panel(r);
        draw_text("Work priorities  (click: 1 = first ... 4 = last, - = never)", r.x + 10.0, r.y + 22.0, 17.0, WHITE);
        for (c, wt) in WorkType::ALL.iter().enumerate() {
            draw_text(wt.name(), r.x + 10.0 + name_w + c as f32 * cell_w, r.y + 48.0, 16.0, LIGHTGRAY);
        }
        for (k, (id, name, prios, skills)) in rows.iter().enumerate() {
            let y = r.y + 58.0 + k as f32 * row_h;
            draw_text(name, r.x + 10.0, y + 18.0, 17.0, WHITE);
            for (c, wt) in WorkType::ALL.iter().enumerate() {
                let prio = prios[*wt as usize];
                let label = match (prio, wt.skill()) {
                    (0, _) => "-".to_string(),
                    (p, Some(s)) => format!("{}  (lv {})", p, skills[s as usize]),
                    (p, None) => p.to_string(),
                };
                let cell = Rect::new(r.x + 10.0 + name_w + c as f32 * cell_w, y, cell_w - 6.0, row_h - 4.0);
                if self.button(cell, &label, prio == 1) {
                    let next = if prio == 0 { 1 } else if prio == 4 { 0 } else { prio + 1 };
                    self.world.set_priority(*id, *wt, next);
                }
            }
        }
    }

    fn draw_help(&mut self) {
        let w = 760.0;
        let h = 30.0 + HELP.len() as f32 * 20.0;
        let r = Rect::new((screen_width() - w) / 2.0, 80.0, w, h);
        self.panel(r);
        for (k, line) in HELP.iter().enumerate() {
            draw_text(line, r.x + 14.0, r.y + 26.0 + k as f32 * 20.0, 17.0, WHITE);
        }
    }
}

fn mood_color(mood: f32) -> Color {
    if mood < 25.0 {
        RED
    } else if mood < 45.0 {
        ORANGE
    } else {
        GREEN
    }
}

fn capacity_color(v: f32) -> Color {
    if v < 0.5 {
        RED
    } else if v < 0.85 {
        ORANGE
    } else {
        GREEN
    }
}

/// RimWorld-style health tab: overall state, capacities, and every condition.
fn draw_health_tab(p: &Pawn, x: f32, mut y: f32) {
    let h = &p.health;
    let pain = match h.pain {
        v if v >= 0.6 => "mind-shattering",
        v if v >= 0.4 => "intense",
        v if v >= 0.2 => "serious",
        v if v >= 0.05 => "minor",
        _ => "none",
    };
    let overall = format!("Overall {:.0}%     Pain {:.0}% ({})", h.summary() * 100.0, h.pain * 100.0, pain);
    draw_text(&overall, x, y, 16.0, WHITE);
    y += 18.0;

    let bleed = h.bleed_rate();
    let (text, color) = match h.time_to_bleed_out() {
        Some(secs) => (format!("Bleeding {:.0}%/day - bleeds out in ~{:.0}h", bleed * 100.0, secs / DAY_LEN * 24.0), RED),
        None if bleed > 0.0 => (format!("Bleeding {:.0}%/day (body keeps up)", bleed * 100.0), ORANGE),
        None => ("Not bleeding".to_string(), LIGHTGRAY),
    };
    draw_text(&text, x, y, 16.0, color);
    y += 18.0;
    let mut extra = Vec::new();
    if h.blood_loss > 0.01 {
        extra.push(format!("Blood loss {:.0}%", h.blood_loss * 100.0));
    }
    if h.malnutrition > 0.01 {
        extra.push(format!("Malnutrition {:.0}%", h.malnutrition * 100.0));
    }
    draw_text(&extra.join("     "), x, y, 16.0, ORANGE);
    y += 22.0;

    for (name, v) in h.caps.list() {
        draw_text(name, x, y, 15.0, LIGHTGRAY);
        bar(x + 125.0, y - 9.0, 150.0, v, capacity_color(v));
        draw_text(&format!("{:.0}%", v * 100.0), x + 285.0, y, 15.0, WHITE);
        y += 18.0;
    }
    y += 6.0;

    if h.hediffs.is_empty() {
        draw_text("No injuries or illnesses", x, y, 15.0, GREEN);
        return;
    }
    for d in &h.hediffs {
        let part = d.part.map_or("Whole body", |p| p.name());
        let (detail, color) = match d.kind {
            HediffKind::Injury { permanent: true, .. } => (String::new(), GRAY),
            HediffKind::Injury { .. } => (format!(" {:.0}", d.severity), if d.bleed() > 0.0 { RED } else { ORANGE }),
            HediffKind::Missing => (String::new(), if d.bleed() > 0.0 { RED } else { GRAY }),
            HediffKind::Infection { immunity } | HediffKind::Flu { immunity } => {
                (format!(" {:.0}% (immunity {:.0}%)", d.severity * 100.0, immunity * 100.0), YELLOW)
            }
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
        draw_text(&text, x, y, 15.0, color);
        y += 18.0;
    }
}
