//! Squads, roles and combat loadouts, set up during normal play for the
//! turn-based battles (`battle.rs`). A squad is a leader and the colonists
//! whose role is `Member(leader)`; they fight as one unit.

use crate::defs::{ItemId, Slot};
use crate::pawn::{Pawn, Role};
use crate::world::World;

/// Most colonists in one squad, the leader included.
pub const SQUAD_MAX: usize = 5;
/// Least bedroom quality tier a leader is content with (2 = Decent).
pub const LEADER_ROOM_TIER: u8 = 2;

/// The slot a weapon or piece of apparel takes in a loadout (`None`: the weapon).
fn loadout_slot(kind: ItemId) -> Option<Option<Slot>> {
    let d = kind.def();
    match (&d.weapon, &d.apparel) {
        (Some(_), _) => Some(None),
        (None, Some(a)) => Some(Some(a.slot)),
        _ => None,
    }
}

impl World {
    /// The members of `leader`'s squad (not the leader).
    pub fn squad_members(&self, leader: u32) -> Vec<u32> {
        self.pawns.iter().filter(|p| p.role == Role::Member(leader) && !p.dead).map(|p| p.id).collect()
    }

    /// Give colonist `id` a role. Joining a squad needs a living leader with
    /// room in it; a leader stepping down releases their members.
    pub fn set_role(&mut self, id: u32, role: Role) -> Result<(), String> {
        let p = self.pawn(id).ok_or("no such colonist")?;
        if !p.is_colonist() || p.dead {
            return Err("only living colonists have roles".into());
        }
        if let Role::Member(leader) = role {
            let l = self.pawn(leader).ok_or("no such leader")?;
            if leader == id || l.role != Role::Leader || l.dead {
                return Err(format!("{} doesn't lead a squad", l.name));
            }
            if p.role != role && 1 + self.squad_members(leader).len() >= SQUAD_MAX {
                return Err(format!("{}'s squad is full ({} at most)", l.name, SQUAD_MAX));
            }
        }
        let was_leader = p.role == Role::Leader;
        let i = self.pawn_index(id).unwrap();
        self.pawns[i].role = role;
        if was_leader && role != Role::Leader {
            self.release_squad(id);
        }
        Ok(())
    }

    fn release_squad(&mut self, leader: u32) {
        for p in self.pawns.iter_mut().filter(|p| p.role == Role::Member(leader)) {
            p.role = Role::Fighter;
        }
    }

    /// Set colonist `id`'s combat loadout: weapons and apparel only, at most
    /// one weapon and one piece per apparel slot (later ones win).
    pub fn set_loadout(&mut self, id: u32, items: &[ItemId]) {
        let mut out: Vec<ItemId> = Vec::new();
        for &k in items {
            let Some(slot) = loadout_slot(k) else { continue };
            out.retain(|&o| loadout_slot(o) != Some(slot));
            out.push(k);
        }
        if let Some(i) = self.pawn_index(id) {
            self.pawns[i].loadout = out;
        }
    }

    /// Once a second: squads whose leader is gone break up, and leaders
    /// check their quarters.
    pub(crate) fn update_squads(&mut self) {
        let gone: Vec<u32> = self
            .pawns
            .iter()
            .filter_map(|p| match p.role {
                Role::Member(l) if self.pawn(l).is_none_or(|l| l.dead || l.left || l.role != Role::Leader) => Some(l),
                _ => None,
            })
            .collect();
        for l in gone {
            self.release_squad(l);
        }
        let quarters: Vec<(usize, bool)> = (0..self.pawns.len())
            .filter(|&i| self.pawns[i].role == Role::Leader)
            .map(|i| (i, self.has_proper_quarters(&self.pawns[i])))
            .collect();
        for p in self.pawns.iter_mut() {
            p.poor_quarters = false;
        }
        for (i, ok) in quarters {
            self.pawns[i].poor_quarters = !ok;
        }
    }

    /// A bed of their own in a room of at least `LEADER_ROOM_TIER` that
    /// nobody else sleeps in.
    pub fn has_proper_quarters(&self, p: &Pawn) -> bool {
        let Some(bed) = p.bed else { return false };
        let m = &self.map;
        let room = m.room.get(m.idx(bed)).copied().unwrap_or(0);
        if room == 0 || self.room_at(bed).is_none_or(|r| r.tier() < LEADER_ROOM_TIER) {
            return false;
        }
        !self.pawns.iter().any(|o| {
            o.id != p.id && !o.dead && o.bed.is_some_and(|b| m.room.get(m.idx(b)).copied() == Some(room))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Feature, Map, FLAT_Z as Z};
    use crate::rng::Rng;

    fn world() -> (World, Vec<u32>) {
        let mut w = World::with_map(Map::flat(20, 20), Rng::new(3));
        w.story.next_raid = f32::MAX;
        w.story.next_event = f32::MAX;
        let ids = (0..7).map(|k| w.spawn_colonist((2 + k, 2, Z))).collect();
        (w, ids)
    }

    #[test]
    fn squads_have_a_leader_and_a_size_limit() {
        let (mut w, ids) = world();
        assert_eq!(w.pawn(ids[0]).unwrap().role, Role::Fighter, "everyone fights by default");
        assert!(w.set_role(ids[1], Role::Member(ids[0])).is_err(), "not a leader yet");
        w.set_role(ids[0], Role::Leader).unwrap();
        for &m in &ids[1..SQUAD_MAX] {
            w.set_role(m, Role::Member(ids[0])).unwrap();
        }
        assert_eq!(w.squad_members(ids[0]).len(), SQUAD_MAX - 1);
        assert!(w.set_role(ids[SQUAD_MAX], Role::Member(ids[0])).is_err(), "squad is full");
        assert!(w.set_role(ids[0], Role::Member(ids[0])).is_err(), "can't follow yourself");

        // Stepping down releases the squad.
        w.set_role(ids[0], Role::Civilian).unwrap();
        assert!(w.squad_members(ids[0]).is_empty());
        assert_eq!(w.pawn(ids[1]).unwrap().role, Role::Fighter);
    }

    #[test]
    fn a_dead_leaders_squad_breaks_up() {
        let (mut w, ids) = world();
        w.set_role(ids[0], Role::Leader).unwrap();
        w.set_role(ids[1], Role::Member(ids[0])).unwrap();
        let mut p = w.pawn(ids[0]).unwrap().clone();
        w.kill(&mut p, "fell");
        let i = w.pawn_index(ids[0]).unwrap();
        w.pawns[i] = p;
        w.update_squads();
        assert_eq!(w.pawn(ids[1]).unwrap().role, Role::Fighter);
    }

    #[test]
    fn leaders_want_a_decent_room_of_their_own() {
        let (mut w, ids) = world();
        w.set_role(ids[0], Role::Leader).unwrap();
        w.update_squads();
        let thoughts = |w: &World| w.pawn(ids[0]).unwrap().thoughts().iter().map(|t| t.0).collect::<Vec<_>>();
        assert!(thoughts(&w).contains(&"No proper quarters for a leader"));
        assert!(thoughts(&w).contains(&"Responsible for a squad"));

        // A walled, roofed, furnished bedroom with their bed.
        for y in 8..=12 {
            for x in 8..=12 {
                let i = w.map.idx((x, y, Z));
                if x == 8 || y == 8 || x == 12 || y == 12 {
                    w.map.set_feature(i, Feature::built(if (x, y) == (10, 8) { "door" } else { "wall" }));
                }
                let roof = w.map.idx((x, y, Z + 1));
                w.map.set_floor(roof, true);
            }
        }
        for (x, y, f) in [(10, 10, "bed"), (9, 9, "lamp"), (11, 9, "table"), (9, 11, "chair"), (11, 11, "shelf")] {
            let i = w.map.idx((x, y, Z));
            w.map.set_feature(i, Feature::built(f));
        }
        w.map.recompute_rooms();
        w.update_room_stats();
        let i = w.pawn_index(ids[0]).unwrap();
        w.pawns[i].bed = Some((10, 10, Z));
        let tier = w.room_at((10, 10, Z)).unwrap().tier();
        assert!(tier >= LEADER_ROOM_TIER, "the test room is decent (tier {})", tier);
        w.update_squads();
        assert!(!thoughts(&w).contains(&"No proper quarters for a leader"));

        // Sharing it doesn't count.
        let j = w.pawn_index(ids[1]).unwrap();
        w.pawns[j].bed = Some((10, 10, Z));
        w.update_squads();
        assert!(thoughts(&w).contains(&"No proper quarters for a leader"));
    }

    #[test]
    fn loadouts_hold_one_weapon_and_one_piece_per_slot() {
        let (mut w, ids) = world();
        let k = |s: &str| ItemId::named(s);
        w.set_loadout(ids[0], &[k("rifle"), k("wood"), k("clothes"), k("club"), k("steel_armor")]);
        assert_eq!(w.pawn(ids[0]).unwrap().loadout, vec![k("clothes"), k("club"), k("steel_armor")]);
    }
}
