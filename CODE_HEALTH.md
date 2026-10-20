# Code health backlog

Work through this before continuing with the next steps in [PLAN.md](PLAN.md), in this
order. Findings come from a code review on 2026-10-20; each was checked against the code.
Tick items off as they land, and delete this file once it is empty. Follow the type
guidance in [AGENTS.md](AGENTS.md) for all of it.

## 1. Confirmed bugs

- [ ] **Game clock loses precision.** `World::time` is an `f32` that gets `TICK` (1/30 s)
  added each tick (`world.rs`, `tick`). One game second of ticks advances 1.055 s around
  day 136 and 0.94 s around day 300. By day 1100 time runs at almost double speed, and
  around day 2200 it stops, which also stops seasons, raids and timers. Count ticks in a
  `u64` (or keep `f64` time) and derive `f32` values from it.
- [ ] **A squad's leader is just whoever is first in `members`.** When the real leader
  falls, the next member takes `members[0]`. Their death then costs a second −20
  "lost its leader" morale hit (`battle.rs`, `refresh_units`), and the raider AI spares
  them like a leader (`pick_target`). Store `leader: Option<PawnId>` on `Unit` (squads
  only). Decide whether a successor takes command; ask the user if unsure.
- [ ] **Saves are written in place.** `std::fs::write` goes straight to the target
  (`front/input.rs`: save, save as, autosave on exit). A crash mid-write destroys the only
  copy. "Load newest" also picks the exit autosave over a deliberate save. Write to a
  temporary file and rename it, keep a `.bak`, and keep the autosave out of "Load newest"
  (or make that choice explicit).

## 2. Type modelling

- [ ] **`Pawn.buried` belongs in `Life::Dead { buried }`.** Today a living pawn can be
  buried. Changes the save format.
- [ ] **Two opposite priority scales share `u8`.** Work priority is 0 = off, 1 = highest
  … 4 = lowest (`Pawn.priorities`). Stockpile priority is 1 = low … 4 = critical. Both are
  used as raw array indices. Only the UI setter clamps the stockpile priority, so an
  out-of-range value in a save panics in the job scan (`jobs.rs`, `nearest_cell[k][prio]`).
  Give each its own type (for example `WorkPriority`, and
  `StorePriority { Low, Normal, Preferred, Critical }`).
- [ ] **Expedition and offer ids are raw `u32`** and share `World.next_id` with pawns.
  An offer id passed as an expedition id compiles, and the trade window uses `0` as
  "none" (`front/expeditions_ui.rs`, `Haggle`). Add `ExpeditionId`/`OfferId` newtypes
  (`#[serde(transparent)]`) and `Option`. Also replace the `next_id + 10_000` offset for
  site worlds with something explicit.
- [ ] **Remaining `_ =>` arms on project enums:** `MissionKind::fits` and
  `Expedition::affords` (`expeditions.rs`), and `HediffKind` in `bleed`, `needs_tending`
  and the death cause (`health.rs`; a new disease would be reported as "died of an
  infection"). Also make `affords` look inside nested `Chance`/`Fight` effects.
- [ ] **Decoration and furniture-move states:** `Decor { delivered, built, remove }` and
  `Move { lifted, bills, owner }`. Model them as state enums whose fields only exist in
  the right state. Both change the save format.
- [ ] **Indices into def and world tables:** `RoomStats.kind: Option<usize>` (add `RoomId`
  through `def_id!`), and `Zone::Stockpile(u16)` (add `StockpileId`).
- [ ] Later, if worthwhile:
  - Move the fight site into `Stage::Fighting`.
  - Key skills and priorities by enum rather than by array position, so reordering
    `Skill` or `WorkType` can't shift old saves.
  - Use `Option` for the injury `infection_timer` (`-1.0` sentinel).
  - Add a `Dir` enum for facing directions.

## 3. Saves and robustness

- [ ] **Validate per-cell vector lengths on load** (`save::from_json`), so a damaged save
  fails with a message instead of panicking during a later tick.
- [ ] **Recompute derived state on load.** `running`, `trapped` and `cell_temp` are
  `#[serde(skip)]`; recompute them in `load_json`, so a loaded game continues exactly like
  an uninterrupted one.
- [ ] **Migrations that never run.** Any save that isn't the current version is refused
  (`save.rs`), so the in-place migrations never run on old files. Remove them, or add
  version-stepped migrations.

## 4. Performance

- [ ] **Full-map passes every tick.** Plant growth (`grow_plants`) and blood fading scan
  the whole map at 30 Hz, multiplied by game speed. Move them into the once-a-second pass
  with an accumulated dt.
- [ ] **`World::release` scans three full-map reservation vectors** on every job end.
  Keep a per-pawn list of reserved cells instead.
- [ ] **Pathing.** Every BFS allocates and clears about 900 KB. The trapped check runs a
  BFS per colonist per second. Do one BFS seeded from the map edges, exit early once the
  goal is reached, and reuse buffers.
- [ ] **Front end.** `sync_props`, `draw_markers` and the overlay rebuild from the whole
  map every frame. Gate them on a map change counter, as `roof_changes` already does.

## 5. Tests

- [ ] **Determinism test:** same seed, N ticks, same world hash.
- [ ] **Save-and-continue test:** save, load and continue must match an uninterrupted run.
- [ ] **Assertions on log text.** About 31 assertions match log or event text, and
  `push_log` keeps only 50 lines. Assert on state or typed events instead.
- [ ] **Direct tests** for `combat`, `medical`, `jobs` and `battle_ai`.

## 6. Structure and tools (lower priority)

- [ ] **Split the longest functions:** `battle_panels` (431 lines), `sync_pawns` (341),
  `Defs::check` (232), `sync_props`, `complete_work` and `start_battle`.
- [ ] **Group derived World state.** Move derived caches (`room_stats`, `cell_temp`,
  `running`, `power`, `trapped`, `quarry_*`) into a `Derived` sub-struct, so the save
  boundary is obvious.
- [ ] **Shared Python helpers.** Factor the generators' shared mesh, glTF and math helpers
  in `tools/` into one module; there are three glTF writers and diverging copies of
  `unit`/`matrix`. Add a test that regenerates the committed `.gltf`/`.bin` files and
  diffs them.
- [ ] **Clippy:** about 23 warnings, mostly style. In `temp_color`, freezing and cold
  currently share a colour.
