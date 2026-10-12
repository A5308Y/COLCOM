//! Production: workbenches and their bills, stockpile settings, and
//! colonists picking up better weapons and apparel.
//!
//! A workbench is any buildable that recipes name. Each has a list of bills
//! (what to make, and how many). Once a second every bench settles on the
//! first bill that is wanted and whose ingredients exist; haulers then
//! deliver the ingredients into the bench's stock, and a colonist with the
//! recipe's work type makes it. Products are dropped beside the bench and
//! hauled to stockpiles like anything else.

use crate::defs::{defs, ItemId, RecipeId, Slot};
use crate::map::{Feature, Pos, Stack, Zone};
use crate::pawn::{Job, Pawn};
use crate::world::{Step, World};
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
}

impl Stockpile {
    pub fn new(number: usize) -> Stockpile {
        Stockpile { name: format!("Stockpile {}", number), priority: 2, forbid: Vec::new() }
    }

    pub fn accepts(&self, kind: ItemId) -> bool {
        !self.forbid.contains(&kind)
    }
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
    fn bench_mut(&mut self, pos: Pos) -> Option<&mut Bench> {
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
        for s in self.map.items.iter().flatten().chain(self.pawns.iter().filter_map(|p| p.carrying.as_ref())) {
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
        for a in &r.def().products {
            self.map.drop_item(pos, Stack { kind: a.item, count: a.count });
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
    /// level, or a new one.
    pub(crate) fn stockpile_for(&mut self, p: Pos) -> u16 {
        let next = self.map.neighbors(p).find_map(|nb| self.map.zone[self.map.idx(nb)].stockpile());
        next.unwrap_or_else(|| {
            self.stockpiles.push(Stockpile::new(self.stockpiles.len() + 1));
            (self.stockpiles.len() - 1) as u16
        })
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

    /// The nearest free weapon or apparel better than what `p` has.
    pub(crate) fn equip_candidate(&self, p: &Pawn, nearest: &[Option<(i32, Pos)>]) -> Option<Job> {
        let current_weapon = p.weapon.and_then(|w| w.def().weapon.as_ref()).map_or(0.0, |w| w.score());
        let better = |kind: ItemId| {
            let d = kind.def();
            if let Some(w) = &d.weapon {
                return w.score() > current_weapon * 1.1;
            }
            if let Some(a) = &d.apparel {
                let worn = p.worn(a.slot).and_then(|k| k.def().apparel.as_ref()).map_or(f32::MIN, |w| w.score());
                return a.score() > worn;
            }
            false
        };
        ItemId::all()
            .filter(|&k| better(k))
            .filter_map(|k| nearest[k.index()])
            .min_by_key(|(d, _)| *d)
            .map(|(_, from)| Job::Equip { from })
    }

    pub(crate) fn run_equip(&mut self, p: &mut Pawn, from: Pos, dt: f32) {
        match self.go_to(p, from, false, dt) {
            Step::Moving => {}
            Step::Unreachable => self.end_job(p),
            Step::Arrived => {
                let i = self.map.idx(from);
                let kind = self.map.items[i].map(|s| s.kind);
                if let Some(s) = kind.and_then(|k| self.map.take_item(i, k, 1)) {
                    self.res_item[i] = 0;
                    let d = s.kind.def();
                    let old = if d.weapon.is_some() {
                        p.weapon.replace(s.kind)
                    } else {
                        let slot = d.apparel.as_ref().map_or(Slot::Body, |a| a.slot);
                        let old = p.worn(slot);
                        p.apparel.retain(|&a| Some(a) != old);
                        p.apparel.push(s.kind);
                        old
                    };
                    if let Some(old) = old {
                        self.map.drop_item(p.pos, Stack { kind: old, count: 1 });
                    }
                    self.push_log(format!("{} picked up {}.", p.name, d.name.to_lowercase()));
                }
                self.end_job(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defs::PlantId;
    use crate::map::{Block, Desig, Map, FLAT_Z as Z};
    use crate::pawn::Memory;
    use crate::rng::Rng;
    use crate::world::Order;
    use crate::TICK;

    fn item(id: &str) -> ItemId {
        ItemId::named(id)
    }

    fn test_world(w: usize, h: usize) -> World {
        let mut world = World::with_map(Map::flat(w, h), Rng::new(7));
        world.story.next_raid = f32::MAX;
        world.story.next_event = f32::MAX;
        world.regrow_timer = f32::MAX;
        world
    }

    fn colonist(world: &mut World, pos: Pos) -> u32 {
        let id = world.spawn_colonist(pos);
        let i = world.pawn_index(id).unwrap();
        world.pawns[i].hunger = 100.0;
        world.pawns[i].rest = 100.0;
        id
    }

    fn run(world: &mut World, secs: f32) {
        for _ in 0..(secs / TICK) as usize {
            world.tick(TICK);
        }
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
            // Unarmed, so a weapon that gets made is easy to count.
            let i = w.pawn_index(id).unwrap();
            w.pawns[i].weapon = None;
            let bench = (8, 8, Z);
            place_bench(&mut w, &d.bench.id().def().id, bench);
            for (k, a) in d.ingredients.iter().enumerate() {
                w.map.drop_item((3 + 2 * k, 12, Z), Stack { kind: a.item, count: a.count });
            }
            w.add_bill(bench, r, BillMode::Count(1));
            assert_eq!(w.bench_at(bench).unwrap().bills.len(), 1, "{}: bill added", d.id);
            run(&mut w, 60.0);
            for a in &d.products {
                assert_eq!(have(&w, a.item), a.count, "{} makes {}", d.id, a.item.def().id);
            }
            for a in &d.ingredients {
                assert_eq!(have(&w, a.item), 0, "{} uses its {}", d.id, a.item.def().id);
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
        run(&mut w, 30.0);
        let in_b: u32 = (2..5).filter_map(|x| w.map.items[w.map.idx((x, 16, Z))]).map(|s| s.count).sum();
        assert_eq!(in_b, 10, "wood skips the stockpile that refuses it");

        // A more important stockpile pulls things out of a less important one.
        w.set_stockpile_priority(a, 4);
        w.set_stockpile_allows(a, item("wood"), true);
        run(&mut w, 30.0);
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
        run(&mut w, 20.0);
        let p = w.pawn(id).unwrap();
        assert_eq!(p.weapon, Some(item("knife")));
        assert!(p.apparel.contains(&item("clothes")) && p.apparel.contains(&item("steel_armor")), "{:?}", p.apparel);
        assert!(p.armor() > 0.3);
        assert!(p.thoughts().iter().any(|t| t.0 == item("clothes").name()), "clothes feel nice");

        // A better weapon replaces the old one, which is left on the ground.
        w.map.drop_item((12, 12, Z), Stack { kind: item("rifle"), count: 1 });
        run(&mut w, 20.0);
        assert_eq!(w.pawn(id).unwrap().weapon, Some(item("rifle")));
        assert_eq!(w.map.count_items(item("knife")), 1);
    }

    #[test]
    fn colonists_prefer_meals_and_dislike_raw_food() {
        let mut w = test_world(16, 16);
        let id = colonist(&mut w, (2, 2, Z));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        w.map.drop_item((4, 4, Z), Stack { kind: item("raw_food"), count: 5 });
        w.map.drop_item((10, 10, Z), Stack { kind: item("meal"), count: 1 });
        run(&mut w, 15.0);
        assert_eq!(w.map.count_items(item("meal")), 0, "the meal, though further away");
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::AteMeal));
        let i = w.pawn_index(id).unwrap();
        w.pawns[i].hunger = 15.0;
        run(&mut w, 15.0);
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
        for _ in 0..(15.0 / TICK) as usize {
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
        run(&mut w, 15.0);
        assert!(w.pawn(id).unwrap().memories.iter().any(|m| m.kind == Memory::AteWithoutTable));
    }

    #[test]
    fn shelves_hold_more() {
        let mut w = test_world(16, 16);
        colonist(&mut w, (2, 2, Z));
        assert!(w.apply_order((5, 5, Z), Order::Stockpile));
        place_bench(&mut w, "shelf", (5, 5, Z));
        let shelf = w.map.idx((5, 5, Z));
        assert_eq!(w.map.capacity(shelf), 150);
        w.map.drop_item((12, 12, Z), Stack { kind: item("wood"), count: 120 });
        run(&mut w, 60.0);
        assert_eq!(w.map.items[shelf].map(|s| s.count), Some(120), "all on the one shelf");
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
