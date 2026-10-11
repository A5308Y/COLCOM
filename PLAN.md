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
