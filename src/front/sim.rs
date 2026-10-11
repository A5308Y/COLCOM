//! Owns the simulation and advances it in fixed steps.

use bevy::prelude::*;
use my_world::world::World as SimWorld;
use my_world::TICK;

#[derive(Resource)]
pub struct Sim {
    pub world: SimWorld,
    /// 1, 2 or 3 simulation steps per real step.
    pub speed: u32,
    pub paused: bool,
    /// Bumped whenever `world` is replaced (new game, load), so visuals rebuild.
    pub generation: u64,
    acc: f32,
}

impl Sim {
    pub fn replace(&mut self, world: SimWorld) {
        self.world = world;
        self.generation += 1;
        self.acc = 0.0;
    }

    pub fn cycle_speed(&mut self) {
        self.speed = match self.speed {
            1 => 2,
            2 => 3,
            _ => 1,
        };
    }
}

pub fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(42)
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Sim { world: SimWorld::new(seed()), speed: 1, paused: false, generation: 0, acc: 0.0 })
            .add_systems(Update, step_simulation);
    }
}

fn step_simulation(time: Res<Time>, mut sim: ResMut<Sim>) {
    let sim = &mut *sim;
    if sim.paused || sim.world.colony_lost() {
        return;
    }
    sim.acc += time.delta_secs().min(0.25) * sim.speed as f32;
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
