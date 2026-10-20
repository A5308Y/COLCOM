//! Prepared worlds to start the game in, for trying things out by hand
//! (`cargo run --release -- --scenario battle`). Each one is built the same
//! way every time: a fixed map, fixed colonists and a fixed rng seed.

use crate::battle::Parley;
use crate::defs::ItemId;
use crate::map::{Block, Feature, Map, Pos, Stack, Zone, FLAT_Z as Z};
use crate::pawn::{Role, Skill};
use crate::production::Stockpile;
use crate::rng::Rng;
use crate::world::World;

/// Scenario names and what they set up.
pub const SCENARIOS: [(&str, &str); 1] = [("battle", "Squads against two raider groups in a walled yard")];

/// The scenario called `name`, ready to play; `None` if there's no such scenario.
pub fn build(name: &str) -> Option<World> {
    match name {
        "battle" => Some(battle_arena()),
        _ => None,
    }
}

const ARENA_W: usize = 48;
const ARENA_H: usize = 32;
/// The house: outer walls from `HOUSE.0` to `HOUSE.1` (x, y), inclusive.
const HOUSE: ((usize, usize), (usize, usize)) = ((3, 12), (9, 17));
/// Where the raiders come in, on the east edge.
pub const RAID_ENTRY: Pos = (ARENA_W - 1, ARENA_H / 2, Z);

fn item(id: &str) -> ItemId {
    ItemId::named(id)
}

fn build_at(w: &mut World, x: usize, y: usize, id: &str) {
    let i = w.map.idx((x, y, Z));
    w.map.set_feature(i, Feature::built(id));
}

/// A colonist with set skills and gear, fed, rested and content.
fn colonist(w: &mut World, at: (usize, usize), shooting: u8, melee: u8, weapon: &str) -> u32 {
    let id = w.spawn_colonist((at.0, at.1, Z));
    let i = w.pawn_index(id).unwrap();
    let p = &mut w.pawns[i];
    p.skills = [4; Skill::ALL.len()];
    p.skills[Skill::Shooting as usize] = shooting;
    p.skills[Skill::Melee as usize] = melee;
    p.weapon = Some(item(weapon));
    p.wear(item("clothes"));
    p.hunger = 100.0;
    p.rest = 100.0;
    p.mood = 60.0;
    id
}

/// A 48×32 yard: the colony's house and stockpile in the west, a broken
/// wall, a fence, a rock ledge and trees between it and the raiders, who
/// come in from the east. The battle is on: round 1, the colony's turn.
fn battle_arena() -> World {
    let mut w = World::with_map(Map::flat(ARENA_W, ARENA_H), Rng::new(7));
    w.story.next_raid = f32::MAX;
    w.story.next_event = f32::MAX;
    w.regrow_timer = f32::MAX;

    // The house: walls with a door and a window, a roof, a bed for the
    // leader and enough furniture for a decent room. Inside is the safe zone.
    let ((x0, y0), (x1, y1)) = HOUSE;
    for y in y0..=y1 {
        for x in x0..=x1 {
            if x == x0 || x == x1 || y == y0 || y == y1 {
                build_at(&mut w, x, y, "wall");
            } else {
                let roof = w.map.idx((x, y, Z + 1));
                w.map.set_roof(roof, true);
            }
        }
    }
    build_at(&mut w, x1, 14, "door");
    build_at(&mut w, 6, y0, "window");
    for (x, y, f) in [(4, 13, "bed"), (5, 13, "lamp"), (4, 16, "table"), (5, 16, "chair")] {
        build_at(&mut w, x, y, f);
    }
    for y in 13..=16 {
        for x in 6..=8 {
            let i = w.map.idx((x, y, Z));
            w.map.zone[i] = Zone::Safe;
        }
    }

    // The stockpile south of the house, with gear to fight in.
    w.stockpiles.push(Stockpile::new(1));
    for y in 19..=21 {
        for x in 3..=9 {
            let i = w.map.idx((x, y, Z));
            w.map.zone[i] = Zone::Stockpile(0);
        }
    }
    for (x, kind, count) in [(4, "rifle", 2), (5, "steel_armor", 1), (6, "knife", 1), (7, "steel", 30)] {
        w.map.drop_item((x, 19, Z), Stack { kind: item(kind), count });
    }

    // Cover in the middle field: a broken wall (full cover) and a fence (half).
    for y in 6..=26 {
        if y % 5 != 0 {
            build_at(&mut w, 22, y, "wall");
        }
        if y % 4 != 1 {
            build_at(&mut w, 31, y, "fence");
        }
    }
    // A rock ledge two levels high, with a one-level step up on its west side.
    for y in 2..=6 {
        for x in 24..=27 {
            for z in Z..=Z + 1 {
                let i = w.map.idx((x, y, z));
                w.map.set_block(i, Block::Rock);
            }
        }
    }
    let step = w.map.idx((23, 4, Z));
    w.map.set_block(step, Block::Rock);
    // Trees in the east: half cover for the raiders.
    for (x, y) in [(36, 6), (38, 12), (35, 19), (40, 23), (37, 27), (42, 15)] {
        let i = w.map.idx((x, y, Z));
        w.map.set_feature(i, Feature::plant("tree"));
    }
    w.map.recompute_rooms();
    w.update_room_stats();

    // The colony: Ada leads a squad of three, one of them 15 cells away;
    // two lone fighters (one geared up from the stockpile), one civilian.
    let ada = colonist(&mut w, (12, 15), 10, 6, "rifle");
    w.set_role(ada, Role::Leader).unwrap();
    let i = w.pawn_index(ada).unwrap();
    w.pawns[i].bed = Some((4, 13, Z));
    for (at, weapon) in [((12, 14), "assault_rifle"), ((12, 16), "pistol"), ((12, 30), "rifle")] {
        let id = colonist(&mut w, at, 7, 5, weapon);
        w.set_role(id, Role::Member(ada)).unwrap();
        if weapon == "pistol" {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].wear(item("parka"));
            w.pawns[i].wear(item("wool_hat"));
        }
    }
    let powered = colonist(&mut w, (14, 9), 8, 4, "laser_rifle");
    let pi = w.pawn_index(powered).unwrap();
    w.pawns[pi].wear(item("powered_armor"));
    let geared = colonist(&mut w, (14, 21), 6, 6, "knife");
    w.set_loadout(geared, &[item("rifle"), item("steel_armor")]).unwrap();
    // Already fetched from the stockpile (the knife put down there).
    let gi = w.pawn_index(geared).unwrap();
    for kind in [item("rifle"), item("steel_armor")] {
        let Some(cell) = (0..w.map.len()).find(|&c| w.map.count_in(c, kind) > 0) else { continue };
        w.map.take_item(cell, kind, 1);
        if let Some(old) = w.pawns[gi].wear(kind) {
            w.map.drop_item(w.map.pos(cell), Stack { kind: old, count: 1 });
        }
    }
    w.pawns[gi].wear(item("steel_helmet"));
    let civilian = colonist(&mut w, (11, 19), 2, 2, "knife");
    w.set_role(civilian, Role::Civilian).unwrap();

    // Keep a spare of every weapon, apparel item and medicine on separate
    // stockpile cells for inspecting models and trying different loadouts.
    // Read definitions so newly added combat gear is included automatically.
    for kind in ItemId::all().filter(|k| k.def().is_gear() || k.def().medicine > 0.0) {
        if w.map.count_items(kind) > 0 {
            continue;
        }
        let cell = (0..w.map.len())
            .find(|&i| w.map.zone[i] == Zone::Stockpile(0) && !w.map.has_items(i))
            .expect("the battle stockpile has room for every combat item");
        w.map.drop_item(w.map.pos(cell), Stack { kind, count: 1 });
    }

    // The raiders: a squad of four and a pair, armed the same way every time.
    w.start_battle(Parley { squads: vec![4, 2], entry: RAID_ENTRY, demand: vec![Stack { kind: item("steel"), count: 10 }] });
    let raiders: Vec<usize> = (0..w.pawns.len()).filter(|&i| !w.pawns[i].is_colonist()).collect();
    for (&i, weapon) in raiders.iter().zip(["rifle", "rifle", "pistol", "club", "rifle", "knife"]) {
        w.pawns[i].weapon = Some(item(weapon));
    }
    w.log.clear();
    w.push_log("Scenario: battle. Raiders attack from the east.".into());
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pawn::Faction;
    use crate::TICK;

    fn battle_world() -> World {
        build("battle").expect("the battle scenario exists")
    }

    #[test]
    fn every_listed_scenario_builds() {
        for (name, _) in SCENARIOS {
            assert!(build(name).is_some(), "{} builds", name);
        }
        assert!(build("nonsense").is_none());
    }

    #[test]
    fn the_battle_starts_on_the_colonys_turn() {
        let w = battle_world();
        let b = w.battle.as_ref().expect("a battle is on");
        assert!(w.parley.is_none(), "no parley to answer");
        assert_eq!(b.round, 1);
        assert_eq!(b.side_to_act, Faction::Colony);
        let raiders: Vec<usize> = b.units.iter().filter(|u| u.side == Faction::Raider).map(|u| u.members.len()).collect();
        assert_eq!(raiders, vec![4, 2]);
        // It's the same every time.
        let again = battle_world();
        let pos = |w: &World| w.pawns.iter().map(|p| (p.pos, p.weapon)).collect::<Vec<_>>();
        assert_eq!(pos(&w), pos(&again));
    }

    #[test]
    fn the_colony_fights_as_described() {
        let w = battle_world();
        let b = w.battle.as_ref().unwrap();
        let ada = w.pawns.iter().find(|p| p.name == "Ada").unwrap();
        assert_eq!(ada.role, Role::Leader);
        assert!(w.has_proper_quarters(ada), "Ada has a decent bedroom");
        let squad = &b.units[b.unit_of(ada.id).unwrap()];
        assert!(squad.led);
        assert_eq!(squad.members.len(), 3, "Ada and the two beside her");
        assert_eq!(b.arrivals.len(), 1, "one member is on the way");
        assert_eq!(b.arrivals[0].rounds, 2);

        let colony: Vec<_> = b.units.iter().filter(|u| u.side == Faction::Colony).collect();
        assert_eq!(colony.iter().filter(|u| !u.led && !u.passive).count(), 2, "two lone fighters");
        let civilians: Vec<_> = colony.iter().filter(|u| u.passive).collect();
        assert_eq!(civilians.len(), 1);
        let c = w.pawn(civilians[0].members[0]).unwrap();
        assert_eq!(w.map.zone[w.map.idx(c.pos)], Zone::Safe, "the civilian shelters in the house");
        assert!(w.map.roofed(c.pos));

        // The fighter with a loadout took a rifle and the armour from the stockpile.
        let geared = w.pawns.iter().find(|p| !p.loadout.is_empty()).unwrap();
        assert_eq!(geared.weapon, Some(item("rifle")));
        assert!(geared.apparel.contains(&item("steel_armor")));
        assert_eq!(w.map.count_items(item("rifle")), 1);
        assert_eq!(w.map.count_items(item("steel_armor")), 1, "a spare remains for inspection");
    }

    #[test]
    fn every_combat_item_is_available_and_fighting_gear_is_equipped() {
        let w = battle_world();
        for kind in ItemId::all() {
            let def = kind.def();
            if def.is_gear() || def.medicine > 0.0 {
                let stocked: u32 = (0..w.map.len())
                    .filter(|&i| w.map.zone[i] == Zone::Stockpile(0))
                    .map(|i| w.map.count_in(i, kind))
                    .sum();
                assert!(stocked >= 1, "{} has a spare in the stockpile", def.id);
            }
            if def.is_gear() {
                assert!(w.pawns.iter().any(|p| p.role != Role::Civilian && p.has_on(kind)), "{} appears on a fighter", def.id);
            }
        }
    }

    #[test]
    fn the_battle_plays_to_an_end() {
        let mut w = battle_world();
        w.battle.as_mut().unwrap().auto_colony = true;
        for _ in 0..(1800.0 / TICK) as usize {
            if w.battle.is_none() {
                break;
            }
            w.tick(TICK);
        }
        assert!(w.battle.is_none(), "over in time");
    }
}
