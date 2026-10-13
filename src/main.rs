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
        for _ in 0..30 {
            app.update();
        }
        // Frame cost of our systems (no GPU work here): useful with --release --nocapture.
        let t = std::time::Instant::now();
        for _ in 0..270 {
            app.update();
        }
        eprintln!("headless frame: {:.2} ms", t.elapsed().as_secs_f64() * 1e3 / 270.0);
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
            m.feature[stairs] = my_world::map::Feature::built("stairs");
            for x in cx + 3..cx + 7 {
                let i = m.idx((x, cy, z + 1));
                m.set_floor(i, true);
            }
            let lamp = m.idx((cx, cy + 2, z));
            m.feature[lamp] = my_world::map::Feature::built("lamp");
            let (fx, fy, fz) = (cx as f32, cy as f32, z as f32);
            sim.world.effects.push(my_world::world::Effect { from: (fx, fy, fz), to: (fx + 5.0, fy, fz), ttl: 0.15, hit: true });
        }
        app.world_mut().resource_mut::<front::UiState>().view_level = Some(5);
        for _ in 0..30 {
            app.update();
        }
        app.world_mut().resource_mut::<front::UiState>().view_level = None;
        for _ in 0..10 {
            app.update();
        }

        // Several colonists selected: F drafts them all; F with nothing
        // selected drafts everyone and selects them.
        let ids: Vec<u32> = app.world().resource::<front::Sim>().world.pawns.iter().filter(|p| p.is_colonist()).map(|p| p.id).collect();
        assert!(ids.len() >= 3, "a starting colony has a few colonists");
        app.world_mut().resource_mut::<front::UiState>().select_pawns(ids[..2].to_vec());
        let toggle = |app: &mut App| {
            app.world_mut().resource_scope(|world, mut sim: Mut<front::Sim>| {
                front::toggle_draft(&mut sim, &mut world.resource_mut::<front::UiState>());
            });
        };
        let drafted = |app: &App| -> Vec<u32> {
            app.world().resource::<front::Sim>().world.pawns.iter().filter(|p| p.drafted).map(|p| p.id).collect()
        };
        toggle(&mut app);
        assert_eq!(drafted(&app), ids[..2].to_vec(), "the two selected are drafted");
        toggle(&mut app);
        assert!(drafted(&app).is_empty(), "and undrafted again");
        app.world_mut().resource_mut::<front::UiState>().select(None);
        toggle(&mut app);
        assert_eq!(drafted(&app).len(), ids.len(), "everyone drafted");
        assert_eq!(app.world().resource::<front::UiState>().selected_pawns(), ids, "and selected");
        toggle(&mut app);
        for _ in 0..3 {
            app.update();
        }

        // Hidden roofs: a walled room with a built roof, seen into from above.
        let (room, z) = {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let m = &mut sim.world.map;
            let (cx, cy) = (m.w / 2 - 8, m.h / 2 - 8);
            let z = m.surface(cx, cy).unwrap();
            for y in cy..cy + 5 {
                for x in cx..cx + 5 {
                    let i = m.idx((x, y, z));
                    if x == cx || y == cy || x == cx + 4 || y == cy + 4 {
                        m.set_feature(i, my_world::map::Feature::built("wall"));
                    }
                    let above = m.idx((x, y, z + 1));
                    m.set_floor(above, true);
                }
            }
            m.recompute_rooms();
            ((cx + 2, cy + 2), z)
        };
        app.world_mut().resource_mut::<front::UiState>().hide_roofs = true;
        for _ in 0..5 {
            app.update();
        }
        {
            let ui = app.world().resource::<front::UiState>();
            let m = &app.world().resource::<front::Sim>().world.map;
            assert!(m.roofed((room.0, room.1, z)), "the test room is a room");
            assert_eq!(ui.top_at(m, room.0, room.1), z, "the roof over the room is hidden");
            // Only columns under a built roof are cut (not caves or tunnels under rock).
            for (k, &c) in ui.roof_cut.iter().enumerate() {
                if c != u8::MAX {
                    assert!(m.floor[m.idx((k % m.w, k / m.w, c as usize + 1))], "cut under a built floor");
                }
            }
        }
        app.world_mut().resource_mut::<front::UiState>().hide_roofs = false;
        for _ in 0..5 {
            app.update();
        }
        {
            let ui = app.world().resource::<front::UiState>();
            let m = &app.world().resource::<front::Sim>().world.map;
            assert!(ui.top_at(m, room.0, room.1) > z, "roofs back");
        }

        // The inspector's bills, stockpile and grow zone panels.
        let (bench, stock, grow) = {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            let (cx, cy) = (w.map.w / 2, w.map.h / 2);
            let z = w.map.surface(cx, cy).unwrap();
            let bench = (cx + 5, cy - 3, z);
            let i = w.map.idx(bench);
            w.map.feature[i] = my_world::map::Feature::built("stove");
            let cook = my_world::defs::RecipeId::named("cook_meal");
            w.add_bill(bench, cook, my_world::production::BillMode::Until(5));
            let grow = (cx - 6, cy - 3, z);
            let crop = my_world::defs::defs().sown().unwrap();
            w.apply_order(grow, my_world::world::Order::Zone(my_world::map::Zone::Grow(crop)));
            // Phase 8 pieces: fence rails join up, glass, a flickering torch, items on a shelf.
            for (k, id) in ["fence", "fence", "fence", "window", "torch", "shelf", "table", "chair"].into_iter().enumerate() {
                let i = w.map.idx((cx - 6 + k, cy + 6, z));
                w.map.feature[i] = my_world::map::Feature::built(id);
            }
            let shelf = w.map.idx((cx - 1, cy + 6, z));
            w.map.items[shelf] = Some(my_world::map::Stack { kind: my_world::defs::ItemId::named("wood"), count: 120 });
            (bench, (cx - 4, cy + 2, z), grow)
        };
        // The build menu docks on the left, collapses, and filters.
        let dock = app.world().resource::<front::UiState>().dock_width;
        assert!(dock > 150.0, "build menu is docked ({} wide)", dock);
        app.world_mut().resource_mut::<front::UiState>().build_filter = "chair".into();
        app.update();
        app.world_mut().resource_mut::<front::UiState>().show_build = false;
        app.update();
        app.update();
        let closed = app.world().resource::<front::UiState>().dock_width;
        assert!(closed > 0.0 && closed < 60.0, "collapsed to a strip ({} wide)", closed);
        app.world_mut().resource_mut::<front::UiState>().show_build = true;
        app.world_mut().resource_mut::<front::UiState>().show_resources = true;
        app.world_mut().resource_mut::<front::UiState>().show_sound = true;
        for t in [bench, stock, grow] {
            app.world_mut().resource_mut::<front::UiState>().selected = Some(front::Selection::Tile(t));
            for _ in 0..5 {
                app.update();
            }
        }

        // Sound: the placeholder files load (WAV support is on), ambience
        // loops exist, and a sound asked for gets played.
        let focus = app.world().resource::<front::CameraRig>().focus;
        app.world_mut().resource_mut::<front::SoundQueue>().play("hammer", Some(focus));
        app.world_mut().resource_mut::<front::SoundQueue>().play("ui_click", None);
        for _ in 0..20 {
            app.update();
        }
        let clip: Handle<AudioSource> = app.world().resource::<AssetServer>().load("sounds/hammer_1.wav");
        for _ in 0..200 {
            if app.world().resource::<AssetServer>().is_loaded(&clip) {
                break;
            }
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.world().resource::<AssetServer>().is_loaded(&clip), "WAV files load");
        let players = app.world_mut().query::<&AudioPlayer>().iter(app.world()).count();
        assert!(players >= 3, "ambience loops and effects ({} players)", players);
    }

    /// The character models and animations in `assets/models` load in Bevy
    /// (with all their buffers and textures). Loading glTF needs a GPU, so
    /// this only runs when asked: `cargo test --release -- --ignored`.
    #[test]
    #[ignore = "needs a GPU"]
    fn character_models_load() {
        let mut app = App::new();
        app.add_plugins(DefaultPlugins.build().disable::<WinitPlugin>());
        let files = [
            "models/characters/base/Superhero_Male_FullBody.gltf",
            "models/characters/base/Superhero_Female_FullBody.gltf",
            "models/characters/outfits/Male_Ranger.gltf",
            "models/characters/hair/Hair_SimpleParted.gltf",
            "models/characters/animations/UAL1_Standard.glb",
        ];
        let handles: Vec<Handle<Gltf>> = files.iter().map(|f| app.world().resource::<AssetServer>().load(*f)).collect();
        for _ in 0..2000 {
            app.update();
            let server = app.world().resource::<AssetServer>();
            if handles.iter().all(|h| server.is_loaded_with_dependencies(h) || server.load_state(h).is_failed()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let server = app.world().resource::<AssetServer>();
        for (f, h) in files.iter().zip(&handles) {
            assert!(server.is_loaded_with_dependencies(h), "{} didn't load: {:?}", f, server.load_state(h));
        }
        let gltfs = app.world().resource::<Assets<Gltf>>();
        let animations = gltfs.get(&handles[4]).unwrap().named_animations.len();
        assert!(animations > 20, "the animation library has {} animations", animations);
        eprintln!("animation library: {} animations", animations);
    }

    /// The whole game with a GPU: pawns' characters spawn, get their
    /// animations and a hand for weapons, and animate.
    /// `cargo test --release -- --ignored`
    #[test]
    #[ignore = "needs a GPU"]
    fn characters_animate() {
        let mut app = App::new();
        app.add_plugins(DefaultPlugins.build().disable::<WinitPlugin>())
            .add_plugins(EguiPlugin::default())
            .add_plugins(front::FrontPlugin)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        let ready = |app: &mut App| {
            let mut q = app.world_mut().query::<(&front::Rig, &AnimationPlayer)>();
            q.iter(app.world()).filter(|(r, _)| r.ready()).count()
        };
        for _ in 0..3000 {
            app.update();
            if ready(&mut app) >= 3 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let colonists = app.world().resource::<front::Sim>().world.pawns.len();
        assert_eq!(ready(&mut app), colonists, "every pawn's character is ready");
        for _ in 0..60 {
            app.update();
        }
        let mut q = app.world_mut().query::<(&front::Rig, &AnimationPlayer)>();
        let hands: Vec<Entity> = q
            .iter(app.world())
            .map(|(rig, player)| {
                assert!(player.playing_animations().next().is_some(), "an animation plays");
                rig.hand.expect("found the right hand bone")
            })
            .collect();
        // Weapons in hands are sized from their definitions, not left as unit cubes.
        let mut held = app.world_mut().query::<(&ChildOf, &Transform, &Mesh3d)>();
        for (parent, tf, _) in held.iter(app.world()) {
            if hands.contains(&parent.parent()) {
                assert!(tf.scale.max_element() < 1.0, "weapon scaled {:?}", tf.scale);
            }
        }
    }
}
