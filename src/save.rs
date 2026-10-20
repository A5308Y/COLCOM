//! Save files: a format version around the world, and compact storage of the
//! big per-cell arrays (mostly long runs of identical values).
//!
//! There are no migrations: when the format changes, `SAVE_VERSION` goes up
//! and saves of any other version are refused with a clear message. Loading
//! a current save must not change the game (see `check` and
//! `World::load_json`, which only rebuild what's derived).

use crate::map::Pos;
use crate::world::World;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bump whenever the save format changes incompatibly.
pub const SAVE_VERSION: u32 = 16;

#[derive(Serialize)]
struct SaveOut<'a> {
    version: u32,
    world: &'a World,
}

#[derive(Deserialize)]
struct Header {
    version: Option<u32>,
}

#[derive(Deserialize)]
struct SaveIn {
    world: World,
}

pub fn to_json(world: &World) -> Result<String, String> {
    serde_json::to_string(&SaveOut { version: SAVE_VERSION, world }).map_err(|e| e.to_string())
}

pub fn from_json(s: &str) -> Result<World, String> {
    let header: Header = serde_json::from_str(s).map_err(|e| format!("not a save file ({})", e))?;
    match header.version {
        Some(SAVE_VERSION) => {}
        Some(v) if v > SAVE_VERSION => {
            return Err(format!("this save is from a newer version of My World (format {})", v));
        }
        Some(v) => return Err(format!("this save is from an older version of My World (format {})", v)),
        None => return Err("this save is from an older version of My World".into()),
    }
    let save: SaveIn = serde_json::from_str(s).map_err(|e| format!("the save file is damaged ({})", e))?;
    check(&save.world).map_err(|e| format!("the save file is damaged ({})", e))?;
    Ok(save.world)
}

/// Everything indexed by cell has one entry per cell, and what refers to
/// cells, stockpiles or the map stays inside them, so a damaged save is
/// refused here rather than panicking some ticks later.
fn check(w: &World) -> Result<(), String> {
    let m = &w.map;
    let n = m.w.checked_mul(m.h).and_then(|a| a.checked_mul(m.d)).filter(|&n| n > 0).ok_or("the map has no size")?;
    let cells = |name: &str, len: usize| {
        if len == n { Ok(()) } else { Err(format!("{} has {} cells, the map {}", name, len, n)) }
    };
    cells("block", m.block.len())?;
    cells("feature", m.feature.len())?;
    cells("items", m.items.len())?;
    cells("desig", m.desig.len())?;
    cells("zone", m.zone.len())?;
    cells("floor", m.floor.len())?;
    cells("blood", m.blood.len())?;
    cells("roof", m.roof.len())?;
    cells("res_target", w.res_target.len())?;
    cells("res_item", w.res_item.len())?;
    cells("res_cell", w.res_cell.len())?;
    cells("cell_temp", w.derived.cell_temp.len())?;
    if let Some(&i) = m.shelved.keys().chain(m.decor.keys()).find(|&&i| i as usize >= n) {
        return Err(format!("something is stored in cell {} outside the map", i));
    }
    if let Some(id) = m.zone.iter().filter_map(|z| z.stockpile()).find(|id| w.stockpile(*id).is_none()) {
        return Err(format!("a zone belongs to stockpile {:?}, which doesn't exist", id));
    }
    let inside = |p: Pos| p.0 < m.w && p.1 < m.h && p.2 < m.d;
    let positions = w.pawns.iter().map(|p| p.pos)
        .chain(w.benches.iter().map(|b| b.pos))
        .chain(w.moves.iter().flat_map(|mv| [mv.from, mv.to]));
    if let Some(p) = positions.into_iter().find(|&p| !inside(p)) {
        return Err(format!("{:?} is outside the map", p));
    }
    Ok(())
}

/// `rle` for per-cell temperatures, with NaN (outside rooms) as `null`:
/// JSON has no NaN.
pub mod rle_nan {
    use super::*;

    pub fn serialize<S: Serializer>(v: &[f32], s: S) -> Result<S::Ok, S::Error> {
        let values: Vec<Option<f32>> = v.iter().map(|&t| (!t.is_nan()).then_some(t)).collect();
        rle::serialize(&values, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f32>, D::Error> {
        let values: Vec<Option<f32>> = rle::deserialize(d)?;
        Ok(values.into_iter().map(|t| t.unwrap_or(f32::NAN)).collect())
    }
}

/// Run-length encoding for per-cell vectors: `[[count, value], ...]`.
/// Use with `#[serde(with = "crate::save::rle")]`.
pub mod rle {
    use super::*;

    pub fn serialize<S: Serializer, T: Serialize + PartialEq>(v: &[T], s: S) -> Result<S::Ok, S::Error> {
        let mut runs: Vec<(u32, &T)> = Vec::new();
        for x in v {
            match runs.last_mut() {
                Some((n, y)) if *y == x => *n += 1,
                _ => runs.push((1, x)),
            }
        }
        runs.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de> + Clone>(d: D) -> Result<Vec<T>, D::Error> {
        let runs: Vec<(u32, T)> = Vec::deserialize(d)?;
        let mut out = Vec::with_capacity(runs.iter().map(|r| r.0 as usize).sum());
        for (n, x) in runs {
            out.extend(std::iter::repeat_n(x, n as usize));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_are_compact_and_round_trip() {
        let w = World::new(42);
        let json = to_json(&w).unwrap();
        assert!(json.len() < 1_000_000, "save is {} bytes", json.len());
        let loaded = from_json(&json).unwrap();
        assert_eq!(loaded.map.block, w.map.block);
        assert_eq!(loaded.map.items, w.map.items);
        assert_eq!(loaded.pawns.len(), w.pawns.len());
    }

    #[test]
    fn cached_state_keeps_the_version_16_save_layout() {
        let mut w = World::with_map(crate::map::Map::flat(8, 8), crate::rng::Rng::new(42));
        let id = w.spawn_colonist((3, 3, crate::map::FLAT_Z));
        w.derived.cell_temp[0] = 21.5;
        w.derived.running.insert(7);
        w.derived.power.unpowered = 2;
        w.derived.trapped.push(id);
        w.derived.quarry_unreachable.push(3);
        w.derived.quarry_out_of_reach.insert(12);
        let json = to_json(&w).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["version"], 16);
        let fields = value["world"].as_object().unwrap();
        for name in ["cell_temp", "running", "power", "trapped", "quarry_unreachable", "quarry_out_of_reach"] {
            assert!(fields.contains_key(name), "{name} remains at the save's world level");
        }
        assert!(!fields.contains_key("derived"));
        assert!(!fields.contains_key("room_stats"), "rebuilt on load");
        let loaded = from_json(&json).unwrap();
        assert_eq!(loaded.derived.cell_temp[0], 21.5);
        assert_eq!(loaded.derived.running, w.derived.running);
        assert_eq!(loaded.derived.power, w.derived.power);
        assert_eq!(loaded.derived.trapped, w.derived.trapped);
        assert_eq!(loaded.derived.quarry_unreachable, w.derived.quarry_unreachable);
        assert_eq!(loaded.derived.quarry_out_of_reach, w.derived.quarry_out_of_reach);
        assert!(loaded.derived.room_stats.is_empty());
    }

    #[test]
    fn loading_restores_what_the_last_second_worked_out() {
        let mut w = World::new(42);
        // A walled, roofed room to be warm in.
        let z = (0..w.map.d).find(|&z| w.map.standable((10, 10, z))).unwrap();
        for x in 8..=12 {
            for y in 8..=12 {
                if x == 8 || x == 12 || y == 8 || y == 12 {
                    let i = w.map.idx((x, y, z));
                    w.map.set_feature(i, crate::map::Feature::built("wall"));
                }
                let r = w.map.idx((x, y, z + 1));
                w.map.set_roof(r, true);
            }
        }
        for _ in 0..(3.0 / crate::TICK) as usize {
            w.tick(crate::TICK);
        }
        let room = w.map.room[w.map.idx((10, 10, z))];
        assert!(room != 0, "the test room is a room");
        // As the last once-a-second pass left it: the room warmed up, a heater on.
        for i in 0..w.map.len() {
            if w.map.room[i] == room {
                w.derived.cell_temp[i] = 21.5;
            }
        }
        w.derived.running.insert(w.map.idx((10, 10, z)) as u32);
        let loaded = crate::world::World::load_json(&w.save_json().unwrap()).unwrap();
        let bits = |v: &[f32]| v.iter().map(|t| t.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&loaded.derived.cell_temp), bits(&w.derived.cell_temp), "rooms as warm as they were");
        assert_eq!(loaded.derived.running, w.derived.running);
        assert_eq!(loaded.derived.power, w.derived.power);
        assert_eq!(loaded.derived.trapped, w.derived.trapped);
        assert_eq!(loaded.derived.quarry_out_of_reach, w.derived.quarry_out_of_reach);
    }

    #[test]
    fn damaged_saves_are_refused_when_loading() {
        let w = World::new(42);
        let json = to_json(&w).unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        // A per-cell layer cut short (the RLE runs of `floor`): one cell missing.
        let runs = v["world"]["map"]["floor"].as_array_mut().unwrap();
        let last = runs.last_mut().unwrap();
        last[0] = (last[0].as_u64().unwrap() - 1).into();
        let err = from_json(&v.to_string()).err().expect("refused");
        assert!(err.contains("damaged") && err.contains("floor"), "{err}");
        // A pawn off the map.
        let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
        v["world"]["pawns"][0]["pos"] = serde_json::json!([9999, 0, 0]);
        assert!(from_json(&v.to_string()).err().expect("refused").contains("outside the map"));
        // The real thing loads.
        assert!(from_json(&json).is_ok());
    }

    #[test]
    fn priorities_save_as_before_and_reject_nonsense() {
        use crate::pawn::WorkPriority;
        use crate::production::StorePriority;
        assert_eq!(serde_json::to_string(&StorePriority::Low).unwrap(), "1");
        assert_eq!(serde_json::from_str::<StorePriority>("4").unwrap(), StorePriority::Critical);
        assert!(serde_json::from_str::<StorePriority>("0").is_err());
        assert!(serde_json::from_str::<StorePriority>("7").is_err(), "used to panic in the job scan");
        assert_eq!(serde_json::to_string(&WorkPriority::Never).unwrap(), "0");
        assert_eq!(serde_json::from_str::<WorkPriority>("3").unwrap(), WorkPriority::Third);
        assert!(serde_json::from_str::<WorkPriority>("5").is_err());
    }

    #[test]
    fn reservations_and_life_states_round_trip() {
        use crate::pawn::{Life, PawnId};
        assert_eq!(std::mem::size_of::<Option<PawnId>>(), 4, "a free cell costs no more than an id");
        let mut w = World::new(42);
        let id = w.pawns[0].id;
        w.res_target.set(7, Some(id));
        w.pawns[0].life = Life::Downed { time: 12.5 };
        let loaded = from_json(&to_json(&w).unwrap()).unwrap();
        assert_eq!(loaded.res_target, w.res_target);
        assert_eq!(loaded.pawns[0].life, Life::Downed { time: 12.5 });
    }

    #[test]
    fn old_and_newer_saves_are_refused_with_a_clear_message() {
        let w = World::new(1);
        let unversioned = serde_json::to_string(&w).unwrap();
        assert!(from_json(&unversioned).err().unwrap().contains("older version"));
        let newer = to_json(&w).unwrap().replacen(
            &format!("\"version\":{}", SAVE_VERSION),
            &format!("\"version\":{}", SAVE_VERSION + 1),
            1,
        );
        assert!(from_json(&newer).err().unwrap().contains("newer version"));
        assert!(from_json("hello").err().unwrap().contains("not a save file"));
    }

    #[test]
    fn loading_changes_nothing_that_was_saved() {
        let mut w = World::new(3);
        for _ in 0..(20.0 / crate::TICK) as usize {
            w.tick(crate::TICK);
        }
        let json = w.save_json().unwrap();
        let again = crate::world::World::load_json(&json).unwrap().save_json().unwrap();
        assert!(json == again, "a save loaded and saved again is the same save");
    }
}
