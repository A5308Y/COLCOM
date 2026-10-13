//! Sound: effects placed in the world and heard relative to the camera's
//! focus (quieter further away and when zoomed out, panned left and right),
//! footsteps and work sounds from what pawns are doing, interface clicks,
//! and ambience loops for day, night and underground. Sound names, files and
//! levels come from `assets/sounds.ron`.

use super::camera::CameraRig;
use super::input::{InspectorTab, Tool, UiState};
use super::sim::Sim;
use super::terrain::LEVEL_H;
use super::tile_point;
use bevy::audio::{AudioSink, AudioSinkPlayback, SpatialListener, Volume};
use bevy::prelude::*;
use my_world::map::{Block, Feature};
use my_world::pawn::{Job, Task};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

/// At most this many effects play at once.
const MAX_PLAYING: usize = 32;
/// Pawns further than this from the camera's focus make no footsteps or work sounds.
const PAWN_SOUND_RANGE: f32 = 40.0;

/// Which volume slider a sound follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum SoundKind {
    Effect,
    Alert,
    Interface,
}

fn unit_pitch() -> (f32, f32) {
    (1.0, 1.0)
}

fn four() -> usize {
    4
}

#[derive(Debug, Deserialize)]
pub struct SoundDef {
    pub files: Vec<String>,
    pub volume: f32,
    #[serde(default = "unit_pitch")]
    pub pitch: (f32, f32),
    /// Cells from the focus it can be heard; 0 = everywhere, not placed.
    #[serde(default)]
    pub range: f32,
    #[serde(default = "four")]
    pub max: usize,
    /// Seconds between strokes while working.
    #[serde(default)]
    pub every: Option<f32>,
    pub kind: SoundKind,
}

#[derive(Debug, Deserialize)]
pub struct LoopDef {
    pub file: String,
    pub volume: f32,
}

#[derive(Debug, Deserialize)]
pub struct AmbienceDefs {
    pub day: LoopDef,
    pub night: LoopDef,
    pub cave: LoopDef,
}

#[derive(Debug, Deserialize)]
pub struct SoundTable {
    pub sounds: BTreeMap<String, SoundDef>,
    pub ambience: AmbienceDefs,
}

/// The sound table, built in like the other definitions.
pub fn sound_table() -> &'static SoundTable {
    static TABLE: LazyLock<SoundTable> = LazyLock::new(|| {
        ron::from_str(include_str!("../../assets/sounds.ron")).unwrap_or_else(|e| panic!("assets/sounds.ron: {}", e))
    });
    &TABLE
}

/// Names the front end itself plays (the simulation's are in `my_world::world::SIM_SOUNDS`).
#[cfg(test)]
const FRONT_SOUNDS: [&str; 13] = [
    "step_grass",
    "step_stone",
    "step_wood",
    "rustle",
    "hammer",
    "craft",
    "mine",
    "dig",
    "door",
    "ui_click",
    "ui_place",
    "ui_open",
    "swing",
];

/// Volume per kind of sound, 0..1.
#[derive(Resource)]
pub struct AudioSettings {
    pub master: f32,
    pub effects: f32,
    pub alerts: f32,
    pub interface: f32,
    pub ambience: f32,
    pub muted: bool,
}

impl Default for AudioSettings {
    fn default() -> Self {
        AudioSettings { master: 0.8, effects: 1.0, alerts: 1.0, interface: 0.8, ambience: 0.8, muted: false }
    }
}

impl AudioSettings {
    fn level(&self, kind: SoundKind) -> f32 {
        if self.muted {
            return 0.0;
        }
        self.master
            * match kind {
                SoundKind::Effect => self.effects,
                SoundKind::Alert => self.alerts,
                SoundKind::Interface => self.interface,
            }
    }
}

/// Sounds asked for this frame: a name from the table, and where (world
/// space; `None` is heard everywhere).
#[derive(Resource, Default)]
pub struct SoundQueue(Vec<(&'static str, Option<Vec3>)>);

impl SoundQueue {
    pub fn play(&mut self, name: &'static str, at: Option<Vec3>) {
        self.0.push((name, at));
    }
}

#[derive(Resource)]
struct SoundBank {
    clips: HashMap<&'static str, Vec<Handle<AudioSource>>>,
}

/// A sound effect being played.
#[derive(Component)]
struct Playing {
    name: &'static str,
    started: f32,
}

#[derive(Component)]
struct Listener;

#[derive(Clone, Copy, PartialEq, Eq)]
enum AmbienceKind {
    Day,
    Night,
    Cave,
}

#[derive(Component)]
struct AmbienceLoop {
    kind: AmbienceKind,
    level: f32,
}

/// What each pawn was doing last frame, to time footsteps and work strokes.
#[derive(Default)]
struct PawnTrack {
    at: Vec3,
    walked: f32,
    progress: f32,
    stroke: f32,
}

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .init_resource::<SoundQueue>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (sim_sounds, pawn_sounds, ui_sounds, follow_camera, play_queued, ambience, clean_up).chain(),
            );
    }
}

fn setup(mut commands: Commands, assets: Res<AssetServer>) {
    let table = sound_table();
    let clips = table
        .sounds
        .iter()
        .map(|(name, def)| (name.as_str(), def.files.iter().map(|f| assets.load(f.as_str())).collect()))
        .collect();
    commands.insert_resource(SoundBank { clips });
    commands.spawn((Listener, SpatialListener::new(0.6), Transform::default()));
    for (kind, def) in [
        (AmbienceKind::Day, &table.ambience.day),
        (AmbienceKind::Night, &table.ambience.night),
        (AmbienceKind::Cave, &table.ambience.cave),
    ] {
        commands.spawn((
            AudioPlayer::new(assets.load(def.file.as_str())),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
            AmbienceLoop { kind, level: 0.0 },
        ));
    }
}

/// World-space centre of a cell given in (fractional) cell coordinates.
fn world_at(x: f32, y: f32, z: f32) -> Vec3 {
    tile_point(x, y, z) + Vec3::Y * 0.5 * LEVEL_H
}

/// Sounds the simulation noted since last frame.
fn sim_sounds(mut sim: ResMut<Sim>, mut queue: ResMut<SoundQueue>) {
    for e in sim.world.take_sounds() {
        queue.play(e.name, e.at.map(|(x, y, z)| world_at(x, y, z)));
    }
}

/// The sound of a stroke of work.
fn work_sound(w: &my_world::world::World, target: my_world::map::Pos, task: Task) -> &'static str {
    let m = &w.map;
    let i = m.idx(target);
    match task {
        Task::Harvest(kind) => kind.def().harvest.sound.as_deref().unwrap_or("rustle"),
        Task::Sow(kind) => kind.def().sow.as_ref().and_then(|s| s.sound.as_deref()).unwrap_or("rustle"),
        Task::Mine => match m.block[i] {
            Block::Soil => "dig",
            Block::Rock | Block::Ore => "mine",
            _ => "hammer",
        },
        Task::Construct | Task::Deconstruct => match m.feature[i] {
            Feature::Blueprint { kind, .. } | Feature::Built { kind, .. } => kind.def().sound.as_deref().unwrap_or("hammer"),
            _ => "hammer",
        },
        Task::Craft => w
            .bench_at(target)
            .and_then(|b| b.current)
            .and_then(|r| r.def().sound.as_deref())
            .unwrap_or("craft"),
    }
}

/// Footsteps by what's underfoot, and the strokes of whatever work pawns do.
fn pawn_sounds(
    sim: Res<Sim>,
    rig: Res<CameraRig>,
    time: Res<Time>,
    mut queue: ResMut<SoundQueue>,
    mut tracks: Local<HashMap<u32, PawnTrack>>,
) {
    let w = &sim.world;
    let m = &w.map;
    let dt = time.delta_secs();
    tracks.retain(|id, _| w.pawn(*id).is_some());
    for p in w.pawns.iter().filter(|p| !p.dead && !p.left) {
        let here = Vec3::new(p.x, p.y, p.z);
        let t = tracks.entry(p.id).or_insert_with(|| PawnTrack { at: here, ..default() });
        let moved = (here - t.at).length();
        t.at = here;
        let pos = world_at(p.x, p.y, p.z);
        if pos.distance(rig.focus) > PAWN_SOUND_RANGE {
            continue;
        }
        // A step every three quarters of a cell walked.
        if !p.lying() && moved < 1.0 {
            t.walked += moved;
            if t.walked >= 0.75 {
                t.walked = 0.0;
                let i = m.idx(p.pos);
                let below = m.below(p.pos).map(|b| m.block[m.idx(b)]);
                let step = if m.floor[i] || m.below(p.pos).is_some_and(|b| m.stairs_dir(b).is_some()) {
                    "step_wood"
                } else if matches!(below, Some(Block::Rock | Block::Ore)) {
                    "step_stone"
                } else {
                    "step_grass"
                };
                queue.play(step, Some(pos));
            }
        }
        // Work strokes while the work moves on.
        let working = match p.job {
            Some(Job::Work { target, task }) if p.path.is_empty() && p.job_progress > t.progress => Some((target, task)),
            _ => None,
        };
        t.progress = p.job_progress;
        match working {
            Some((target, task)) => {
                let name = work_sound(w, target, task);
                let every = sound_table().sounds.get(name).and_then(|d| d.every).unwrap_or(0.7);
                t.stroke += dt;
                if t.stroke >= every {
                    t.stroke = 0.0;
                    let at = world_at(target.0 as f32, target.1 as f32, target.2 as f32);
                    queue.play(name, Some(at));
                }
            }
            // The first stroke comes soon after starting.
            None => t.stroke = 0.5,
        }
    }
}

/// Clicks for choosing tools and opening windows.
fn ui_sounds(ui: Res<UiState>, mut queue: ResMut<SoundQueue>, mut last: Local<Option<(Tool, [bool; 8])>>) {
    let windows = [
        ui.show_work,
        ui.show_help,
        ui.show_build,
        ui.show_resources,
        ui.show_sound,
        ui.show_squads,
        ui.inspector_tab == InspectorTab::Health,
        ui.inspector_tab == InspectorTab::Combat,
    ];
    if let Some((tool, open)) = *last {
        if tool != ui.tool {
            queue.play("ui_click", None);
        }
        if open != windows {
            queue.play("ui_open", None);
        }
    }
    *last = Some((ui.tool, windows));
}

/// The listener sits at the camera's focus, ears along the view's left-right.
fn follow_camera(rig: Res<CameraRig>, mut listener: Single<&mut Transform, With<Listener>>) {
    let (_, right) = rig.axes();
    **listener = Transform::from_translation(rig.focus).with_rotation(Quat::from_rotation_arc(Vec3::X, right));
}

/// Small deterministic random numbers for variants and pitch.
fn next_random(state: &mut u64) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 40) as f32 / (1u64 << 24) as f32
}

#[allow(clippy::too_many_arguments)]
fn play_queued(
    mut commands: Commands,
    mut queue: ResMut<SoundQueue>,
    bank: Res<SoundBank>,
    settings: Res<AudioSettings>,
    rig: Res<CameraRig>,
    ui: Res<UiState>,
    sim: Res<Sim>,
    time: Res<Time>,
    playing: Query<&Playing>,
    mut seed: Local<u64>,
) {
    if *seed == 0 {
        *seed = 0x9e37_79b9_7f4a_7c15;
    }
    let now = time.elapsed_secs();
    let mut counts: HashMap<&'static str, (usize, f32)> = HashMap::new();
    for p in &playing {
        let c = counts.entry(p.name).or_insert((0, f32::MIN));
        c.0 += 1;
        c.1 = c.1.max(p.started);
    }
    let mut total = playing.iter().count();
    let (forward, _) = rig.axes();
    let cut = ui.top_level(&sim.world.map) as f32;
    for (name, at) in queue.0.drain(..) {
        let Some(def) = sound_table().sounds.get(name) else {
            warn!("no sound \"{}\" in assets/sounds.ron", name);
            continue;
        };
        let Some(clips) = bank.clips.get(name).filter(|c| !c.is_empty()) else { continue };
        let (count, last) = counts.get(name).copied().unwrap_or((0, f32::MIN));
        // Not too many at once, and never the same sound twice in the same instant.
        if total >= MAX_PLAYING || count >= def.max || now - last < 0.03 {
            continue;
        }
        let mut volume = def.volume * settings.level(def.kind);
        let mut place = None;
        if let (Some(p), true) = (at, def.range > 0.0) {
            let d = p - rig.focus;
            let reach = Vec3::new(d.x, d.y * 1.5, d.z).length();
            let zoom = ((rig.distance - 12.0) / 90.0).clamp(0.0, 1.0);
            volume *= (1.0 - reach / def.range).clamp(0.0, 1.0).powf(1.5) * (1.0 - 0.55 * zoom);
            // Hidden by the layer view: muffled.
            if p.y / LEVEL_H > cut + 1.0 {
                volume *= 0.35;
            }
            // Panning only: placed one unit from the listener in the sound's direction,
            // so the stereo spread is right but distance is up to the volume above.
            let flat = Vec3::new(d.x, 0.0, d.z);
            let dir = if flat.length() > 0.5 { flat.normalize() } else { forward };
            place = Some(rig.focus + dir);
        }
        if volume < 0.01 {
            continue;
        }
        let clip = clips[(next_random(&mut seed) * clips.len() as f32) as usize % clips.len()].clone();
        let speed = def.pitch.0 + (def.pitch.1 - def.pitch.0) * next_random(&mut seed);
        let settings = PlaybackSettings::DESPAWN
            .with_volume(Volume::Linear(volume))
            .with_speed(speed)
            .with_spatial(place.is_some());
        commands.spawn((
            AudioPlayer::new(clip),
            settings,
            Transform::from_translation(place.unwrap_or(rig.focus)),
            Playing { name, started: now },
        ));
        counts.insert(name, (count + 1, now));
        total += 1;
    }
}

/// Day, night and cave loops fade with the time of day and whether the
/// camera looks at somewhere underground.
fn ambience(
    sim: Res<Sim>,
    rig: Res<CameraRig>,
    ui: Res<UiState>,
    settings: Res<AudioSettings>,
    time: Res<Time>,
    mut loops: Query<(&mut AmbienceLoop, Option<&mut AudioSink>)>,
) {
    let w = &sim.world;
    let m = &w.map;
    let (x, y) = (rig.focus.x.floor() as i32, rig.focus.z.floor() as i32);
    let under = m.at(x, y, 0).is_some_and(|(x, y, _)| {
        let surface = m.surface(x, y).unwrap_or(0);
        let level = ((rig.focus.y / LEVEL_H).round() as usize).min(m.d - 1);
        // Looking below ground in the layer view, or at a covered spot (a tunnel).
        ui.view_level.is_some_and(|l| l < surface) || (level < surface && m.covered((x, y, level)))
    });
    let day = ((w.daylight() - 0.2) / 0.5).clamp(0.0, 1.0);
    let table = &sound_table().ambience;
    let master = if settings.muted { 0.0 } else { settings.master * settings.ambience };
    let ease = (time.delta_secs() * 0.8).min(1.0);
    for (mut amb, sink) in &mut loops {
        let (target, def) = match amb.kind {
            AmbienceKind::Day => (day * (1.0 - under as u8 as f32), &table.day),
            AmbienceKind::Night => ((1.0 - day) * (1.0 - under as u8 as f32), &table.night),
            AmbienceKind::Cave => (under as u8 as f32, &table.cave),
        };
        amb.level += (target - amb.level) * ease;
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(amb.level * def.volume * master));
        }
    }
}

/// Effects normally remove themselves when done; this catches any that
/// never started (no audio device).
fn clean_up(mut commands: Commands, time: Res<Time>, playing: Query<(Entity, &Playing)>) {
    let now = time.elapsed_secs();
    for (e, p) in &playing {
        if now - p.started > 10.0 {
            commands.entity(e).despawn();
        }
    }
}

/// Settings window contents: a slider per kind of sound.
pub fn settings_ui(ui: &mut bevy_egui::egui::Ui, s: &mut AudioSettings) {
    use bevy_egui::egui;
    ui.checkbox(&mut s.muted, "Mute");
    egui::Grid::new("sound_levels").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        for (label, v) in [
            ("Master", &mut s.master),
            ("Effects", &mut s.effects),
            ("Alerts", &mut s.alerts),
            ("Interface", &mut s.interface),
            ("Ambience", &mut s.ambience),
        ] {
            ui.label(label);
            ui.add(egui::Slider::new(v, 0.0..=1.0).show_value(false));
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use my_world::defs::{BuildId, ItemId, PlantId, RecipeId};

    fn exists(file: &str) -> bool {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(file).is_file()
    }

    #[test]
    fn every_sound_has_its_files() {
        let t = sound_table();
        for (name, def) in &t.sounds {
            assert!(!def.files.is_empty(), "{}: no files", name);
            for f in &def.files {
                assert!(exists(f), "{}: missing {}", name, f);
            }
            assert!(def.volume > 0.0 && def.volume <= 1.0 && def.pitch.0 <= def.pitch.1 && def.max > 0, "{}", name);
        }
        for l in [&t.ambience.day, &t.ambience.night, &t.ambience.cave] {
            assert!(exists(&l.file), "missing {}", l.file);
        }
    }

    #[test]
    fn every_sound_the_game_uses_is_defined() {
        let mut used: Vec<&str> = FRONT_SOUNDS.to_vec();
        used.extend(my_world::world::SIM_SOUNDS);
        for k in PlantId::all() {
            let d = k.def();
            used.extend(d.harvest.sound.as_deref());
            used.extend(d.harvest.done_sound.as_deref());
            used.extend(d.sow.as_ref().and_then(|s| s.sound.as_deref()));
        }
        used.extend(BuildId::all().filter_map(|k| k.def().sound.as_deref()));
        used.extend(RecipeId::all().filter_map(|k| k.def().sound.as_deref()));
        used.extend(ItemId::all().filter_map(|k| k.def().weapon.as_ref().and_then(|w| w.sound.as_deref())));
        for name in used {
            assert!(sound_table().sounds.contains_key(name), "\"{}\" isn't in assets/sounds.ron", name);
        }
    }
}
