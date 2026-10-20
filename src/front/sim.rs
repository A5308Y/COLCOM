//! Owns the simulation and advances it in fixed steps.

use bevy::prelude::*;
use my_world::scenarios;
use my_world::world::World as SimWorld;
use my_world::TICK;

/// Game speeds: keys 1-4 pick one, the speed button cycles through them.
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
    /// While an expedition's fight is played, `world` is its site and the
    /// colony waits here (with the expedition's id).
    pub home: Option<(my_world::expeditions::ExpeditionId, SimWorld)>,
    acc: f32,
}

impl Sim {
    /// A game in `scenario`, or the last saved game, or a new colony.
    pub fn new(mut scenario: Option<String>) -> Sim {
        let world = start_world(&mut scenario, true);
        Sim { world, speed: 1, paused: false, generation: 0, scenario, home: None, acc: 0.0 }
    }

    /// A new game: the scenario again, or a new colony on a new map (never the save).
    pub fn fresh_world(&mut self) -> SimWorld {
        start_world(&mut self.scenario, false)
    }

    pub fn replace(&mut self, world: SimWorld) {
        self.world = world;
        self.home = None;
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

    /// The colony, also while a fight is played at an expedition's site.
    pub fn home_world(&self) -> &SimWorld {
        self.home.as_ref().map_or(&self.world, |h| &h.1)
    }

    /// The whole game to save: the colony, with a site being played put back into its expedition.
    pub fn saveable(&self) -> std::borrow::Cow<'_, SimWorld> {
        match &self.home {
            Some((id, home)) => {
                let mut home = home.clone();
                home.put_site(*id, self.world.clone());
                std::borrow::Cow::Owned(home)
            }
            None => std::borrow::Cow::Borrowed(&self.world),
        }
    }

    /// An expedition's fight starts: play its site. It's over: back to the colony.
    pub fn swap_sites(&mut self) {
        if self.home.is_some() {
            if self.world.battle.is_none() {
                let (id, home) = self.home.take().unwrap();
                let site = std::mem::replace(&mut self.world, home);
                self.world.finish_site(id, site);
                self.generation += 1;
                self.acc = 0.0;
            }
        } else if let Some((id, site)) = self.world.take_site() {
            let home = std::mem::replace(&mut self.world, site);
            self.home = Some((id, home));
            self.generation += 1;
            self.acc = 0.0;
        }
    }

    /// The game is over: no colonists, here or away (never during an expedition's fight).
    pub fn lost(&self) -> bool {
        self.home.is_none() && self.world.colony_lost()
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

/// The world of `scenario`, or (`resume`, when the game starts) the last
/// saved game, or a new colony. An unknown scenario is reported (with the
/// ones there are) and forgotten.
fn start_world(scenario: &mut Option<String>, resume: bool) -> SimWorld {
    if let Some(name) = scenario.as_deref() {
        if let Some(w) = scenarios::build(name) {
            return w;
        }
        let known: Vec<&str> = scenarios::SCENARIOS.iter().map(|s| s.0).collect();
        eprintln!("No scenario called '{}' (there are: {}); starting a new colony.", name, known.join(", "));
        *scenario = None;
    }
    // Carry on from the last game, unless asked for a new one (`--new`).
    // Tests always start fresh, whatever saves lie around.
    let fresh = !resume || cfg!(test) || std::env::args().any(|a| a == "--new");
    if !fresh {
        match super::input::read_newest_save() {
            Ok(Some((mut w, which))) => {
                w.push_log(format!("Continuing from {}.", which));
                return w;
            }
            Ok(None) => {}
            Err(e) => {
                let mut w = SimWorld::new(seed());
                w.push_log(format!("Couldn't continue the last game ({}); this is a new colony.", e));
                return w;
            }
        }
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
    sim.swap_sites();
    if sim.paused || sim.lost() {
        return;
    }
    sim.acc += time.delta_secs().min(0.25) * sim.rate() as f32;
    let mut steps = 0;
    while sim.acc >= TICK && steps < 40 {
        let fighting = sim.world.battle.is_some();
        sim.world.tick(TICK);
        sim.acc -= TICK;
        steps += 1;
        // After a battle the colony carries on at 1x, whatever speed it had before.
        if fighting && sim.world.battle.is_none() {
            sim.speed = 1;
            sim.acc = 0.0;
            break;
        }
    }
    if steps == 40 {
        sim.acc = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use my_world::expeditions::{MissionKind, Offer, Stage};
    use my_world::DAY_LEN;

    #[test]
    fn an_expeditions_fight_is_played_in_place_of_the_colony_and_saved_with_it() {
        let mut world = SimWorld::new(7);
        world.story.next_raid = f32::MAX;
        world.offers.push(Offer { id: my_world::expeditions::OfferId::new(500), kind: MissionKind::BanditCamp, faction: 2, days: 1.0, until: world.time() + DAY_LEN });
        let squad: Vec<my_world::pawn::PawnId> = world.pawns.iter().filter(|p| p.is_living_colonist()).take(2).map(|p| p.id).collect();
        world.send_expedition(my_world::expeditions::OfferId::new(500), &squad, &[]).unwrap();
        let id = world.expeditions[0].id;
        while !matches!(world.expeditions[0].stage, Stage::Decide { .. }) {
            world.tick(TICK);
        }
        world.choose(id, 0).unwrap();
        let home_w = world.map.w;
        let mut sim = Sim { world, speed: 1, paused: false, generation: 0, scenario: None, home: None, acc: 0.0 };
        sim.swap_sites();
        assert!(sim.home.is_some() && sim.generation == 1, "the site is played");
        assert!(sim.world.map.w < home_w && sim.world.battle.is_some());
        assert!(!sim.lost());
        // A save made now has the site in its expedition, and plays it again when loaded.
        let saved = SimWorld::load_json(&sim.saveable().save_json().unwrap()).unwrap();
        assert!(saved.expeditions[0].site.is_some());
        // The fight ends: back to the colony, with the squad in the expedition again.
        sim.world.battle = None;
        sim.swap_sites();
        assert!(sim.home.is_none() && sim.world.map.w == home_w);
        assert_eq!(sim.world.expeditions[0].members.len(), 2);
    }
}
