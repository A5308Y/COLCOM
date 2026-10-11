//! Shooting, melee, injuries and raider behaviour.

use crate::health::DamageKind;
use crate::map::{Pos, Stack};
use crate::pathing::{bfs, line_of_sight};
use crate::pawn::{Job, Memory, Pawn, Skill};
use crate::world::{Effect, Step, World};

fn dist(a: &Pawn, b: &Pawn) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

impl World {
    /// Index of the nearest conscious enemy of `p` within `max_dist`.
    pub(crate) fn nearest_hostile(&self, p: &Pawn, max_dist: f32, need_los: bool) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, o) in self.pawns.iter().enumerate() {
            if o.id == p.id || o.faction == p.faction || !o.active() {
                continue;
            }
            let d = dist(p, o);
            if d > max_dist || (need_los && !line_of_sight(&self.map, p.pos, o.pos)) {
                continue;
            }
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    fn in_attack_position(&self, p: &Pawn, ti: usize) -> bool {
        let t = &self.pawns[ti];
        let a = p.attack();
        dist(p, t) <= a.range && (!a.ranged || line_of_sight(&self.map, p.pos, t.pos))
    }

    /// Fire at whatever enemy is in range, if the weapon is ready.
    pub(crate) fn auto_fire(&mut self, p: &mut Pawn) {
        if p.cooldown > 0.0 {
            return;
        }
        let a = p.attack();
        if let Some(ti) = self.nearest_hostile(p, a.range, a.ranged) {
            self.attack(p, ti);
        }
    }

    fn attack(&mut self, p: &mut Pawn, ti: usize) {
        let w = p.attack();
        let skill = if w.ranged { Skill::Shooting } else { Skill::Melee };
        let (tx, ty, tz) = (self.pawns[ti].x, self.pawns[ti].y, self.pawns[ti].z);
        let d = dist(p, &self.pawns[ti]);
        let level = p.skill(skill) as f32;
        let base = if w.ranged { 0.55 + level * 0.025 - d * 0.025 } else { 0.6 + level * 0.02 };
        // Bad eyes or hands make for bad aim.
        let caps = &p.health.caps;
        let chance = (base * (0.4 + 0.6 * caps.sight.min(1.0)) * (0.5 + 0.5 * caps.manipulation.min(1.0))).clamp(0.05, 0.95);
        let hit = self.rng.chance(chance);
        p.cooldown = w.cooldown;
        p.learn(skill, 2.0);
        if w.ranged {
            let to = if hit {
                (tx, ty, tz)
            } else {
                (tx + self.rng.range_f32(-1.0, 1.0), ty + self.rng.range_f32(-1.0, 1.0), tz)
            };
            self.effects.push(Effect { from: (p.x, p.y, p.z), to, ttl: 0.15, hit });
        }
        if hit {
            let (lo, hi) = w.damage;
            let dmg = self.rng.range_f32(lo, hi);
            self.damage(ti, dmg, w.kind, &p.name);
        }
    }

    pub fn damage(&mut self, ti: usize, amount: f32, kind: DamageKind, by: &str) {
        let mut t = self.pawns[ti].clone();
        if t.dead {
            return;
        }
        let armor = t.armor();
        let hit = t.health.apply_damage(amount, kind, armor, &mut self.rng);
        t.health.recompute();
        let wound = hit.map_or(String::new(), |h| {
            format!(" ({} to the {})", kind.name().to_lowercase(), h.part.name().to_lowercase())
        });
        if t.health.death_cause().is_some() {
            self.kill(&mut t, &format!("was killed by {}{}", by, wound));
        } else {
            if let Some(h) = hit.filter(|h| h.destroyed) {
                self.push_log(format!("{}'s {} was destroyed by {}.", t.name, h.part.name().to_lowercase(), by));
            }
            if !t.downed {
                if let Some(reason) = t.health.downed_reason() {
                    self.end_job(&mut t);
                    t.downed = true;
                    t.drafted = false;
                    t.downed_time = 0.0;
                    self.push_log(format!("{} was downed by {}{}: {}.", t.name, by, wound, reason));
                }
            }
        }
        self.pawns[ti] = t;
    }

    pub(crate) fn kill(&mut self, p: &mut Pawn, cause: &str) {
        if p.dead {
            return;
        }
        self.end_job(p);
        // Their gear stays behind for others to use.
        for item in p.weapon.take().into_iter().chain(p.apparel.drain(..)) {
            self.map.drop_item(p.pos, Stack { kind: item, count: 1 });
        }
        p.dead = true;
        p.downed = false;
        p.drafted = false;
        self.push_log(format!("{} {}.", p.name, cause));
        if p.is_colonist() {
            for o in self.pawns.iter_mut() {
                if o.is_colonist() && o.id != p.id && !o.dead {
                    o.add_memory(Memory::ColonistDied);
                }
            }
        }
    }

    pub(crate) fn run_attack(&mut self, p: &mut Pawn, target: u32, dt: f32) {
        let Some(ti) = self.pawn_index(target) else {
            self.end_job(p);
            return;
        };
        if !self.pawns[ti].active() {
            self.end_job(p);
            return;
        }
        if self.in_attack_position(p, ti) {
            p.path.clear();
            p.path_goal = None;
            if p.cooldown <= 0.0 {
                self.attack(p, ti);
            }
            return;
        }
        let tpos = self.pawns[ti].pos;
        if let Step::Unreachable = self.go_to(p, tpos, true, dt) {
            self.end_job(p);
        }
    }

    pub(crate) fn raider_think(&mut self, p: &mut Pawn, dt: f32) {
        if matches!(p.job, Some(Job::Leave { .. })) {
            return;
        }
        p.leave_timer -= dt;
        let hurt = p.health.pain > 0.35 || p.health.summary() < 0.6;
        if p.leave_timer <= 0.0 || hurt {
            if hurt {
                self.push_log(format!("{} is fleeing.", p.name));
            }
            self.end_job(p);
            let to = self.nearest_edge(p.pos);
            self.start_job(p, Job::Leave { to });
            return;
        }
        // Shoot at anyone in range, even on the move.
        self.auto_fire(p);

        p.think_timer -= dt;
        if p.think_timer > 0.0 && p.job.is_some() {
            return;
        }
        p.think_timer = 1.0;
        match self.nearest_hostile(p, f32::MAX, false) {
            Some(ti) => {
                let job = Job::Attack { target: self.pawns[ti].id };
                if p.job != Some(job) {
                    self.end_job(p);
                    self.start_job(p, job);
                }
            }
            None => {
                if p.job.is_none() {
                    self.start_wander(p, 6);
                }
            }
        }
    }

    fn nearest_edge(&self, from: Pos) -> Pos {
        // Reachable cells come nearest first.
        let df = bfs(&self.map, from);
        let (w, h) = (self.map.w, self.map.h);
        df.order
            .iter()
            .map(|&i| self.map.pos(i))
            .find(|p| p.0 == 0 || p.1 == 0 || p.0 == w - 1 || p.1 == h - 1)
            .unwrap_or(from)
    }
}
