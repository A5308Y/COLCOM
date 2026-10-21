//! Times the simulation on a busy colony, plus saving and loading.
//!
//! `cargo run --release --example profile`

use colcom::defs::{ItemId, RecipeId};
use colcom::map::{Block, Desig, Feature, Stack};
use colcom::production::BillMode;
use colcom::world::{Order, World};
use colcom::{DAY_LEN, TICK};
use std::time::Instant;

fn main() {
    let start = Instant::now();
    let mut w = World::new(11);
    println!("generate world        {:>8.1} ms", start.elapsed().as_secs_f64() * 1e3);

    // A busier colony: more people and lots of designated work.
    let (cx, cy) = (w.map.w / 2, w.map.h / 2);
    let z = w.map.surface(cx, cy).unwrap();
    for k in 0..7 {
        w.spawn_colonist((cx - 3 + k, cy + 1, z));
    }
    w.story.next_raid = DAY_LEN * 1.2;
    let mut chops = 0;
    let mut mines = 0;
    for i in 0..w.map.len() {
        let p = w.map.pos(i);
        if w.map.feature[i].harvest_desig() == Some(Desig::Chop) && chops < 120 && w.apply_order(p, Order::Desig(Desig::Chop)) {
            chops += 1;
        }
        let exposed = w.map.above(p).is_some_and(|a| w.map.block[w.map.idx(a)] == Block::Air);
        if w.map.block[i] == Block::Rock && exposed && mines < 120 && w.apply_order(p, Order::Desig(Desig::Mine)) {
            mines += 1;
        }
    }
    for y in cy + 2..cy + 6 {
        for x in cx - 6..cx + 6 {
            w.apply_order((x, y, z), Order::Stockpile);
        }
    }
    // Workbenches kept busy for the whole run.
    for (k, (bench, recipe)) in [("stove", "cook_meal"), ("sawbench", "saw_planks"), ("crafting_bench", "make_club")].into_iter().enumerate() {
        let at = (cx + 6, cy - 4 + 2 * k, z);
        let i = w.map.idx(at);
        w.map.feature[i] = Feature::built(bench);
        w.add_bill(at, RecipeId::named(recipe), BillMode::Forever);
    }
    w.map.drop_item((cx + 4, cy - 4, z), Stack { kind: ItemId::named("raw_food"), count: 50 });
    println!("colonists {}, chop orders {}, mine orders {}, 3 workbenches", w.pawns.len(), chops, mines);

    let ticks = (2.0 * DAY_LEN / TICK) as usize;
    let (mut worst, mut changed_time, mut changed_ticks) = (0.0f64, 0.0f64, 0usize);
    let mut rooms_time = 0.0f64;
    let mut slow = [0usize; 3]; // ticks over 0.5, 1 and 2 ms
    let start = Instant::now();
    for _ in 0..ticks {
        // Ticks after the map changed recompute rooms and check what fell.
        let changed = w.map.rooms_dirty || w.map.support_dirty;
        if changed {
            let mut copy = w.map.clone();
            let t = Instant::now();
            copy.recompute_rooms();
            rooms_time += t.elapsed().as_secs_f64();
        }
        // Raids: fight them, with the AI playing the colony too (so battles get timed).
        if w.parley.is_some() {
            w.refuse_tribute();
            if let Some(b) = w.battle.as_mut() {
                b.auto_colony = true;
            }
        }
        let t = Instant::now();
        w.tick(TICK);
        let dt = t.elapsed().as_secs_f64();
        worst = worst.max(dt);
        for (k, limit) in [0.5e-3, 1e-3, 2e-3].into_iter().enumerate() {
            slow[k] += (dt > limit) as usize;
        }
        if changed {
            changed_time += dt;
            changed_ticks += 1;
        }
    }
    let total = start.elapsed().as_secs_f64() - rooms_time;
    let avg_ms = total * 1e3 / ticks as f64;
    println!(
        "simulate 2 days       {:>8.1} ms total, {:.3} ms per tick on average, {:.2} ms worst ({} ticks)",
        total * 1e3,
        avg_ms,
        worst * 1e3,
        ticks
    );
    println!("  slow ticks: {} over 0.5 ms, {} over 1 ms, {} over 2 ms", slow[0], slow[1], slow[2]);
    // At 3x speed and 60 fps, 1.5 ticks run per frame.
    println!("  at 3x speed the sim uses {:.2}% of a 60 fps frame on average", avg_ms * 1.5 / 16.67 * 100.0);
    if changed_ticks > 0 {
        println!(
            "  {} ticks after map changes: {:.2} ms each on average; recomputing rooms alone {:.2} ms",
            changed_ticks,
            changed_time * 1e3 / changed_ticks as f64,
            rooms_time * 1e3 / changed_ticks as f64
        );
    }
    let totals = w.item_totals();
    let made: Vec<String> =
        ["meal", "planks", "club"].iter().map(|k| format!("{} {}", totals[ItemId::named(k).index()], k)).collect();
    println!("on hand after two days: {}", made.join(", "));
    println!("colonists alive {}, log tail: {:?}", w.colonists_alive(), w.log.last());

    let t = Instant::now();
    let json = w.save_json().unwrap();
    println!("save                  {:>8.1} ms, {:.1} MB", t.elapsed().as_secs_f64() * 1e3, json.len() as f64 / 1e6);
    let t = Instant::now();
    let _loaded = World::load_json(&json).unwrap();
    println!("load                  {:>8.1} ms", t.elapsed().as_secs_f64() * 1e3);
}
