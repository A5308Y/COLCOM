//! Squads, roles and loadouts (the gear each colonist uses), set up during
//! normal play for the turn-based battles (`battle.rs`). A squad is a leader and the colonists
//! whose role is `Member(leader)`; they fight as one unit. A squad is either
//! a ranged or a melee squad: everyone in it, the leader too, fights the
//! same way (`fights_at_range`), whatever weapon exactly.

use crate::defs::{ItemId, Slot};
use crate::pawn::{Pawn, PawnId, Role};
use crate::world::World;

/// Most colonists in one squad, the leader included.
pub const SQUAD_MAX: usize = 4;
/// Least bedroom quality tier a leader is content with (2 = Decent).
pub const LEADER_ROOM_TIER: u8 = 2;

/// Whether `a` and `b` can't both be in a loadout: two weapons, two belt
/// items, or apparel for the same slot. (The sidearm is kept apart.)
pub(crate) fn conflicts(a: ItemId, b: ItemId) -> bool {
    let (a, b) = (a.def(), b.def());
    match (&a.apparel, &b.apparel) {
        (Some(x), Some(y)) => x.overlaps(y),
        _ => a.weapon.is_some() && b.weapon.is_some() || a.utility.is_some() && b.utility.is_some(),
    }
}

/// Whether colonist `p` may carry `kind` as a sidearm: anyone outside a
/// squad, and melee squads anything (a pistol to shoot while closing in);
/// ranged squads only something that shoots, as they fight at range.
pub fn sidearm_fits_squad(p: &Pawn, kind: ItemId) -> bool {
    let in_squad = matches!(p.role, Role::Leader | Role::Member(_));
    let shoots = kind.def().weapon.as_ref().is_some_and(|w| w.ranged());
    !in_squad || !fights_at_range(p) || shoots
}

/// The loadout item for apparel `slot`, if any.
pub fn loadout_worn(p: &Pawn, slot: Slot) -> Option<ItemId> {
    p.loadout.iter().copied().find(|k| k.def().apparel.as_ref().is_some_and(|a| a.covers(slot)))
}

/// Whether colonist `p` fights at range: with the weapon in their loadout
/// (what they'll have once they've fetched it), or else the one they carry.
/// Bare hands are melee.
pub fn fights_at_range(p: &Pawn) -> bool {
    let weapon = p.loadout.iter().copied().find(|k| k.def().weapon.is_some()).or(p.weapon);
    // A personal shield (planned or worn) stops their shots: they fight in melee.
    let belt = p.loadout.iter().copied().find(|k| k.def().utility.is_some()).or(p.utility.map(|s| s.kind));
    let shielded = belt.and_then(|k| k.def().utility.as_ref()).is_some_and(|u| matches!(u.effect, crate::defs::UtilityEffect::Shield { .. }));
    !shielded && weapon.and_then(|k| k.def().weapon.as_ref()).is_some_and(|w| w.ranged())
}

/// "ranged" or "melee".
pub fn style_name(ranged: bool) -> &'static str {
    if ranged {
        "ranged"
    } else {
        "melee"
    }
}

impl World {
    /// The members of `leader`'s squad (not the leader).
    pub fn squad_members(&self, leader: PawnId) -> Vec<PawnId> {
        self.pawns.iter().filter(|p| p.role == Role::Member(leader) && !p.is_dead()).map(|p| p.id).collect()
    }

    /// Give colonist `id` a role. Joining a squad needs a living leader with
    /// room in it; a leader stepping down releases their members.
    pub fn set_role(&mut self, id: PawnId, role: Role) -> Result<(), String> {
        let p = self.pawn(id).ok_or("no such colonist")?;
        if !p.is_colonist() || p.is_dead() {
            return Err("only living colonists have roles".into());
        }
        if let Role::Member(leader) = role {
            let l = self.pawn(leader).ok_or("no such leader")?;
            if leader == id || l.role != Role::Leader || l.is_dead() {
                return Err(format!("{} doesn't lead a squad", l.name));
            }
            if p.role != role && 1 + self.squad_members(leader).len() >= SQUAD_MAX {
                return Err(format!("{}'s squad is full ({} at most)", l.name, SQUAD_MAX));
            }
            let (mine, theirs) = (fights_at_range(p), fights_at_range(l));
            if mine != theirs {
                return Err(format!(
                    "{} fights {} and {}'s squad is a {} squad",
                    p.name,
                    style_name(mine),
                    l.name,
                    style_name(theirs)
                ));
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

    fn release_squad(&mut self, leader: PawnId) {
        for p in self.pawns.iter_mut().filter(|p| p.role == Role::Member(leader)) {
            p.role = Role::Fighter;
        }
    }

    /// Put `kind` in colonist `id`'s loadout, in place of what's there for its
    /// slot. A squad member taking a weapon of the other kind leaves the squad.
    pub fn set_loadout_item(&mut self, id: PawnId, kind: ItemId) {
        let Some(p) = self.pawn(id) else { return };
        let mut items = p.loadout.clone();
        items.push(kind);
        if self.set_loadout(id, &items).is_err() {
            if let Some(i) = self.pawn_index(id) {
                let name = self.pawns[i].name.clone();
                self.pawns[i].role = Role::Fighter;
                self.push_log(format!("{} left their squad to fight with {}.", name, kind.name().to_lowercase()));
            }
            let _ = self.set_loadout(id, &items);
        }
    }

    /// Set colonist `id`'s sidearm to fetch and keep (`None`: whatever they have).
    /// Only light weapons can be sidearms.
    pub fn set_loadout_sidearm(&mut self, id: PawnId, kind: Option<ItemId>) -> Result<(), String> {
        if kind.is_some_and(|k| !k.def().weapon.as_ref().is_some_and(|w| w.sidearm)) {
            return Err("too heavy for a sidearm".into());
        }
        let i = self.pawn_index(id).ok_or("no such colonist")?;
        if let Some(k) = kind.filter(|&k| !sidearm_fits_squad(&self.pawns[i], k)) {
            return Err(format!("a ranged squad's sidearms shoot too: no {}", k.name().to_lowercase()));
        }
        self.pawns[i].loadout_sidearm = kind;
        Ok(())
    }

    /// Set colonist `id`'s loadout, the gear they use: weapons, apparel and
    /// belt items only, at most one weapon, one belt item and one piece per
    /// apparel slot (later ones win, taking out what they overlap). A squad
    /// member can't take a weapon of the other kind than their squad's; a
    /// leader can, and the members who no longer fit leave the squad.
    pub fn set_loadout(&mut self, id: PawnId, items: &[ItemId]) -> Result<(), String> {
        let mut out: Vec<ItemId> = Vec::new();
        for &k in items {
            if !k.def().is_gear() {
                continue;
            }
            out.retain(|&o| !conflicts(o, k));
            out.push(k);
        }
        let i = self.pawn_index(id).ok_or("no such colonist")?;
        let mut p = self.pawns[i].clone();
        p.loadout = out;
        let ranged = fights_at_range(&p);
        if let Role::Member(l) = p.role {
            if let Some(l) = self.pawn(l).filter(|l| fights_at_range(l) != ranged) {
                return Err(format!("{}'s squad is a {} squad", l.name, style_name(!ranged)));
            }
        }
        let leader = p.role == Role::Leader;
        self.pawns[i] = p;
        if leader {
            for m in self.squad_members(id) {
                let mi = self.pawn_index(m).unwrap();
                if fights_at_range(&self.pawns[mi]) != ranged {
                    self.pawns[mi].role = Role::Fighter;
                    let msg = format!(
                        "{} left {}'s squad: it's a {} squad now.",
                        self.pawns[mi].name,
                        self.pawns[i].name,
                        style_name(ranged)
                    );
                    self.push_log(msg);
                }
            }
        }
        Ok(())
    }

    /// Once a second: squads whose leader is gone break up, and leaders
    /// check their quarters.
    pub(crate) fn update_squads(&mut self) {
        let gone: Vec<PawnId> = self
            .pawns
            .iter()
            .filter_map(|p| match p.role {
                Role::Member(l) if self.pawn(l).is_none_or(|l| !l.present() || l.role != Role::Leader) => Some(l),
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
            o.id != p.id && !o.is_dead() && o.bed.is_some_and(|b| m.room.get(m.idx(b)).copied() == Some(room))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Feature, FLAT_Z as Z};

    fn world() -> (World, Vec<PawnId>) {
        let mut w = crate::testing::quiet_world(20, 20, 3);
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

    /// A walled, roofed, furnished bedroom from (8, 8) to (12, 12), with a bed at (10, 10).
    fn bedroom(w: &mut World) {
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
    }

    #[test]
    fn beds_can_be_given_to_a_leader() {
        let (mut w, ids) = world();
        w.set_role(ids[0], Role::Leader).unwrap();
        bedroom(&mut w);
        let outside = (3, 3, Z);
        let i = w.map.idx(outside);
        w.map.set_feature(i, Feature::built("bed"));
        // Someone else got the good bed first.
        assert!(w.assign_bed((10, 10, Z), Some(ids[1])));
        assert!(w.assign_bed(outside, Some(ids[0])));
        w.update_squads();
        assert!(w.pawn(ids[0]).unwrap().poor_quarters);

        assert!(w.assign_bed((10, 10, Z), Some(ids[0])));
        assert_eq!(w.bed_owner((10, 10, Z)), Some(ids[0]));
        assert_eq!(w.bed_owner(outside), None, "the leader's old bed is free");
        assert_eq!(w.pawn(ids[1]).unwrap().bed, None, "and the other looks for one");
        w.update_squads();
        assert!(!w.pawn(ids[0]).unwrap().poor_quarters);
        assert_eq!(w.find_bed(w.pawn(ids[1]).unwrap()), Some(outside), "not the leader's");

        assert!(w.assign_bed((10, 10, Z), None));
        assert_eq!(w.bed_owner((10, 10, Z)), None);
        assert!(!w.assign_bed((5, 5, Z), Some(ids[0])), "no bed there");
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
        bedroom(&mut w);
        assert!(w.assign_bed((10, 10, Z), Some(ids[0])));
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

    fn arm(w: &mut World, id: PawnId, weapon: Option<&str>) {
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = weapon.map(ItemId::named);
    }

    #[test]
    fn a_squad_is_ranged_or_melee() {
        let (mut w, ids) = world();
        arm(&mut w, ids[0], Some("rifle"));
        arm(&mut w, ids[1], Some("pistol"));
        arm(&mut w, ids[2], Some("knife"));
        arm(&mut w, ids[3], None);
        w.set_role(ids[0], Role::Leader).unwrap();
        w.set_role(ids[1], Role::Member(ids[0])).expect("any gun goes in a ranged squad");
        let err = w.set_role(ids[2], Role::Member(ids[0])).unwrap_err();
        assert!(err.contains("ranged squad"), "{}", err);
        assert!(w.set_role(ids[3], Role::Member(ids[0])).is_err(), "bare hands are melee");
        // The loadout counts, not what they carry now.
        w.set_loadout(ids[2], &[ItemId::named("rifle")]).unwrap();
        w.set_role(ids[2], Role::Member(ids[0])).unwrap();
        // Members can't switch kind...
        assert!(w.set_loadout(ids[1], &[ItemId::named("club")]).is_err());
        assert_eq!(w.pawn(ids[1]).unwrap().loadout, vec![], "unchanged");
        // ...but leaders can, and those who don't fit leave.
        w.set_loadout(ids[0], &[ItemId::named("club")]).unwrap();
        assert!(w.squad_members(ids[0]).is_empty());
        assert_eq!(w.pawn(ids[1]).unwrap().role, Role::Fighter);
        w.set_role(ids[3], Role::Member(ids[0])).expect("now it's a melee squad");
    }

    #[test]
    fn loadouts_hold_one_weapon_and_one_piece_per_slot() {
        let (mut w, ids) = world();
        let k = |s: &str| ItemId::named(s);
        w.set_loadout(ids[0], &[k("rifle"), k("wood"), k("clothes"), k("club"), k("steel_armor")]).unwrap();
        assert_eq!(w.pawn(ids[0]).unwrap().loadout, vec![k("clothes"), k("club"), k("steel_armor")]);
    }

    #[test]
    fn ranged_squads_take_pistols_as_sidearms_melee_squads_anything() {
        let (mut w, ids) = world();
        let k = |s: &str| ItemId::named(s);
        w.set_loadout(ids[0], &[k("rifle")]).unwrap();
        assert!(w.set_loadout_sidearm(ids[0], Some(k("knife"))).is_ok(), "on their own, anything");
        w.set_role(ids[0], Role::Leader).unwrap();
        assert!(w.set_loadout_sidearm(ids[0], Some(k("knife"))).is_err(), "a ranged squad's sidearms shoot");
        assert!(w.set_loadout_sidearm(ids[0], Some(k("pistol"))).is_ok());
        assert!(w.set_loadout_sidearm(ids[0], Some(k("rifle"))).is_err(), "too heavy");
        w.set_loadout(ids[1], &[k("club")]).unwrap();
        w.set_role(ids[1], Role::Leader).unwrap();
        assert!(w.set_loadout_sidearm(ids[1], Some(k("pistol"))).is_ok(), "a melee squad may shoot on the way in");
        assert!(w.set_loadout_sidearm(ids[1], Some(k("knife"))).is_ok());
    }
}
