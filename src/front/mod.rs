//! Bevy front end: 3D scene, camera, input and egui interface around the
//! simulation library.

mod camera;
mod input;
mod look;
mod overlay;
mod pawns;
mod props;
mod scene;
mod sim;
mod terrain;
mod ui;

use bevy::prelude::*;
use my_world::map::Pos;

#[cfg(test)]
pub use sim::Sim;

pub struct FrontPlugin;

impl Plugin for FrontPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            sim::SimPlugin,
            camera::CameraPlugin,
            scene::ScenePlugin,
            terrain::TerrainPlugin,
            props::PropsPlugin,
            pawns::PawnsPlugin,
            input::InputPlugin,
            ui::UiPlugin,
        ));
    }
}

/// World-space centre of a tile on the ground. Tiles are 1×1, x → X, y → Z.
pub fn tile_center(p: Pos) -> Vec3 {
    Vec3::new(p.0 as f32 + 0.5, 0.0, p.1 as f32 + 0.5)
}

/// World-space position for a fractional tile coordinate (pawn positions).
pub fn tile_point(x: f32, y: f32) -> Vec3 {
    Vec3::new(x + 0.5, 0.0, y + 0.5)
}
