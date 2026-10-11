# Plan: multi-level 3D world in Bevy

## Design decisions (defaults agreed 2026-10-11)

1. **World:** a 3D block grid, about 100×70 tiles and 12–16 levels deep. Each block is air, soil, rock,
   a wall or water, and can hold a floor, furniture, plants and items. Terrain comes from a height map:
   hills, cliffs and mountains you can dig into.
2. **Getting between levels:** pawns walk up or down natural one-level slopes. Built walls are never
   climbable. Anything else needs stairs.
3. **Building upwards:** floors on upper levels must be held up by walls or ground within a few tiles.
   Collapses may come later.
4. **Rooms and roofs:** a space enclosed by walls with something solid above counts as indoors.
   Underground caves are rooms automatically and are dark.
5. **Layer view:** PageUp/PageDown picks the level being viewed and hides everything above it.
   Designations and building happen on the level being viewed.

## How it fits together

- The simulation stays a separate library with no Bevy in it, tested without a window. Bevy holds the
  `World` as a resource and calls `tick()` at a fixed 30 times per second.
- **Rendering mirrors the simulation:**
  - **Terrain:** split into chunks of 16×16 tiles across all levels, each drawn as one mesh that only
    includes visible faces. A chunk is rebuilt only when something in it changes.
  - **Everything else:** pawns, trees, items and furniture each become a Bevy entity, kept in sync with
    the simulation by id.
  - **Models:** low-poly shapes built in code (no asset files).
- **Camera:** RTS-style: pan, rotate, zoom and tilt.
- **Mouse clicks:** found by tracing a ray from the cursor through the block grid, respecting the layer
  view.
- **UI:** `bevy_egui` for the toolbar, inspector (with the Health tab), work grid, alerts and log.
- **Lighting:** a sun with shadows that moves through the day/night cycle, and lamps for interiors and
  caves.

## Phases

| # | Phase | Status |
|---|---|---|
| 0 | Current 2D version builds, tests pass, committed | done |
| 1 | Bevy front end on the existing flat simulation: 3D view, camera, clicking, egui UI, macroquad removed | done |
| 2 | 3D simulation: positions with a level, terrain from a height map, walking graph with slopes, job search over reachable cells, systems and tests moved over | done |
| 3 | Multi-level gameplay: digging, stairs, floors, support rule, 3D rooms and roofs, layer view, building on any level | done |
| 4 | Visual polish: walking/hop/fall animation, lamps, translucent water, fog, ambient occlusion, bloom, gunfire flashes | done (needs a visual check) |
| 5 | Performance and saves: profiling, faster searches, versioned and compact save files | done |
| 6 | Data-driven definitions: items, buildables, plants and recipes described in a data file, not code | |
| 7 | Production chains: workbenches, job queues, Cooking/Crafting, new resources, stockpile filters | |
| 8 | More buildables: furniture, shelves, windows, torches, fences, room quality | |
| 9 | Sound: actions, ambience, interface | |
| 10 | Graphics: real models, ramps, water shader, weather, seasons | |

## Next phases (agreed 2026-10-11)

Order matters: definitions first (so content is cheap to add), then content, then sound and graphics once
the list of things has settled.

### 6. Data-driven definitions
- Today `ItemKind`, `BuildKind`, `Feature`, `Task` and their `match` statements hard-code every thing;
  adding one touches 5–10 places (`map.rs`, `jobs.rs`, `world.rs`, `front/props.rs`, `front/input.rs`).
- Move items, buildables, plants and (later) recipes into a definitions file (e.g. `assets/defs.ron` or
  JSON via serde), loaded into a `Defs` registry. Refer to things by a small id (`ItemId(u16)`).
  Keep genuinely special behaviour (walls block rooms, stairs connect levels, beds for sleep, lamps
  give light) as flags or "behaviour" fields in the definition.
- Each definition holds: name, cost, work time, passable / blocks room / blocks sight, which work type
  and skill, what it drops when removed, and how the front end draws it (shape parts or, later, a model).
- Saves store ids; bump `SAVE_VERSION`.
- Keep all current tests passing; add a test that every definition loads and is used consistently.

### 7. Production chains
- Workbenches (buildables with a work spot) and bills: "make N", "make until you have N", "forever".
- New work types and skills: Cooking, Crafting (later Smithing, Tailoring).
- Starter chains:
  - crops → cooked meals at a stove (raw food risks food poisoning: a new hediff in `health.rs`);
  - wood → planks at a sawing bench, stone → blocks at a stonecutter;
  - healroot (new crop) → herbal medicine;
  - metal ore deposits in rock layers (generated in `Map::generate`) → steel at a smelter → weapons and
    armour (armour reduces damage in `health.rs`), which gives deep digging a purpose;
  - cotton (new crop) → cloth → clothing.
- Stockpile filters (which item kinds go where) and priorities, needed once there are many items.
- Haulers bring ingredients to workbenches; products go to stockpiles.

### 8. More buildables
- Tables and chairs (mood penalty for eating without a table), shelves (stack more per cell), windows,
  torches, fences, maybe doors of different materials.
- Room quality (size, furniture, cleanliness, light) feeding mood.

### 9. Sound
- Bevy audio is built in; it needs sound files. Use a CC0 pack (e.g. Kenney); the user downloads it into
  `assets/` (the sandbox can't). Footsteps, chopping, mining, building, gunshots, impacts, interface
  clicks, and background ambience for day, night and caves. Positional sound relative to the camera.

### 10. Graphics
- Replace code-built shapes with CC0 glTF models (Kenney, Quaternius) category by category, through the
  definitions from phase 6.
- Ramps on natural slopes (needs a terrain-model change: a ramp half-fills the lower cell), a water
  shader with gentle waves, weather (rain, snow, fog) and seasons (colours, crop growth).
- Phase 4 visuals still need a tuning pass from real play: lamp brightness, bloom/ambient occlusion
  strength, water opacity, fog distance, hop height.

## Working in this repo (handoff notes)

- Build and test from the sandbox with
  `PATH="$HOME/.cargo/bin:$PATH" CARGO_HOME="$TMPDIR/cargo-home" cargo test`
  (the sandbox can read but not write `~/.cargo`; crates.io is allowed).
- The sandbox has no GPU: the game can't be seen there. `cargo test --bin my-world` runs the whole app
  headless for a few hundred frames (catches Bevy setup and query errors); the user checks visuals.
- `cargo run --release --example profile` times the simulation and saving on a busy colony.
- Don't touch `.git`; the user commits.
- Bevy 0.20 / bevy_egui 0.43 / egui 0.36 are newer than most training data: check APIs in the
  downloaded sources under `$TMPDIR/cargo-home/registry/src/` before using them.

## Notes

- Phase 5 measurements (`cargo run --release --example profile`: 10 colonists, 240 work orders, two
  in-game days on an M4 Max): about 0.05 ms per simulation tick on average (under 0.5% of a 60 fps frame
  at 3x speed), 6 ticks over 2 ms in two days. The front end's own systems take about 1.3 ms per frame
  (headless, without GPU work). A* or cached regions weren't needed. Changes made: job search checks each
  work type once instead of once per priority level, and pathfinding searches use zero-initialised
  32-bit arrays.
- Saves carry a format version (`save.rs`, `SAVE_VERSION`) and refuse older or newer formats with a
  readable message. Per-cell arrays are stored as runs, which took a fresh colony's save from 5.2 MB to
  0.1 MB and save/load from 4/8 ms to about 1 ms each.

- Pinned to Bevy 0.20 and bevy_egui 0.43.
- Old save games don't load since Phase 2 (the format changed).
- Phase 2 kept breadth-first search instead of A*: it only visits walkable cells, which number about the
  same as the old 2D map, so it is still fast. Job search, beds and item lookups now walk those reached
  cells nearest-first instead of scanning the whole volume.
- Since Phase 3, things with nothing underneath fall (pawns, items; plants are destroyed). Built
  structures don't collapse yet: removing a floor's support leaves it standing.
- Stairs are a single-cell stairwell connecting a cell to the one directly above it. They face a
  direction (chosen when built): you enter from the low end and can't cross them sideways.
- Phase 4 left out ramps on slopes: a ramp would fill half the lower cell where pawns stand, so it needs a
  different terrain model. Pawns hop up and down steps instead.
- "Covered" (anything solid overhead, e.g. a tunnel) is separate from "roofed" (inside an enclosed room).
