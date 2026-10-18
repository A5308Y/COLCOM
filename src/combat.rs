//! Injuries and deaths in battle (see `battle.rs` for the battles themselves).

use crate::defs::ItemId;
use crate::health::DamageKind;
use crate::map::{Pos, Stack};
use crate::pawn::{Memory, Pawn};
use crate::world::World;

impl World {
    pub fn damage(&mut self, ti: usize, amount: f32, kind: DamageKind, by: &str) {
        let mut t = self.pawns[ti].clone();
        if t.dead {
            return;
        }
        let armor = t.armor();
        let hit = t.health.apply_damage(amount, kind, armor, &mut self.rng);
        t.health.recompute();
        // Blows that don't get through the armour glance off.
        self.sound(if hit.is_some() { "hit" } else { "glance" }, Some(t.pos));
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
                    t.downed_time = 0.0;
                    self.push_log(format!("{} was downed by {}{}: {}.", t.name, by, wound, reason));
                    self.sound("thud", Some(t.pos));
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
        p.chosen.clear();
        let stashed: Vec<ItemId> = p.stash.drain(..).filter_map(|(_, prev)| prev).collect();
        for item in p.weapon.take().into_iter().chain(p.apparel.drain(..)).chain(stashed) {
            self.map.drop_item(p.pos, Stack { kind: item, count: 1 });
        }
        // Raiders carry odds and ends worth salvaging.
        if p.faction == crate::pawn::Faction::Raider {
            if let Some(scrap) = ItemId::all().find(|k| k.def().id == "scrap_metal") {
                let count = 1 + self.rng.below(2) as u32;
                self.map.drop_item(p.pos, Stack { kind: scrap, count });
            }
        }
        p.dead = true;
        p.downed = false;
        self.sound("death", Some(p.pos));
        self.push_log(format!("{} {}.", p.name, cause));
        if p.is_colonist() {
            for o in self.pawns.iter_mut() {
                if o.is_colonist() && o.id != p.id && !o.dead {
                    o.add_memory(Memory::ColonistDied);
                }
            }
        }
    }

    /// The nearest map-edge cell `from` can walk to (where raiders flee).
    pub(crate) fn nearest_edge(&self, from: Pos) -> Pos {
        let (w, h) = (self.map.w, self.map.h);
        self.nearest_cell(from, crate::pathing::ANY_DISTANCE, |i| {
            let p = self.map.pos(i);
            p.0 == 0 || p.1 == 0 || p.0 == w - 1 || p.1 == h - 1
        })
        .unwrap_or(from)
    }
}
