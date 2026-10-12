//! Save files: a format version around the world, and compact storage of the
//! big per-cell arrays (mostly long runs of identical values).

use crate::world::World;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bump whenever the save format changes incompatibly.
pub const SAVE_VERSION: u32 = 4;

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
    Ok(save.world)
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
}
