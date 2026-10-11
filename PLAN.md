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
| 3 | Multi-level gameplay: digging, stairs, floors, support rule, 3D rooms and roofs, layer view, building on any level | done (needs a visual check) |
| 4 | Visual polish: models, shadows and day/night, interior and cave lighting, animations, blood marks, gunfire effects, water | |
| 5 | Performance and saves: profiling, A* / cached regions if pathfinding gets slow, a save format version number | |

## Notes

- Pinned to Bevy 0.20 and bevy_egui 0.43.
- Old save games don't load since Phase 2 (the format changed).
- Phase 2 kept breadth-first search instead of A*: it only visits walkable cells, which number about the
  same as the old 2D map, so it is still fast. Job search, beds and item lookups now walk those reached
  cells nearest-first instead of scanning the whole volume.
- Since Phase 3, things with nothing underneath fall (pawns, items; plants are destroyed). Built
  structures don't collapse yet: removing a floor's support leaves it standing.
- Stairs are a single-cell stairwell connecting a cell to the one directly above it.
- "Covered" (anything solid overhead, e.g. a tunnel) is separate from "roofed" (inside an enclosed room).
