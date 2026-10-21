//! Screenshot mode: `--screenshot [name,name...]` (or `MY_WORLD_SCREENSHOT`)
//! opens one interface state after another (`SHOTS`), saves each as
//! `target/screenshots/<name>.png` (`<scenario>-<name>.png` with `--scenario`) and quits. The simulation is frozen (battles too), so
//! shots of the same game are comparable before and after a UI change.
//!
//! With a GPU the whole window is captured. With `MY_WORLD_NO_GPU=1` the app
//! runs without a window and only the egui interface is drawn, by a small
//! software rasterizer, over a plain backdrop: enough to review layout and
//! colours, not the 3D scene. These are marked `-cpu` in their file names.

use super::info::InfoTarget;
use super::input::{InspectorTab, Selection, UiState};
use super::sim::{Clock, Sim};
use bevy::app::AppExit;
use image::{Rgb, RgbImage};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy_egui::egui::{self, epaint, Color32, Pos2, Rect};
use bevy_egui::{EguiInput, EguiPostUpdateSet, EguiPreUpdateSet, EguiRenderOutput, PrimaryEguiContext};
use my_world::pawn::PawnId;
use my_world::defs::{BuildId, ItemId};
use std::collections::HashMap;
use std::path::PathBuf;

/// Interface size in the windowless mode (logical pixels, one pixel each).
const CPU_SIZE: [usize; 2] = [1280, 800];
/// Frames to let the game settle before the first shot (models, layout).
const WARMUP: u32 = 30;
/// Frames between setting up a shot and taking it: egui sizes windows over a
/// couple of frames and fade them in.
const SETTLE: u32 = 12;

/// One interface state: its file name and how to get there from a clean screen.
pub struct Shot {
    pub name: &'static str,
    pub setup: fn(&mut UiState, &mut Sim),
}

fn first_colonist(sim: &Sim) -> Option<PawnId> {
    sim.world.pawns.iter().find(|p| p.is_living_colonist()).map(|p| p.id)
}

fn pawn_tab(st: &mut UiState, sim: &Sim, tab: InspectorTab) {
    if let Some(id) = first_colonist(sim) {
        st.select(Some(Selection::Pawn(id)));
        st.inspector_tab = tab;
    }
}

fn help_topic(st: &mut UiState, id: &str) {
    st.show_help = true;
    st.help_topic = super::help::topic_index(id);
}

pub const SHOTS: &[Shot] = &[
    Shot { name: "hud", setup: |_, _| {} },
    Shot { name: "help", setup: |st, _| help_topic(st, "basics") },
    Shot { name: "help-building", setup: |st, _| help_topic(st, "building") },
    Shot { name: "help-battle", setup: |st, _| help_topic(st, "battle") },
    Shot { name: "help-controls", setup: |st, _| help_topic(st, "controls") },
    Shot {
        name: "help-search",
        setup: |st, _| {
            help_topic(st, "rooms");
            st.help_filter = "roof".into();
        },
    },
    Shot { name: "menu", setup: |st, _| st.show_menu = true },
    Shot { name: "work", setup: |st, _| st.show_work = true },
    Shot {
        name: "schedule",
        setup: |st, _| {
            st.show_work = true;
            st.work_schedule = true;
        },
    },
    Shot { name: "resources", setup: |st, _| st.show_resources = true },
    Shot {
        name: "info",
        setup: |st, _| {
            st.show_info = true;
            st.info = ItemId::find("assault_rifle").map(InfoTarget::Item);
        },
    },
    Shot {
        name: "info-building",
        setup: |st, _| {
            st.show_info = true;
            st.info = BuildId::find("stove").map(InfoTarget::Build);
        },
    },
    Shot { name: "rooms", setup: |st, _| st.show_rooms = true },
    Shot { name: "squads", setup: |st, _| st.show_squads = true },
    Shot { name: "expeditions", setup: |st, _| st.expeditions.open = true },
    Shot { name: "spawn", setup: |st, _| st.show_spawn = true },
    Shot { name: "colonist", setup: |st, sim| pawn_tab(st, sim, InspectorTab::Overview) },
    Shot { name: "colonist-health", setup: |st, sim| pawn_tab(st, sim, InspectorTab::Health) },
    Shot { name: "colonist-combat", setup: |st, sim| pawn_tab(st, sim, InspectorTab::Combat) },
    Shot {
        name: "cell",
        setup: |st, sim| {
            if let Some(p) = sim.world.pawns.iter().find(|p| p.is_living_colonist()) {
                st.select(Some(Selection::Tile(p.pos)));
            }
        },
    },
    Shot {
        name: "build-menu",
        setup: |st, _| st.build_filter = "wall".into(),
    },
];

/// The shots asked for on the command line, if screenshot mode is on.
/// `--screenshot` alone (or `all`) takes them all.
pub fn requested() -> Option<Vec<&'static Shot>> {
    let args: Vec<String> = std::env::args().collect();
    let arg = args.iter().position(|a| a == "--screenshot").map(|k| {
        args.get(k + 1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default()
    });
    let names = arg.or_else(|| std::env::var("MY_WORLD_SCREENSHOT").ok())?;
    let names: Vec<&str> = names.split(',').map(str::trim).filter(|n| !n.is_empty() && *n != "all").collect();
    if names.is_empty() {
        return Some(SHOTS.iter().collect());
    }
    let mut shots = Vec::new();
    for name in names {
        match SHOTS.iter().find(|s| s.name == name) {
            Some(s) => shots.push(s),
            None => {
                let known: Vec<&str> = SHOTS.iter().map(|s| s.name).collect();
                eprintln!("No screenshot called '{}' (there are: {}).", name, known.join(", "));
            }
        }
    }
    Some(shots)
}

/// Whether drawing is off (`MY_WORLD_NO_GPU`): shots are then rasterized on the CPU.
pub fn cpu_only() -> bool {
    std::env::var_os("MY_WORLD_NO_GPU").is_some()
}

pub fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target").join("screenshots")
}

/// Present while screenshots are taken: the game isn't saved on exit.
#[derive(Resource)]
pub struct ScreenshotMode;

#[derive(Resource)]
struct Plan {
    shots: Vec<&'static Shot>,
    cpu: bool,
    /// The shot being prepared or taken, whether it's set up, and frames until it's taken.
    next: usize,
    set_up: bool,
    wait: u32,
    /// A GPU capture was asked for and hasn't been saved yet.
    pending: bool,
}

/// egui's textures as the software rasterizer sees them (kept up to date
/// from every frame's texture changes).
#[derive(Resource, Default)]
struct CpuTextures(HashMap<egui::TextureId, egui::ColorImage>);

pub struct ScreenshotPlugin(pub Vec<&'static Shot>);

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        let cpu = cpu_only();
        app.insert_resource(ScreenshotMode).insert_resource(Plan { shots: self.0.clone(), cpu, next: 0, set_up: false, wait: WARMUP, pending: false })
            .add_systems(Update, step)
            .add_observer(saved);
        if cpu {
            app.init_resource::<CpuTextures>()
                .add_systems(
                    PreUpdate,
                    windowless_screen.after(EguiPreUpdateSet::ProcessInput).before(EguiPreUpdateSet::BeginPass),
                )
                .add_systems(PostUpdate, cpu_capture.after(EguiPostUpdateSet::ProcessOutput));
        }
    }
}

/// Clears everything a shot might have opened.
fn reset(st: &mut UiState) {
    st.select(None);
    st.show_work = false;
    st.work_schedule = false;
    st.show_help = false;
    st.help_topic = 0;
    st.help_filter.clear();
    st.inspector_tab = InspectorTab::Overview;
    st.show_squads = false;
    st.show_rooms = false;
    st.expeditions.open = false;
    st.show_spawn = false;
    st.show_build = true;
    st.build_filter.clear();
    st.show_resources = false;
    st.show_menu = false;
    st.confirm_new = false;
    st.show_info = false;
    st.info = None;
    st.info_back.clear();
    st.job_menu = None;
}

fn step(mut plan: ResMut<Plan>, mut st: ResMut<UiState>, mut sim: ResMut<Sim>, mut commands: Commands, mut exit: MessageWriter<AppExit>) {
    sim.clock = Clock::Frozen;
    if plan.pending {
        return;
    }
    if plan.next >= plan.shots.len() {
        eprintln!("Screenshots saved in {}", out_dir().display());
        exit.write(AppExit::Success);
        return;
    }
    if !plan.set_up {
        reset(&mut st);
        (plan.shots[plan.next].setup)(&mut st, &mut sim);
        plan.set_up = true;
    }
    if plan.wait > 0 {
        plan.wait -= 1;
        return;
    }
    plan.pending = true;
    if !plan.cpu {
        commands.spawn(Screenshot::primary_window());
    }
}

/// `<scenario>-<shot>[-cpu].png`: shots of a scenario don't replace those of a colony.
fn shot_path(sim: &Sim, name: &str, cpu: bool) -> PathBuf {
    let name = match &sim.scenario {
        Some(scenario) => format!("{}-{}", scenario, name),
        None => name.to_string(),
    };
    out_dir().join(if cpu { format!("{}-cpu.png", name) } else { format!("{}.png", name) })
}

fn finish(plan: &mut Plan) {
    plan.pending = false;
    plan.next += 1;
    plan.set_up = false;
    plan.wait = SETTLE;
}

/// A GPU capture arrived: save it and go on to the next shot.
fn saved(captured: On<ScreenshotCaptured>, mut plan: ResMut<Plan>, sim: Res<Sim>) {
    if !plan.pending || plan.cpu {
        return;
    }
    let path = shot_path(&sim, plan.shots[plan.next].name, false);
    let saved = captured
        .image
        .clone()
        .try_into_dynamic()
        .map_err(|e| e.to_string())
        .and_then(|img| save_png(img.to_rgb8(), &path));
    report(&path, saved);
    finish(&mut plan);
}

fn save_png(img: RgbImage, path: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(out_dir()).map_err(|e| e.to_string())?;
    img.save(path).map_err(|e| e.to_string())
}

fn report(path: &std::path::Path, result: Result<(), String>) {
    match result {
        Ok(()) => eprintln!("saved {}", path.display()),
        Err(e) => eprintln!("couldn't save {}: {}", path.display(), e),
    }
}

/// Without a window egui doesn't know how big the screen is: give it one.
fn windowless_screen(mut inputs: Query<&mut EguiInput, With<PrimaryEguiContext>>) {
    for mut input in &mut inputs {
        input.0.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(CPU_SIZE[0] as f32, CPU_SIZE[1] as f32)));
        if let Some(vp) = input.0.viewports.get_mut(&egui::ViewportId::ROOT) {
            vp.native_pixels_per_point = Some(1.0);
        }
    }
}

fn cpu_capture(
    mut plan: ResMut<Plan>,
    sim: Res<Sim>,
    mut textures: ResMut<CpuTextures>,
    outputs: Query<&EguiRenderOutput, With<PrimaryEguiContext>>,
) {
    let Some(out) = outputs.iter().next() else { return };
    for (id, delta) in out.textures_delta.set.iter().flat_map(|(id, ds)| ds.iter().map(move |d| (id, d))) {
        let epaint::ImageData::Color(patch) = &delta.image;
        match delta.pos {
            None => {
                textures.0.insert(*id, (**patch).clone());
            }
            Some([x0, y0]) => {
                if let Some(tex) = textures.0.get_mut(id) {
                    for y in 0..patch.size[1] {
                        for x in 0..patch.size[0] {
                            tex.pixels[(y0 + y) * tex.size[0] + x0 + x] = patch.pixels[y * patch.size[0] + x];
                        }
                    }
                }
            }
        }
    }
    if !plan.pending || !plan.cpu {
        for id in &out.textures_delta.free {
            textures.0.remove(id);
        }
        return;
    }
    let img = rasterize(&out.paint_jobs, &textures.0, CPU_SIZE);
    for id in &out.textures_delta.free {
        textures.0.remove(id);
    }
    let path = shot_path(&sim, plan.shots[plan.next].name, true);
    let rgb = RgbImage::from_fn(CPU_SIZE[0] as u32, CPU_SIZE[1] as u32, |x, y| {
        let c = img[y as usize * CPU_SIZE[0] + x as usize];
        Rgb([c[0] as u8, c[1] as u8, c[2] as u8])
    });
    report(&path, save_png(rgb, &path));
    finish(&mut plan);
}

/// The backdrop standing in for the 3D scene: a dull field colour with a
/// faint grid, so translucent panels can be judged.
fn backdrop(x: usize, y: usize) -> [f32; 3] {
    let grid = x.is_multiple_of(64) || y.is_multiple_of(64);
    let shade = 0.9 + 0.1 * (y as f32 / CPU_SIZE[1] as f32);
    let base = if grid { [92.0, 102.0, 78.0] } else { [78.0, 88.0, 66.0] };
    [base[0] * shade, base[1] * shade, base[2] * shade]
}

/// Draws egui's triangles the way its GPU renderers do: vertex colour times
/// the (bilinearly sampled) texture, premultiplied alpha, blended in gamma space.
fn rasterize(jobs: &[egui::ClippedPrimitive], textures: &HashMap<egui::TextureId, egui::ColorImage>, size: [usize; 2]) -> Vec<[f32; 3]> {
    let [w, h] = size;
    let mut px: Vec<[f32; 3]> = (0..w * h).map(|i| backdrop(i % w, i / w)).collect();
    let white = egui::ColorImage::new([1, 1], vec![Color32::WHITE]);
    for job in jobs {
        let epaint::Primitive::Mesh(mesh) = &job.primitive else { continue };
        let tex = textures.get(&mesh.texture_id).unwrap_or(&white);
        let clip = job.clip_rect.intersect(Rect::from_min_size(Pos2::ZERO, egui::vec2(w as f32, h as f32)));
        if clip.width() <= 0.0 || clip.height() <= 0.0 {
            continue;
        }
        for tri in mesh.indices.as_chunks::<3>().0 {
            let v = [&mesh.vertices[tri[0] as usize], &mesh.vertices[tri[1] as usize], &mesh.vertices[tri[2] as usize]];
            triangle(&mut px, w, clip, v, tex);
        }
    }
    px
}

fn triangle(px: &mut [[f32; 3]], w: usize, clip: Rect, v: [&epaint::Vertex; 3], tex: &egui::ColorImage) {
    let (a, b, c) = (v[0].pos, v[1].pos, v[2].pos);
    let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
    if area.abs() < 1e-6 {
        return;
    }
    let x0 = a.x.min(b.x).min(c.x).max(clip.min.x).floor() as i32;
    let x1 = a.x.max(b.x).max(c.x).min(clip.max.x).ceil() as i32;
    let y0 = a.y.min(b.y).min(c.y).max(clip.min.y).floor() as i32;
    let y1 = a.y.max(b.y).max(c.y).min(clip.max.y).ceil() as i32;
    let rgba = |c: Color32| [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32];
    let col = [rgba(v[0].color), rgba(v[1].color), rgba(v[2].color)];
    for y in y0.max(0)..y1 {
        for x in x0.max(0)..x1 {
            let p = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
            if !clip.contains(p) {
                continue;
            }
            let w0 = ((b.x - p.x) * (c.y - p.y) - (b.y - p.y) * (c.x - p.x)) / area;
            let w1 = ((c.x - p.x) * (a.y - p.y) - (c.y - p.y) * (a.x - p.x)) / area;
            let w2 = 1.0 - w0 - w1;
            // Pixel centres on an edge belong to one of the two triangles sharing it.
            if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                continue;
            }
            let u = w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x;
            let t = w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y;
            let s = sample(tex, u, t);
            let mut src = [0.0f32; 4];
            for k in 0..4 {
                let vc = w0 * col[0][k] + w1 * col[1][k] + w2 * col[2][k];
                src[k] = vc * s[k] / 255.0;
            }
            let dst = &mut px[y as usize * w + x as usize];
            let keep = 1.0 - src[3] / 255.0;
            for k in 0..3 {
                dst[k] = (src[k] + dst[k] * keep).clamp(0.0, 255.0);
            }
        }
    }
}

/// Bilinear texture lookup (premultiplied RGBA, 0-255).
fn sample(tex: &egui::ColorImage, u: f32, v: f32) -> [f32; 4] {
    let [tw, th] = tex.size;
    let fx = (u * tw as f32 - 0.5).clamp(0.0, (tw - 1) as f32);
    let fy = (v * th as f32 - 0.5).clamp(0.0, (th - 1) as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
    let (dx, dy) = (fx - x0 as f32, fy - y0 as f32);
    let at = |x: usize, y: usize| {
        let c = tex.pixels[y * tw + x];
        [c.r() as f32, c.g() as f32, c.b() as f32, c.a() as f32]
    };
    let (p00, p10, p01, p11) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = p00[k] + (p10[k] - p00[k]) * dx;
        let bottom = p01[k] + (p11[k] - p01[k]) * dx;
        out[k] = top + (bottom - top) * dy;
    }
    out
}
