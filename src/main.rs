//! My World: a small RimWorld-style colony sim. The simulation lives in the
//! library crate; this binary is the Bevy app that shows and controls it.

mod front;

use bevy::prelude::*;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::window::ExitCondition;
use bevy_egui::EguiPlugin;

fn main() {
    // MY_WORLD_NO_GPU=1 runs everything except drawing: a smoke test for
    // machines (or sandboxes) without GPU access.
    let mut wgpu = WgpuSettings::default();
    let no_gpu = std::env::var_os("MY_WORLD_NO_GPU").is_some();
    if no_gpu {
        wgpu.backends = None;
    }
    // `--screenshot`: save interface shots to target/screenshots and quit.
    // Without a GPU there's no window either: the shots are drawn on the CPU.
    let shots = front::screenshot::requested();
    let windowless = no_gpu && shots.is_some();
    let mut plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: (!windowless).then(|| Window {
                title: "My World".into(),
                resolution: (1280u32, 800u32).into(),
                ..default()
            }),
            // Windowless shots end by themselves, not by closing the window.
            exit_condition: if windowless { ExitCondition::DontExit } else { ExitCondition::OnAllClosed },
            ..default()
        })
        .set(RenderPlugin { render_creation: RenderCreation::Automatic(Box::new(wgpu)), ..default() });
    if windowless {
        plugins = plugins.disable::<bevy::winit::WinitPlugin>();
    }
    let mut app = App::new();
    app.add_plugins(plugins).add_plugins(EguiPlugin::default()).add_plugins(front::FrontPlugin);
    if let Some(shots) = shots {
        app.add_plugins(front::screenshot::ScreenshotPlugin(shots));
    }
    if windowless {
        app.add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(std::time::Duration::from_millis(16)));
    }
    app.run();
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

        let ids: Vec<my_world::pawn::PawnId> = app.world().resource::<front::Sim>().world.pawns.iter().filter(|p| p.is_colonist()).map(|p| p.id).collect();
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
        // The Health tab's cyberware: a lost leg, then a prosthetic ordered for it,
        // and an elective implant already installed.
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
            h.install(my_world::health::CyberSlot::Eyes, my_world::defs::ItemId::named("bionic_eyes"), 1.25);
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
                assert!(sim.world.order_operation(ids[2], my_world::health::CyberSlot::LeftLeg, leg));
                assert!(sim.world.order_removal(ids[2], my_world::health::CyberSlot::Eyes));
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
            sim.world.pawns[i].surgeries.clear();
        }
        // Armour and a helmet show on whoever wears them.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let i = sim.world.pawn_index(ids[1]).unwrap();
            for kind in ["steel_armor", "steel_helmet"] {
                sim.world.pawns[i].apparel.push(my_world::defs::ItemId::named(kind));
            }
            let i = sim.world.pawn_index(ids[0]).unwrap();
            sim.world.pawns[i].apparel.push(my_world::defs::ItemId::named("powered_armor"));
            sim.world.pawns[i].weapon = Some(my_world::defs::ItemId::named("laser_rifle"));
        }
        for _ in 0..3 {
            app.update();
        }
        // The testing window to drop items, with its tool in hand.
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.show_spawn = true;
            ui.tool = front::Tool::Spawn(my_world::defs::ItemId::named("copper"));
        }
        for _ in 0..3 {
            app.update();
        }
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.show_spawn = false;
            ui.tool = front::Tool::Select;
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
            // A wall with things on it: up, planned, and a wall shelf holding wood.
            for x in cx + 6..=cx + 8 {
                let i = w.map.idx((x, cy + 8, z));
                w.map.feature[i] = my_world::map::Feature::built("wall");
            }
            let decor_wall = w.map.idx((cx + 7, cy + 8, z));
            let up = |id: &str, face, slot, built| my_world::decor::Decor {
                kind: my_world::defs::BuildId::named(id),
                face,
                slot,
                delivered: 0,
                built,
                remove: false,
            };
            let things = vec![up("painting", 2, 0, true), up("wall_lamp", 2, 2, true), up("wall_shelf", 2, 4, true), up("poster", 3, 1, false)];
            w.map.decor.insert(decor_wall as u32, things);
            w.map.set_stacks(decor_wall, vec![my_world::map::Stack { kind: my_world::defs::ItemId::named("wood"), count: 20 }]);
            // A window to be built into a standing wall.
            let old_wall = w.map.idx((cx + 3, cy + 6, z));
            w.map.feature[old_wall] = my_world::map::Feature::built("wall");
            assert!(w.apply_order((cx + 3, cy + 6, z), my_world::world::Order::Build(my_world::defs::BuildId::named("window"))));
            let shelf = w.map.idx((cx - 1, cy + 6, z));
            // Four different things on it: two on top, two on the lower board.
            let stacks = ["wood", "steel", "copper", "mechanical_components"]
                .map(|id| my_world::map::Stack { kind: my_world::defs::ItemId::named(id), count: 30 });
            w.map.set_stacks(shelf, stacks.to_vec());
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
            w.pawns[k].life = my_world::pawn::Life::Dead;
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
        // The Work window: priorities, then schedules.
        app.world_mut().resource_mut::<front::UiState>().show_work = true;
        app.update();
        app.world_mut().resource_mut::<front::UiState>().work_schedule = true;
        app.update();
        app.world_mut().resource_mut::<front::UiState>().show_work = false;
        // A quarry: painted, its shaft marked while the tool is out, and its inspector.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            let w = &mut sim.world;
            let (cx, cy) = (w.map.w / 2, w.map.h / 2);
            // Somewhere dry near the middle (the map is random: there may be a lake).
            let dry = |w: &my_world::world::World, x0: usize, y0: usize| {
                (y0..y0 + 3).all(|y| (x0..x0 + 4).all(|x| w.map.ground_level(x, y).is_some() && w.map.quarry_at(x, y).is_none()))
            };
            let (x0, y0) = (0..20)
                .flat_map(|k| [(cx - 12 + k, cy - 6), (cx - 12 + k, cy + 6)])
                .find(|&(x, y)| dry(w, x, y))
                .expect("dry ground for a quarry");
            for y in y0..y0 + 3 {
                for x in x0..x0 + 4 {
                    let z = w.map.ground_level(x, y).unwrap();
                    assert!(w.apply_order((x, y, z), my_world::world::Order::Quarry));
                }
            }
            assert!(!w.quarries.is_empty(), "a quarry");
        }
        {
            let quarry = {
                let w = &app.world().resource::<front::Sim>().world;
                let (x, y) = w.quarries[0].cols[0];
                let (x, y) = (x as usize, y as usize);
                (x, y, w.map.ground_level(x, y).unwrap_or(0))
            };
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.tool = front::Tool::Paint(my_world::world::Order::Quarry);
            ui.selected = Some(front::Selection::Tile(quarry));
        }
        app.update();
        app.update();
        // Placing: ghosts where the blueprints would go, under the cursor and over a dragged box.
        {
            let at = {
                let w = &app.world().resource::<front::Sim>().world;
                let (cx, cy) = (w.map.w / 2, w.map.h / 2);
                (cx, cy - 2, w.map.surface(cx, cy - 2).unwrap_or(1))
            };
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.tool = front::Tool::Paint(my_world::world::Order::Build(my_world::defs::BuildId::named("wall")));
            ui.hover = Some(front::Hover { solid: (at.0, at.1, at.2 - 1), stand: at });
            ui.drag_start = Some((at.0 - 3, at.1 - 2, at.2));
        }
        app.update();
        app.update();
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.drag_start = None;
            ui.tool = front::Tool::Select;
        }
        app.update();
        // Hanging a painting: the slot under the cursor is marked, and the wall's inspector lists what's on it.
        {
            let wall = {
                let w = &app.world().resource::<front::Sim>().world;
                let (cx, cy) = (w.map.w / 2, w.map.h / 2);
                (cx + 7, cy + 8, w.map.surface(cx, cy).unwrap())
            };
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.tool = front::Tool::Paint(my_world::world::Order::Build(my_world::defs::BuildId::named("painting")));
            ui.wall_slot = Some((wall, 2, 1));
            ui.selected = Some(front::Selection::Tile(wall));
        }
        app.update();
        app.world_mut().resource_mut::<front::UiState>().tool = front::Tool::Select;
        // The info window: a weapon's stats, with the lookup searching.
        {
            let mut ui = app.world_mut().resource_mut::<front::UiState>();
            ui.open_info(front::InfoTarget::Build(my_world::defs::BuildId::named("stove")));
            ui.open_info(front::InfoTarget::Item(my_world::defs::ItemId::named("rifle")));
            ui.info_filter = "ri".into();
            assert_eq!(ui.info_back.len(), 1, "the stove to go back to");
        }
        // The app runs on real time: give the rooms a moment to be recounted.
        let workshop = |app: &App| {
            let w = &app.world().resource::<front::Sim>().world;
            w.room_at((room.0, room.1, room_z)).and_then(|r| r.kind_name()) == Some("Workshop")
        };
        // Rooms are recounted once a game second: wait up to three of them, however fast the frames come.
        let start = app.world().resource::<front::Sim>().world.time;
        for _ in 0..20_000 {
            app.update();
            if workshop(&app) || app.world().resource::<front::Sim>().world.time - start > 3.0 {
                break;
            }
        }
        assert!(workshop(&app), "the room became a workshop");
        for _ in 0..40 {
            app.update();
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
        // Picked up turned as it stands, then turned a quarter while choosing where it goes.
        let turned = {
            let app_world = app.world_mut();
            let world = app_world.resource::<front::Sim>().world.clone();
            let mut ui = app_world.resource_mut::<front::UiState>();
            ui.start_move(bed, &world);
            assert_eq!(ui.place_dir, world.facing_of(bed));
            assert!(ui.turning(&world).is_some(), "a bed can be turned");
            ui.place_dir = (ui.place_dir + 2) % 4;
            ui.place_dir
        };
        app.update();
        app.world_mut().resource_mut::<front::UiState>().tool = front::Tool::Select;
        assert!(app.world_mut().resource_mut::<front::Sim>().world.order_move_facing(bed, spot, turned));
        {
            let w = &app.world().resource::<front::Sim>().world;
            assert!(matches!(w.map.feature[w.map.idx(spot)], my_world::map::Feature::Blueprint { dir, .. } if dir == turned));
        }
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
            let u = (0..b.units.len()).find(|&u| b.units[u].side == my_world::pawn::Faction::Colony && b.units[u].is_squad()).unwrap();
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
        let mut files = vec![
            "models/characters/base/Superhero_Male_FullBody.gltf",
            "models/characters/base/Superhero_Female_FullBody.gltf",
            "models/characters/outfits/Male_Ranger.gltf",
            "models/characters/hair/Hair_SimpleParted.gltf",
            "models/characters/animations/UAL1_Standard.glb",
        ];
        files.extend(my_world::defs::ItemId::all().filter_map(|kind| kind.def().model.as_ref().map(|model| model.path.as_str())));
        for kind in my_world::defs::ItemId::all() {
            if let Some(skin) = kind.def().apparel.as_ref().and_then(|a| a.skinned.as_ref()) {
                files.extend([skin.male.as_str(), skin.female.as_str()]);
            }
        }
        files.extend(my_world::defs::BuildId::all().filter_map(|kind| kind.def().model.as_ref().map(|model| model.path.as_str())));
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
        app.finish();
        app.cleanup();
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            for (i, pawn) in sim.world.pawns.iter_mut().enumerate() {
                let armor = if i % 2 == 0 { "steel_armor" } else { "powered_armor" };
                pawn.apparel.push(my_world::defs::ItemId::named(armor));
                pawn.apparel.push(my_world::defs::ItemId::named("steel_helmet"));
            }
        }
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
        // Both shape and model visuals now live beneath a stable hand attachment.
        let mut held = app.world_mut().query::<(Entity, &ChildOf, &Children)>();
        let held: Vec<Entity> = held.iter(app.world()).filter(|(_, parent, _)| hands.contains(&parent.parent())).map(|(e, _, _)| e).collect();
        assert!(!held.is_empty(), "weapons attach to the hands");
        let mut stack = held.clone();
        let mut descendants = Vec::new();
        while let Some(e) = stack.pop() {
            if let Some(children) = app.world().get::<Children>(e) {
                for child in children.iter() {
                    descendants.push(child);
                    stack.push(child);
                }
            }
        }
        let mut meshes = app.world_mut().query::<(&GlobalTransform, &Mesh3d)>();
        let mut checked = 0;
        for e in &descendants {
            if let Ok((tf, _)) = meshes.get(app.world(), *e) {
                let scale = tf.to_scale_rotation_translation().0;
                assert!(scale.max_element() < 1.0, "weapon scaled {scale:?}");
                checked += 1;
            }
        }
        assert!(checked >= held.len(), "each attachment contains weapon meshes");
        let mut names = app.world_mut().query::<&Name>();
        assert!(descendants.iter().any(|e| names.get(app.world(), *e).is_ok_and(|n| n.as_str() == "Rifle")), "the rifle model spawned in a hand");
        assert!(names.iter(app.world()).any(|n| n.as_str() == "Chest"), "armor model spawned on a chest bone");
        assert!(names.iter(app.world()).any(|n| n.as_str() == "Helmet"), "helmet model spawned on a head bone");
        // Switching from armor to fabric must use live skinned joints in gun poses.
        {
            let mut sim = app.world_mut().resource_mut::<front::Sim>();
            sim.world.pawns[0].apparel = vec![my_world::defs::ItemId::named("clothes"), my_world::defs::ItemId::named("cargo_trousers")];
            sim.world.pawns[1].apparel = vec![my_world::defs::ItemId::named("parka"), my_world::defs::ItemId::named("flak_leg_guards")];
        }
        let cloth_skins = |app: &mut App| {
            let mut query = app.world_mut().query::<(&Name, &bevy::mesh::skinning::SkinnedMesh)>();
            query.iter(app.world()).filter(|(name, _)| ["SimpleClothes", "Parka", "CargoTrousers", "FlakLegGuards"].iter().any(|prefix| name.as_str().starts_with(prefix)))
                .map(|(name, skin)| (name.to_string(), skin.joints.clone())).collect::<Vec<_>>()
        };
        // Material groups become separate mesh entities; check both parts of
        // each garment rather than assuming a fixed number of primitives.
        let parts = [("SimpleClothes", "Fabric"), ("SimpleClothes", "NativeArms"), ("Parka", "Fabric"), ("Parka", "NativeArms"),
                     ("CargoTrousers", "Legs"), ("CargoTrousers", "Pockets"), ("FlakLegGuards", "Legs"), ("FlakLegGuards", "Pads")];
        let complete = |skins: &[(String, Vec<Entity>)]| parts.iter().all(|(item, part)| skins.iter().any(|(name, _)| name.starts_with(*item) && name.contains(*part)));
        let mut skins = Vec::new();
        for _ in 0..2000 {
            app.update();
            skins = cloth_skins(&mut app);
            if complete(&skins) && skins.iter().flat_map(|(_, joints)| joints).all(|&joint| app.world().get::<bevy::animation::AnimatedBy>(joint).is_some()) { break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(complete(&skins), "upper clothing, leg apparel and boots on both clothed characters");
        assert!(skins.iter().flat_map(|(_, joints)| joints).all(|&joint| app.world().get::<bevy::animation::AnimatedBy>(joint).is_some()), "clothes follow the character animation, with no imported bind-pose joints left");
        app.world_mut().resource_mut::<front::Sim>().world.pawns[0].apparel = vec![my_world::defs::ItemId::named("powered_armor")];
        let mut suit = Vec::new();
        for _ in 0..2000 {
            app.update();
            let mut query = app.world_mut().query::<(&Name, &bevy::mesh::skinning::SkinnedMesh)>();
            suit = query.iter(app.world()).filter(|(name, _)| name.as_str().starts_with("PoweredArmor"))
                .map(|(name, skin)| (name.to_string(), skin.joints.clone())).collect::<Vec<_>>();
            if ["Details", "Shell"].iter().all(|part| suit.iter().any(|(name, _)| name.contains(*part)))
                && suit.iter().flat_map(|(_, joints)| joints).all(|&joint| app.world().get::<bevy::animation::AnimatedBy>(joint).is_some()) { break; }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(cloth_skins(&mut app).iter().all(|(name, _)| !name.starts_with("CargoTrousers")), "the powered suit replaces the previous leg outfit");
        assert!(["Details", "Shell"].iter().all(|part| suit.iter().any(|(name, _)| name.contains(*part))), "the powered suit includes complete body coverage");
        assert!(suit.iter().flat_map(|(_, joints)| joints).all(|&joint| app.world().get::<bevy::animation::AnimatedBy>(joint).is_some()), "the sealed suit follows the live outfit skeleton");
    }
}
