//! Pan/zoom view onto the map.

use macroquad::prelude::*;
use my_world::map::Pos;

/// Tile size in world pixels at zoom 1.
pub const TS: f32 = 16.0;

pub struct View {
    /// World-pixel position of the screen's top-left corner.
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
    drag_last: Option<(f32, f32)>,
}

impl View {
    pub fn new() -> Self {
        View { x: 0.0, y: 0.0, zoom: 1.5, drag_last: None }
    }

    pub fn ts(&self) -> f32 {
        TS * self.zoom
    }

    /// Screen position of a point given in tile units.
    pub fn tile_to_screen(&self, tx: f32, ty: f32) -> (f32, f32) {
        ((tx * TS - self.x) * self.zoom, (ty * TS - self.y) * self.zoom)
    }

    pub fn screen_to_tile(&self, sx: f32, sy: f32, w: usize, h: usize) -> Option<Pos> {
        let tx = ((sx / self.zoom + self.x) / TS).floor();
        let ty = ((sy / self.zoom + self.y) / TS).floor();
        if tx < 0.0 || ty < 0.0 {
            return None;
        }
        let (x, y) = (tx as usize, ty as usize);
        if x < w && y < h {
            Some((x, y))
        } else {
            None
        }
    }

    pub fn center_on(&mut self, tx: f32, ty: f32, view_h: f32) {
        self.x = (tx + 0.5) * TS - screen_width() / 2.0 / self.zoom;
        self.y = (ty + 0.5) * TS - view_h / 2.0 / self.zoom;
    }

    /// WASD/arrows to pan, middle or right mouse drag to pan, wheel to zoom.
    pub fn update(&mut self, view_h: f32, map_w: usize, map_h: usize, mouse_on_map: bool, allow_right_drag: bool) {
        let dt = get_frame_time();
        let speed = 700.0 * dt / self.zoom;
        if is_key_down(KeyCode::A) || is_key_down(KeyCode::Left) {
            self.x -= speed;
        }
        if is_key_down(KeyCode::D) || is_key_down(KeyCode::Right) {
            self.x += speed;
        }
        if is_key_down(KeyCode::W) || is_key_down(KeyCode::Up) {
            self.y -= speed;
        }
        if is_key_down(KeyCode::S) || is_key_down(KeyCode::Down) {
            self.y += speed;
        }

        let (mx, my) = mouse_position();
        let dragging = is_mouse_button_down(MouseButton::Middle)
            || (allow_right_drag && is_mouse_button_down(MouseButton::Right));
        if dragging {
            if let Some((lx, ly)) = self.drag_last {
                self.x -= (mx - lx) / self.zoom;
                self.y -= (my - ly) / self.zoom;
            }
            self.drag_last = Some((mx, my));
        } else {
            self.drag_last = None;
        }

        let wheel = mouse_wheel().1;
        if wheel != 0.0 && mouse_on_map {
            let before = (mx / self.zoom + self.x, my / self.zoom + self.y);
            let factor = if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 };
            self.zoom = (self.zoom * factor).clamp(0.5, 3.0);
            self.x = before.0 - mx / self.zoom;
            self.y = before.1 - my / self.zoom;
        }

        let margin = 64.0;
        let max_x = map_w as f32 * TS - screen_width() / self.zoom + margin;
        let max_y = map_h as f32 * TS - view_h / self.zoom + margin;
        self.x = self.x.max(-margin).min(max_x.max(-margin));
        self.y = self.y.max(-margin).min(max_y.max(-margin));
    }
}
