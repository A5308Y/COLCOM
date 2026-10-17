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

        let ids: Vec<u32> = app.world().resource::<front::Sim>().world.pawns.iter().filter(|p| p.is_colonist()).map(|p| p.id).collect();
        assert!(ids.len() >= 3, "a starting colony has a few colonists");
        app.world_mut().resource_mut::<front::UiState>().select_pawns(ids[..2].to_vec());
        for _ in 0..3 {
            app.update();
        }

        // Squads: a leader with a member, a civilian; the Squads window and the Combat tab.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            w.set_role(ids[0], my_world::pawn::Role::Leader).unwrap();
            w.set_role(ids[1], my_world::pawn::Role::Member(ids[0])).unwrap();
            w.set_role(ids[2], my_world::pawn::Role::Civilian).unwrap();
        }
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.show_squads = true;
            ui.select(Some(front::Selection::Pawn(ids[0])));
            ui.inspector_tab = front::InspectorTab::Combat;
        }
        for _ in 0..5 {
            app.update();
        }
        // The Health tab's operations: a lost leg, then a prosthetic ordered for it.
        let healthy = {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            let i = w.pawn_index(ids[2]).unwrap();
            let healthy = w.pawns[i].health.clone();
            let mut rng = my_world::rng::Rng::new(1);
            let h = &mut w.pawns[i].health;
            while !h.is_missing(my_world::health::Part::LeftLeg) {
                h.injure(my_world::health::Part::LeftLeg, 50.0, my_world::health::DamageKind::Cut, &mut rng);
            }
            h.recompute();
            healthy
        };
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.show_squads = false;
            ui.select(Some(front::Selection::Pawn(ids[2])));
            ui.inspector_tab = front::InspectorTab::Health;
        }
        for ordered in [false, true] {
            if ordered {
                let leg = my_world::defs::ItemId::named("prosthetic_leg");
                let mut sim = app.world_mut().resource_mut::<front::Sim>();
                assert!(sim.world.order_operation(ids[2], my_world::health::Part::LeftLeg, leg));
            }
            for _ in 0..3 {
                app.update();
            }
        }
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let i = sim.world.pawn_index(ids[2]).unwrap();
            sim.world.pawns[i].health = healthy;
            sim.world.pawns[i].health.recompute();
            sim.world.pawns[i].operations.clear();
        }
        // The right-click menu: equip a rifle lying beside them, then pick it.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            // Up and about (the app runs on real time, so they may be in any state).
            let i = w.pawn_index(ids[0]).unwrap();
            w.pawns[i].break_timer = 0.0;
            let p = w.pawns[i].pos;
            let rifle = my_world::defs::ItemId::named("rifle");
            w.map.drop_item(p, my_world::map::Stack { kind: rifle, count: 1 });
            // Where it landed (beside them if their own cell was taken).
            let at = (0..w.map.len())
                .filter(|&k| w.map.items[k].is_some_and(|s| s.kind == rifle))
                .map(|k| w.map.pos(k))
                .min_by_key(|c| c.0.abs_diff(p.0) + c.1.abs_diff(p.1) + c.2.abs_diff(p.2))
                .unwrap();
            let options: Vec<_> = w.job_options(ids[0], at).into_iter().map(|(l, j)| (l, at, j)).collect();
            assert!(!options.is_empty(), "something to do there");
            app.world_mut().resource_mut::<front::UiState>().job_menu = Some(front::JobMenu { pawn: ids[0], options, at: None });
        }
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().resource::<front::UiState>().job_menu.is_some(), "stays open until something is picked");
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.job_menu = None;
            ui.select(None);
            ui.inspector_tab = front::InspectorTab::Overview;
        }

        // A raid: the parley dialog, then a battle. The player's unit moves; then
        // the AI plays both sides to the end while the battle screen draws.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            w.map.drop_item((w.map.w / 2, w.map.h / 2 + 3, w.map.surface(w.map.w / 2, w.map.h / 2 + 3).unwrap()),
                my_world::map::Stack { kind: my_world::defs::ItemId::named("steel"), count: 40 });
            w.spawn_raid();
            assert!(w.parley.is_some(), "raiders make a demand");
        }
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().resource::<front::UiState>().dock_width, 0.0, "no build menu during a raid");
        app.world_mut().resource_mut::<front::Sim>().world.refuse_tribute();
        for _ in 0..5 {
            app.update();
        }
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            let b = w.battle.as_ref().expect("a battle");
            assert_eq!(b.side_to_act, my_world::pawn::Faction::Colony);
            let u = (0..b.units.len()).find(|&u| b.units[u].side == my_world::pawn::Faction::Colony && b.units[u].can_act()).unwrap();
            let leader = b.units[u].members[0];
            w.battle_order(my_world::battle::BattleOrder::Activate(u)).unwrap();
            let lead = w.pawn(leader).unwrap().pos;
            let to = (lead.0 + 1, lead.1, lead.2);
            if w.move_cost(u, to).is_ok() {
                w.battle_order(my_world::battle::BattleOrder::Move(to)).unwrap();
            }
        }
        for _ in 0..30 {
            app.update();
        }
        app.world_mut().resource_mut::<front::Sim>().world.battle.as_mut().unwrap().auto_colony = true;
        app.world_mut().resource_mut::<front::Sim>().speed = 5;
        for _ in 0..20000 {
            if app.world().resource::<front::Sim>().world.battle.is_none() {
                break;
            }
            app.update();
        }
        assert!(app.world().resource::<front::Sim>().world.battle.is_none(), "the battle ends");
        assert_eq!(app.world().resource::<front::Sim>().speed, 1, "back to 1x after the battle");
        app.world_mut().resource_mut::<front::Sim>().speed = 1;
        for _ in 0..10 {
            app.update();
        }

        // Roofs: a walled room with a built roof, drawn as a roof, and hidden to see into it.
        let (room, z) = {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let m = &mut sim.world.map;
            // A flat, empty 5x5 patch near the middle (the map is random).
            let flat = |m: &my_world::map::Map, cx: usize, cy: usize| {
                let z = m.surface(cx, cy)?;
                (cy..cy + 5)
                    .all(|y| (cx..cx + 5).all(|x| m.surface(x, y) == Some(z) && m.feature[m.idx((x, y, z))] == my_world::map::Feature::None))
                    .then_some(z)
            };
            let (cx, cy, z) = (8..m.h - 8)
                .flat_map(|y| (8..m.w - 8).map(move |x| (x, y)))
                .find_map(|(x, y)| flat(m, x, y).map(|z| (x, y, z)))
                .expect("somewhere flat");
            for y in cy..cy + 5 {
                for x in cx..cx + 5 {
                    let i = m.idx((x, y, z));
                    if x == cx || y == cy || x == cx + 4 || y == cy + 4 {
                        m.set_feature(i, my_world::map::Feature::built("wall"));
                    } else {
                        let above = m.idx((x, y, z + 1));
                        m.set_roof(above, true);
                    }
                }
            }
            m.recompute_rooms();
            ((cx + 2, cy + 2), z)
        };
        let roof_meshes = |app: &mut App| app.world_mut().query::<(&front::RoofMesh, &Mesh3d)>().iter(app.world()).count();
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(roof_meshes(&mut app), 1, "the roof is drawn");
        app.world_mut().resource_mut::<front::UiState>().view_level = Some(z);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(roof_meshes(&mut app), 0, "the layer view below the roof hides it");
        app.world_mut().resource_mut::<front::UiState>().view_level = None;
        app.world_mut().resource_mut::<front::UiState>().hide_roofs = true;
        for _ in 0..5 {
            app.update();
        }
        {
            let ui = app.world().resource::<front::UiState>();
            let m = &app.world().resource::<front::Sim>().world.map;
            assert!(m.roofed((room.0, room.1, z)), "the test room is a room");
            assert_eq!(ui.top_at(m, room.0, room.1), z, "the roof over the room is hidden");
            // Only columns under a built roof or floor are cut (not caves or tunnels under rock).
            for (k, &c) in ui.roof_cut.iter().enumerate() {
                if c != u8::MAX {
                    let i = m.idx((k % m.w, k / m.w, c as usize + 1));
                    assert!(m.roof[i] || m.roof_blueprint(i) || m.floor[i], "cut under a roof or floor");
                }
            }
        }
        assert_eq!(roof_meshes(&mut app), 0, "no roofs drawn while they're hidden");
        app.world_mut().resource_mut::<front::UiState>().hide_roofs = false;
        for _ in 0..5 {
            app.update();
        }
        {
            let ui = app.world().resource::<front::UiState>();
            let m = &app.world().resource::<front::Sim>().world.map;
            assert!(ui.top_at(m, room.0, room.1) > z, "roofs back");
        }
        assert_eq!(roof_meshes(&mut app), 1, "and drawn again");

        // The inspector's bills, stockpile, grow zone and bed panels.
        let room_z = z;
        let (bench, stock, grow, bed, grave) = {
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
            // A bed given to a squad leader.
            let bed = (cx + 5, cy - 5, z);
            let i = w.map.idx(bed);
            w.map.feature[i] = my_world::map::Feature::built("bed");
            let leader = w.pawns[0].id;
            w.set_role(leader, my_world::pawn::Role::Leader).unwrap();
            assert!(w.assign_bed(bed, Some(leader)));
            // A grave with someone in it.
            let grave = (cx + 7, cy - 5, z);
            let i = w.map.idx(grave);
            w.map.feature[i] = my_world::map::Feature::built("grave");
            let dead = w.spawn_colonist(grave);
            let k = w.pawn_index(dead).unwrap();
            w.pawns[k].dead = true;
            w.pawns[k].buried = true;
            // The roof test's room, made a workshop.
            for (dx, id) in [(-1, "sawbench"), (1, "table")] {
                let i = w.map.idx(((room.0 as i32 + dx) as usize, room.1, room_z));
                w.map.feature[i] = my_world::map::Feature::built(id);
            }
            w.map.rooms_dirty = true;
            (bench, (cx - 4, cy + 2, z), grow, bed, grave)
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
        app.world_mut().resource_mut::<front::UiState>().show_menu = true;
        app.world_mut().resource_mut::<front::UiState>().show_rooms = true;
        for _ in 0..40 {
            app.update();
        }
        {
            let w = &app.world().resource::<front::Sim>().world;
            assert_eq!(w.room_at((room.0, room.1, room_z)).and_then(|r| r.kind_name()), Some("Workshop"));
        }
        for t in [bench, stock, grow, bed, grave, (room.0, room.1, room_z)] {
            app.world_mut().resource_mut::<front::UiState>().selected = Some(front::Selection::Tile(t));
            for _ in 0..5 {
                app.update();
            }
        }
        // Moving furniture: the tool choosing a spot, then the move's arrow and its inspector.
        let spot = {
            let w = &app.world().resource::<front::Sim>().world;
            [(1, 0), (0, 1), (-1, 0), (0, -1), (2, 0), (0, 2)]
                .into_iter()
                .map(|(dx, dy)| ((bed.0 as i32 + dx) as usize, (bed.1 as i32 + dy) as usize, bed.2))
                .find(|&to| w.move_target_ok(bed, to))
                .expect("room beside the bed")
        };
        app.world_mut().resource_mut::<front::UiState>().tool = front::Tool::Move(Some(bed));
        app.update();
        app.world_mut().resource_mut::<front::UiState>().tool = front::Tool::Select;
        assert!(app.world_mut().resource_mut::<front::Sim>().world.order_move(bed, spot));
        for t in [bed, spot] {
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
        // Every kind of sound file the game uses decodes: WAV (placeholders),
        // OGG (Kenney) and MP3 (the raid music).
        for file in ["sounds/hammer_1.wav", "sounds/kenney/footstep_grass_000.ogg", "sounds/music/dream_raid.mp3"] {
            let clip: Handle<AudioSource> = app.world().resource::<AssetServer>().load(file);
            for _ in 0..400 {
                if app.world().resource::<AssetServer>().is_loaded(&clip) {
                    break;
                }
                app.update();
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(app.world().resource::<AssetServer>().is_loaded(&clip), "{} loads", file);
        }
        let players = app.world_mut().query::<&AudioPlayer>().iter(app.world()).count();
        assert!(players >= 3, "ambience loops and effects ({} players)", players);
    }

    /// The app started in the battle scenario: the battle screen draws from
    /// the first frame, the player's unit moves, and restarting brings the
    /// scenario back.
    #[test]
    fn battle_scenario_runs_headless() {
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
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)));
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            sim.scenario = Some("battle".into());
            let world = sim.fresh_world();
            sim.replace(world);
        }
        for _ in 0..60 {
            app.update();
        }
        let roofs = app.world_mut().query::<&front::RoofMesh>().iter(app.world()).count();
        assert_eq!(roofs, 1, "the house's roof is drawn");
        let (leader, to) = {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            let b = w.battle.as_ref().expect("the scenario starts in battle");
            assert_eq!(b.round, 1);
            let u = (0..b.units.len()).find(|&u| b.units[u].side == my_world::pawn::Faction::Colony && b.units[u].led).unwrap();
            let leader = b.units[u].members[0];
            w.battle_order(my_world::battle::BattleOrder::Activate(u)).unwrap();
            let lead = w.pawn(leader).unwrap().pos;
            let to = (lead.0 + 2, lead.1, lead.2);
            w.battle_order(my_world::battle::BattleOrder::Move(to)).unwrap();
            (leader, to)
        };
        for _ in 0..240 {
            app.update();
        }
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            assert!(sim.world.battle.is_some(), "the battle goes on");
            assert_eq!(sim.world.pawn(leader).unwrap().pos, to, "the squad leader walked there");
            let world = sim.fresh_world();
            sim.replace(world);
            assert_eq!(sim.scenario.as_deref(), Some("battle"));
            assert!(sim.world.battle.as_ref().is_some_and(|b| b.round == 1), "restarted");
        }
        for _ in 0..10 {
            app.update();
        }
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
