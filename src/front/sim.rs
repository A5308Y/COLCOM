//! Owns the simulation and advances it in fixed steps.

use bevy::prelude::*;
use my_world::scenarios;
use my_world::world::World as SimWorld;
use my_world::TICK;

/// Game speeds Tab and the speed button cycle through.
pub const SPEEDS: [u32; 4] = [1, 2, 5, 10];

#[derive(Resource)]
pub struct Sim {
    pub world: SimWorld,
    /// Game seconds per real second: one of `SPEEDS`.
    pub speed: u32,
    pub paused: bool,
    /// Bumped whenever `world` is replaced (new game, load), so visuals rebuild.
    pub generation: u64,
    /// The scenario the game started in (`--scenario <name>`); Shift+N restarts it.
    pub scenario: Option<String>,
    acc: f32,
}

impl Sim {
    /// A game in `scenario`, or a new colony if there's none (or no such scenario).
    pub fn new(mut scenario: Option<String>) -> Sim {
        let world = start_world(&mut scenario);
        Sim { world, speed: 1, paused: false, generation: 0, scenario, acc: 0.0 }
    }

    /// The scenario's world again, or a new colony.
    pub fn fresh_world(&mut self) -> SimWorld {
        start_world(&mut self.scenario)
    }

    pub fn replace(&mut self, world: SimWorld) {
        self.world = world;
        self.generation += 1;
        self.acc = 0.0;
    }

    /// Game speed in effect: battles always play at 1x, so moves and shots can be followed.
    pub fn rate(&self) -> u32 {
        if self.world.battle.is_some() {
            1
        } else {
            self.speed
        }
    }

    pub fn cycle_speed(&mut self) {
        let next = SPEEDS.iter().position(|&s| s == self.speed).map_or(0, |k| (k + 1) % SPEEDS.len());
        self.speed = SPEEDS[next];
    }
}

pub fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(42)
}

/// The world of `scenario`, or a new colony. An unknown scenario is
/// reported (with the ones there are) and forgotten.
fn start_world(scenario: &mut Option<String>) -> SimWorld {
    if let Some(name) = scenario.as_deref() {
        if let Some(w) = scenarios::build(name) {
            return w;
        }
        let known: Vec<&str> = scenarios::SCENARIOS.iter().map(|s| s.0).collect();
        eprintln!("No scenario called '{}' (there are: {}); starting a new colony.", name, known.join(", "));
        *scenario = None;
    }
    SimWorld::new(seed())
}

/// The scenario asked for with `--scenario <name>` or `MY_WORLD_SCENARIO`.
fn scenario_option() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let arg = args.iter().position(|a| a == "--scenario").and_then(|k| args.get(k + 1)).cloned();
    arg.or_else(|| std::env::var("MY_WORLD_SCENARIO").ok()).filter(|s| !s.is_empty())
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Sim::new(scenario_option()))
            .add_systems(Update, step_simulation);
    }
}

fn step_simulation(time: Res<Time>, mut sim: ResMut<Sim>) {
    let sim = &mut *sim;
    if sim.paused || sim.world.colony_lost() {
        return;
    }
    sim.acc += time.delta_secs().min(0.25) * sim.rate() as f32;
    let mut steps = 0;
    while sim.acc >= TICK && steps < 40 {
        sim.world.tick(TICK);
        sim.acc -= TICK;
        steps += 1;
    }
    if steps == 40 {
        sim.acc = 0.0;
    }
}
