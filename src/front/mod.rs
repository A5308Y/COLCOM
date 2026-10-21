//! Bevy front end: 3D scene, camera, input and egui interface around the
//! simulation library.

mod audio;
mod battle_ui;
mod build_menu;
mod camera;
mod carry;
mod characters;
mod expeditions_ui;
mod help;
mod info;
mod input;
mod look;
mod overlay;
mod pawns;
mod props;
mod roofs;
mod scene;
pub mod screenshot;
mod sim;
mod squads_ui;
mod terrain;
mod theme;
mod ui;
mod weather;

use bevy::prelude::*;
use colcom::map::Pos;

#[cfg(test)]
pub use input::{InspectorTab, JobMenu, Selection, Tool, UiState};
#[cfg(test)]
pub use info::InfoTarget;
#[cfg(test)]
pub use input::Hover;
#[cfg(test)]
pub use audio::SoundQueue;
#[cfg(test)]
pub use camera::CameraRig;
#[cfg(test)]
pub use sim::Sim;
#[cfg(test)]
pub use characters::Rig;
#[cfg(test)]
pub use roofs::RoofMesh;

pub struct FrontPlugin;

impl Plugin for FrontPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            sim::SimPlugin,
            camera::CameraPlugin,
            scene::ScenePlugin,
            terrain::TerrainPlugin,
            props::PropsPlugin,
            roofs::RoofsPlugin,
            characters::CharactersPlugin,
            pawns::PawnsPlugin,
            carry::CarryPlugin,
            battle_ui::BattleUiPlugin,
            input::InputPlugin,
            ui::UiPlugin,
            audio::AudioPlugin,
            weather::WeatherPlugin,
        ));
    }
}

/// World-space centre of a cell's floor. Cells are 1×1×LEVEL_H: x → X,
/// level → Y, y → Z.
pub fn tile_center(p: Pos) -> Vec3 {
    tile_point(p.0 as f32, p.1 as f32, p.2 as f32)
}

/// World-space position for fractional cell coordinates (pawn positions).
pub fn tile_point(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x + 0.5, z * terrain::LEVEL_H, y + 0.5)
}
