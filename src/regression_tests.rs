//! Whole-simulation regressions. Kept under one filter for runtime comparisons.

use crate::map::Desig;
use crate::pawn::Job;
use crate::world::{Order, World};
use crate::TICK;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn colony(seed: u64) -> World {
    let mut w = World::new(seed);
    // Put the generated map to work, exercising jobs, reservations and RNG.
    let trees: Vec<_> = (0..w.map.len())
        .filter(|&i| w.map.feature[i].harvest_desig() == Some(Desig::Chop))
        .take(20)
        .map(|i| w.map.pos(i))
        .collect();
    for at in trees {
        w.apply_order(at, Order::Desig(Desig::Chop));
    }
    w
}

fn battle() -> World {
    let mut w = crate::scenarios::build("battle").unwrap();
    w.battle.as_mut().unwrap().auto_colony = true;
    w
}

fn first_difference(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
    use serde_json::Value;
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                match (a.get(key), b.get(key)) {
                    (Some(a), Some(b)) => {
                        if let Some(diff) = first_difference(a, b, &format!("{path}.{key}")) { return Some(diff); }
                    }
                    (a, b) => return Some(format!("{path}.{key}: {a:?} != {b:?}")),
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a.iter().zip(b).enumerate()
            .find_map(|(i, (a, b))| first_difference(a, b, &format!("{path}[{i}]"))),
        (a, b) if a == b => None,
        (a, b) => Some(format!("{path}: {a} != {b}")),
    }
}

// Saves contain the RNG, timers, paths, reservations and pending battle strikes.
// Compare complete saves as well as hashing them; no golden hash needs updating
// when an intentional game rule or save format changes.
fn world_difference(a: &World, b: &World, context: &str) -> Option<String> {
    let (a, b) = (a.save_json().unwrap(), b.save_json().unwrap());
    if a == b { return None; }
    let diff = first_difference(&serde_json::from_str(&a).unwrap(), &serde_json::from_str(&b).unwrap(), "save");
    let diff = diff.unwrap_or_else(|| {
        let at = a.bytes().zip(b.bytes()).position(|(a, b)| a != b).unwrap_or(a.len().min(b.len()));
        let snippet = |s: &str| String::from_utf8_lossy(&s.as_bytes()[at.saturating_sub(30)..(at + 60).min(s.len())]).into_owned();
        format!("JSON byte {at}: {:?} != {:?}", snippet(&a), snippet(&b))
    });
    Some(format!("{context}: {diff}"))
}

fn assert_same_world(a: &World, b: &World, context: &str) {
    let hash = |w: &World| {
        let mut h = DefaultHasher::new();
        w.save_json().unwrap().hash(&mut h);
        h.finish()
    };
    if let Some(diff) = world_difference(a, b, context) { panic!("{diff}"); }
    assert_eq!(hash(a), hash(b), "{context}: world hashes differ");
}

fn check_determinism(make: impl Fn() -> World) {
    let (mut a, mut b) = (make(), make());
    assert_same_world(&a, &b, "initial state");
    for tick in 1..=600 {
        a.tick(TICK);
        b.tick(TICK);
        if tick % 120 == 0 {
            assert_same_world(&a, &b, &format!("after tick {tick}"));
        }
    }
}

#[test]
fn seeded_colonies_are_deterministic() {
    for seed in [3, 11] {
        check_determinism(|| colony(seed));
    }
}

#[test]
fn automated_battles_are_deterministic() {
    check_determinism(battle);
}

fn check_continuation(mut uninterrupted: World, checkpoint: impl Fn(&World) -> bool) -> Option<String> {
    // Save between one-second passes, while a job or battle action is in flight.
    let mut saved_at = None;
    for tick in 1..=600 {
        uninterrupted.tick(TICK);
        if tick > 30 && checkpoint(&uninterrupted) {
            saved_at = Some(tick);
            break;
        }
    }
    let saved_at = saved_at.expect("fixture must reach an in-flight action");
    let mut loaded = World::load_json(&uninterrupted.save_json().unwrap()).unwrap();
    let mut first_mismatch = world_difference(&uninterrupted, &loaded, "immediately after loading");
    // Drain the transient audio queue as the front end does; sounds are not saved.
    uninterrupted.take_sounds();
    loaded.take_sounds();
    for tick in 1..=600 {
        uninterrupted.tick(TICK);
        loaded.tick(TICK);
        if uninterrupted.take_sounds() != loaded.take_sounds() && first_mismatch.is_none() {
            first_mismatch = Some(format!("sounds after continuation tick {tick}"));
        }
        if tick == 1 || tick % 120 == 0 {
            let mismatch = world_difference(&uninterrupted, &loaded, &format!("saved at {saved_at}, continued {tick} ticks"));
            if first_mismatch.is_none() { first_mismatch = mismatch; }
        }
    }
    // Finish the intended workload before asserting, so the benchmark measures
    // the same number of ticks even when an early comparison catches a bug.
    first_mismatch
}

#[test]
fn colony_save_and_continue_matches_uninterrupted_work() {
    let mismatches: Vec<_> = [3, 11].into_iter().filter_map(|seed| {
        check_continuation(colony(seed), |w| {
            w.colony_timer > 0.1 && w.colony_timer < 0.9
                && w.pawns.iter().any(|p| matches!(p.job, Some(Job::Haul { .. } | Job::Work { .. })))
        }).map(|diff| format!("seed {seed}: {diff}"))
    }).collect();
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn battle_save_and_continue_matches_uninterrupted_actions() {
    let mismatch = check_continuation(battle(), |w| {
        w.battle.as_ref().is_some_and(|b| !b.moving.is_empty() || !b.strikes.is_empty())
    });
    assert!(mismatch.is_none(), "{}", mismatch.unwrap());
}
