//! Battles on screen: the parley dialog, the battle panel, hit chances over
//! enemies, reachable cells and cover markers, clicks and keys in a battle,
//! and the camera following whoever acts.

use super::audio::SoundQueue;
use super::camera::{CameraRig, MainCamera};
use super::input::{cell_outline, pawn_at, Selection, UiState};
use super::sim::Sim;
use super::terrain::{FLOOR_T, LEVEL_H};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use std::collections::{HashMap, HashSet};
use super::{tile_center, tile_point};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::egui::{self, Align2, Color32, ProgressBar, RichText};
use bevy_egui::input::EguiWantsInput;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use my_world::battle::{Battle, BattleOrder, Cover, AP_ATTACK, AP_HUNKER, AP_MAX, AP_MOVE};
use my_world::map::{Pos, DIRS};
use my_world::pathing::bfs;
use my_world::pawn::Faction;
use my_world::world::World as SimWorld;
use std::f32::consts::FRAC_PI_2;

pub struct BattleUiPlugin;

impl Plugin for BattleUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BattleView>()
            .init_resource::<ReachArea>()
            .init_gizmo_group::<BattleGizmos>()
            .add_systems(Startup, setup)
            .add_systems(Update, (battle_input, follow_action, update_reach, draw_battle).chain())
            .add_systems(EguiPrimaryContextPass, battle_panels);
    }
}

#[derive(Resource, Default)]
struct BattleView {
    /// Why the last order was refused, and seconds to keep showing it.
    message: Option<(String, f32)>,
    /// The unit the camera last went to, and seconds left to keep easing there.
    followed: Option<usize>,
    follow: f32,
}

/// Bold lines for what matters in a battle: the edges of where the acting
/// unit can go, and the chosen destination.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct BattleGizmos;

/// Where the acting unit can move this activation: cell -> AP it costs.
/// Drawn as a see-through area (blue: can still attack after; yellow: a
/// full run), outlined in bold.
#[derive(Resource, Default)]
struct ReachArea {
    /// What it was worked out for: unit, leader's cell, AP, view (level, roofs).
    key: Option<(usize, Pos, i32, usize, u64)>,
    cost: HashMap<Pos, i32>,
    /// Columns (x, y) in each zone, for outlines.
    attack_cols: HashSet<(usize, usize)>,
    all_cols: HashSet<(usize, usize)>,
    entity: Option<Entity>,
    material: Handle<StandardMaterial>,
}

const ATTACK_ZONE: [f32; 4] = [0.25, 0.6, 1.0, 0.32];
const RUN_ZONE: [f32; 4] = [1.0, 0.72, 0.15, 0.28];

fn setup(mut config: ResMut<GizmoConfigStore>, mut area: ResMut<ReachArea>, mut materials: ResMut<Assets<StandardMaterial>>) {
    config.config_mut::<BattleGizmos>().0.line.width = 4.0;
    area.material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
}

/// Height of the floor a pawn stands on in cell `c`.
fn floor_y(m: &my_world::map::Map, c: Pos) -> f32 {
    c.2 as f32 * LEVEL_H + if m.floor[m.idx(c)] { FLOOR_T } else { 0.0 }
}

/// Work out (when it changes) where the acting unit can go, and rebuild the area's mesh.
fn update_reach(mut commands: Commands, sim: Res<Sim>, ui: Res<UiState>, mut area: ResMut<ReachArea>, mut meshes: ResMut<Assets<Mesh>>) {
    let w = &sim.world;
    let m = &w.map;
    let key = w.battle.as_ref().and_then(|b| {
        let u = my_unit(b).filter(|_| !b.busy())?;
        let lead = w.pawn(*b.units[u].members.first()?)?;
        Some((u, lead.pos, b.units[u].ap, ui.top_level(m), ui.roof_version))
    });
    if key == area.key {
        return;
    }
    area.key = key;
    area.cost.clear();
    area.attack_cols.clear();
    area.all_cols.clear();
    let Some((_, from, ap, _, _)) = key else {
        if let Some(e) = area.entity {
            commands.entity(e).insert(Visibility::Hidden);
        }
        return;
    };
    let df = bfs(m, from);
    let (mut pos, mut col, mut idx) = (Vec::new(), Vec::new(), Vec::new());
    for &i in &df.order {
        let Some(d) = df.dist_i(i) else { continue };
        let cost = d * AP_MOVE;
        if cost > ap {
            break;
        }
        let c = m.pos(i);
        if d == 0 || !ui.shows_at(m, c.0 as f32, c.1 as f32, c.2 as f32) {
            continue;
        }
        let attack = ap - cost >= AP_ATTACK;
        area.cost.insert(c, cost);
        area.all_cols.insert((c.0, c.1));
        if attack {
            area.attack_cols.insert((c.0, c.1));
        }
        let (x, z, y) = (c.0 as f32, c.1 as f32, floor_y(m, c) + 0.03);
        let n = pos.len() as u32;
        let (a, b) = (0.04, 0.96);
        pos.extend([[x + a, y, z + a], [x + a, y, z + b], [x + b, y, z + b], [x + b, y, z + a]]);
        col.extend([if attack { ATTACK_ZONE } else { RUN_ZONE }; 4]);
        idx.extend([n, n + 1, n + 2, n, n + 2, n + 3]);
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    let mesh = meshes.add(mesh);
    match area.entity {
        Some(e) => {
            commands.entity(e).insert((Mesh3d(mesh), Visibility::Inherited));
        }
        None => {
            let material = area.material.clone();
            area.entity = Some(commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default())).id());
        }
    }
}

/// The colony's unit taking its turn, when the player can give it orders.
fn my_unit(b: &Battle) -> Option<usize> {
    (b.side_to_act == Faction::Colony && !b.auto_colony).then_some(b.active).flatten()
}

fn names(w: &SimWorld, members: &[u32]) -> String {
    let names: Vec<&str> = members.iter().filter_map(|&id| w.pawn(id)).map(|p| p.name.as_str()).collect();
    match names.len() {
        0 => "nobody".into(),
        1 => names[0].into(),
        _ => format!("{}'s squad ({})", names[0], names[1..].join(", ")),
    }
}

/// The colony's units that can still act this round, in order.
fn ready_units(b: &Battle) -> Vec<usize> {
    (0..b.units.len()).filter(|&u| b.units[u].side == Faction::Colony && b.units[u].can_act() && !b.units[u].acted).collect()
}

#[allow(clippy::too_many_arguments)]
fn battle_input(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    egui_input: Res<EguiWantsInput>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut sim: ResMut<Sim>,
    mut ui: ResMut<UiState>,
    mut view: ResMut<BattleView>,
    mut sounds: ResMut<SoundQueue>,
    time: Res<Time>,
) {
    if let Some((_, left)) = &mut view.message {
        *left -= time.delta_secs();
        if *left <= 0.0 {
            view.message = None;
        }
    }
    let Some(b) = &sim.world.battle else { return };
    let over_ui = egui_input.wants_any_pointer_input() || ui.over_dock(&window);
    let typing = egui_input.wants_any_keyboard_input();
    let mut orders = Vec::new();
    if !typing && keys.just_pressed(KeyCode::Space) && my_unit(b).is_some() {
        orders.push(BattleOrder::End);
    }
    if !typing && keys.just_pressed(KeyCode::Tab) {
        let ready = ready_units(b);
        let next = match b.active {
            Some(a) => ready.iter().copied().find(|&u| u > a).or(ready.first().copied()),
            None => ready.first().copied(),
        };
        orders.extend(next.map(BattleOrder::Activate));
    }
    if buttons.just_pressed(MouseButton::Left) && !over_ui {
        if let Some(h) = ui.hover {
            let w = &sim.world;
            match pawn_at(w, h.stand).and_then(|id| Some((id, b.unit_of(id)?))) {
                Some((id, u)) => {
                    ui.select(Some(Selection::Pawn(id)));
                    let unit = &b.units[u];
                    if unit.side == Faction::Colony && unit.can_act() {
                        orders.push(BattleOrder::Activate(u));
                    } else if unit.side != Faction::Colony && my_unit(b).is_some() {
                        orders.push(BattleOrder::Attack(u));
                    }
                }
                None if my_unit(b).is_some() => orders.push(BattleOrder::Move(h.stand)),
                None => {}
            }
        }
    }
    for order in orders {
        match sim.world.battle_order(order) {
            Ok(()) => {
                view.message = None;
                if !matches!(order, BattleOrder::Activate(_)) {
                    sounds.play("ui_place", None);
                }
            }
            Err(e) => {
                view.message = Some((e, 3.0));
                sounds.play("ui_click", None);
            }
        }
    }
}

/// Ease the camera to the unit whose turn it is, and keep up with it while it walks.
fn follow_action(sim: Res<Sim>, mut view: ResMut<BattleView>, mut rig: ResMut<CameraRig>, time: Res<Time>) {
    let Some(b) = &sim.world.battle else {
        view.followed = None;
        return;
    };
    if b.active != view.followed {
        view.followed = b.active;
        view.follow = 1.0;
    }
    if !b.moving.is_empty() {
        view.follow = view.follow.max(0.3);
    }
    if view.follow <= 0.0 {
        return;
    }
    view.follow -= time.delta_secs();
    let Some(lead) = b.active.and_then(|u| b.units[u].members.first()).and_then(|&id| sim.world.pawn(id)) else {
        return;
    };
    let target = tile_point(lead.x, lead.y, lead.z);
    rig.focus = rig.focus.lerp(target, 1.0 - (-time.delta_secs() * 5.0).exp());
}

fn draw_battle(sim: Res<Sim>, ui: Res<UiState>, area: Res<ReachArea>, mut gizmos: Gizmos, mut bold: Gizmos<BattleGizmos>) {
    let w = &sim.world;
    let Some(b) = &w.battle else { return };
    let m = &w.map;
    // Rings: the acting unit gold, the colony's others blue (grey once done),
    // civilians green, raiders red; lines from squad leaders to their members.
    for (u, unit) in b.units.iter().enumerate() {
        let color = match unit.side {
            Faction::Raider => Color::srgb(0.95, 0.2, 0.15),
            _ if b.active == Some(u) => Color::srgb(1.0, 0.82, 0.1),
            _ if unit.passive => Color::srgb(0.4, 0.85, 0.5),
            _ if unit.acted => Color::srgb(0.55, 0.55, 0.6),
            _ => Color::srgb(0.35, 0.65, 1.0),
        };
        let lead = unit.members.first().and_then(|&id| w.pawn(id));
        for &id in &unit.members {
            let Some(p) = w.pawn(id).filter(|p| ui.shows_at(m, p.x, p.y, p.z)) else { continue };
            let feet = tile_point(p.x, p.y, p.z) + Vec3::Y * 0.04;
            gizmos.circle(Isometry3d::new(feet, Quat::from_rotation_x(FRAC_PI_2)), 0.42, color);
            if let Some(l) = lead.filter(|l| unit.led && l.id != id) {
                gizmos.line(tile_point(l.x, l.y, l.z) + Vec3::Y * 0.04, feet, color.with_alpha(0.5));
            }
        }
    }

    let Some(u) = my_unit(b).filter(|_| !b.busy()) else { return };
    let unit = &b.units[u];
    let Some(lead) = unit.members.first().and_then(|&id| w.pawn(id)) else { return };
    // Bold edges round the area it can reach: blue round where it can still
    // attack afterwards, yellow round where a full run takes it.
    for &c in area.cost.keys() {
        let y = floor_y(m, c) + 0.05;
        for (dx, dy) in DIRS {
            let (nx, ny) = (c.0 as i32 + dx, c.1 as i32 + dy);
            let outside = |zone: &HashSet<(usize, usize)>| nx < 0 || ny < 0 || !zone.contains(&(nx as usize, ny as usize));
            let mid = Vec3::new(c.0 as f32 + 0.5 + dx as f32 * 0.5, y, c.1 as f32 + 0.5 + dy as f32 * 0.5);
            let along = Vec3::new(dy as f32, 0.0, dx as f32) * 0.5;
            if outside(&area.all_cols) {
                bold.line(mid - along, mid + along, Color::srgb(1.0, 0.75, 0.15));
            }
            if area.attack_cols.contains(&(c.0, c.1)) && outside(&area.attack_cols) {
                let inset = Vec3::new(dx as f32, 0.0, dy as f32) * -0.06;
                bold.line(mid - along + inset, mid + along + inset, Color::srgb(0.3, 0.65, 1.0));
            }
        }
    }
    let Some(h) = ui.hover else { return };
    // The chosen destination: a bold ring and the way there.
    if let Some(&cost) = area.cost.get(&h.stand) {
        let attack = unit.ap - cost >= AP_ATTACK;
        let color = if attack { Color::srgb(0.45, 0.8, 1.0) } else { Color::srgb(1.0, 0.8, 0.25) };
        let at = Vec3::new(h.stand.0 as f32 + 0.5, floor_y(m, h.stand) + 0.06, h.stand.1 as f32 + 0.5);
        let flat = Quat::from_rotation_x(FRAC_PI_2);
        bold.circle(Isometry3d::new(at, flat), 0.36, color);
        gizmos.circle(Isometry3d::new(at, flat), 0.2, color);
        let df = bfs(m, lead.pos);
        let mut points = vec![Vec3::new(lead.x + 0.5, floor_y(m, lead.pos) + 0.08, lead.y + 0.5)];
        points.extend(df.path_to(h.stand).iter().rev().map(|&c| Vec3::new(c.0 as f32 + 0.5, floor_y(m, c) + 0.08, c.1 as f32 + 0.5)));
        bold.linestrip(points, Color::WHITE.with_alpha(0.9));
    }
    if pawn_at(w, h.stand).is_none() && m.standable(h.stand) {
        for (dx, dy) in DIRS {
            let Some(c) = m.at(h.stand.0 as i32 + dx, h.stand.1 as i32 + dy, h.stand.2 as i32) else { continue };
            let height = match w.cover_at(c) {
                Cover::None => continue,
                Cover::Half => 0.35,
                Cover::Full => 0.75,
            };
            let edge = tile_center(h.stand) + Vec3::new(dx as f32 * 0.42, 0.05, dy as f32 * 0.42);
            let side = Vec3::new(dy as f32, 0.0, dx as f32) * 0.25;
            let color = Color::srgb(0.4, 0.75, 1.0);
            let up = Vec3::Y * height * LEVEL_H;
            gizmos.linestrip([edge - side, edge - side + up, edge + side + up, edge + side], color);
        }
    }
    if let Some(t) = pawn_at(w, h.stand).and_then(|id| b.unit_of(id)).filter(|&t| b.units[t].side != Faction::Colony) {
        for &id in &b.units[t].members {
            if let Some(p) = w.pawn(id) {
                cell_outline(&mut gizmos, p.pos, Color::srgb(1.0, 0.3, 0.2));
            }
        }
    }
}

/// Best chance anyone in unit `u` has against pawn `target`, if anyone can reach it.
fn best_chance(w: &SimWorld, u: usize, target: u32) -> Option<f32> {
    let b = w.battle.as_ref()?;
    let t = b.unit_of(target)?;
    let target_pawn = w.pawn(target)?;
    w.attack_options(u, t)
        .iter()
        .filter(|o| o.1 == target)
        .filter_map(|o| Some(w.hit_chance(w.pawn(o.0)?, target_pawn)))
        .reduce(f32::max)
}

#[allow(clippy::too_many_arguments)]
fn battle_panels(
    mut contexts: EguiContexts,
    mut sim: ResMut<Sim>,
    mut ui: ResMut<UiState>,
    view: Res<BattleView>,
    area: Res<ReachArea>,
    mut sounds: ResMut<SoundQueue>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    parley_dialog(&ctx, &mut sim, &mut sounds);
    let w = &sim.world;
    let Some(b) = &w.battle else { return Ok(()) };
    let mut orders = Vec::new();

    // Hit chances over the enemies the acting unit can reach.
    if let Some(u) = my_unit(b) {
        let (cam, gt) = *camera;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("battle_odds")));
        // What moving to the hovered cell costs, by the destination marker.
        if let Some((h, &cost)) = ui.hover.and_then(|h| Some((h, area.cost.get(&h.stand)?))) {
            let at = tile_center(h.stand) + Vec3::Y * 0.3;
            if let Ok(p) = cam.world_to_viewport(gt, at) {
                let left = b.units[u].ap - cost;
                let (text, color) = if left >= AP_ATTACK {
                    (format!("{} AP - can still attack", cost), Color32::from_rgb(150, 205, 255))
                } else {
                    (format!("{} AP - no attack after", cost), Color32::from_rgb(255, 210, 110))
                };
                let pos = egui::pos2(p.x, p.y - 18.0);
                let font = egui::FontId::proportional(15.0);
                painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::from_black_alpha(220));
                painter.text(pos, Align2::CENTER_BOTTOM, text, font, color);
            }
        }
        for unit in b.units.iter().filter(|t| t.side != Faction::Colony) {
            for &id in &unit.members {
                let Some(p) = w.pawn(id).filter(|p| ui.shows_at(&w.map, p.x, p.y, p.z)) else { continue };
                let Ok(at) = cam.world_to_viewport(gt, tile_point(p.x, p.y, p.z) + Vec3::Y * 1.3) else { continue };
                let (text, color) = match best_chance(w, u, id) {
                    Some(c) => (format!("{:.0}%", c * 100.0), Color32::from_rgb(255, 225, 120)),
                    None => ("out of reach".to_string(), Color32::from_gray(170)),
                };
                let pos = egui::pos2(at.x, at.y);
                let font = egui::FontId::proportional(14.0);
                painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::from_black_alpha(200));
                painter.text(pos, Align2::CENTER_BOTTOM, text, font, color);
            }
        }
    }

    egui::Window::new(RichText::new(format!("Battle - round {}", b.round)).strong())
        .id(egui::Id::new("battle_panel"))
        .anchor(Align2::RIGHT_TOP, [-8.0, 92.0])
        .collapsible(false)
        .resizable(false)
        .default_width(300.0)
        .show(&ctx, |panel| {
            let turn = if b.side_to_act == Faction::Raider {
                RichText::new("The raiders are moving...").color(Color32::from_rgb(255, 140, 120))
            } else if b.active.is_none() {
                RichText::new("Your turn: pick a unit (click it, or Tab)").color(Color32::from_rgb(140, 200, 255))
            } else {
                RichText::new("Your turn").color(Color32::from_rgb(140, 200, 255))
            };
            panel.label(turn);
            if let Some(u) = my_unit(b) {
                let unit = &b.units[u];
                panel.separator();
                panel.label(RichText::new(names(w, &unit.members)).strong());
                panel.add(
                    ProgressBar::new(unit.ap as f32 / AP_MAX as f32)
                        .text(format!("{} action points", unit.ap))
                        .fill(Color32::from_rgb(90, 160, 230)),
                );
                let mut tags = Vec::new();
                if unit.pinned() {
                    tags.push("pinned down");
                }
                if unit.broken() {
                    tags.push("shaken");
                }
                if !tags.is_empty() {
                    panel.label(RichText::new(tags.join(", ")).color(Color32::from_rgb(255, 160, 100)));
                }
                panel.label(
                    RichText::new(format!(
                        "Click a cell to move ({} AP a step), an enemy to attack ({} AP).",
                        AP_MOVE, AP_ATTACK
                    ))
                    .small()
                    .weak(),
                );
                if let Some(h) = ui.hover.filter(|_| !b.busy()) {
                    if let Ok(cost) = w.move_cost(u, h.stand) {
                        let cover = DIRS
                            .iter()
                            .filter_map(|&(dx, dy)| w.map.at(h.stand.0 as i32 + dx, h.stand.1 as i32 + dy, h.stand.2 as i32))
                            .map(|c| w.cover_at(c))
                            .max()
                            .unwrap_or(Cover::None);
                        panel.label(format!("Move there: {} AP, {} nearby", cost, cover.name()));
                    }
                }
                panel.horizontal(|row| {
                    if row.add_enabled(unit.ap >= AP_HUNKER, egui::Button::new("Hunker down")).on_hover_text("Better cover until the next round; ends the turn").clicked() {
                        orders.push(BattleOrder::Hunker);
                    }
                    if row.button("End turn (Space)").clicked() {
                        orders.push(BattleOrder::End);
                    }
                });
            }
            if let Some((msg, _)) = &view.message {
                panel.label(RichText::new(msg).color(Color32::from_rgb(255, 170, 90)));
            }

            panel.separator();
            panel.label(RichText::new("Your units").strong());
            for (u, unit) in b.units.iter().enumerate().filter(|(_, u)| u.side == Faction::Colony && u.can_act()) {
                let status = if b.active == Some(u) {
                    "acting".to_string()
                } else if unit.acted {
                    "done".to_string()
                } else {
                    format!("{} AP", unit.ap)
                };
                let text = format!("{} - {}", names(w, &unit.members), status);
                let ready = !unit.acted && b.side_to_act == Faction::Colony;
                if panel.add_enabled(ready, egui::Button::new(text).small()).clicked() {
                    orders.push(BattleOrder::Activate(u));
                }
            }
            for a in &b.arrivals {
                if let (Some(p), Some(l)) = (w.pawn(a.pawn), w.pawn(a.leader)) {
                    let rounds = if a.rounds == 1 { "next round".to_string() } else { format!("in {} rounds", a.rounds) };
                    panel.label(RichText::new(format!("{} joins {} {}", p.name, l.name, rounds)).small().weak());
                }
            }
            let civilians = b.units.iter().filter(|u| u.passive && !u.out()).count();
            if civilians > 0 {
                panel.label(RichText::new(format!("{} civilian(s) sheltering", civilians)).small().weak());
            }
            let raiders: usize = b.units.iter().filter(|u| u.side == Faction::Raider).map(|u| u.members.len()).sum();
            panel.label(RichText::new(format!("{} raider(s) still fighting", raiders)).color(Color32::from_rgb(255, 140, 120)));

            panel.separator();
            for e in b.events.iter().rev().take(6).rev() {
                panel.label(RichText::new(e).small());
            }
        });

    for order in orders {
        match sim.world.battle_order(order) {
            Ok(()) => sounds.play("ui_click", None),
            Err(_) => sounds.play("ui_click", None),
        }
    }
    // Keep the inspector on whoever is acting.
    if let Some(b) = &sim.world.battle {
        if let Some(lead) = my_unit(b).and_then(|u| b.units[u].members.first().copied()) {
            if !matches!(ui.selected, Some(Selection::Pawn(id)) if b.unit_of(id) == b.unit_of(lead)) {
                ui.select(Some(Selection::Pawn(lead)));
            }
        }
    }
    Ok(())
}

/// Raiders at the gate: pay them off, or fight.
fn parley_dialog(ctx: &egui::Context, sim: &mut Sim, sounds: &mut SoundQueue) {
    let Some(p) = &sim.world.parley else { return };
    let mut choice = None;
    egui::Window::new(RichText::new("Raiders!").strong().color(Color32::from_rgb(255, 120, 100)))
        .id(egui::Id::new("parley"))
        .anchor(Align2::CENTER_CENTER, [0.0, -40.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let groups = p.squads.len();
            ui.label(format!(
                "{} raider{} in {} group{} stand at the edge of the colony.",
                p.raiders(),
                if p.raiders() == 1 { "" } else { "s" },
                groups,
                if groups == 1 { "" } else { "s" }
            ));
            if p.demand.is_empty() {
                ui.label("You have nothing they want badly enough. They want a fight.");
            } else {
                ui.label("They'll leave if you hand over:");
                for s in &p.demand {
                    ui.label(RichText::new(format!("  {} x{}", s.kind.name(), s.count)).strong());
                }
            }
            ui.label(
                RichText::new("If you fight and lose, they take half as much again. The colony stops until you decide.")
                    .small()
                    .weak(),
            );
            ui.horizontal(|ui| {
                if !p.demand.is_empty() && ui.button("Pay them").clicked() {
                    choice = Some(true);
                }
                if ui.button("Fight").clicked() {
                    choice = Some(false);
                }
            });
        });
    match choice {
        Some(true) => {
            sim.world.pay_tribute();
            sounds.play("ui_place", None);
        }
        Some(false) => {
            sim.world.refuse_tribute();
            sounds.play("ui_place", None);
        }
        None => {}
    }
}
