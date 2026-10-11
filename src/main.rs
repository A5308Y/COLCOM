//! My World: a small RimWorld-style colony sim. The simulation lives in the
//! library crate; this binary is just the window, rendering and input.

mod art;
mod camera;
mod render;
mod ui;

use macroquad::prelude::*;

fn window_conf() -> Conf {
    Conf {
        window_title: "My World".to_owned(),
        window_width: 1280,
        window_height: 800,
        window_resizable: true,
        // Multisampling smooths the edges of all the drawn shapes.
        sample_count: 4,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut app = ui::App::new();
    loop {
        app.frame();
        next_frame().await;
    }
}
