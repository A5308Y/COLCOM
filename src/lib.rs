//! Simulation core for My World. Nothing in here touches macroquad, so the
//! whole colony can be simulated (and unit tested) without a window.

pub mod combat;
pub mod jobs;
pub mod map;
pub mod pathing;
pub mod pawn;
pub mod rng;
pub mod story;
pub mod world;

/// Game seconds in one in-game day.
pub const DAY_LEN: f32 = 240.0;
/// Length of one fixed simulation step, in game seconds.
pub const TICK: f32 = 1.0 / 30.0;
