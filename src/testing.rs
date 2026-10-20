//! Shared helpers for the tests: a quiet world, colonists, running time.

use crate::defs::ItemId;
use crate::map::{Feature, Map, Pos};
use crate::pawn::PawnId;
use crate::rng::Rng;
use crate::world::World;
use crate::TICK;

/// A flat `w`×`h` map where nothing happens by itself: no raids, no random
/// events, no wild plants growing back.
pub fn quiet_world(w: usize, h: usize, seed: u64) -> World {
    let mut world = World::with_map(Map::flat(w, h), Rng::new(seed));
    world.story.next_raid = f32::MAX;
    world.story.next_event = f32::MAX;
    world.regrow_timer = f32::MAX;
    world
}

/// A fed, rested and entertained colonist at `at`, with no fixed hours (any
/// hour for anything), so tests time only the work.
pub fn colonist(w: &mut World, at: Pos) -> PawnId {
    let id = w.spawn_colonist(at);
    let i = w.pawn_index(id).unwrap();
    w.pawns[i].hunger = 100.0;
    w.pawns[i].rest = 100.0;
    w.pawns[i].joy = 100.0;
    w.pawns[i].schedule = [crate::recreation::Block::Anything; 24];
    id
}

/// A fed and rested colonist in a steady mood (no mental breaks, no mood
/// bonus to work speed), for tests that time things.
pub fn content_colonist(w: &mut World, at: Pos) -> PawnId {
    let id = colonist(w, at);
    let i = w.pawn_index(id).unwrap();
    w.pawns[i].mood = 60.0;
    id
}

/// Let `secs` seconds of game time pass.
pub fn run(w: &mut World, secs: f32) {
    for _ in 0..(secs / TICK) as usize {
        w.tick(TICK);
    }
}

pub fn item(id: &str) -> ItemId {
    ItemId::named(id)
}

/// A finished structure of `id` at `at`.
pub fn build(w: &mut World, at: Pos, id: &str) {
    let i = w.map.idx(at);
    w.map.set_feature(i, Feature::built(id));
}
