//! Lookout posts (a buildable with `special: Lookout`) and the colonists
//! given watch at them. A watcher stands at their post when raiders come
//! (`World::man_lookouts`), and pays for that readiness with watch duty:
//! `WATCH_HOURS` a day owed, worked off standing at the post in their
//! work and anything hours (`Job::Watch`).

use crate::defs::Special;
use crate::map::{Feature, Pos};
use crate::pawn::{Job, Pawn, PawnId, Role, Watch};
use crate::recreation::Block;
use crate::world::World;
use crate::DAY_LEN;

/// Hours of watch a day each watcher owes.
pub const WATCH_HOURS: f32 = 2.5;
/// Owed time (in hours) that sends a watcher to their post: shifts of about this long.
const SHIFT_HOURS: f32 = 1.0;
/// Most owed (in hours): time missed while ill or busy isn't all made up.
const OWED_MAX_HOURS: f32 = 2.0;

fn hours(h: f32) -> f32 {
    h * DAY_LEN / 24.0
}

/// Who can keep watch: they fight on their own or lead a squad (members stay
/// with their leader, civilians shelter).
pub fn can_watch(role: Role) -> bool {
    match role {
        Role::Fighter | Role::Leader => true,
        Role::Member(_) | Role::Civilian => false,
    }
}

impl World {
    pub fn is_lookout(&self, p: Pos) -> bool {
        matches!(self.map.feature[self.map.idx(p)], Feature::Built { kind, .. } if kind.def().special == Special::Lookout)
    }

    /// Who keeps watch at the post at `post`, if anyone.
    pub fn watcher(&self, post: Pos) -> Option<PawnId> {
        self.pawns.iter().find(|p| !p.is_dead() && p.watch.is_some_and(|w| w.post == post)).map(|p| p.id)
    }

    /// Give the watch at `post` to colonist `id` (who gives up any other post),
    /// or to nobody: the post then stays empty when raiders come.
    pub fn set_watch(&mut self, post: Pos, id: Option<PawnId>) -> Result<(), String> {
        if !self.is_lookout(post) {
            return Err("there's no lookout post there".into());
        }
        if let Some(id) = id {
            let p = self.pawn(id).filter(|p| p.is_living_colonist()).ok_or("no such colonist")?;
            if !can_watch(p.role) {
                return Err(format!("{} is a squad member or civilian: only fighters and squad leaders keep watch", p.name));
            }
        }
        for p in self.pawns.iter_mut() {
            let here = p.watch.is_some_and(|w| w.post == post);
            if Some(p.id) == id {
                if !here {
                    p.watch = Some(Watch { post, owed: 0.0 });
                }
            } else if here {
                p.watch = None;
            }
        }
        Ok(())
    }

    /// Once a second: watch owed builds up, and watches end whose post is gone
    /// or whose watcher can no longer keep it.
    pub(crate) fn update_watches(&mut self, dt: f32) {
        let posts: Vec<Option<Pos>> = self.pawns.iter().map(|p| p.watch.map(|w| w.post)).collect();
        let valid: Vec<bool> = posts.iter().map(|post| post.is_some_and(|at| self.is_lookout(at))).collect();
        let accrue = self.battle.is_none();
        for (p, ok) in self.pawns.iter_mut().zip(valid) {
            let keeps = ok && p.is_living_colonist() && can_watch(p.role);
            let Some(w) = p.watch.as_mut() else { continue };
            if !keeps {
                p.watch = None;
            } else if accrue {
                w.owed = (w.owed + dt * WATCH_HOURS / 24.0).min(hours(OWED_MAX_HOURS));
            }
        }
    }

    /// Off to their post, if a shift is due and it's their hours for it.
    pub(crate) fn watch_shift(&self, p: &Pawn, block: Block) -> Option<Job> {
        let hours_for_it = match block {
            Block::Anything | Block::Work => true,
            Block::Fun | Block::Sleep => false,
        };
        let w = p.watch?;
        (hours_for_it && w.owed >= hours(SHIFT_HOURS)).then_some(Job::Watch { post: w.post })
    }

    /// Standing watch at `post` until nothing's owed.
    pub(crate) fn run_watch(&mut self, p: &mut Pawn, post: Pos, dt: f32) {
        if !p.watch.is_some_and(|w| w.post == post && w.owed > 0.0) || !self.is_lookout(post) {
            self.end_job(p);
            return;
        }
        if !self.walk_to(p, post, false, dt) {
            return;
        }
        if let Some(w) = p.watch.as_mut() {
            w.owed -= dt;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLAT_Z as Z;
    use crate::testing::{colonist, quiet_world, run};

    #[test]
    fn watchers_spend_part_of_the_day_at_their_post() {
        let mut w = quiet_world(30, 30, 3);
        let post = (20, 20, Z);
        let i = w.map.idx(post);
        w.map.set_feature(i, Feature::built("lookout_post"));
        let id = colonist(&mut w, (3, 3, Z));
        assert!(w.set_watch((3, 3, Z), Some(id)).is_err(), "not a post");
        w.set_watch(post, Some(id)).unwrap();
        assert_eq!(w.watcher(post), Some(id));
        // A day: on watch for about `WATCH_HOURS` of it.
        let mut at_post = 0.0;
        let tick = crate::TICK;
        for _ in 0..(DAY_LEN / tick) as usize {
            w.tick(tick);
            if w.pawn(id).unwrap().pos == post && matches!(w.pawn(id).unwrap().job, Some(Job::Watch { .. })) {
                at_post += tick;
            }
        }
        let hours_on_watch = at_post * 24.0 / DAY_LEN;
        assert!((1.0..=WATCH_HOURS + 0.5).contains(&hours_on_watch), "{:.2} hours on watch", hours_on_watch);
        // Squad members and civilians don't keep watch: their watch ends.
        w.set_role(id, Role::Civilian).unwrap();
        run(&mut w, 2.0);
        assert_eq!(w.watcher(post), None);
        assert!(w.set_watch(post, Some(id)).is_err());
    }

    #[test]
    fn one_watcher_per_post_and_one_post_per_watcher() {
        let mut w = quiet_world(20, 20, 3);
        let (a, b) = ((10, 10, Z), (12, 10, Z));
        for p in [a, b] {
            let i = w.map.idx(p);
            w.map.set_feature(i, Feature::built("lookout_post"));
        }
        let (x, y) = (colonist(&mut w, (2, 2, Z)), colonist(&mut w, (3, 2, Z)));
        w.set_watch(a, Some(x)).unwrap();
        w.set_watch(a, Some(y)).unwrap();
        assert_eq!(w.watcher(a), Some(y));
        assert!(w.pawn(x).unwrap().watch.is_none());
        w.set_watch(b, Some(y)).unwrap();
        assert_eq!((w.watcher(a), w.watcher(b)), (None, Some(y)));
        // Taken down, the post's watch goes.
        let i = w.map.idx(b);
        w.map.set_feature(i, Feature::None);
        run(&mut w, 2.0);
        assert!(w.pawn(y).unwrap().watch.is_none());
    }
}
