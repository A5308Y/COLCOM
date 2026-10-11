//! My World: a small RimWorld-style colony sim. The simulation lives in the
//! library crate; this binary is the Bevy app that shows and controls it.

mod front;

use bevy::prelude::*;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy_egui::EguiPlugin;

fn main() {
    // MY_WORLD_NO_GPU=1 runs everything except drawing: a smoke test for
    // machines (or sandboxes) without GPU access.
    let mut wgpu = WgpuSettings::default();
    if std::env::var_os("MY_WORLD_NO_GPU").is_some() {
        wgpu.backends = None;
    }
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "My World".into(),
                        resolution: (1280u32, 800u32).into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(wgpu)), ..default() }),
        )
        .add_plugins(EguiPlugin::default())
        .add_plugins(front::FrontPlugin)
        .run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy::winit::WinitPlugin;
    use std::time::Duration;

    /// Runs the whole app (simulation, scene sync, input, UI) for a few
    /// hundred frames without a window system or GPU, to catch Bevy
    /// scheduling and query conflicts that only show up at runtime.
    #[test]
    fn app_runs_headless() {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(Box::new(WgpuSettings { backends: None, ..default() })),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .add_plugins(front::FrontPlugin)
        // Fixed 60 fps clock, independent of how fast the test machine is.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        let start = app.world().resource::<front::Sim>().world.time;
        for _ in 0..300 {
            app.update();
        }
        let sim = app.world().resource::<front::Sim>();
        assert!(sim.world.time > start + 3.0, "simulation should have advanced");
        // Terrain chunks, trees, items and pawns all become meshes.
        let meshes = app.world_mut().query::<&Mesh3d>().iter(app.world()).count();
        assert!(meshes > 1000, "only {} meshes in the scene", meshes);

        // Build a floor and stairs by hand, then look at them in the layer view.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let m = &mut sim.world.map;
            let (cx, cy) = (m.w / 2, m.h / 2);
            let z = m.surface(cx, cy).unwrap();
            let stairs = m.idx((cx + 3, cy, z));
            m.feature[stairs] = my_world::map::Feature::Stairs;
            for x in cx + 3..cx + 7 {
                let i = m.idx((x, cy, z + 1));
                m.set_floor(i, true);
            }
        }
        app.world_mut().resource_mut::<front::UiState>().view_level = Some(5);
        for _ in 0..30 {
            app.update();
        }
        app.world_mut().resource_mut::<front::UiState>().view_level = None;
        for _ in 0..10 {
            app.update();
        }
    }
}
