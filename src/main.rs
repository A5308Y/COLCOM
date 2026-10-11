//! My World: a tiny RimWorld-style colony sim prototype.
//!
//! You don't control colonists directly. You designate work (chop, mine,
//! harvest, build) and they pick up jobs on their own, walk there, do the
//! work and eat when hungry.

use macroquad::prelude::*;
use macroquad::rand;
use std::collections::VecDeque;

const W: usize = 70;
const H: usize = 42;
const TS: f32 = 16.0;
const HUD_H: f32 = 140.0;
const WALL_COST: i32 = 5;
const DAY_LEN: f32 = 120.0;
const MOVE_SPEED: f32 = 4.0;

type Pos = (usize, usize);

fn idx(p: Pos) -> usize {
    p.1 * W + p.0
}

fn pos_of(i: usize) -> Pos {
    (i % W, i / W)
}

fn neighbors(p: Pos) -> impl Iterator<Item = Pos> {
    let (x, y) = (p.0 as i32, p.1 as i32);
    [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && ny >= 0 && (nx as usize) < W && (ny as usize) < H {
                Some((nx as usize, ny as usize))
            } else {
                None
            }
        })
}

fn rnd() -> f32 {
    rand::gen_range(0.0f32, 1.0f32)
}

fn rand_idx(n: usize) -> usize {
    ((rnd() * n as f32) as usize).min(n.saturating_sub(1))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tile {
    Grass,
    Tree,
    Rock,
    Water,
    Bush,
    Wall,
}

impl Tile {
    fn passable(self) -> bool {
        self == Tile::Grass
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
    Chop,
    Mine,
    Harvest,
    Build,
}

impl Job {
    fn work_time(self) -> f32 {
        match self {
            Job::Chop => 3.0,
            Job::Mine => 5.0,
            Job::Harvest => 1.5,
            Job::Build => 4.0,
        }
    }

    fn valid_on(self, t: Tile) -> bool {
        matches!(
            (self, t),
            (Job::Chop, Tile::Tree)
                | (Job::Mine, Tile::Rock)
                | (Job::Mine, Tile::Wall)
                | (Job::Harvest, Tile::Bush)
                | (Job::Build, Tile::Grass)
        )
    }

    fn color(self) -> Color {
        match self {
            Job::Chop => ORANGE,
            Job::Mine => SKYBLUE,
            Job::Harvest => PINK,
            Job::Build => YELLOW,
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Job::Chop => "chopping",
            Job::Mine => "mining",
            Job::Harvest => "harvesting",
            Job::Build => "building",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Chop,
    Mine,
    Harvest,
    Build,
    Cancel,
}

impl Tool {
    const ALL: [Tool; 5] = [Tool::Chop, Tool::Mine, Tool::Harvest, Tool::Build, Tool::Cancel];

    fn label(self) -> &'static str {
        match self {
            Tool::Chop => "[1] Chop",
            Tool::Mine => "[2] Mine",
            Tool::Harvest => "[3] Harvest",
            Tool::Build => "[4] Build wall",
            Tool::Cancel => "[5] Cancel",
        }
    }

    fn job(self) -> Option<Job> {
        match self {
            Tool::Chop => Some(Job::Chop),
            Tool::Mine => Some(Job::Mine),
            Tool::Harvest => Some(Job::Harvest),
            Tool::Build => Some(Job::Build),
            Tool::Cancel => None,
        }
    }
}

enum State {
    Idle { wait: f32 },
    Moving { path: Vec<Pos>, job: Option<(Pos, Job)> },
    Working { target: Pos, job: Job, progress: f32 },
    Eating { t: f32 },
    Dead,
}

struct Colonist {
    name: String,
    tile: Pos,
    x: f32,
    y: f32,
    hunger: f32,
    health: f32,
    color: Color,
    state: State,
}

impl Colonist {
    fn new(name: &str, tile: Pos) -> Self {
        Colonist {
            name: name.to_string(),
            tile,
            x: tile.0 as f32,
            y: tile.1 as f32,
            hunger: 70.0 + rnd() * 30.0,
            health: 100.0,
            color: Color::new(0.5 + rnd() * 0.5, 0.4 + rnd() * 0.5, 0.4 + rnd() * 0.5, 1.0),
            state: State::Idle { wait: rnd() },
        }
    }

    fn is_dead(&self) -> bool {
        matches!(self.state, State::Dead)
    }

    fn claimed_job(&self) -> Option<Pos> {
        match &self.state {
            State::Moving { job: Some((p, _)), .. } => Some(*p),
            State::Working { target, .. } => Some(*target),
            _ => None,
        }
    }

    fn status(&self) -> String {
        match &self.state {
            State::Idle { .. } => "idle".into(),
            State::Moving { job: Some((_, j)), .. } => format!("going {}", j.verb()),
            State::Moving { job: None, .. } => "wandering".into(),
            State::Working { job, .. } => job.verb().into(),
            State::Eating { .. } => "eating".into(),
            State::Dead => "dead".into(),
        }
    }
}

const WANDERER_NAMES: [&str; 10] = ["Dax", "Eve", "Finn", "Gus", "Hana", "Ivo", "Juno", "Kai", "Lia", "Milo"];

struct World {
    tiles: Vec<Tile>,
    desig: Vec<Option<Job>>,
    claimed: Vec<bool>,
    colonists: Vec<Colonist>,
    wood: i32,
    stone: i32,
    food: i32,
    time: f32,
    speed: u32,
    paused: bool,
    tool: Tool,
    log: Vec<String>,
    regrow_timer: f32,
    wanderer_timer: f32,
    wanderers_arrived: usize,
}

impl World {
    fn new() -> Self {
        let mut tiles = vec![Tile::Grass; W * H];

        let blob = |tiles: &mut Vec<Tile>, t: Tile, cx: i32, cy: i32, r: i32| {
            for y in (cy - r)..=(cy + r) {
                for x in (cx - r)..=(cx + r) {
                    if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
                        continue;
                    }
                    let d = (((x - cx).pow(2) + (y - cy).pow(2)) as f32).sqrt();
                    if d <= r as f32 - rnd() * 1.5 {
                        tiles[y as usize * W + x as usize] = t;
                    }
                }
            }
        };

        for _ in 0..8 {
            let (cx, cy) = (rand_idx(W) as i32, rand_idx(H) as i32);
            blob(&mut tiles, Tile::Rock, cx, cy, 3 + rand_idx(4) as i32);
        }
        for _ in 0..3 {
            let (cx, cy) = (rand_idx(W) as i32, rand_idx(H) as i32);
            blob(&mut tiles, Tile::Water, cx, cy, 2 + rand_idx(3) as i32);
        }
        for t in tiles.iter_mut() {
            if *t == Tile::Grass {
                let r = rnd();
                if r < 0.12 {
                    *t = Tile::Tree;
                } else if r < 0.15 {
                    *t = Tile::Bush;
                }
            }
        }
        // Clear a landing zone in the middle.
        let (cx, cy) = (W / 2, H / 2);
        for y in cy - 4..=cy + 4 {
            for x in cx - 5..=cx + 5 {
                tiles[idx((x, y))] = Tile::Grass;
            }
        }

        let colonists = vec![
            Colonist::new("Ada", (cx - 1, cy)),
            Colonist::new("Bram", (cx, cy)),
            Colonist::new("Cleo", (cx + 1, cy)),
        ];

        World {
            tiles,
            desig: vec![None; W * H],
            claimed: vec![false; W * H],
            colonists,
            wood: 0,
            stone: 0,
            food: 10,
            time: DAY_LEN * 0.25,
            speed: 1,
            paused: false,
            tool: Tool::Chop,
            log: vec!["Three colonists crash-landed. Designate work for them!".into()],
            regrow_timer: 6.0,
            wanderer_timer: 150.0,
            wanderers_arrived: 0,
        }
    }

    fn push_log(&mut self, msg: String) {
        self.log.push(msg);
        if self.log.len() > 4 {
            self.log.remove(0);
        }
    }

    // ---------- pathfinding ----------

    fn bfs(&self, start: Pos) -> (Vec<i32>, Vec<usize>) {
        let mut dist = vec![-1; W * H];
        let mut parent = vec![usize::MAX; W * H];
        let mut q = VecDeque::new();
        dist[idx(start)] = 0;
        q.push_back(start);
        while let Some(p) = q.pop_front() {
            let d = dist[idx(p)];
            for n in neighbors(p) {
                let ni = idx(n);
                if dist[ni] < 0 && self.tiles[ni].passable() {
                    dist[ni] = d + 1;
                    parent[ni] = idx(p);
                    q.push_back(n);
                }
            }
        }
        (dist, parent)
    }

    /// Path from start to a reachable goal, stored reversed so `pop()` yields the next step.
    fn path_from(parent: &[usize], start: Pos, goal: Pos) -> Vec<Pos> {
        let mut path = Vec::new();
        let s = idx(start);
        let mut cur = idx(goal);
        while cur != s {
            path.push(pos_of(cur));
            cur = parent[cur];
        }
        path
    }

    fn can_afford_wall(&self) -> bool {
        self.stone >= WALL_COST || self.wood >= WALL_COST
    }

    /// Nearest unclaimed designation reachable from `start`: (path, target, job).
    fn find_job(&self, start: Pos) -> Option<(Vec<Pos>, Pos, Job)> {
        let (dist, parent) = self.bfs(start);
        let mut best: Option<(i32, Pos, Pos, Job)> = None;
        for i in 0..W * H {
            let Some(job) = self.desig[i] else { continue };
            if self.claimed[i] || (job == Job::Build && !self.can_afford_wall()) {
                continue;
            }
            let target = pos_of(i);
            for n in neighbors(target) {
                let d = dist[idx(n)];
                if d >= 0 && best.map_or(true, |b| d < b.0) {
                    best = Some((d, n, target, job));
                }
            }
        }
        best.map(|(_, stand, target, job)| (Self::path_from(&parent, start, stand), target, job))
    }

    fn wander_path(&self, start: Pos) -> Vec<Pos> {
        let (dist, parent) = self.bfs(start);
        let candidates: Vec<usize> = (0..W * H).filter(|&i| dist[i] > 0 && dist[i] <= 5).collect();
        if candidates.is_empty() {
            return Vec::new();
        }
        let goal = candidates[rand_idx(candidates.len())];
        Self::path_from(&parent, start, pos_of(goal))
    }

    // ---------- simulation ----------

    fn update(&mut self, dt: f32) {
        self.time += dt;

        self.regrow_timer -= dt;
        if self.regrow_timer <= 0.0 {
            self.regrow_timer = 6.0;
            let i = rand_idx(W * H);
            let occupied = self.colonists.iter().any(|c| idx(c.tile) == i);
            if self.tiles[i] == Tile::Grass && self.desig[i].is_none() && !occupied {
                self.tiles[i] = if rnd() < 0.5 { Tile::Bush } else { Tile::Tree };
            }
        }

        self.wanderer_timer -= dt;
        if self.wanderer_timer <= 0.0 {
            self.wanderer_timer = 150.0;
            self.try_spawn_wanderer();
        }

        let mut cols = std::mem::take(&mut self.colonists);
        for c in cols.iter_mut() {
            self.update_colonist(c, dt);
        }
        self.colonists = cols;
    }

    fn try_spawn_wanderer(&mut self) {
        let alive = self.colonists.iter().filter(|c| !c.is_dead()).count();
        if alive == 0 || alive >= 8 || self.wanderers_arrived >= WANDERER_NAMES.len() {
            return;
        }
        for _ in 0..100 {
            let p = if rnd() < 0.5 {
                (rand_idx(W), if rnd() < 0.5 { 0 } else { H - 1 })
            } else {
                (if rnd() < 0.5 { 0 } else { W - 1 }, rand_idx(H))
            };
            if self.tiles[idx(p)].passable() {
                let name = WANDERER_NAMES[self.wanderers_arrived];
                self.wanderers_arrived += 1;
                self.colonists.push(Colonist::new(name, p));
                self.push_log(format!("{} wandered in and joined the colony.", name));
                return;
            }
        }
    }

    fn update_colonist(&mut self, c: &mut Colonist, dt: f32) {
        if c.is_dead() {
            return;
        }

        c.hunger = (c.hunger - dt).max(0.0);
        if c.hunger <= 0.0 {
            c.health -= dt * 3.0;
            if c.health <= 0.0 {
                if let Some(p) = c.claimed_job() {
                    self.claimed[idx(p)] = false;
                }
                c.state = State::Dead;
                self.push_log(format!("{} starved to death.", c.name));
                return;
            }
        } else {
            c.health = (c.health + dt * 0.5).min(100.0);
        }

        if c.hunger < 30.0 && self.food > 0 && !matches!(c.state, State::Eating { .. }) {
            if let Some(p) = c.claimed_job() {
                self.claimed[idx(p)] = false;
            }
            self.food -= 1;
            c.state = State::Eating { t: 2.0 };
        }

        let state = std::mem::replace(&mut c.state, State::Idle { wait: 0.0 });
        c.state = match state {
            State::Idle { wait } => {
                let wait = wait - dt;
                if wait > 0.0 {
                    State::Idle { wait }
                } else if let Some((path, target, job)) = self.find_job(c.tile) {
                    self.claimed[idx(target)] = true;
                    State::Moving { path, job: Some((target, job)) }
                } else if rnd() < 0.3 {
                    State::Moving { path: self.wander_path(c.tile), job: None }
                } else {
                    State::Idle { wait: 0.5 }
                }
            }
            State::Moving { mut path, job } => {
                let cancelled = job.map_or(false, |(t, j)| self.desig[idx(t)] != Some(j));
                if cancelled {
                    State::Idle { wait: 0.2 }
                } else if let Some(&next) = path.last() {
                    if !self.tiles[idx(next)].passable() {
                        // Something got built or grew in the way; re-plan.
                        if let Some((t, _)) = job {
                            self.claimed[idx(t)] = false;
                        }
                        State::Idle { wait: 0.2 }
                    } else {
                        let (tx, ty) = (next.0 as f32, next.1 as f32);
                        let (dx, dy) = (tx - c.x, ty - c.y);
                        let d = (dx * dx + dy * dy).sqrt();
                        let step = MOVE_SPEED * dt;
                        if d <= step {
                            c.x = tx;
                            c.y = ty;
                            c.tile = next;
                            path.pop();
                        } else {
                            c.x += dx / d * step;
                            c.y += dy / d * step;
                        }
                        State::Moving { path, job }
                    }
                } else if let Some((target, job)) = job {
                    State::Working { target, job, progress: 0.0 }
                } else {
                    State::Idle { wait: 0.5 + rnd() * 2.0 }
                }
            }
            State::Working { target, job, progress } => {
                if self.desig[idx(target)] != Some(job) {
                    State::Idle { wait: 0.2 }
                } else {
                    let progress = progress + dt / job.work_time();
                    if progress >= 1.0 {
                        self.complete(target, job);
                        State::Idle { wait: 0.1 }
                    } else {
                        State::Working { target, job, progress }
                    }
                }
            }
            State::Eating { t } => {
                let t = t - dt;
                if t <= 0.0 {
                    c.hunger = (c.hunger + 60.0).min(100.0);
                    State::Idle { wait: 0.1 }
                } else {
                    State::Eating { t }
                }
            }
            State::Dead => State::Dead,
        };
    }

    fn complete(&mut self, p: Pos, job: Job) {
        let i = idx(p);
        self.desig[i] = None;
        self.claimed[i] = false;
        match job {
            Job::Chop => {
                self.tiles[i] = Tile::Grass;
                self.wood += 6;
            }
            Job::Mine => {
                self.stone += if self.tiles[i] == Tile::Rock { 4 } else { 2 };
                self.tiles[i] = Tile::Grass;
            }
            Job::Harvest => {
                self.tiles[i] = Tile::Grass;
                self.food += 3;
            }
            Job::Build => {
                if self.stone >= WALL_COST {
                    self.stone -= WALL_COST;
                    self.tiles[i] = Tile::Wall;
                } else if self.wood >= WALL_COST {
                    self.wood -= WALL_COST;
                    self.tiles[i] = Tile::Wall;
                } else {
                    self.push_log("Not enough materials to finish a wall.".into());
                }
            }
        }
    }

    // ---------- input ----------

    fn handle_input(&mut self) {
        if is_key_pressed(KeyCode::Space) {
            self.paused = !self.paused;
        }
        if is_key_pressed(KeyCode::Tab) {
            self.speed = match self.speed {
                1 => 2,
                2 => 4,
                _ => 1,
            };
        }
        let keys = [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5];
        for (k, tool) in keys.iter().zip(Tool::ALL) {
            if is_key_pressed(*k) {
                self.tool = tool;
            }
        }

        let (mx, my) = mouse_position();
        if let Some(p) = screen_to_tile(mx, my) {
            if is_mouse_button_down(MouseButton::Left) {
                self.apply_tool(p, self.tool);
            }
            if is_mouse_button_down(MouseButton::Right) {
                self.apply_tool(p, Tool::Cancel);
            }
        }
    }

    fn apply_tool(&mut self, p: Pos, tool: Tool) {
        let i = idx(p);
        match tool.job() {
            Some(job) => {
                if job.valid_on(self.tiles[i]) && self.desig[i] != Some(job) {
                    self.desig[i] = Some(job);
                    self.claimed[i] = false;
                }
            }
            None => {
                self.desig[i] = None;
                self.claimed[i] = false;
            }
        }
    }

    // ---------- rendering ----------

    fn draw(&self) {
        clear_background(BLACK);

        for y in 0..H {
            for x in 0..W {
                let i = idx((x, y));
                draw_tile(self.tiles[i], x, y);
                if let Some(j) = self.desig[i] {
                    let (sx, sy) = (x as f32 * TS, y as f32 * TS);
                    draw_rectangle_lines(sx + 1.0, sy + 1.0, TS - 2.0, TS - 2.0, 2.0, j.color());
                }
            }
        }

        for c in &self.colonists {
            let (cx, cy) = (c.x * TS + TS / 2.0, c.y * TS + TS / 2.0);
            if c.is_dead() {
                draw_line(cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0, 2.0, DARKGRAY);
                draw_line(cx - 5.0, cy + 5.0, cx + 5.0, cy - 5.0, 2.0, DARKGRAY);
                continue;
            }
            draw_circle(cx, cy, TS * 0.42, c.color);
            draw_circle_lines(cx, cy, TS * 0.42, 1.5, BLACK);
            let dims = measure_text(&c.name, None, 14, 1.0);
            draw_text(&c.name, cx - dims.width / 2.0, cy - TS * 0.6, 14.0, WHITE);
            if let State::Working { progress, .. } = c.state {
                draw_rectangle(cx - 8.0, cy + TS * 0.5, 16.0, 3.0, DARKGRAY);
                draw_rectangle(cx - 8.0, cy + TS * 0.5, 16.0 * progress, 3.0, GREEN);
            }
            if matches!(c.state, State::Eating { .. }) {
                draw_text("nom", cx + 6.0, cy + 10.0, 12.0, YELLOW);
            }
        }

        // Day/night tint over the map.
        let tod = (self.time % DAY_LEN) / DAY_LEN;
        let darkness = ((tod * std::f32::consts::TAU).cos() * 0.5 + 0.5) * 0.5;
        draw_rectangle(0.0, 0.0, W as f32 * TS, H as f32 * TS, Color::new(0.0, 0.0, 0.12, darkness));

        let (mx, my) = mouse_position();
        if let Some((x, y)) = screen_to_tile(mx, my) {
            draw_rectangle_lines(x as f32 * TS, y as f32 * TS, TS, TS, 1.0, WHITE);
        }

        self.draw_hud(tod);
    }

    fn draw_hud(&self, tod: f32) {
        let y0 = H as f32 * TS;
        draw_rectangle(0.0, y0, W as f32 * TS, HUD_H, Color::from_rgba(25, 25, 30, 255));

        let day = (self.time / DAY_LEN) as i32 + 1;
        let minutes = (tod * 24.0 * 60.0) as i32;
        let status = format!(
            "Day {}  {:02}:{:02}    Wood: {}   Stone: {}   Food: {}    Speed: {}x{}",
            day,
            minutes / 60,
            minutes % 60,
            self.wood,
            self.stone,
            self.food,
            self.speed,
            if self.paused { "  [PAUSED]" } else { "" }
        );
        draw_text(&status, 10.0, y0 + 22.0, 20.0, WHITE);

        let mut x = 10.0;
        for tool in Tool::ALL {
            let color = if tool == self.tool { GOLD } else { GRAY };
            let dims = draw_text(tool.label(), x, y0 + 46.0, 18.0, color);
            x += dims.width + 18.0;
        }
        draw_text(
            "LMB: designate   RMB: cancel   Space: pause   Tab: speed   R: restart",
            10.0,
            y0 + 66.0,
            16.0,
            LIGHTGRAY,
        );

        for (i, msg) in self.log.iter().enumerate() {
            draw_text(msg, 10.0, y0 + 88.0 + i as f32 * 14.0, 15.0, Color::from_rgba(200, 200, 160, 255));
        }

        let lx = W as f32 * TS - 330.0;
        for (i, c) in self.colonists.iter().enumerate() {
            let line = format!(
                "{:<5} HP {:>3.0}  Food {:>3.0}  {}",
                c.name,
                c.health.max(0.0),
                c.hunger,
                c.status()
            );
            let color = if c.is_dead() {
                DARKGRAY
            } else if c.hunger < 15.0 {
                RED
            } else {
                c.color
            };
            draw_text(&line, lx, y0 + 18.0 + i as f32 * 15.0, 15.0, color);
        }

        if self.colonists.iter().all(|c| c.is_dead()) {
            let msg = "Your colony has perished. Press R to start over.";
            let dims = measure_text(msg, None, 32, 1.0);
            draw_text(
                msg,
                (W as f32 * TS - dims.width) / 2.0,
                H as f32 * TS / 2.0,
                32.0,
                RED,
            );
        }
    }
}

fn screen_to_tile(mx: f32, my: f32) -> Option<Pos> {
    if mx < 0.0 || my < 0.0 {
        return None;
    }
    let (x, y) = ((mx / TS) as usize, (my / TS) as usize);
    if x < W && y < H {
        Some((x, y))
    } else {
        None
    }
}

fn draw_tile(t: Tile, x: usize, y: usize) {
    let (sx, sy) = (x as f32 * TS, y as f32 * TS);
    let v = ((x * 31 + y * 17) % 5) as u8 * 4;
    let grass = Color::from_rgba(78 + v, 118 + v, 62, 255);
    match t {
        Tile::Grass => draw_rectangle(sx, sy, TS, TS, grass),
        Tile::Tree => {
            draw_rectangle(sx, sy, TS, TS, grass);
            draw_rectangle(sx + TS * 0.44, sy + TS * 0.5, TS * 0.12, TS * 0.45, Color::from_rgba(100, 70, 40, 255));
            draw_circle(sx + TS / 2.0, sy + TS * 0.42, TS * 0.38, Color::from_rgba(30, 80, 35, 255));
        }
        Tile::Bush => {
            draw_rectangle(sx, sy, TS, TS, grass);
            draw_circle(sx + TS / 2.0, sy + TS / 2.0, TS * 0.32, Color::from_rgba(55, 110, 50, 255));
            for (ox, oy) in [(0.35, 0.4), (0.62, 0.45), (0.48, 0.65)] {
                draw_circle(sx + TS * ox, sy + TS * oy, 1.8, Color::from_rgba(200, 40, 60, 255));
            }
        }
        Tile::Rock => {
            let r = 105 + v;
            draw_rectangle(sx, sy, TS, TS, Color::from_rgba(r, r, r + 6, 255));
            draw_rectangle(sx + 3.0, sy + 3.0, TS * 0.35, TS * 0.25, Color::from_rgba(r + 20, r + 20, r + 26, 255));
        }
        Tile::Water => draw_rectangle(sx, sy, TS, TS, Color::from_rgba(40, 90 + v, 160 + v, 255)),
        Tile::Wall => {
            draw_rectangle(sx, sy, TS, TS, Color::from_rgba(120, 100, 80, 255));
            draw_rectangle_lines(sx, sy, TS, TS, 2.0, Color::from_rgba(70, 55, 40, 255));
        }
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "My World - a tiny colony sim".to_owned(),
        window_width: (W as f32 * TS) as i32,
        window_height: (H as f32 * TS + HUD_H) as i32,
        window_resizable: false,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(42);
    rand::srand(seed);

    let mut world = World::new();
    loop {
        if is_key_pressed(KeyCode::R) {
            world = World::new();
        }
        world.handle_input();
        if !world.paused {
            let dt = get_frame_time().min(0.05);
            for _ in 0..world.speed {
                world.update(dt);
            }
        }
        world.draw();
        next_frame().await;
    }
}
