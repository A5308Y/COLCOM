//! Production: workbenches and their bills, stockpile settings, and
//! colonists picking up better weapons and apparel.
//!
//! A workbench is any buildable that recipes name. Each has a list of bills
//! (what to make, and how many). Once a second every bench settles on the
//! first bill that is wanted and whose ingredients exist; haulers then
//! deliver the ingredients into the bench's stock, and a colonist with the
//! recipe's work type makes it. Products are dropped beside the bench and
//! hauled to stockpiles like anything else.

use crate::defs::{defs, ItemId, RecipeId};
use crate::map::{Feature, Pos, Stack, Zone};
use crate::pawn::{Job, Pawn};
use crate::world::World;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum BillMode {
    /// Make this many more.
    Count(u32),
    /// Keep making until the colony has this many of the (first) product.
    Until(u32),
    Forever,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Bill {
    pub recipe: RecipeId,
    pub mode: BillMode,
    pub suspended: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bench {
    pub pos: Pos,
    /// In order of priority.
    pub bills: Vec<Bill>,
    /// The recipe ingredients are being gathered for.
    pub current: Option<RecipeId>,
    /// Ingredients delivered so far.
    pub stock: Vec<Stack>,
}

impl Bench {
    pub fn has(&self, kind: ItemId) -> u32 {
        self.stock.iter().filter(|s| s.kind == kind).map(|s| s.count).sum()
    }

    /// The next ingredient the current recipe still lacks.
    pub fn need(&self) -> Option<Stack> {
        let r = self.current?.def();
        r.ingredients
            .iter()
            .find(|a| self.has(a.item) < a.count)
            .map(|a| Stack { kind: a.item, count: a.count - self.has(a.item) })
    }

    /// Everything for the current recipe is here.
    pub fn ready(&self) -> bool {
        self.current.is_some() && self.need().is_none()
    }

    fn add(&mut self, s: Stack) {
        match self.stock.iter_mut().find(|t| t.kind == s.kind) {
            Some(t) => t.count += s.count,
            None => self.stock.push(s),
        }
    }
}

pub const PRIORITY_NAMES: [&str; 5] = ["", "Low", "Normal", "Preferred", "Critical"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stockpile {
    pub name: String,
    /// 1 = low .. 4 = critical. Items move to higher-priority stockpiles.
    pub priority: u8,
    /// Item kinds it doesn't take.
    pub forbid: Vec<ItemId>,
    /// The storage of shelves (storage furniture) standing outside any
    /// stockpile, rather than a zone the player painted.
    #[serde(default)]
    pub container: bool,
}

impl Stockpile {
    pub fn new(number: usize) -> Stockpile {
        Stockpile { name: format!("Stockpile {}", number), priority: 2, forbid: Vec::new(), container: false }
    }

    pub fn accepts(&self, kind: ItemId) -> bool {
        !self.forbid.contains(&kind)
    }
}

/// Whether `p` would take `kind` up of their own accord: better than what
/// they have (apparel for the weather, `outdoors` °C), for a slot their
/// loadout leaves free (they keep their loadout's).
fn wants_upgrade(p: &Pawn, kind: ItemId, outdoors: f32) -> bool {
    let planned = |slot| p.loadout.iter().any(|&k| crate::squads::loadout_slot(k) == Some(slot));
    let d = kind.def();
    if let Some(w) = &d.weapon {
        let current = p.weapon.and_then(|w| w.def().weapon.as_ref()).map_or(0.0, |w| w.score());
        return !planned(None) && w.score() > current * 1.1;
    }
    if let Some(a) = &d.apparel {
        let worn = p.worn(a.slot).and_then(|k| k.def().apparel.as_ref()).map_or(f32::MIN, |w| w.score_at(outdoors));
        return !planned(Some(a.slot)) && a.score_at(outdoors) > worn;
    }
    false
}

impl World {
    // ---------- benches and bills ----------

    pub fn bench_at(&self, pos: Pos) -> Option<&Bench> {
        self.benches.iter().find(|b| b.pos == pos)
    }

    fn is_workbench(&self, pos: Pos) -> bool {
        self.map.feature[self.map.idx(pos)].structure().is_some_and(|d| !d.recipes.is_empty())
    }

    /// The bench record for a workbench at `pos`, made if it's new.
    pub(crate) fn bench_mut(&mut self, pos: Pos) -> Option<&mut Bench> {
        if !self.is_workbench(pos) {
            return None;
        }
        let k = match self.benches.iter().position(|b| b.pos == pos) {
            Some(k) => k,
            None => {
                self.benches.push(Bench { pos, bills: Vec::new(), current: None, stock: Vec::new() });
                self.benches.len() - 1
            }
        };
        Some(&mut self.benches[k])
    }

    pub fn add_bill(&mut self, pos: Pos, recipe: RecipeId, mode: BillMode) {
        let here = self.map.feature[self.map.idx(pos)].structure_id();
        if here != Some(recipe.def().bench.id()) {
            return;
        }
        if let Some(b) = self.bench_mut(pos) {
            b.bills.push(Bill { recipe, mode, suspended: false });
        }
        self.choose_recipes();
    }

    pub fn remove_bill(&mut self, pos: Pos, k: usize) {
        if let Some(b) = self.bench_mut(pos) {
            if k < b.bills.len() {
                b.bills.remove(k);
            }
        }
        self.choose_recipes();
    }

    pub fn set_bill(&mut self, pos: Pos, k: usize, mode: BillMode, suspended: bool) {
        if let Some(bill) = self.bench_mut(pos).and_then(|b| b.bills.get_mut(k)) {
            bill.mode = mode;
            bill.suspended = suspended;
        }
        self.choose_recipes();
    }

    /// Move a bill one place up (earlier bills come first).
    pub fn raise_bill(&mut self, pos: Pos, k: usize) {
        if let Some(b) = self.bench_mut(pos) {
            if k > 0 && k < b.bills.len() {
                b.bills.swap(k - 1, k);
            }
        }
        self.choose_recipes();
    }

    /// How many of each item the colony has: on the map and being carried
    /// (bench stock doesn't count; it's spoken for).
    pub fn item_totals(&self) -> Vec<u32> {
        let mut totals = vec![0; defs().items.len()];
        let shelved = self.map.shelved.values().flatten();
        for s in self.map.items.iter().flatten().chain(shelved).chain(self.pawns.iter().filter_map(|p| p.carrying.as_ref())) {
            totals[s.kind.index()] += s.count;
        }
        totals
    }

    fn bill_wanted(bill: &Bill, totals: &[u32]) -> bool {
        !bill.suspended
            && match bill.mode {
                BillMode::Count(n) => n > 0,
                BillMode::Until(n) => totals[bill.recipe.def().products[0].item.index()] < n,
                BillMode::Forever => true,
            }
    }

    /// Once a second: sync benches with the map and re-pick their recipes.
    pub(crate) fn update_production(&mut self) {
        // Benches that were removed give back their stock; new ones get a record.
        let gone: Vec<usize> = (0..self.benches.len()).filter(|&k| !self.is_workbench(self.benches[k].pos)).collect();
        for k in gone.into_iter().rev() {
            let b = self.benches.remove(k);
            for s in b.stock {
                self.map.drop_item(b.pos, s);
            }
        }
        for i in 0..self.map.len() {
            if matches!(self.map.feature[i], Feature::Built { .. }) {
                let pos = self.map.pos(i);
                self.bench_mut(pos);
            }
        }
        self.choose_recipes();
    }

    /// Each bench works on the first bill that is wanted and can be made with
    /// what's around, sticking with its current one while it still can.
    /// Stock the new recipe doesn't use is put back down beside the bench.
    pub(crate) fn choose_recipes(&mut self) {
        let totals = self.item_totals();
        for k in 0..self.benches.len() {
            let b = &self.benches[k];
            let feasible = |r: RecipeId| r.def().ingredients.iter().all(|a| b.has(a.item) + totals[a.item.index()] >= a.count);
            let wanted = |r: RecipeId| b.bills.iter().any(|bill| bill.recipe == r && Self::bill_wanted(bill, &totals));
            let current = b.current.filter(|&r| wanted(r));
            let next = match current {
                Some(r) if b.ready() || feasible(r) => Some(r),
                _ => b
                    .bills
                    .iter()
                    .find(|bill| Self::bill_wanted(bill, &totals) && feasible(bill.recipe))
                    .map(|bill| bill.recipe)
                    .or(current),
            };
            if next == b.current {
                continue;
            }
            let b = &mut self.benches[k];
            b.current = next;
            let mut keep = Vec::new();
            let mut spill = Vec::new();
            for s in b.stock.drain(..) {
                let want = next.and_then(|r| r.def().ingredients.iter().find(|a| a.item == s.kind)).map_or(0, |a| a.count);
                let kept = s.count.min(want);
                if kept > 0 {
                    keep.push(Stack { kind: s.kind, count: kept });
                }
                if s.count > kept {
                    spill.push(Stack { kind: s.kind, count: s.count - kept });
                }
            }
            b.stock = keep;
            let pos = b.pos;
            for s in spill {
                self.map.drop_item(pos, s);
            }
        }
    }

    /// Deliver part of an ingredient to the bench at `pos`; returns how many it took.
    pub(crate) fn stock_bench(&mut self, pos: Pos, s: Stack) -> u32 {
        let Some(b) = self.benches.iter_mut().find(|b| b.pos == pos) else { return 0 };
        let Some(need) = b.need().filter(|n| n.kind == s.kind) else { return 0 };
        let used = need.count.min(s.count);
        b.add(Stack { kind: s.kind, count: used });
        used
    }

    /// Use up the ingredients at the bench at `pos`, drop the products, and
    /// count the bill down. Returns what was made.
    pub(crate) fn finish_recipe(&mut self, pos: Pos) -> Option<RecipeId> {
        let totals = self.item_totals();
        let b = self.benches.iter_mut().find(|b| b.pos == pos)?;
        if !b.ready() {
            return None;
        }
        let r = b.current?;
        for a in &r.def().ingredients {
            let mut left = a.count;
            for s in b.stock.iter_mut().filter(|s| s.kind == a.item) {
                let take = left.min(s.count);
                s.count -= take;
                left -= take;
            }
        }
        b.stock.retain(|s| s.count > 0);
        if let Some(k) = b.bills.iter().position(|bill| bill.recipe == r && Self::bill_wanted(bill, &totals)) {
            if let BillMode::Count(n) = &mut b.bills[k].mode {
                *n -= 1;
                if *n == 0 {
                    b.bills.remove(k);
                }
            }
        }
        // A themed room may give a spare of each product.
        let extra = self.room_effects(pos).map_or(0.0, |(_, e)| e.extra);
        let spare = (extra > 0.0 && self.rng.chance(extra)) as u32;
        for a in &r.def().products {
            self.map.drop_item(pos, Stack { kind: a.item, count: a.count + spare });
        }
        self.choose_recipes();
        Some(r)
    }

    // ---------- stockpiles and zones ----------

    /// Priority of the stockpile at cell `i` for `kind`, if it takes it.
    pub(crate) fn storage_priority(&self, i: usize, kind: ItemId) -> Option<u8> {
        let id = self.map.zone[i].stockpile()?;
        let s = self.stockpiles.get(id as usize)?;
        s.accepts(kind).then_some(s.priority)
    }

    /// Stockpile id for a new cell at `p`: a stockpile next to it on the same
    /// level (not a shelf's own storage), or a new one.
    pub(crate) fn stockpile_for(&mut self, p: Pos) -> u16 {
        let next = self.map.neighbors(p).find_map(|nb| {
            let id = self.map.zone[self.map.idx(nb)].stockpile()?;
            (!self.is_container(id)).then_some(id)
        });
        next.unwrap_or_else(|| {
            let number = self.stockpiles.iter().filter(|s| !s.container).count() + 1;
            self.stockpiles.push(Stockpile::new(number));
            (self.stockpiles.len() - 1) as u16
        })
    }

    /// Whether stockpile `id` is the storage of shelves rather than a painted zone.
    pub fn is_container(&self, id: u16) -> bool {
        self.stockpiles.get(id as usize).is_some_and(|s| s.container)
    }

    /// Whether cell `i` belongs to a shelf's own storage.
    pub fn container_at(&self, i: usize) -> bool {
        self.map.zone[i].stockpile().is_some_and(|id| self.is_container(id))
    }

    /// Storage furniture is storage by itself. One standing outside any
    /// stockpile gets settings of its own, shared with the same furniture
    /// beside it that has its own. Where the furniture is gone, its storage
    /// goes too, and whatever no longer fits the cell spills over.
    /// What makes cell `i` storage by itself: its furniture (a shelf), or the
    /// shelves on its wall.
    fn storage_kind(&self, i: usize) -> Option<crate::defs::BuildId> {
        if self.map.wall_shelves(i) > 0 {
            return self.map.decor(i).iter().find(|d| d.built && d.kind.def().storage.is_some()).map(|d| d.kind);
        }
        self.map.feature[i].structure_id().filter(|k| k.def().storage.is_some())
    }

    pub(crate) fn sync_containers(&mut self) {
        for i in 0..self.map.len() {
            // Most cells have neither furniture nor storage: skip them quickly.
            let zoned = matches!(self.map.zone[i], Zone::Stockpile(_));
            if !zoned && !matches!(self.map.feature[i], Feature::Built { .. }) {
                continue;
            }
            let kind = self.storage_kind(i);
            let storage = kind.is_some();
            let p = self.map.pos(i);
            if storage && self.map.zone[i].stockpile().is_none() {
                let next = self.map.neighbors(p).find_map(|nb| {
                    let ni = self.map.idx(nb);
                    let id = self.map.zone[ni].stockpile()?;
                    (self.is_container(id) && self.storage_kind(ni) == kind).then_some(id)
                });
                let id = next.unwrap_or_else(|| {
                    let name = kind.map_or("Storage", |k| k.name());
                    let number = self.stockpiles.iter().filter(|s| s.container && s.name.starts_with(name)).count() + 1;
                    // Preferred, so things move onto shelves from plain stockpiles.
                    let store = Stockpile { name: format!("{} {}", name, number), priority: 3, container: true, ..Stockpile::new(0) };
                    self.stockpiles.push(store);
                    (self.stockpiles.len() - 1) as u16
                });
                self.map.zone[i] = Zone::Stockpile(id);
            } else if !storage && self.container_at(i) {
                self.map.zone[i] = Zone::None;
            }
            // More than fits (the shelf went, or a shelf from an old save with one
            // big stack): into the slots it has, the rest put down nearby.
            let stacks = self.map.cell_stacks(i);
            if stacks.len() > self.map.slots(i) || stacks.iter().any(|s| s.count > crate::map::STACK_MAX) {
                self.map.settle_cell(i);
            }
        }
    }

    pub fn set_stockpile_priority(&mut self, id: u16, priority: u8) {
        if let Some(s) = self.stockpiles.get_mut(id as usize) {
            s.priority = priority.clamp(1, 4);
        }
    }

    pub fn set_stockpile_allows(&mut self, id: u16, kind: ItemId, allow: bool) {
        if let Some(s) = self.stockpiles.get_mut(id as usize) {
            s.forbid.retain(|&k| k != kind);
            if !allow {
                s.forbid.push(kind);
            }
        }
    }

    /// Change the crop of the whole grow zone (same level, connected) around `p`.
    pub fn set_grow_crop(&mut self, p: Pos, crop: crate::defs::PlantId) {
        if self.map.zone[self.map.idx(p)].crop().is_none() || crop.def().sow.is_none() {
            return;
        }
        let mut stack = vec![p];
        let mut seen = vec![p];
        while let Some(q) = stack.pop() {
            let i = self.map.idx(q);
            self.map.zone[i] = Zone::Grow(crop);
            for nb in self.map.neighbors(q) {
                if self.map.zone[self.map.idx(nb)].crop().is_some() && !seen.contains(&nb) {
                    seen.push(nb);
                    stack.push(nb);
                }
            }
        }
    }

    // ---------- gear ----------

    /// Gear `p` should go and get: what's in their loadout that they don't
    /// have on (nearest first), or else for a slot their loadout leaves free,
    /// something better than what they have.
    pub(crate) fn equip_candidate(&self, p: &Pawn, nearest: &[Option<(i32, Pos)>]) -> Option<Job> {
        let missing = p.loadout.iter().copied().filter(|&k| !p.has_on(k)).filter_map(|k| nearest[k.index()]).min_by_key(|(d, _)| *d);
        if let Some((_, from)) = missing {
            return Some(Job::Equip { from, chosen: false });
        }
        ItemId::all()
            .filter(|&k| wants_upgrade(p, k, self.outdoor_temperature()))
            .filter_map(|k| nearest[k.index()])
            .min_by_key(|(d, _)| *d)
            .map(|(_, from)| Job::Equip { from, chosen: false })
    }

    pub(crate) fn run_equip(&mut self, p: &mut Pawn, from: Pos, chosen: bool, dt: f32) {
        // (Told to by the player: `order_equip` has put it in their loadout.)
        let there = self.walk_to(p, from, false, dt);
        if p.job.is_none() && chosen {
            self.push_log(format!("{} can't reach that.", p.name));
        }
        if there {
            let i = self.map.idx(from);
            // What they came for: their loadout's, else (gone already) an
            // upgrade for a slot it leaves free. Told to by the player: the
            // first weapon or piece of apparel here (a shelf may hold several things).
            let gear: Vec<ItemId> = self.map.stacks(i).map(|s| s.kind).filter(|k| k.def().is_gear()).collect();
            let outdoors = self.outdoor_temperature();
            let kind = gear
                .iter()
                .copied()
                .find(|&k| p.loadout.contains(&k) && !p.has_on(k))
                .or_else(|| gear.iter().copied().find(|&k| wants_upgrade(p, k, outdoors)))
                .or(gear.first().copied().filter(|_| chosen));
            if let Some(s) = kind.and_then(|k| self.map.take_item(i, k, 1)) {
                self.res_item[i] = 0;
                let d = s.kind.def();
                if let Some(old) = p.wear(s.kind) {
                    self.map.drop_item(p.pos, Stack { kind: old, count: 1 });
                }
                self.push_log(format!("{} picked up {}.", p.name, d.name.to_lowercase()));
            }
            self.end_job(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{colonist, item, quiet_world, run};
    use crate::defs::PlantId;
    use crate::map::{Block, Desig, FLAT_Z as Z};
    use crate::pawn::Memory;
    use crate::world::Order;
    use crate::TICK;


    fn test_world(w: usize, h: usize) -> World {
        quiet_world(w, h, 7)
    }



    /// On the map, carried, or worn.
    fn have(w: &World, kind: ItemId) -> u32 {
        let worn = w.pawns.iter().filter(|p| p.weapon == Some(kind) || p.apparel.contains(&kind)).count() as u32;
        w.item_totals()[kind.index()] + worn
    }

    fn place_bench(w: &mut World, id: &str, at: Pos) {
        let i = w.map.idx(at);
        w.map.set_feature(i, Feature::built(id));
    }

    /// Every recipe's ingredients get delivered to its bench and turned into its products.
    #[test]
    fn every_recipe_can_be_made() {
        for r in RecipeId::all() {
            let d = r.def();
            let mut w = test_world(16, 16);
            let id = colonist(&mut w, (2, 2, Z));
            // Unarmed, so a weapon that gets made is easy to count; otherwise
            // in the best gear, so they don't pick up what's to be taken apart.
            let i = w.pawn_index(id).unwrap();
            let makes = |f: fn(&crate::defs::ItemDef) -> bool| d.products.iter().any(|a| f(a.item.def()));
            w.pawns[i].weapon = if makes(|d| d.weapon.is_some()) { None } else { Some(item("laser_rifle")) };
            if !makes(|d| d.apparel.is_some()) {
                w.pawns[i].apparel = vec![item("clothes"), item("powered_armor"), item("steel_helmet")];
            }
            let bench = (8, 8, Z);
            place_bench(&mut w, &d.bench.id().def().id, bench);
            for (k, a) in d.ingredients.iter().enumerate() {
                w.map.drop_item((3 + 2 * k, 12, Z), Stack { kind: a.item, count: a.count });
            }
            w.add_bill(bench, r, BillMode::Count(1));
            assert_eq!(w.bench_at(bench).unwrap().bills.len(), 1, "{}: bill added", d.id);
            // A trip per ingredient, then the work.
            run(&mut w, 40.0 + 15.0 * d.ingredients.len() as f32 + 2.0 * d.time);
            for a in &d.products {
                assert_eq!(have(&w, a.item), a.count, "{} makes {}", d.id, a.item.def().id);
            }
            // (On the map or carried: what the colonist wears isn't an ingredient.)
            for a in &d.ingredients {
                assert_eq!(w.item_totals()[a.item.index()], 0, "{} uses its {}", d.id, a.item.def().id);
            }
            assert!(w.bench_at(bench).unwrap().bills.is_empty(), "{}: bill done", d.id);
        }
    }

    #[test]
    fn until_bills_stop_at_the_target() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        let stove = (8, 8, Z);
        place_bench(&mut w, "stove", stove);
        w.map.drop_item((3, 12, Z), Stack { kind: item("raw_food"), count: 12 });
        let cook = RecipeId::named("cook_meal");
        w.add_bill(stove, cook, BillMode::Until(2));
        run(&mut w, 90.0);
        assert_eq!(w.map.count_items(item("meal")), 2);
        assert_eq!(w.map.count_items(item("raw_food")), 8, "leftover ingredients are put back");
        assert_eq!(w.bench_at(stove).unwrap().bills.len(), 1, "until-bills stay");
    }

    #[test]
    fn stockpiles_respect_filters_and_priorities() {
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        for x in 2..5 {
            assert!(w.apply_order((x, 10, Z), Order::Stockpile));
            assert!(w.apply_order((x, 16, Z), Order::Stockpile));
        }
        let (a, b) = (w.map.zone[w.map.idx((2, 10, Z))].stockpile().unwrap(), w.map.zone[w.map.idx((2, 16, Z))].stockpile().unwrap());
        assert_ne!(a, b, "separate areas are separate stockpiles");
        assert_eq!(w.map.zone[w.map.idx((4, 10, Z))].stockpile(), Some(a), "painting next to one extends it");
        w.set_stockpile_allows(a, item("wood"), false);
        w.map.drop_item((12, 12, Z), Stack { kind: item("wood"), count: 10 });
        run(&mut w, 120.0);
        let in_b: u32 = (2..5).filter_map(|x| w.map.items[w.map.idx((x, 16, Z))]).map(|s| s.count).sum();
        assert_eq!(in_b, 10, "wood skips the stockpile that refuses it");

        // A more important stockpile pulls things out of a less important one.
        w.set_stockpile_priority(a, 4);
        w.set_stockpile_allows(a, item("wood"), true);
        run(&mut w, 120.0);
        let in_a: u32 = (2..5).filter_map(|x| w.map.items[w.map.idx((x, 10, Z))]).map(|s| s.count).sum();
        assert_eq!(in_a, 10);
    }

    #[test]
    fn colonists_pick_up_better_gear() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = None;
        w.map.drop_item((8, 8, Z), Stack { kind: item("knife"), count: 1 });
        w.map.drop_item((10, 4, Z), Stack { kind: item("clothes"), count: 1 });
        w.map.drop_item((4, 10, Z), Stack { kind: item("steel_armor"), count: 1 });
        run(&mut w, 80.0);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.weapon, Some(item("knife")));
        assert!(p.apparel.contains(&item("clothes")) && p.apparel.contains(&item("steel_armor")), "{:?}", p.apparel);
        assert!(p.armor() > 0.3);
        assert!(p.thoughts().iter().any(|t| t.0 == item("clothes").name()), "clothes feel nice");

        // A better weapon replaces the old one, which is left on the ground.
        w.map.drop_item((12, 12, Z), Stack { kind: item("rifle"), count: 1 });
        run(&mut w, 80.0);
        assert_eq!(w.pawn(id).unwrap().weapon, Some(item("rifle")));
        assert_eq!(w.map.count_items(item("knife")), 1);
    }

    #[test]
    fn colonists_equip_what_they_are_told_and_keep_it() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        assert_eq!(w.pawn(id).unwrap().weapon, Some(item("rifle")));
        w.map.drop_item((8, 8, Z), Stack { kind: item("club"), count: 1 });
        w.map.drop_item((6, 2, Z), Stack { kind: item("wood"), count: 5 });
        assert!(!w.order_equip(id, (6, 2, Z)), "wood isn't gear");
        assert!(!w.order_equip(id, (9, 9, Z)), "nothing there");
        assert!(w.order_equip(id, (8, 8, Z)));
        run(&mut w, 40.0);
        // A worse weapon, because the player said so; the rifle is left behind
        // and not picked up again.
        assert_eq!(w.pawn(id).unwrap().weapon, Some(item("club")));
        run(&mut w, 80.0);
        assert_eq!(w.pawn(id).unwrap().weapon, Some(item("club")));
        assert_eq!(w.map.count_items(item("rifle")), 1);

        // Switching drops the old choice.
        let rifle = (0..w.map.len()).find(|&i| w.map.items[i].is_some_and(|s| s.kind == item("rifle"))).unwrap();
        assert!(w.order_equip(id, w.map.pos(rifle)));
        run(&mut w, 40.0);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.weapon, Some(item("rifle")));
        assert_eq!(p.loadout, vec![item("rifle")], "what they were told is their loadout");
    }

    #[test]
    fn gear_kept_in_old_saves_moves_into_the_loadout() {
        let mut w = test_world(12, 12);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].loadout = vec![item("steel_armor")];
        let mut v: serde_json::Value = serde_json::from_str(&w.save_json().unwrap()).unwrap();
        // As saved before: the player's kept rifle and armour (the loadout's armour slot is taken).
        let pawn = v["world"]["pawns"].as_array_mut().unwrap().iter_mut().find(|p| p["id"] == id).unwrap();
        pawn["chosen"] = serde_json::json!(["rifle", "powered_armor"]);
        let loaded = World::load_json(&v.to_string()).unwrap();
        let p = loaded.pawn(id).unwrap();
        assert_eq!(p.loadout, vec![item("steel_armor"), item("rifle")]);
        assert!(p.chosen.is_empty());
    }

    #[test]
    fn fetching_their_loadout_they_take_nothing_else() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].weapon = Some(item("rifle"));
        // A shelf with a club and the laser rifle the loadout asks for.
        place_bench(&mut w, "shelf", (12, 12, Z));
        w.sync_containers();
        let shelf = w.map.idx((12, 12, Z));
        w.map.set_stacks(shelf, vec![Stack { kind: item("club"), count: 1 }, Stack { kind: item("laser_rifle"), count: 1 }]);
        w.set_loadout(id, &[item("laser_rifle")]).unwrap();
        run(&mut w, 2.0);
        assert!(matches!(w.pawn(id).unwrap().job, Some(Job::Equip { .. })), "on the way");
        // Someone else takes the laser rifle first.
        w.map.take_item(shelf, item("laser_rifle"), 1);
        run(&mut w, 60.0);
        assert_eq!(w.pawn(id).unwrap().weapon, Some(item("rifle")), "not the club instead");
        assert_eq!(w.map.count_in(shelf, item("club")), 1);
    }

    #[test]
    fn colonists_dress_for_the_weather() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        w.map.drop_item((8, 8, Z), Stack { kind: item("steel_armor"), count: 1 });
        w.map.drop_item((9, 8, Z), Stack { kind: item("parka"), count: 1 });
        let outer = |w: &World| w.pawn(id).unwrap().worn(crate::defs::Slot::Outer);
        // Summer: the armour.
        w.time = crate::seasons::SEASON_LEN + 2.6 * crate::DAY_LEN;
        run(&mut w, 60.0);
        assert_eq!(outer(&w), Some(item("steel_armor")));
        // Winter: the parka, the armour put down.
        w.time = 3.0 * crate::seasons::SEASON_LEN + 0.6 * crate::DAY_LEN;
        run(&mut w, 60.0);
        assert_eq!(outer(&w), Some(item("parka")));
        assert_eq!(w.map.count_items(item("steel_armor")), 1);
    }

    #[test]
    fn colonists_fetch_their_loadout_and_keep_it() {
        let mut w = test_world(20, 20);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].apparel = vec![item("clothes")];
        w.pawns[i].weapon = Some(item("knife"));
        w.map.drop_item((8, 8, Z), Stack { kind: item("steel_armor"), count: 1 });
        w.map.drop_item((9, 8, Z), Stack { kind: item("club"), count: 1 });
        w.map.drop_item((10, 8, Z), Stack { kind: item("rifle"), count: 1 });
        // Their loadout: the club, though the rifle is better. Armour isn't in it.
        w.set_loadout(id, &[item("club")]).unwrap();
        run(&mut w, 120.0);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.weapon, Some(item("club")), "the loadout's weapon, not the better one");
        assert!(p.apparel.contains(&item("steel_armor")), "a free slot: the best they find");
        assert!(p.apparel.contains(&item("clothes")));
        assert_eq!(w.map.count_items(item("rifle")), 1, "left alone");
        assert_eq!(w.map.count_items(item("knife")), 1, "what the club replaced is put down");
    }

    #[test]
    fn colonists_prefer_meals_and_dislike_raw_food() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        w.map.drop_item((4, 4, Z), Stack { kind: item("raw_food"), count: 5 });
        w.map.drop_item((10, 10, Z), Stack { kind: item("meal"), count: 1 });
        run(&mut w, 60.0);
        assert_eq!(w.map.count_items(item("meal")), 0, "the meal, though further away");
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::AteMeal));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        run(&mut w, 60.0);
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::AteRawFood));
    }

    #[test]
    fn mining_ore_yields_iron_ore() {
        let mut w = test_world(12, 12);
        colonist(&mut w, (2, 2, Z));
        let i = w.map.idx((6, 6, Z));
        w.map.set_block(i, Block::Ore);
        assert!(w.apply_order((6, 6, Z), Order::Desig(Desig::Mine)));
        run(&mut w, 20.0);
        let ore = &crate::defs::defs().ore_drops;
        assert_eq!(w.map.count_items(ore.item), ore.count);
    }

    #[test]
    fn grow_zones_plant_their_chosen_crop() {
        let mut w = test_world(12, 12);
        colonist(&mut w, (2, 2, Z));
        let first = crate::defs::defs().sown().unwrap();
        for x in 5..8 {
            assert!(w.apply_order((x, 6, Z), Order::Zone(Zone::Grow(first))));
        }
        let healroot = PlantId::named("healroot");
        w.set_grow_crop((5, 6, Z), healroot);
        run(&mut w, 15.0);
        for x in 5..8 {
            let f = w.map.feature[w.map.idx((x, 6, Z))];
            assert!(matches!(f, Feature::Plant { kind, .. } if kind == healroot), "{:?}", f);
        }
    }

    #[test]
    fn colonists_eat_at_a_table_if_there_is_one() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        w.map.drop_item((4, 4, Z), Stack { kind: item("meal"), count: 1 });
        place_bench(&mut w, "table", (10, 10, Z));
        place_bench(&mut w, "chair", (10, 11, Z));
        let mut sat = false;
        for _ in 0..(60.0 / TICK) as usize {
            w.tick(TICK);
            let p = w.pawn(id).unwrap();
            sat |= matches!(p.job, Some(Job::Eat { picked: true, .. })) && p.pos == (10, 11, Z);
        }
        assert!(sat, "ate sitting on the chair");
        let p = w.pawn(id).unwrap();
        assert!(p.hunger > 50.0);
        assert!(!p.memories.iter().any(|m| m.kind == Memory::AteWithoutTable));

        // Without a table it's a sad meal on the spot.
        let t = w.map.idx((10, 10, Z));
        w.map.set_feature(t, Feature::None);
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        w.map.drop_item((4, 4, Z), Stack { kind: item("meal"), count: 1 });
        run(&mut w, 60.0);
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::AteWithoutTable));
    }

    /// Run for `secs`, returning the most anyone carried at once.
    fn most_carried(w: &mut World, secs: f32) -> u32 {
        let mut most = 0;
        for _ in 0..(secs / TICK) as usize {
            w.tick(TICK);
            most = most.max(w.pawns.iter().filter_map(|p| p.carrying).map(|c| c.count).max().unwrap_or(0));
        }
        most
    }

    #[test]
    fn haulers_gather_nearby_piles_into_one_load() {
        let mut w = test_world(24, 24);
        colonist(&mut w, (2, 2, Z));
        assert!(w.apply_order((20, 20, Z), Order::Stockpile));
        // Five chopped trees' worth, a few cells apart.
        for (x, y) in [(4, 4), (6, 4), (5, 6), (7, 7), (3, 7)] {
            w.map.drop_item((x, y, Z), Stack { kind: item("wood"), count: 10 });
        }
        let most = most_carried(&mut w, 90.0);
        assert!(most >= 40, "picked up the piles together: at most {} at once", most);
        let stored = w.map.items[w.map.idx((20, 20, Z))].map_or(0, |s| s.count);
        assert_eq!(stored, 50, "all of it in the stockpile");
    }

    #[test]
    fn haulers_supply_several_blueprints_per_trip() {
        let mut w = test_world(30, 16);
        let id = colonist(&mut w, (2, 8, Z));
        // A row of six walls (4 stone each), the stone far away.
        let wall = crate::defs::BuildId::named("wall");
        for x in 20..26 {
            assert!(w.apply_order((x, 8, Z), Order::Build(wall)));
        }
        w.map.drop_item((2, 2, Z), Stack { kind: item("stone"), count: 30 });
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].priorities[crate::pawn::WorkType::Construction as usize] = 1;
        let most = most_carried(&mut w, 120.0);
        assert!(most >= 20, "stone for several walls at once: at most {}", most);
        let built = (20..26).filter(|&x| w.map.feature[w.map.idx((x, 8, Z))] == Feature::built("wall")).count();
        assert_eq!(built, 6, "all walls up");
        assert_eq!(w.map.count_items(item("stone")), 6, "the rest left at the pile");
    }

    #[test]
    fn helmets_armour_and_clothes_are_worn_together() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].apparel = vec![item("clothes")];
        for kind in ["steel_helmet", "steel_armor"] {
            w.map.drop_item((6, 6, Z), Stack { kind: item(kind), count: 1 });
            run(&mut w, 20.0);
        }
        let worn = &w.pawn(id).unwrap().apparel;
        for kind in ["clothes", "steel_armor", "steel_helmet"] {
            assert!(worn.contains(&item(kind)), "wears {}: {:?}", kind, worn);
        }
        // What shows of them: plates on the chest and shoulders, a helmet over the hair.
        let pieces = |k: &str| item(k).def().apparel.as_ref().unwrap().pieces.clone();
        let bones: Vec<String> = pieces("steel_armor").iter().map(|p| p.bone.clone()).collect();
        assert_eq!(bones, ["spine_03", "upperarm_l", "upperarm_r"]);
        assert!(pieces("steel_helmet").iter().any(|p| p.bone == "Head" && p.hides_hair));
        // Powered armour: plates on the chest and arms, and a sealed helmet with a glowing visor.
        let powered = pieces("powered_armor");
        assert!(powered.iter().any(|p| p.bone == "spine_03"));
        assert!(powered.iter().any(|p| p.bone == "Head" && p.hides_hair && p.parts.iter().any(|q| q.color == "visor")));
        assert!(item("powered_armor").def().apparel.as_ref().unwrap().armor > item("steel_armor").def().apparel.as_ref().unwrap().armor);
    }

    #[test]
    fn cooks_fetch_their_own_ingredients() {
        let mut w = test_world(20, 20);
        let cook = colonist(&mut w, (2, 2, Z));
        // Nobody hauls: only cooking is switched on.
        let i = w.pawn_index(cook).unwrap();
        w.pawns[i].priorities = [0; crate::pawn::WorkType::ALL.len()];
        w.pawns[i].priorities[crate::pawn::WorkType::Cooking as usize] = 1;
        let stove = (10, 10, Z);
        place_bench(&mut w, "stove", stove);
        place_bench(&mut w, "shelf", (16, 16, Z));
        w.sync_containers();
        let on_shelf = w.map.idx((16, 16, Z));
        w.map.items[on_shelf] = Some(Stack { kind: item("raw_food"), count: 30 });
        w.add_bill(stove, RecipeId::named("cook_meal"), BillMode::Count(3));
        // The stove offers the bill to be prioritized, ingredients and all.
        let options = w.job_options(cook, stove);
        assert!(options.iter().any(|(l, _)| l == "Prioritize cook simple meal at Stove (bringing raw food first)"), "{:?}", options);
        run(&mut w, 120.0);
        assert_eq!(w.map.count_items(item("meal")), 3, "fetched the raw food and cooked");
    }

    /// Where the only (cooking) colonist stands once they're cooking at the stove at `stove`.
    fn cooking_spot(w: &mut World, cook: u32, stove: Pos) -> Pos {
        let i = w.pawn_index(cook).unwrap();
        w.pawns[i].priorities = [0; crate::pawn::WorkType::ALL.len()];
        w.pawns[i].priorities[crate::pawn::WorkType::Cooking as usize] = 1;
        w.map.drop_item((stove.0 - 2, stove.1, Z), Stack { kind: item("raw_food"), count: 10 });
        w.add_bill(stove, RecipeId::named("cook_meal"), BillMode::Count(5));
        for _ in 0..(60.0 / TICK) as usize {
            w.tick(TICK);
            let p = w.pawn(cook).unwrap();
            if matches!(p.job, Some(Job::Work { task: crate::pawn::Task::Craft, .. })) && p.job_progress > 0.0 {
                return p.pos;
            }
        }
        panic!("never got to cooking");
    }

    #[test]
    fn crafters_stand_at_the_front() {
        let mut w = test_world(20, 20);
        let cook = colonist(&mut w, (2, 2, Z));
        let stove = (10, 10, Z);
        place_bench(&mut w, "stove", stove);
        let i = w.map.idx(stove);
        w.map.set_feature(i, Feature::Built { kind: crate::defs::BuildId::named("stove"), dir: 2 });
        assert_eq!(cooking_spot(&mut w, cook, stove), (9, 10, Z), "at the front, turned towards -x");
        // With the front walled off, from beside it.
        let mut w = test_world(20, 20);
        let cook = colonist(&mut w, (2, 2, Z));
        place_bench(&mut w, "stove", stove);
        let front = w.map.front_cell(stove).unwrap();
        let fi = w.map.idx(front);
        w.map.set_feature(fi, Feature::built("wall"));
        let at = cooking_spot(&mut w, cook, stove);
        assert!(crate::map::adjacent(at, stove) && at != front, "{:?}", at);
    }

    #[test]
    fn a_big_stack_feeds_everyone_at_once() {
        let mut w = test_world(20, 20);
        let ids: Vec<u32> = (0..4).map(|k| colonist(&mut w, (2 + k, 2, Z))).collect();
        for &id in &ids {
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].hunger = 10.0;
            w.pawns[i].priorities = [0; crate::pawn::WorkType::ALL.len()];
        }
        place_bench(&mut w, "shelf", (12, 12, Z));
        w.sync_containers();
        let on_shelf = w.map.idx((12, 12, Z));
        w.map.items[on_shelf] = Some(Stack { kind: item("raw_food"), count: 100 });
        run(&mut w, 4.0);
        let eating = ids.iter().filter(|&&id| matches!(w.pawn(id).unwrap().job, Some(Job::Eat { .. }))).count();
        assert_eq!(eating, 4, "all of them go for the one stack");
        run(&mut w, 40.0);
        assert!(ids.iter().all(|&id| w.pawn(id).unwrap().hunger > 30.0), "and all of them ate");
    }

    #[test]
    fn the_starving_wake_up_or_snap_out_to_eat() {
        let mut w = test_world(16, 16);
        let (sleeper, dazed) = (colonist(&mut w, (2, 2, Z)), colonist(&mut w, (4, 2, Z)));
        w.map.drop_item((8, 8, Z), Stack { kind: item("raw_food"), count: 10 });
        let (a, b) = (w.pawn_index(sleeper).unwrap(), w.pawn_index(dazed).unwrap());
        w.pawns[a].rest = 10.0;
        w.pawns[a].job = Some(Job::Sleep { bed: None });
        w.pawns[b].break_timer = 200.0;
        for i in [a, b] {
            w.pawns[i].hunger = 5.0;
        }
        run(&mut w, 30.0);
        assert!(w.pawn(sleeper).unwrap().hunger > 30.0, "woke up to eat");
        assert!(w.pawn(dazed).unwrap().hunger > 30.0, "ate, even in a daze");
    }

    #[test]
    fn refused_items_are_cleared_out() {
        // Another stockpile takes them: they go there.
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        assert!(w.apply_order((5, 5, Z), Order::Stockpile));
        assert!(w.apply_order((15, 15, Z), Order::Stockpile));
        w.map.drop_item((5, 5, Z), Stack { kind: item("wood"), count: 20 });
        let a = w.map.zone[w.map.idx((5, 5, Z))].stockpile().unwrap();
        w.set_stockpile_allows(a, item("wood"), false);
        run(&mut w, 60.0);
        assert_eq!(w.map.items[w.map.idx((15, 15, Z))].map(|s| s.count), Some(20), "to the stockpile that takes it");

        // Nothing takes them: out onto open ground beside it, and left there.
        let mut w = test_world(20, 20);
        colonist(&mut w, (2, 2, Z));
        for x in 5..8 {
            assert!(w.apply_order((x, 5, Z), Order::Stockpile));
        }
        place_bench(&mut w, "shelf", (10, 10, Z));
        w.sync_containers();
        w.map.drop_item((6, 5, Z), Stack { kind: item("wood"), count: 20 });
        let on_shelf = w.map.idx((10, 10, Z));
        w.map.items[on_shelf] = Some(Stack { kind: item("stone"), count: 30 });
        let a = w.map.zone[w.map.idx((5, 5, Z))].stockpile().unwrap();
        let shelf = w.map.zone[w.map.idx((10, 10, Z))].stockpile().unwrap();
        w.set_stockpile_allows(a, item("wood"), false);
        w.set_stockpile_allows(a, item("stone"), false);
        w.set_stockpile_allows(shelf, item("stone"), false);
        w.set_stockpile_allows(shelf, item("wood"), false);
        run(&mut w, 90.0);
        let stored = |w: &World, kind| (0..w.map.len()).filter(|&i| w.map.zone[i].stockpile().is_some()).filter_map(|i| w.map.items[i]).filter(|s| s.kind == kind).map(|s| s.count).sum::<u32>();
        assert_eq!(stored(&w, item("wood")), 0, "the wood is out of the stockpile");
        assert_eq!(stored(&w, item("stone")), 0, "the stone is off the shelf");
        assert_eq!(w.map.count_items(item("wood")), 20, "none lost");
        assert_eq!(w.map.count_items(item("stone")), 30);
    }

    #[test]
    fn shelves_block_the_way() {
        let mut w = test_world(16, 16);
        // A row of shelves across the map, with one gap at y = 14.
        for y in 0..16 {
            if y != 14 {
                place_bench(&mut w, "shelf", (8, y, Z));
            }
        }
        let df = crate::pathing::bfs(&w.map, (2, 2, Z));
        assert_eq!(df.dist((12, 2, Z)), Some(10 + 2 * 12), "round by the gap, not through");
        assert!(df.dist((8, 2, Z)).is_some(), "but the shelves can be reached");
        assert_eq!(df.approach(&w.map, (8, 2, Z), false).map(|a| a.0), Some((7, 2, Z)), "from beside");
    }

    #[test]
    fn a_shelf_holds_four_different_things() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        place_bench(&mut w, "shelf", (5, 5, Z));
        w.sync_containers();
        let shelf = w.map.idx((5, 5, Z));
        assert_eq!(w.map.slots(shelf), 4);
        // Five kinds lying about: four fit on the shelf, a stack each.
        for (k, (id, n)) in [("wood", 50), ("stone", 40), ("steel", 30), ("copper", 20), ("cloth", 10)].into_iter().enumerate() {
            w.map.drop_item((10 + k % 3, 10 + k / 3, Z), Stack { kind: item(id), count: n });
        }
        run(&mut w, 300.0);
        let on: Vec<Stack> = w.map.cell_stacks(shelf);
        assert_eq!(on.len(), 4, "four slots used: {:?}", on);
        let kinds: std::collections::HashSet<_> = on.iter().map(|s| s.kind).collect();
        assert_eq!(kinds.len(), 4, "four different things");
        assert!(on.iter().all(|s| s.count <= crate::map::STACK_MAX));
        assert_eq!(w.map.count_items_where(|_| true), 150, "nothing lost (one kind waits on the ground)");
        // A cook takes raw food from among other things.
        let shelf2 = (12, 5, Z);
        place_bench(&mut w, "shelf", shelf2);
        w.sync_containers();
        let s2 = w.map.idx(shelf2);
        w.map.set_stacks(s2, vec![Stack { kind: item("stone"), count: 10 }, Stack { kind: item("raw_food"), count: 20 }]);
        assert_eq!(w.map.take_item(s2, item("raw_food"), 5).map(|s| s.count), Some(5));
        assert_eq!(w.map.count_in(s2, item("raw_food")), 15);
        assert_eq!(w.nearest_food((2, 2, Z)), Some(shelf2), "food on a mixed shelf is found");
    }

    #[test]
    fn shelves_are_storage_by_themselves() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        // Two side by side share their settings; one apart has its own.
        for x in [5, 6, 10] {
            place_bench(&mut w, "shelf", (x, 5, Z));
        }
        w.sync_containers();
        let id = |w: &World, x: usize| w.map.zone[w.map.idx((x, 5, Z))].stockpile().unwrap();
        assert_eq!(id(&w, 5), id(&w, 6));
        assert_ne!(id(&w, 5), id(&w, 10));
        assert!(w.is_container(id(&w, 5)));
        assert_eq!(w.stockpiles[id(&w, 10) as usize].name, "Shelf 2");
        // The one apart takes no wood, so it all goes on the pair.
        w.set_stockpile_allows(id(&w, 10), item("wood"), false);
        w.map.drop_item((12, 12, Z), Stack { kind: item("wood"), count: 200 });
        // Stocked from beside: nobody steps onto a shelf.
        for _ in 0..(300.0 / TICK) as usize {
            w.tick(TICK);
            assert!(w.pawns.iter().all(|p| ![5, 6, 10].contains(&p.pos.0) || p.pos.1 != 5), "on a shelf");
        }
        let on = |w: &World, x: usize| w.map.count_in(w.map.idx((x, 5, Z)), item("wood"));
        assert_eq!((on(&w, 5) + on(&w, 6), on(&w, 10)), (200, 0));
        assert!(on(&w, 5).max(on(&w, 6)) > crate::map::STACK_MAX, "they hold more than the ground");
    }

    #[test]
    fn old_shelves_with_one_big_stack_settle_into_slots() {
        let mut w = test_world(16, 16);
        place_bench(&mut w, "shelf", (5, 5, Z));
        let i = w.map.idx((5, 5, Z));
        // As saves from before had it: 200 stone in one stack.
        w.map.items[i] = Some(Stack { kind: item("stone"), count: 200 });
        w.sync_containers();
        let on = w.map.cell_stacks(i);
        assert_eq!(on.len(), 4);
        assert!(on.iter().all(|s| s.kind == item("stone") && s.count == crate::map::STACK_MAX));
        // Saved and loaded, the slots stay.
        let back = crate::save::from_json(&crate::save::to_json(&w).unwrap()).unwrap();
        assert_eq!(back.map.cell_stacks(i), on);
    }

    #[test]
    fn a_shelfs_storage_goes_with_it() {
        let mut w = test_world(16, 16);
        place_bench(&mut w, "shelf", (5, 5, Z));
        w.sync_containers();
        let i = w.map.idx((5, 5, Z));
        assert!(w.container_at(i));
        assert!(!w.apply_order((5, 5, Z), Order::Zone(Zone::None)), "clearing zones leaves shelves be");
        assert!(!w.apply_order((5, 5, Z), Order::Zone(Zone::Safe)));
        // A stockpile painted beside it is a stockpile of its own.
        assert!(w.apply_order((6, 5, Z), Order::Stockpile));
        let beside = w.map.zone[w.map.idx((6, 5, Z))].stockpile().unwrap();
        assert!(!w.is_container(beside));
        assert_eq!(w.stockpiles[beside as usize].name, "Stockpile 1");
        // Taken down: no more storage, and what doesn't fit on the ground spills over.
        w.map.items[i] = Some(Stack { kind: item("wood"), count: 120 });
        w.map.set_feature(i, Feature::None);
        w.sync_containers();
        assert_eq!(w.map.zone[i], Zone::None);
        assert_eq!(w.map.items[i].map(|s| s.count), Some(crate::map::STACK_MAX));
        assert_eq!(w.map.count_items(item("wood")), 120);
    }

    #[test]
    fn removing_a_bench_gives_back_its_stock() {
        let mut w = test_world(12, 12);
        let at = (6, 6, Z);
        place_bench(&mut w, "stove", at);
        w.add_bill(at, RecipeId::named("cook_meal"), BillMode::Forever);
        let raw = item("raw_food");
        w.map.drop_item((2, 2, Z), Stack { kind: raw, count: 5 });
        w.choose_recipes();
        assert_eq!(w.stock_bench(at, Stack { kind: raw, count: 5 }), 2, "only what the recipe needs");
        let i = w.map.idx(at);
        w.map.set_feature(i, Feature::None);
        run(&mut w, 2.0);
        assert!(w.bench_at(at).is_none());
        assert_eq!(w.map.count_items(raw), 7, "the 2 delivered come back");
    }
}
