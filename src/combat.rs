//! Shooting, melee, injuries and raider behaviour.

use crate::map::Pos;
use crate::pathing::{bfs, line_of_sight};
use crate::pawn::{Faction, Job, Memory, Pawn, Skill};
use crate::world::{Effect, Step, World};

fn dist(a: &Pawn, b: &Pawn) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
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
        dist(p, t) <= p.weapon.range() && (!p.weapon.ranged() || line_of_sight(&self.map, p.pos, t.pos))
    }

    /// Fire at whatever enemy is in range, if the weapon is ready.
    pub(crate) fn auto_fire(&mut self, p: &mut Pawn) {
        if p.cooldown > 0.0 {
            return;
        }
        if let Some(ti) = self.nearest_hostile(p, p.weapon.range(), p.weapon.ranged()) {
            self.attack(p, ti);
        }
    }

    fn attack(&mut self, p: &mut Pawn, ti: usize) {
        let w = p.weapon;
        let skill = if w.ranged() { Skill::Shooting } else { Skill::Melee };
        let (tx, ty) = (self.pawns[ti].x, self.pawns[ti].y);
        let d = ((tx - p.x).powi(2) + (ty - p.y).powi(2)).sqrt();
        let level = p.skill(skill) as f32;
        let chance = if w.ranged() {
            (0.55 + level * 0.025 - d * 0.025).clamp(0.1, 0.95)
        } else {
            (0.6 + level * 0.02).min(0.95)
        };
        let hit = self.rng.chance(chance);
        p.cooldown = w.cooldown();
        p.learn(skill, 2.0);
        if w.ranged() {
            let to = if hit {
                (tx, ty)
            } else {
                (tx + self.rng.range_f32(-1.0, 1.0), ty + self.rng.range_f32(-1.0, 1.0))
            };
            self.effects.push(Effect { from: (p.x, p.y), to, ttl: 0.15, hit });
        }
        if hit {
            let (lo, hi) = w.damage();
            let dmg = self.rng.range_f32(lo, hi);
            self.damage(ti, dmg, &p.name);
        }
    }

    pub fn damage(&mut self, ti: usize, dmg: f32, by: &str) {
        let mut t = self.pawns[ti].clone();
        if t.dead {
            return;
        }
        t.health -= dmg;
        // Raiders don't lie around downed; they're simply out of the fight.
        if t.health <= 0.0 || (t.faction == Faction::Raider && t.health < 25.0) {
            self.kill(&mut t, &format!("was killed by {}", by));
        } else if t.health < 25.0 && !t.downed {
            self.end_job(&mut t);
            t.downed = true;
            t.drafted = false;
            self.push_log(format!("{} was downed by {}.", t.name, by));
        }
        self.pawns[ti] = t;
    }

    pub(crate) fn kill(&mut self, p: &mut Pawn, cause: &str) {
        if p.dead {
            return;
        }
        self.end_job(p);
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
        if p.leave_timer <= 0.0 || p.health < 50.0 {
            if p.health < 50.0 {
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
        let df = bfs(&self.map, from);
        let (w, h) = (self.map.w, self.map.h);
        let mut best: Option<(i32, Pos)> = None;
        for i in 0..w * h {
            let p = self.map.pos(i);
            if p.0 != 0 && p.1 != 0 && p.0 != w - 1 && p.1 != h - 1 {
                continue;
            }
            if let Some(d) = df.dist(p) {
                if best.map_or(true, |(bd, _)| d < bd) {
                    best = Some((d, p));
                }
            }
        }
        best.map_or(from, |(_, p)| p)
    }
}
