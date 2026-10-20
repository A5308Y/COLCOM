//! Save files: a format version around the world, and compact storage of the
//! big per-cell arrays (mostly long runs of identical values).

use crate::map::{Desig, Pos};
use crate::world::World;
use crate::DAY_LEN;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bump whenever the save format changes incompatibly.
pub const SAVE_VERSION: u32 = 14;

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
    let mut save: SaveIn = serde_json::from_str(s).map_err(|e| format!("the save file is damaged ({})", e))?;
    let m = &mut save.world.map;
    // Saves from before roofs have none.
    if m.roof.len() != m.len() {
        m.roof = vec![false; m.len()];
    }
    // Mine used to take down structures and floors too; that's Deconstruct now.
    for i in 0..m.len() {
        let p = m.pos(i);
        if m.desig[i] == Some(Desig::Mine) && !m.mineable(p) && m.deconstructable(p) {
            m.desig[i] = Some(Desig::Deconstruct);
        }
    }
    // Days used to be shorter: keep the day, hour and timers where they were.
    let w = &mut save.world;
    if w.day_len != DAY_LEN {
        let k = DAY_LEN / w.day_len;
        w.set_time(w.time() * k);
        w.story.next_raid *= k;
        w.story.next_event *= k;
        w.day_len = DAY_LEN;
    }
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
        w.res_target[7] = Some(id);
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
    fn saves_from_shorter_days_keep_their_day_and_hour() {
        let mut w = World::new(5);
        w.set_time(240.0 * 4.5); // noon on day 5 when days were 240 s
        w.day_len = 240.0;
        w.story.next_raid = 240.0 * 6.0;
        let json = to_json(&w).unwrap().replacen("\"day_len\":240.0,", "", 1);
        assert!(!json.contains("day_len"), "an old save has no day length");
        let loaded = from_json(&json).unwrap();
        assert_eq!(loaded.day(), 5);
        assert!((loaded.hour() - 12.0).abs() < 0.01);
        assert_eq!(loaded.story.next_raid, 6.0 * DAY_LEN);
        assert_eq!(loaded.day_len, DAY_LEN);
    }

    #[test]
    fn mine_orders_on_structures_load_as_deconstruct() {
        let mut w = World::new(3);
        let (x, y) = (w.map.w / 2, w.map.h / 2);
        let z = w.map.surface(x, y).unwrap();
        let wall = w.map.idx((x, y, z));
        w.map.set_feature(wall, crate::map::Feature::built("wall"));
        w.map.desig[wall] = Some(Desig::Mine);
        let rock = w.map.idx((x, y, z - 1));
        w.map.desig[rock] = Some(Desig::Mine);
        let loaded = from_json(&to_json(&w).unwrap()).unwrap();
        assert_eq!(loaded.map.desig[wall], Some(Desig::Deconstruct));
        assert_eq!(loaded.map.desig[rock], Some(Desig::Mine));
    }
}
