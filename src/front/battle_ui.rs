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
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};
use bevy_egui::egui::{self, Align2, Color32, ProgressBar, RichText};
use bevy_egui::input::EguiWantsInput;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};
use my_world::battle::{Battle, BattleOrder, Cover, AP_ATTACK, AP_DEPLOY, AP_HUNKER, AP_MAX, AP_MOVE};
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
            .add_systems(Update, (battle_input, follow_action, update_reach, draw_battle, cursor).chain())
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
    /// The acting unit last put in the inspector.
    inspected: Option<usize>,
    /// The round last announced, and seconds the "Round N" banner still shows.
    announced: Option<u32>,
    banner: f32,
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
        // A deployed squad can't move: no area.
        let u = my_unit(b).filter(|&u| !b.busy() && !b.units[u].deployed)?;
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

/// Seconds the "Round N" banner shows.
const BANNER_SECS: f32 = 1.8;

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
    drag: Res<super::camera::DragTracker>,
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
    // A right click (not a camera drag) lets go of the selection.
    if buttons.just_released(MouseButton::Right) && drag.right_moved <= 4.0 && !over_ui {
        ui.select(None);
    }
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
            match fighter_at(w, b, h.stand).or_else(|| pawn_at(w, h.stand)).and_then(|id| Some((id, b.unit_of(id)?))) {
                Some((id, u)) => {
                    // Clicking a unit again goes through its members (the fallen too).
                    let roster = &b.units[u].roster;
                    let next = match ui.selected {
                        Some(Selection::Pawn(cur)) if roster.contains(&cur) && roster.len() > 1 => {
                            let k = roster.iter().position(|&r| r == cur).unwrap();
                            roster[(k + 1) % roster.len()]
                        }
                        _ => id,
                    };
                    ui.select(Some(Selection::Pawn(next)));
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
/// Ease the camera once to a unit as its turn starts. Moving the camera
/// yourself (keys, dragging, the wheel) takes over straight away.
#[allow(clippy::too_many_arguments)]
fn follow_action(
    sim: Res<Sim>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    scroll: Res<bevy::input::mouse::AccumulatedMouseScroll>,
    mut view: ResMut<BattleView>,
    mut rig: ResMut<CameraRig>,
    time: Res<Time>,
) {
    let Some(b) = &sim.world.battle else {
        view.followed = None;
        return;
    };
    if b.active != view.followed {
        view.followed = b.active;
        view.follow = 0.8;
    }
    let camera_keys = [
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::KeyQ,
        KeyCode::KeyE,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
    ];
    if keys.any_pressed(camera_keys) || buttons.any_pressed([MouseButton::Right, MouseButton::Middle]) || scroll.delta != Vec2::ZERO {
        view.follow = 0.0;
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
    // One ring per unit on the cell it holds (following its leader while it
    // moves): the acting unit gold, the colony's others blue (grey once
    // done), civilians green, raiders red.
    for (u, unit) in b.units.iter().enumerate() {
        let color = match unit.side {
            Faction::Raider => Color::srgb(0.95, 0.2, 0.15),
            _ if b.active == Some(u) => Color::srgb(1.0, 0.82, 0.1),
            _ if unit.passive => Color::srgb(0.4, 0.85, 0.5),
            _ if unit.acted => Color::srgb(0.55, 0.55, 0.6),
            _ => Color::srgb(0.35, 0.65, 1.0),
        };
        let Some(lead) = unit.members.first().and_then(|&id| w.pawn(id)) else { continue };
        if !ui.shows_at(m, lead.x, lead.y, lead.z) {
            continue;
        }
        let feet = tile_point(lead.x, lead.y, lead.z) + Vec3::Y * 0.04;
        gizmos.circle(Isometry3d::new(feet, Quat::from_rotation_x(FRAC_PI_2)), 0.5, color);
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
    // Over an enemy, clicking attacks: an aiming line from our square to
    // theirs (red if anyone can reach, grey if not) instead of where the unit
    // could walk.
    if let Some(t) = hovered_enemy(w, b, h.stand) {
        let reach = !w.attack_options(u, t).is_empty();
        let color = if reach { Color::srgb(1.0, 0.25, 0.15) } else { Color::srgb(0.6, 0.6, 0.6) };
        let (Some(from), Some(to)) = (w.unit_cell(u), w.unit_cell(t)) else { return };
        let chest = |c: Pos| tile_center(c) + Vec3::Y * 0.55;
        if reach {
            bold.line(chest(from), chest(to), color.with_alpha(0.85));
        }
        let feet = tile_center(to) + Vec3::Y * 0.05;
        bold.circle(Isometry3d::new(feet, Quat::from_rotation_x(FRAC_PI_2)), 0.5, color);
        cell_outline(&mut gizmos, to, color);
        return;
    }
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
    if fighter_at(w, b, h.stand).is_none() && m.standable(h.stand) {
        for (dx, dy) in DIRS {
            let Some(c) = m.at(h.stand.0 as i32 + dx, h.stand.1 as i32 + dy, h.stand.2 as i32) else { continue };
            let height = match w.cover_at(c) {
                Cover::None => continue,
                Cover::Light => 0.25,
                Cover::Medium => 0.5,
                Cover::Heavy => 0.8,
            };
            let edge = tile_center(h.stand) + Vec3::new(dx as f32 * 0.42, 0.05, dy as f32 * 0.42);
            let side = Vec3::new(dy as f32, 0.0, dx as f32) * 0.25;
            let color = Color::srgb(0.4, 0.75, 1.0);
            let up = Vec3::Y * height * LEVEL_H;
            gizmos.linestrip([edge - side, edge - side + up, edge + side + up, edge + side], color);
        }
    }
}

/// The enemy unit under the cursor, if any.
fn hovered_enemy(w: &SimWorld, b: &Battle, at: Pos) -> Option<usize> {
    fighter_at(w, b, at).and_then(|id| b.unit_of(id)).filter(|&t| b.units[t].side != Faction::Colony)
}

/// Someone still fighting (in a unit) at or right next to cell `at`, nearest
/// first: the downed lying there don't get in the way of clicking the living.
fn fighter_at(w: &SimWorld, b: &Battle, at: Pos) -> Option<u32> {
    w.pawns
        .iter()
        .filter(|p| b.unit_of(p.id).is_some())
        .map(|p| (((p.x - at.0 as f32).powi(2) + (p.y - at.1 as f32).powi(2) + (p.z - at.2 as f32).powi(2)).sqrt(), p.id))
        .filter(|(d, _)| *d < 0.9)
        .min_by(|a, c| a.0.total_cmp(&c.0))
        .map(|(_, id)| id)
}

/// A crosshair over enemies when clicking would attack.
fn cursor(
    mut commands: Commands,
    sim: Res<Sim>,
    ui: Res<UiState>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut aiming: Local<bool>,
) {
    let w = &sim.world;
    let now = w.battle.as_ref().is_some_and(|b| {
        my_unit(b).is_some() && ui.hover.is_some_and(|h| hovered_enemy(w, b, h.stand).is_some())
    });
    if now != *aiming {
        *aiming = now;
        let icon = if now { SystemCursorIcon::Crosshair } else { SystemCursorIcon::Default };
        commands.entity(*window).insert(CursorIcon::System(icon));
    }
}

/// Best chance anyone in unit `u` has against pawn `target`, if anyone can reach it.
/// Best chance anyone in unit `u` would have against pawn `target` from cell `from`.
fn best_chance_from(w: &SimWorld, u: usize, from: Pos, target: u32) -> Option<f32> {
    let b = w.battle.as_ref()?;
    let target_pawn = w.pawn(target)?;
    b.units[u]
        .members
        .iter()
        .filter_map(|&a| w.pawn(a))
        .filter(|a| w.can_attack_from(a, from, target_pawn))
        .map(|a| w.hit_chance_from(a, from, target_pawn))
        .reduce(f32::max)
}

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
    mut view: ResMut<BattleView>,
    area: Res<ReachArea>,
    time: Res<Time>,
    mut sounds: ResMut<SoundQueue>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    parley_dialog(&ctx, &mut sim, &mut sounds);
    let w = &sim.world;
    let Some(b) = &w.battle else {
        view.announced = None;
        return Ok(());
    };
    let mut orders = Vec::new();
    let mut end_all = false;

    // "Round N" in the middle of the screen when a round starts, fading away.
    if view.announced != Some(b.round) {
        view.announced = Some(b.round);
        view.banner = BANNER_SECS;
    }
    if view.banner > 0.0 {
        view.banner -= time.delta_secs();
        let shown = BANNER_SECS - view.banner;
        let alpha = (shown / 0.25).min(view.banner / 0.6).clamp(0.0, 1.0);
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("round_banner")));
        let at = ctx.content_rect().center() - egui::vec2(0.0, ctx.content_rect().height() * 0.22);
        let fade = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (c.a() as f32 * alpha) as u8);
        let big = egui::FontId::proportional(44.0);
        let text = format!("Round {}", b.round);
        painter.text(at + egui::vec2(2.0, 2.0), Align2::CENTER_CENTER, &text, big.clone(), fade(Color32::from_black_alpha(200)));
        painter.text(at, Align2::CENTER_CENTER, text, big, fade(Color32::from_rgb(255, 235, 190)));
    }

    // Each squad's state over its square: a bar per member (health while
    // standing, dark red when downed, grey and crossed out when dead), the
    // leader's outlined, and the squad's name.
    {
        let (cam, gt) = *camera;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("squad_status")));
        for unit in b.units.iter().filter(|u| u.roster.len() > 1 && !u.out()) {
            let Some(lead) = unit.members.first().and_then(|&id| w.pawn(id)) else { continue };
            if !ui.shows_at(&w.map, lead.x, lead.y, lead.z) {
                continue;
            }
            let Ok(p) = cam.world_to_viewport(gt, tile_point(lead.x, lead.y, lead.z) + Vec3::Y * 1.25) else { continue };
            let (bw, gap, bh) = (12.0, 3.0, 5.0);
            let total = unit.roster.len() as f32 * (bw + gap) - gap;
            let left = p.x - total / 2.0;
            for (k, &id) in unit.roster.iter().enumerate() {
                let Some(m) = w.pawn(id) else { continue };
                let x = left + k as f32 * (bw + gap);
                let rect = egui::Rect::from_min_size(egui::pos2(x, p.y), egui::vec2(bw, bh));
                painter.rect_filled(rect, 1.0, Color32::from_black_alpha(190));
                if m.dead {
                    painter.line_segment([rect.left_top(), rect.right_bottom()], egui::Stroke::new(1.5, Color32::from_gray(150)));
                    painter.line_segment([rect.left_bottom(), rect.right_top()], egui::Stroke::new(1.5, Color32::from_gray(150)));
                } else if !m.active() {
                    painter.rect_filled(rect, 1.0, Color32::from_rgb(110, 25, 25));
                } else {
                    let h = m.health.summary().clamp(0.0, 1.0);
                    let fill = if h > 0.7 {
                        Color32::from_rgb(90, 200, 90)
                    } else if h > 0.4 {
                        Color32::from_rgb(230, 190, 60)
                    } else {
                        Color32::from_rgb(230, 70, 50)
                    };
                    let mut inner = rect;
                    inner.set_width(bw * h.max(0.1));
                    painter.rect_filled(inner, 1.0, fill);
                }
                if unit.members.first() == Some(&id) {
                    painter.rect_stroke(rect.expand(1.0), 1.0, egui::Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
                }
            }
            let color = if unit.side == Faction::Raider { Color32::from_rgb(255, 150, 130) } else { Color32::from_rgb(170, 210, 255) };
            let text = format!("{} ({}/{})", lead.name, unit.members.len(), unit.roster.len());
            let pos = egui::pos2(p.x, p.y - 2.0);
            let font = egui::FontId::proportional(12.0);
            painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::from_black_alpha(200));
            painter.text(pos, Align2::CENTER_BOTTOM, text, font, color);
        }
    }

    // Hit chances over the enemies the acting unit can reach.
    if let Some(u) = my_unit(b) {
        let (cam, gt) = *camera;
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("battle_odds")));
        // Over an enemy: what attacking means, by the target.
        let target = ui.hover.and_then(|h| Some((h, hovered_enemy(w, b, h.stand)?)));
        if let Some((h, t)) = target {
            let options = w.attack_options(u, t);
            let shooters = {
                let mut a: Vec<u32> = options.iter().map(|o| o.0).collect();
                a.dedup();
                a.len()
            };
            let name = w.pawn(*b.units[t].members.first().unwrap_or(&0)).map_or("them".to_string(), |p| p.name.clone());
            let best = options.iter().filter_map(|o| Some(w.hit_chance(w.pawn(o.0)?, w.pawn(o.1)?))).reduce(f32::max);
            let ap = b.units[u].ap;
            let (text, color) = match best {
                _ if ap < AP_ATTACK => (format!("Attack {}: not enough AP ({} needed)", name, AP_ATTACK), Color32::from_gray(180)),
                Some(c) => (
                    format!(
                        "Click to attack {}: {:.0}%, {} of {} can reach - {} AP",
                        name,
                        c * 100.0,
                        shooters,
                        b.units[u].members.len(),
                        AP_ATTACK
                    ),
                    Color32::from_rgb(255, 120, 100),
                ),
                None => (format!("{} is out of reach: move closer or find a line of sight", name), Color32::from_gray(180)),
            };
            if let Ok(p) = cam.world_to_viewport(gt, tile_center(h.stand) + Vec3::Y * 1.7) {
                let pos = egui::pos2(p.x, p.y - 16.0);
                let font = egui::FontId::proportional(16.0);
                painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::from_black_alpha(220));
                painter.text(pos, Align2::CENTER_BOTTOM, text, font, color);
            }
        }
        // What moving to the hovered cell costs, by the destination marker.
        if let Some((h, &cost)) = ui.hover.filter(|_| target.is_none()).and_then(|h| Some((h, area.cost.get(&h.stand)?))) {
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
        // One label per enemy unit (its square): the best chance against anyone
        // in it, from where the unit stands or, while a destination is
        // hovered, from there.
        let preview = ui
            .hover
            .filter(|h| hovered_enemy(w, b, h.stand).is_none())
            .and_then(|h| Some((h.stand, *area.cost.get(&h.stand)?)));
        for unit in b.units.iter().filter(|t| t.side != Faction::Colony) {
            let Some(p) = unit.members.first().and_then(|&id| w.pawn(id)) else { continue };
            if !ui.shows_at(&w.map, p.x, p.y, p.z) {
                continue;
            }
            // Above the squad's status row.
            let Ok(at) = cam.world_to_viewport(gt, tile_point(p.x, p.y, p.z) + Vec3::Y * 1.8) else { continue };
            let (text, color) = match preview {
                Some((dest, cost)) => {
                    let best = unit.members.iter().filter_map(|&id| best_chance_from(w, u, dest, id)).reduce(f32::max);
                    let can_fire = b.units[u].ap - cost >= AP_ATTACK;
                    match best {
                        Some(c) if can_fire => (format!("from there: {:.0}%", c * 100.0), Color32::from_rgb(150, 210, 255)),
                        Some(c) => (format!("from there: {:.0}% (no AP left)", c * 100.0), Color32::from_rgb(140, 160, 185)),
                        None => ("out of reach from there".to_string(), Color32::from_gray(160)),
                    }
                }
                None => {
                    let best = unit.members.iter().filter_map(|&id| best_chance(w, u, id)).reduce(f32::max);
                    match best {
                        Some(c) => (format!("{:.0}%", c * 100.0), Color32::from_rgb(255, 225, 120)),
                        None => ("out of reach".to_string(), Color32::from_gray(170)),
                    }
                }
            };
            let pos = egui::pos2(at.x, at.y);
            let font = egui::FontId::proportional(14.0);
            painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::from_black_alpha(200));
            painter.text(pos, Align2::CENTER_BOTTOM, text, font, color);
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
                if unit.ap_start < AP_MAX {
                    tags.push(format!("pinned down: {} AP this round", unit.ap_start));
                } else if unit.pinned() {
                    tags.push("under heavy fire".to_string());
                }
                if unit.deployed {
                    tags.push("deployed: better aim, harder to hit, can't move".to_string());
                }
                if unit.broken() {
                    tags.push("shaken".to_string());
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
                    if unit.led {
                        let (label, hint) = if unit.deployed {
                            (format!("Stand up ({} AP)", AP_DEPLOY), "Back on their feet, so the squad can move")
                        } else {
                            (format!("Deploy ({} AP)", AP_DEPLOY), "Down on one knee: better aim, harder to hit; can't move until standing up")
                        };
                        if row.add_enabled(unit.ap >= AP_DEPLOY, egui::Button::new(label)).on_hover_text(hint).clicked() {
                            orders.push(BattleOrder::Deploy);
                        }
                    }
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
            panel.horizontal(|row| {
                row.label(RichText::new("Your units").strong());
                let waiting = b.units.iter().any(|u| u.side == Faction::Colony && u.can_act() && !u.acted);
                let button = egui::Button::new("End all turns").small();
                if row
                    .add_enabled(waiting && !b.colony_passes, button)
                    .on_hover_text("Every unit that hasn't acted yet ends its turn this round")
                    .clicked()
                {
                    end_all = true;
                }
            });
            for (u, unit) in b.units.iter().enumerate().filter(|(_, u)| u.side == Faction::Colony && u.can_act()) {
                let status = if b.active == Some(u) {
                    "acting".to_string()
                } else if unit.acted {
                    "done".to_string()
                } else {
                    format!("{} AP", unit.ap)
                };
                let standing = if unit.roster.len() > 1 {
                    format!(" - {}/{} standing", unit.members.len(), unit.roster.len())
                } else {
                    String::new()
                };
                let text = format!("{}{} - {}", names(w, &unit.members), standing, status);
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
    if end_all && sim.world.end_colony_turns().is_ok() {
        sounds.play("ui_click", None);
    }
    // Show the acting unit in the inspector when its turn starts (it can be
    // let go of after that).
    if let Some(b) = &sim.world.battle {
        let acting = my_unit(b);
        if acting != view.inspected {
            view.inspected = acting;
            if let Some(lead) = acting.and_then(|u| b.units[u].members.first().copied()) {
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
