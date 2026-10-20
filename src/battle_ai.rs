//! The battle AI: plays the raiders (and, in tests, the colony). One step
//! at a time, so its moves and attacks play out like the player's.

use crate::battle::{battle_sight, BattleOrder, Cover, AP_ATTACK, AP_DEPLOY, AP_HUNKER, AP_MOVE};
use crate::defs::ItemId;
use crate::map::Pos;
use crate::pathing::bfs;
use crate::pawn::{Faction, PawnId};
use crate::world::World;

/// Attacks at least this likely to hit are taken without looking for a better spot.
const GOOD_ODDS: f32 = 0.45;

impl World {
    /// Pick a unit to activate, or give the active one its next order.
    pub(crate) fn ai_step(&mut self) {
        let Some(b) = self.battle.as_ref() else { return };
        let side = b.side_to_act;
        let Some(u) = b.active else {
            let ready: Vec<usize> =
                (0..b.units.len()).filter(|&u| b.units[u].side == side && b.units[u].can_act() && !b.units[u].acted).collect();
            // The unit nearest the enemy goes first.
            let pick = ready.into_iter().min_by_key(|&u| self.enemy_distance(u).unwrap_or(usize::MAX));
            if let Some(u) = pick {
                let _ = self.do_order(BattleOrder::Activate(u));
                if let Some(b) = self.battle.as_mut() {
                    b.busy = 0.3; // a beat, so the player can follow
                }
            }
            return;
        };
        let order = self.ai_order(u);
        if self.do_order(order).is_err() {
            let _ = self.do_order(BattleOrder::End);
        }
    }

    /// Cells (walking steps) from unit `u`'s leader to the nearest enemy.
    fn enemy_distance(&self, u: usize) -> Option<usize> {
        let b = self.battle.as_ref()?;
        let lead = self.pawn(*b.units[u].members.first()?)?;
        self.enemies_of(b.units[u].side)
            .iter()
            .filter_map(|&e| self.pawn(e))
            .map(|e| e.pos.0.abs_diff(lead.pos.0) + e.pos.1.abs_diff(lead.pos.1) + e.pos.2.abs_diff(lead.pos.2))
            .min()
    }

    /// Pawns on the other side who can still be attacked.
    fn enemies_of(&self, side: Faction) -> Vec<PawnId> {
        let Some(b) = self.battle.as_ref() else { return Vec::new() };
        b.units.iter().filter(|u| u.side != side).flat_map(|u| u.members.iter().copied()).collect()
    }

    /// Best attack for unit `u` right now: (target unit, expected hits).
    fn best_attack(&self, u: usize) -> Option<(usize, f32)> {
        let b = self.battle.as_ref()?;
        let mut best: Option<(usize, f32)> = None;
        for t in 0..b.units.len() {
            if b.units[t].side == b.units[u].side || b.units[t].out() {
                continue;
            }
            let options = self.attack_options(u, t);
            if options.is_empty() {
                continue;
            }
            // Expected hits: each attacker's best chance against this unit.
            let mut score = 0.0;
            let mut seen = Vec::new();
            for &(a, o) in &options {
                if seen.contains(&a) {
                    continue;
                }
                seen.push(a);
                let chance = options
                    .iter()
                    .filter(|x| x.0 == a)
                    .filter_map(|x| Some(self.hit_chance(self.pawn(a)?, self.pawn(x.1)?)))
                    .fold(0.0, f32::max);
                let _ = o;
                score += chance;
            }
            // Prefer fighters over civilians cowering in the safe zone.
            if b.units[t].is_civilians() {
                score *= 0.5;
            }
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((t, score));
            }
        }
        best
    }

    fn ai_order(&self, u: usize) -> BattleOrder {
        let b = self.battle.as_ref().unwrap();
        let unit = &b.units[u];
        if unit.broken() {
            return match unit.side {
                // To run, a deployed squad has to stand up first.
                Faction::Raider if unit.is_deployed() && unit.ap >= AP_DEPLOY + AP_MOVE => BattleOrder::Deploy,
                Faction::Raider if !unit.moved && !unit.is_deployed() => self.flee_order(u),
                _ if unit.ap >= AP_HUNKER => BattleOrder::Hunker,
                _ => BattleOrder::End,
            };
        }
        // Belt items: patch up the wounded, or a grenade into a bunched-up squad.
        let affords = |kind: ItemId| kind.def().utility.as_ref().is_some_and(|d| unit.ap >= d.ap);
        let bleeding = unit.members.iter().filter_map(|&m| self.pawn(m)).any(|p| p.health.bleed_rate() > 0.0);
        if bleeding && self.medkit_option(u).is_some_and(|o| affords(o.1)) {
            return BattleOrder::Medkit;
        }
        let grenade = (0..b.units.len())
            .filter(|&t| b.units[t].members.len() >= 2 && !b.units[t].is_civilians())
            .find(|&t| self.throw_option(u, t).is_some_and(|o| affords(o.1)));
        if let Some(t) = grenade {
            return BattleOrder::Throw(t);
        }
        let attack = self.best_attack(u).filter(|&(t, _)| unit.ap >= self.attack_cost(u, t));
        if let Some((t, score)) = attack {
            // A squad staying put to fire goes down on one knee first (a fresh activation only).
            let staying = score >= GOOD_ODDS && !unit.moved && unit.ap == unit.ap_start;
            if !unit.is_deployed() && staying && unit.ap >= AP_DEPLOY + AP_ATTACK && self.can_deploy(u) {
                return BattleOrder::Deploy;
            }
            if score >= GOOD_ODDS || unit.moved || unit.is_deployed() {
                return BattleOrder::Attack(t);
            }
        }
        // To move, a deployed squad stands up first.
        if unit.is_deployed() && !unit.moved && unit.ap >= AP_DEPLOY + 2 * AP_MOVE && self.best_spot(u).is_some() {
            return BattleOrder::Deploy;
        }
        if !unit.moved && !unit.is_deployed() && unit.ap >= AP_MOVE {
            if let Some(to) = self.best_spot(u) {
                return BattleOrder::Move(to);
            }
        }
        match attack {
            Some((t, _)) => BattleOrder::Attack(t),
            None if unit.ap >= AP_HUNKER => BattleOrder::Hunker,
            None => BattleOrder::End,
        }
    }

    /// Run for the nearest edge of the map, as far as the AP allow.
    fn flee_order(&self, u: usize) -> BattleOrder {
        let b = self.battle.as_ref().unwrap();
        let unit = &b.units[u];
        let Some(lead) = unit.members.first().and_then(|&id| self.pawn(id)) else { return BattleOrder::End };
        let edge = self.nearest_edge(lead.pos);
        let df = self.battle_field(unit.side, lead.pos);
        // `path_to` runs from the goal back to the start.
        let path = df.path_to(edge);
        let steps = (unit.ap / self.step_ap(u)) as usize;
        match path.len() {
            n if n == 0 || steps == 0 => BattleOrder::End,
            n => BattleOrder::Move(path[n - steps.min(n)]),
        }
    }

    /// Where unit `u` should move: somewhere it can attack from (with AP
    /// left to do it), in cover; failing that, as far towards the nearest
    /// enemy as it can walk.
    fn best_spot(&self, u: usize) -> Option<Pos> {
        let b = self.battle.as_ref()?;
        let unit = &b.units[u];
        let lead = self.pawn(*unit.members.first()?)?;
        let enemies: Vec<_> = self.enemies_of(unit.side).into_iter().filter_map(|e| self.pawn(e)).collect();
        let weapon = lead.main_attack();
        let step = self.step_ap(u);
        let reach = unit.ap / step;
        let reach_and_attack = (unit.ap - AP_ATTACK).max(0) / step;
        let near = enemies.iter().min_by_key(|e| e.pos.0.abs_diff(lead.pos.0) + e.pos.1.abs_diff(lead.pos.1))?;
        // Walking distance to the nearest enemy, so units go round obstacles.
        let to_enemy = bfs(&self.map, near.pos);
        let df = self.battle_field(unit.side, lead.pos);
        let taken = |c: Pos| self.pawns.iter().any(|p| p.active() && p.pos == c && !unit.members.contains(&p.id));
        let mut best: Option<(Pos, f32)> = None;
        for (i, d) in df.nearest_first(reach) {
            let c = self.map.pos(i);
            if d == 0 || taken(c) {
                continue;
            }
            let gap = to_enemy.dist(c).map_or(1.5 * (c.0.abs_diff(near.pos.0) + c.1.abs_diff(near.pos.1)) as f32, |g| g as f32);
            let mut score = -0.3 * gap - 0.01 * d as f32;
            let attack_after = d <= reach_and_attack;
            if weapon.ranged {
                let can_shoot = enemies.iter().any(|e| {
                    let r = ((e.pos.0 as f32 - c.0 as f32).powi(2) + (e.pos.1 as f32 - c.1 as f32).powi(2)).sqrt();
                    r <= weapon.range && battle_sight(self, c, e.pos)
                });
                let cover = match self.cover(near.pos, c) {
                    Cover::Heavy => 1.0,
                    Cover::Medium => 0.7,
                    Cover::Light => 0.4,
                    Cover::None => 0.0,
                };
                score += match (can_shoot, attack_after) {
                    (true, true) => 10.0 + 2.0 * cover,
                    // In position for next round.
                    (true, false) => 3.0 + 2.0 * cover,
                    _ => 0.5 * cover,
                };
                if c.2 > near.pos.2 {
                    score += 0.5;
                }
            } else if crate::map::adjacent(c, near.pos) && c.2 == near.pos.2 {
                score += if attack_after { 10.0 } else { 3.0 };
            }
            if best.is_none_or(|(_, s)| score > s) {
                best = Some((c, score));
            }
        }
        best.map(|(c, _)| c)
    }
}
