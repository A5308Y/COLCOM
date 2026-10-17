//! Simulation core for My World. Nothing in here touches Bevy, so the
//! whole colony can be simulated (and unit tested) without a window.

pub mod battle;
pub mod battle_ai;
pub mod combat;
pub mod defs;
pub mod graves;
pub mod health;
pub mod jobs;
pub mod map;
pub mod medical;
pub mod moving;
pub mod pathing;
pub mod pawn;
pub mod prioritize;
pub mod production;
pub mod rng;
pub mod rooms;
pub mod save;
pub mod scenarios;
pub mod squads;
pub mod story;
pub mod surgery;
pub mod world;

/// Game seconds in one in-game day.
pub const DAY_LEN: f32 = 240.0;
/// Length of one fixed simulation step, in game seconds.
pub const TICK: f32 = 1.0 / 30.0;
