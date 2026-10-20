# Plan: multi-level 3D world in Bevy

A colony sim in the spirit of RimWorld and Going Medieval: a multi-level 3D block world, colonists who
take orders rather than direct control, production chains, rooms, seasons, turn-based battles and
expeditions. This file holds the status, the next steps, the design decisions and implementation
notes for whoever works on it next.

## Design decisions (defaults agreed 2026-10-11)

1. **World:** a 3D block grid, about 100×70 tiles and 12–16 levels deep. Each block is air, soil, rock,
   a wall or water, and can hold a floor, furniture, plants and items. Terrain comes from a height map:
   hills, cliffs and mountains you can dig into.
2. **Getting between levels:** pawns walk up or down natural one-level slopes. Built walls are never
   climbable. Anything else needs stairs.
3. **Building upwards:** floors on upper levels must be held up by walls or ground within a few tiles.
   Collapses may come later.
4. **Rooms and roofs:** a space enclosed by walls with something solid above counts as indoors.
   Underground caves are rooms automatically and are dark. Enclosed rooms get hipped roofs by
   themselves, which builders put up (see the roofs note below).
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
  - **Models:** characters use Quaternius glTF rigs; items, apparel, furniture, production/power
    equipment and structure props use original generated models. Plants use flat-colour Quaternius
    trees/berries/corn plus original healroot, cotton and rubber plants. Definition-selected scenes
    share meshes/materials; shape parts remain visible until their replacement models are ready.
- **Camera:** RTS-style: pan, rotate, zoom and tilt.
- **Mouse clicks:** found by tracing a ray from the cursor through the block grid, respecting the layer
  view.
- **UI:** `bevy_egui` for the toolbar, inspector (with the Health tab), work grid, alerts and log.
- **Lighting:** a sun with shadows that moves through the day/night cycle, and lamps for interiors and
  caves.

## Status (2026-10-20)

The original phases (agreed 2026-10-11) are done except for parts of the graphics work. Everything built
so far has been checked visually and by ear in the game.

| # | Phase | Status |
|---|---|---|
| 0–3 | 2D start, Bevy front end, 3D simulation, multi-level gameplay (digging, stairs, floors, roofs, layer view) | done |
| 4 | Visual polish: animation, lamps, water, fog, ambient occlusion, bloom, gunfire flashes | done |
| 5 | Performance and versioned, compact saves | done |
| 6 | Data-driven definitions (`assets/defs.ron`) | done |
| 7 | Production chains: workbenches, bills, Cooking/Crafting, stockpile filters | done |
| 8 | More buildables and room quality | done |
| 9 | Sound (Kenney and other real sounds, 2026-10-16) | done |
| 10a–b | Graphics, Valheim-like: art direction, animated characters | done |
| 10c–e | Smoothed terrain, glTF models, atmosphere | 10d implemented, GPU review pending; 10c open, 10e partly: seasonal colours |
| 11 | Follow camera | later, optional |
| 12 | Turn-based battles (MENACE-style), squads, loadouts, parley | done; the user is happy with them for now |

### Built since the original phases (2026-10-16 to 10-19)
Details are in the notes below, under each topic.
- **Building:** roofs that builders put up, thin walls with wall decor (five slots per side),
  overbuilding walls with doors and windows, stairs as slopes and ladders, quarry zones, fill,
  deconstruct, moving and turning furniture, blueprint previews, builders working from diagonal cells.
- **Rooms:** themed rooms with benefits and wallpaper, four-slot shelves, hospital, surgery and
  prosthetics, graves, owned beds, sleeping spots.
- **Colonists:** single-job priorities, fuller hauling, fixes for starving and trapped colonists,
  recreation and daily schedules, a Work window with click-to-change priorities, longer days.
- **Items:** components in three tiers, one loadout for everyday and battle, visible armour; body slots
  (sidearm, belt, head, face, shirt, jacket/vest, legs, back) and cyberware slots, armour by hit
  location, raider equipment tiers up to elite (2026-10-20).
- **World:** seasons, temperature and hypothermia, clothing warmth, a power grid with heaters and coolers,
  a vegetation cap and no wild plants beside buildings.
- **Story:** difficulty settings; expeditions with three factions and goodwill, ten missions, barter
  trading and fights on site maps; raiders and arrivals only enter where they can reach the colony.
- **Interface:** the game menu, save as, autosave, an info window for any item or building,
  multi-selection, a colonist bar, hide roofs, and battle readability (plates, hit chances, cover tiers).

The 2026-10-20 code health review is complete: bugs, type modelling, saves, performance,
tests, structure and shared asset tools. The completed backlog was removed.

## Next steps

In the suggested order. Graphics first: the content has settled, so the look can catch up.

1. **10d. GPU acceptance review.** Implementation is in place for all six plant types and the
   remaining 20 buildable props (12 production stations, four power devices, graves, stairs, ladders
   and fences), extending the already completed item, apparel and furniture work. Static/filled/
   connected scene selection retains grave state and all fence masks; crop plots change from shape
   fallback to models together only when every instance is ready. Stage thresholds, per-cell variation,
   placement/contact dimensions, game rules and save format are unchanged.
   CPU validation: 31 new asset pairs regenerate exactly; geometry, lifecycle and state-selection
   tests pass. `python3 -B tools/preview_world_props.py` builds front/rear views at a common cell scale.
   `cargo run --release --example look -- --world-props` shows actual definition transforms, growth
   stages and all state scenes beside characters in the game's lighting. The app's existing optional
   `character_models_load` GPU test now loads all plant models as well.
   GPU rendering remains unverified: this session could not find a GPU, including outside the sandbox.
   Before closing 10d, review daylight/night, gameplay zoom, rotated placements, construction ghosts,
   occupied workstations, hidden roofs/layers and dense-colony GPU frame time. Keep walls, openings,
   floors, roofs, wallpaper and structural joins in the grid renderer; no prefab-house replacement.
2. **10e. Atmosphere**, the rest of it (seasonal colours and crop growth are done). Weather (rain,
   snow, fog) would build on seasons and temperature and could matter in play (cold, slower walking).
   Wind on grass and trees (custom shader), a water shader with gentle waves, volumetric fog and light
   shafts, plus the phase 4 tuning pass
   from real play: lamp brightness, bloom/ambient occlusion strength, water opacity, fog distance, hop
   height.
3. **10c. Smoothed natural terrain.**
   Naturally generated ground (soil, rock, ore) is drawn as a smooth, rolling surface over the grid
   instead of cubes. Cells that have been mined, and anything built, stay blocky, so dug tunnels and
   shafts keep crisp edges. Needs: a flag per cell for "dug" (or natural vs. changed), a new mesher
   (e.g. smoothed heights or surface nets over natural cells only, blending into the blocky ones), and
   picking that still lands on exact cells. Replaces the old "ramps on slopes" idea; pawns stepping a
   level follow the smooth surface instead of hopping.

### Later or optional
- Follow camera (phase 11): a close third-person view of one colonist, to show off the animations.
- Keyboard shortcuts: a thought-out scheme (all letters are taken; categories, rebinding?).
- Battles (12d, only if play shows a need): tuning AP costs, odds, morale, raid sizes and demand;
  prisoners, explosives and vehicles; the hit chance per member; area attacks on a whole squad.
- Expeditions: raids by the other factions, caravans visiting the colony, more varied site maps, a world
  map, skills that change mission outcomes (hunting by Shooting, prospecting by Mining).
- Power: cables and separate grids, more consumers (lamps, benches).
- Colonists: pick-up and put-down clips, hit reactions, a pose for sleeping on the ground.
- Production: meal quality per cook, fuel, smithing and tailoring as separate skills.
- Digging: prospecting (hidden ore found by digging).

## Design records

### Art direction (10a, decided 2026-10-12)
Target look: like Valheim. Low-poly models, realistic movement, and most of the mood from light and
atmosphere. The simulation stays on its grid; all of this is drawing.

- **10a. Art direction.** Downloaded (2026-10-12, all Quaternius, CC0): Universal Base Characters,
  Modular Character Outfits – Fantasy, Universal Animation Library (in `assets/models/characters/`,
  glTF), and Ultimate Nature, Textured Stylized Trees, Nature Crops, Medieval Village (only OBJ / FBX /
  .blend, kept in `asset-sources/quaternius/`: they need converting to glTF first, e.g. with a Blender
  batch export or an OBJ to glTF converter). Bevy can't load glTF without a GPU, so model checks run with
  `cargo test --release -- --ignored`.
  Done (2026-10-12): the four OBJ packs converted to glTF (`tools/obj_to_gltf.py`, 341 models, 23 MB,
  in `assets/models/nature|trees|crops|village`); a look test, `cargo run --release --example look`:
  characters (bare and dressed) playing any of the library's 43 animations, rows of trees, rocks,
  plants, crops by growth stage and village pieces, all at one scale (a character is one cell tall),
  game blocks for comparison, the game's lighting with time-of-day presets, and textures switchable
  between smooth, sharp (nearest) and pixel (quarter resolution, nearest).
  Findings for later: the character files carry no animation data, so their bones are given animation
  target ids by name path at spawn (they share the library's skeleton exactly); the free packs have no
  separate head, so `tools/make_heads.py` cuts one from each base body (triangles weighted to the head
and neck bones) and dressed characters are outfit + head + hair; each character has one
`AnimationPlayer` on its root driving the bones of all its parts (separate players drifted apart); the library has no chop,
  mine or carry animations (use Sword_Attack, Fixing_Kneeling, PickUp_Table or find more); the packs
  differ in scale (pine 2.7, stylised tree 5.5, house 3.4 units), so definitions need a scale per model.
  **Decided (2026-10-12):** flat-coloured low-poly models (Ultimate Nature, Nature Crops, Medieval
  Village; not the textured stylised trees), Quaternius characters dressed as outfit + cut head + hair
  (the neck seam looks good), one animation player per character (animations line up). Textures stay
  smooth: the pixel idea is dropped, because the world has no textures and the character textures are
  far larger than characters appear on screen (a 4x reduction made no visible difference).
  Originally planned: choose asset packs (Quaternius is closest among CC0 packs; Kenney is too clean;
  mixing in paid packs or Blender-made models is possible) and a texture approach (low-resolution
  textures with nearest-neighbour filtering and an atlas, instead of vertex colours). Build a small test
  scene to judge the look before converting everything. The user downloads assets (the sandbox can't).

### Turn-based battles (agreed 2026-10-13)
MENACE-style battles instead of RimWorld's real-time fights, built around squads organised during
normal play. Decisions:
- **Squads:** a leader plus colonists assigned to them (at most 4), fighting as one unit that moves and
  fires together; members keep their own health.
- **Leaders:** -2 for "Responsible for a squad", and -8 without a decent bedroom of their own.
- **Getting into the fight:** members far from their leader arrive a few rounds in, in their combat
  loadout taken from storage (and put back afterwards). Without leaders everyone fights alone. The
  default role is fighter.
- **Civilians:** they shelter in a safe zone and can still be attacked there.
- **Raids:** raids come as squads too. A parley comes first: pay their demand or fight. No battle
  resolves automatically.
- **Turn order and time:** MENACE turn order (alternating single activations, one AP pool, no
  overwatch), and the whole colony stops during a battle.
- **Real-time combat is removed:** drafting, auto-fire and the attack job.
Milestones 12a (squads in normal play), 12b (battle core), 12c (battle UI) are done and play-tested
(2026-10-19: the user likes the battles as they are). 12d (tuning, see the notes) is optional, only if
play shows a need.

## Working in this repo (handoff notes)

- Build and test from the sandbox with
  `PATH="$HOME/.cargo/bin:$PATH" CARGO_HOME="$TMPDIR/cargo-home" cargo test`
  (the sandbox can read but not write `~/.cargo`; crates.io is allowed).
- The sandbox has no GPU: the game can't be seen there. `cargo test --bin my-world` runs the whole app
  headless for a few hundred frames (catches Bevy setup and query errors); the user checks visuals.
- `cargo run --release --example profile` times the simulation and saving on a busy colony.
- Scenarios (`src/scenarios.rs`) are fixed test worlds: `cargo run --release -- --scenario battle`
  (or `MY_WORLD_SCENARIO=battle`) starts straight in round 1 of a battle, the colony's turn; Shift+N
  restarts the scenario. New ones go in `SCENARIOS` and `build`, built like the battle tests
  (`World::with_map(Map::flat(..))`, storyteller off, set skills and weapons, fixed rng seed).
  `main.rs::battle_scenario_runs_headless` runs the app in it.
- Don't touch `.git`; the user commits.
- Bevy 0.20 / bevy_egui 0.43 / egui 0.36 are newer than most training data: check APIs in the
  downloaded sources under `$TMPDIR/cargo-home/registry/src/` before using them.

## Notes

Implementation notes by topic. Some early notes were superseded later (for example stairs, which became
slopes, and old save versions); the newer note in the same topic says so.

### Interface

- Game menu (`ui::game_menu`; the bar's "Menu (Esc)" button replaces Sound, Save and Load): Save,
  Load, New game (or Restart scenario) with a confirmation, and the sound sliders. Esc goes one
  step at a time: close a window, put the tool down, deselect, open the menu.
- Autosave: closing the window or quitting writes `autosave.json` (`input::autosave_on_exit`, in
  `Last`, on `WindowCloseRequested` or `AppExit`, once). The manual save (K, `savegame.json`) is left
  alone. L loads whichever of the two is newer and says which. Both files are gitignored.
  Starting the game continues from the newer save too (`sim::start_world`). `--new` or a scenario
  starts fresh, and tests always do. A save that can't be loaded gives a new colony, with the
  reason in the log.
- Deselecting: Esc (after closing an open window, with the Select tool), a right click on nothing
  to equip (also in battle), or the inspector's close button.
- Game speeds (keys 1-4, or the HUD button cycles): 1x, 2x, 5x, 10x (`sim::SPEEDS`). Tab used to
  cycle them, but egui takes Tab to move between buttons, so the number keys do it now and no tool or
  buildable has a number key (`SPEED_KEYS`, reserved). At most 40 simulation steps
  run per frame, which 10x needs only below 15 fps.
- Multi-selection: drag a box with the Select tool (colonists inside it; Shift adds) or Shift+click on
  the map or in the colonist bar. `UiState::group` holds them (the first is `selected`, shown in the
  inspector); use `selected_pawns` / `select_pawns` / `toggle_pawn` / `select`. Its drafted-group
  orders went with real-time combat (phase 12); equipping goes to the first selected.
- Right-click anywhere (map or panels, click or camera drag) puts the tool down; with a tool in hand,
  its name follows the cursor.
- The colonist bar is a vertical list down the left (right of the build menu) of same-sized cards
  (`ui::CARD`): name, mood, and the status cut to one line (all of it on hover); it scrolls if it
  doesn't fit. The colonist and cell inspectors open to its right (`COLONIST_LIST_W`).
- Build menu (after phase 8): a full-height dock on the left (`src/front/build_menu.rs`) replaces the
  toolbar: groups (Orders, Zones, then buildable categories), search, cost per entry (red when short),
  shortcut keys, details on hover, a hint for the current tool at the bottom; `` ` `` toggles it.
  Tools are `input::ToolEntry` (name, key, group, hint). egui 0.36 panels are shown inside a root `Ui`;
  they don't count as "pointer over egui", so `UiState::dock_width` / `over_dock` keep clicks and
  scrolling on the dock out of the world. Windows and labels keep to the right of it.
- Save as and spawning (2026-10-18): the Menu saves under a name to `saves/<name>.json` (`input::save_as`,
  names cleaned to letters, digits, spaces, - and _; git-ignored) and lists them newest first to load
  (`named_saves`, `load_file`); K/L still quick-save. "Spawn items (testing)" opens a window to pick an item
  and amount; `Tool::Spawn` drops it where clicked (`World::spawn_items`, logged).
- Info window (2026-10-18): `front/info.rs` builds a stat sheet (`Sheet`: sections of label/value rows and
  "See also" links) for any `InfoTarget` (item, buildable, plant) from the definitions: weapon, apparel,
  food, medicine, prosthetic stats; recipes that make and use an item; a buildable's cost, work, drops,
  properties (`BuildDef::cover_kind`/`PlantDef::cover_kind`, now shared with battles), recipes and room
  themes; a plant's growing time in days (daylight `DAWN`..`DUSK`) and harvest. Opened from a top-bar
  button (with search), Info buttons in the cell inspector, colonist gear, Resources rows and right-click in
  the build menu (`UiState::open_info`, with Back). Buildings have no hit points in the simulation, so none
  are shown. Build menu groups start closed.
- Work window (2026-10-18): left click raises a priority (never -> 4 -> ... -> 1), right click lowers it;
  each level has its own green (`priority_color`, `raise_priority`/`lower_priority`).

### Terrain, building and digging

- Build orders replace a blueprint of something else in place (`order_applies` treats the cell as
  empty; materials already delivered are dropped there for the haulers). The same blueprint again
  doesn't apply.
- Buildables must fit their cell (1.0 high): parts on a built floor are lifted by the slab (0.08), so
  furniture stays below 0.92; doors and windows aren't lifted (they fill the cell like walls) and may
  reach 1.0; stairs reach the level above. `defs::tests::buildables_fit_under_the_ceiling` checks it.
  (The door leaf used to be 1.1 tall and poked through the lintel and the floor above.)
- Roofs (agreed 2026-10-17), separate from floors, which are only for upper storeys. `Map::roof` is a
  flag on the cell above a room (where a floor would go), saved with `serde(default)` so older saves
  load. Roofs close rooms (`covered`) but can't be stood on. The player never places them:
  `recompute_rooms` also collects `roof_wanted` (over the uncovered cells of enclosed regions that built
  walls or doors help enclose, held up like floors: a wall within 3 cells; regions with roofs in them
  are attics and get none), and `World::mark_roofs` puts `roof` blueprints (`special: Roof`, 1 wood)
  there after each room recompute and takes away ones no longer wanted. Builders put them up from below
  with the usual blueprint jobs. A floor ordered over a roof replaces it. A deconstructed roof is marked
  again while the room stays closed unless a No roof area prevents it. Roofs aren't in the build menu,
  can't be ordered or cancelled, and their blueprints show no delivery count.
  - No roof areas (2026-10-20): Zones → No roof / Allow roofs controls the space immediately above
    each painted cell, independently of grow, stockpile and safe zones. Painting No roof immediately
    cancels overhead roof blueprints and refunds delivered materials; existing roofs require explicit
    deconstruction and won't be replanned afterwards. Floors remain allowed. Sparse `Map::no_roof`
    cell indices are saved with an empty default so existing version-16 saves still load. These
    intentional openings produce no roof-support warning. Floor outlines and the inspector identify
    the area; a visible roof targets the room below, or Shift+R exposes the interior for painting.
  - Where a room wants a roof that nothing would hold up, `recompute_rooms` lists the cell in
    `Map::roof_gaps`: drawn as red crosses at the wall tops, explained in the cell inspector (on the
    room's floor too), and counted in an alert, so the player knows where a wall is missing.
  - Drawn by `src/front/roofs.rs` as one hipped mesh, rebuilt when `Map::roof_changes` (roofs built or
    removed, rooms recomputed), the layer view or the roof cut change. Heights come from each point's
    distance to the edge on a half-cell grid (`half_steps`, 0.45 levels per cell, at most 1.3), so
    rectangles get ridges (odd widths too), squares pyramids, L-shapes valleys and big halls flat tops.
    The roof also covers the tops of the walls around it, and eaves overhang 0.3 and drop 0.12. Each
    triangle is drawn from below as well, darker.
  - The battle arena's house has a roof.
- Turning things while placing (2026-10-17): `Feature::Blueprint` has a `dir` (serde default 0), set by
  `World::turn_blueprint` when placed (`UiState::place_dir`, `,`/`.` via `input::turn_dir`) for
  `BuildDef::turnable` things (modelled, not along walls, not floors/roofs/graves); built things keep it
  (stairs only if `Map::stairs_fit`, else `stairs_facing`), and moved furniture keeps its own. `BuildDef::front`
  is the way a model faces (chair: 2, +y): the placement arrow shows it, and seats beside a table are drawn
  facing it (`props::seat_dir`, display only). Blueprint ghosts are drawn turned.
- Fill (2026-10-17): `Desig::Fill` on an open, empty cell (no structure, floor or roof) on the ground, or on
  one whose cell below is marked too (`Map::fillable`). Miners (`Task::Fill`, 3 s, Mining skill) fill it with
  soil once the cell below is solid and nobody stands in it; items in it go up onto the new ground, its zone
  and blood go. Deep shafts are worked from the rim (`Map::fill_spot`); `find_work` also looks at every
  cell marked Fill, since those may be further down than the cells around reachable ones.
- Deconstruct (Orders menu, Backspace): marks structures, floors and roofs to be taken down by builders
  (Construction work and skill) in half their work time (at least 1 s), dropping `drops` (half the cost
  by default). Mine is for rock, ore and soil only. Marked cells are outlined in red.
- Walls, doors and windows (`blocks_room`) can be placed straight on top of a wall or window (anything
  `holds_floors`) without a floor first: click the wall's top, or the cell above it in the layer view.
  Builders work up to one level above where they stand, so the second level is built from the ground;
  higher ones need a floor or stairs beside them to stand on.
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
- Two-storey fixes (2026-10-18): `recompute_rooms` treats planned stairs like built ones (no roof over a
  stairs blueprint; before, a roof was planned over it and came back after deconstructing), nor over the
  cell in front of stairs (`Map::stairs_foot`); both count as covered for rooms, and building stairs takes
  down a roof over either. Doors hold
  floors, so walls stack onto them (and they hold up roofs and floors nearby). Walls, doors and windows can
  be planned on a planned holds-floors structure (`Feature::planned`); such a blueprint waits for the one
  below (`World::stacked_on_plan`, skipped by work search), and cancelling a blueprint takes away the
  blueprints left hanging above it (`cancel_blueprint`).
- Overbuilding (2026-10-18): `Feature::Blueprint::over` holds the wall, door or window still standing in
  the cell; `Feature::structure`/`structure_id`/`passable` report it until the blueprint is built, so the
  cell stays solid and the room closed. Any blocks-room structure can be planned over another kind
  (`order_applies`); building it drops the old one's `drops()`, cancelling restores it, deconstructing
  the old one leaves a plain blueprint. Doors and windows target the clicked wall (`order_target`), walls
  the cell above (stacking). Props draw a slightly bigger ghost shell around the wall.
  `Feature::blueprint(kind, delivered, dir)` builds plain blueprints.
- Thin walls and wall decor (2026-10-18):
  - Look: walls and windows are drawn `decor::WALL_T` (0.2 cell) thick through the middle of their cell:
    a core plus an arm towards each side `Map::wall_links` joins (walls, doors, windows, natural rock and
    soil). `terrain::thin_wall`/`opening`; `under_wall` draws the floor beneath in quarters coloured
    like the floor beside each (indoors/outdoors), on a slab when standing on a built floor. Only on
    natural ground is every quarter drawn; upstairs (on a slab or on a lower wall) a quarter with no
    floor beside it stays open, so no floor sticks out past the wall. A door below counts like a wall below
    (doors are props, not terrain), so a wall over a door has the room's floor beside it. Rock overhead gets a ceiling. Walls no longer hide neighbours' faces (`Look3::covers`). Wallpaper
    is drawn per piece (`MeshBuf::wall_face`, `wall_part` for part of the height) on absolute along-wall
    coordinates, windows included (around the hole, on each side facing a themed room). Doors are 0.22 deep.
    The simulation is unchanged: a wall still fills its cell.
  - Decor (`src/decor.rs`): `BuildDef::on_wall` buildables (painting, poster, calendar, wall lamp, wall
    shelf, hanging plant; category "Wall decor") hang in one of `SLOTS` (5) places per face.
    `Map::decor` (cell -> `Vec<Decor>` {kind, face, slot, delivered, built, remove}, saved).
    `Map::decor_front` says whether a slot is usable (the wall's body spans it; beside a wall running off
    the face it looks at the diagonal cell) and which cell it faces. `World::order_decor`/`cancel_decor`
    (plans are called off, things up are marked to come down, toggled); haulers and builders deliver
    via `delivery_need`/`stock_decor`; `Task::Decorate` puts up or takes down the next ready one from
    its front cell. `update_decor` (once a second) drops decor whose wall or front is gone. Room stats
    count built decor for the room its front cell is in (beauty, light, counts). Wall shelves make the
    wall cell storage (`Map::wall_shelves`, `slots`, `storage_furniture`, `storage_kind` in
    `sync_containers`); their stacks are drawn on the shelves; losing the last shelf puts its items down.
  - Front end: `UiState::wall_slot` from `input::wall_slot_at` (the face plane the cursor ray meets);
    clicking with a wall item's tool calls `order_decor`; a gizmo marks the slot (green/red).
    `props::decor_transform`/`spawn_decor` draw them (ghosts while planned, lamps lit, shelf items).
    Inspector lists a wall's decor (Cancel / Take down / Keep; the calendar shows the date).
- No wild plants beside buildings (2026-10-18): regrowth skips cells with a wall, door or window in any
  of the eight cells around on the same level (`Map::beside_wall`). Plants already there stay until cut.
- Quarries (2026-10-18): `src/quarry.rs`. `World::quarries` (saved) of `Quarry { cols, depth, ground,
  shaft, out }`; `Map::quarry_cols` (not saved, `index_quarries`) maps columns to them. `Order::Quarry`
  paints columns (at each column's own ground level, `Map::ground_level`, trees and all), joining or
  merging neighbouring quarries; Clear zone takes them out (and the Mine marks they made). A first
  version carved natural steps along the edge; it was replaced because the step path moved as digging
  changed the ground, depth counted from the highest column, and steps left ore behind.
  - Layout (`lay_out_quarry`, only when the shape changes or `ground` is 0, e.g. old saves): the shaft
    is the edge column whose outside neighbour is nearest the colonists by path; `ground` is that
    neighbour's level, `out` the way to it. `bottom()` = `ground - depth` (never level 0).
  - Digging (`quarry_plan`, once a second in `update_quarries`): ground above the entrance is marked
    whenever; below it, layer by layer, the shaft cell first, then a stairs blueprint there (6 wood,
    facing `out`, kept by `quarry_stairs_dir` when built), then the rest of the layer once those stairs
    are built. Stairs stacked in a column lead straight up; `Map::supported` also counts stairs resting
    on stairs still being built, so the shaft stays usable while the next stairs go in. Columns with
    water or anything built (but stairs) are left alone, cells beside water kept as a rim, plants on top
    marked for cutting first. `quarry_state` (Digging / Stairs / Done) and `quarry_unreachable` (marked
    cells no colonist can get to, counted once a second) feed the inspector.
  - Front end: Quarry zone tool, ground tint (`QUARRY`), the shaft marked (outline, arrow out, line down
    to the bottom) while the tool is out or a quarry is selected, inspector with depth -/+ and state.
  - Prospecting (hidden ore found by digging) is a possible later step.
- Stairs are built as their blueprint faces (2026-10-18): before, a builder re-chose the direction when
  the placed one couldn't be climbed yet (`stairs_fit`/`stairs_facing`), so finished stairs could face
  differently from their blueprint. Now only a quarry's shaft overrides it (`quarry_stairs_dir`); the
  placement arrow is orange where the stairs couldn't be climbed that way (`input::front_arrow`).
- Ladders (2026-10-18): `ladder` is stairs (`special: Stairs`, so movement, support and quarry code
  treat it the same) with `BuildDef::climb` set: modelled as rails and rungs against its +x side,
  so stacked ones make one long ladder; 3 wood. `Defs::ladder()`/`stairs()` tell them apart.
  Quarries put ladders in their shaft (`update_quarries`), stairs already built there stay.
- Stairs as slopes (2026-10-18): stairs (`Map::ramp_dir`, not ladders) are walked on at their foot and
  off their high end straight onto the cell beside them one level up (`Map::step`; headroom: the cell
  over them must be open), and down again the same way, like a natural one-level slope. The cell over
  stairs is no longer somewhere to stand. Ladders (`Map::ladder_dir`) keep the old vertical climb and
  hold up the cell (and the ladder) above them. `pawns::stairs_height` draws people walking up the
  steps.
- Where builders stand (2026-10-18): building, deconstructing and packing up (`jobs::builds`) and
  deliveries go by `Map::build_around`: the eight cells around (diagonals too) on the level or one up or
  down, not straight above or below, plus the cell below for floors and roofs (`Map::overhead_work`).
  `DistField::approach_build`, `World::go_to_reach(Reach::Build)`; work candidates and nearby
  blueprints look at those cells too. Mining and the rest keep `Map::around`.
- Plants need open sky (2026-10-17): nothing grows where `Map::covered` (roof, floor, rock overhead); grow
  zones can't be painted there and nothing is sown there. Stairs get no roof over them (`recompute_rooms`
  skips them and counts them as covered, so the room is still a room), and stairs built under a finished
  roof take that roof cell down.
- Vegetation cap (2026-10-17): wild plants regrow (one try every 4 s) only while their kind covers less than
  its `wild.density` share of the columns topped by soil (`World::wild_cap`, counted from the terrain, so
  plants on it don't shrink it), so a long game stays about as wooded as a new map.
- Quarries can't shut anyone in (2026-10-19, found in a save: two colonists at the bottom of a
  three-level pit). Columns whose rock stood above the entrance were dug "whenever" and left out of
  the layer order, so lower layers went on elsewhere and left deep pits cut off from the shaft. Now
  `quarry_plan` digs the highest layer of all its columns first (above the entrance no ladder is
  needed); below it, as before, the shaft and its ladder come first. `quarry_work` takes each column's
  highest solid cell not in `World::quarry_out_of_reach` (filled in `update_quarries` from a colonist
  who isn't trapped: unreachable tops are skipped down to a reachable cell, so pillars too tall to
  climb don't stall the layers) and leaves out cells beside water (the rim, which used to stall a
  strict order). Mine marks in quarry columns other than the current layer are taken off. Rescue for
  anyone already in a pit: `bfs_climbing` also climbs up `Map::cling` cells (open air with natural
  rock or soil beside) and over a rim onto standable ground; pawns climbing (`cling`, path left) don't
  fall, and `go_to_reach` keeps climbing mode while on a cling cell. Tests:
  `an_outcrop_in_the_zone_never_shuts_anyone_in`, `someone_shut_in_a_pit_climbs_out_up_its_wall`.

### Rooms and furniture

- Themed rooms (2026-10-17, like ONI / Mind over Magic): `defs.ron` `rooms` lists kinds (bedroom,
  barracks, kitchen, smithy, workshop) with `needs` (any of some buildables, `min`, optional `max`) and a
  floor look (`floor` tint, `checker` tiles). `update_room_stats` counts every buildable in a room and in
  its walls (`RoomStats::counts`); the first kind whose needs are met, with no bed or workbench it doesn't
  ask for (`RoomDef::allows`), is the room's `kind`. Drawn: the floor tint in the terrain (room kinds are
  passed to the mesher through `terrain::Cut::kinds`) and the name over the room (`overlay.rs`). Guided:
  the cell inspector lists every theme with a checklist (closest first, the closest unfinished open), and
  the Rooms window (HUD) shows what each needs and lists the colony's rooms to jump to. New furniture:
  dresser.
- Room benefits (2026-10-17): each theme has `effects` in `defs.ron` (`RoomEffects`): `rest` (rest gained
  in its beds), `thought` (a `Memory::RoomThought` after sleeping there), `work` and `learn` (speed and
  skill gained crafting at its benches), `extra` (chance of a spare of each product). Bedroom: rest +25%,
  +4 mood; barracks: rest +10%, +2 mood; kitchen: work +25%, 15% spare meals; smithy: work +30%, 10% spare;
  workshop: work +20%, learning +50%. Listed in the inspector and the Rooms window.
- Moving furniture (2026-10-17, `src/moving.rs`): `World::order_move(from, to)` puts a blueprint of the
  furniture at `to` (no materials; hauling skips it) and records a `Move` in `World::moves`. A builder packs
  it up (`Task::Uninstall`, half its work time), keeping a bench's bills and a bed's owner, then builds the
  blueprint (half the work time) and `finish_move` sets it down. Cancel at either end calls it off (packed-up
  furniture goes back, or its full cost is dropped if its old cell is taken); `update_moves` drops moves
  whose blueprint or furniture is gone. Not movable: walls, doors, windows, floors, roofs, stairs, graves.
  UI: `Tool::Move` (pick the furniture, then the spot; green/red preview and an arrow), a Move button and
  the move's status in the cell inspector.
- Hospital, surgery room and prosthetics (2026-10-17, `src/surgery.rs`): new furniture `medical_cabinet` and
  `operating_table` (a bed with `medical: true`). Room themes `hospital` (beds, cabinet, 2 windows, lamp:
  `heal` 1.4, `tend` +0.1) and `surgery` (operating table, cabinet, lamp: `tend` +0.2, `surgery` +0.25),
  tried before the others. Care beds (operating tables, beds in rooms whose effects are `medical()`) are
  never claimed or slept in; `find_patient_bed` picks hospital beds, then the patient's own bed, then an
  operating table. `TickCtx::care` carries the hospital's `heal` into healing. Prosthetics are items with
  `prosthetic: (fits, efficiency)` made at the smithy; `HediffKind::Prosthetic` on the part where a loss
  begins (`Health::lost_parts`) replaces it and everything on it (`fit_prosthetic`), scaling its
  efficiency. A colonist with a fitting prosthetic around, a free operating table and another doctor takes
  `Job::Operation` (lies on the table, gives up after 90 s without a doctor); doctors take `Job::Surgery`
  after rescues, tending and feeding. Success: `surgery_chance` (0.55 + 0.025 per Medicine level, times
  hands and sight, plus the room's `surgery`, 5–98%); failure ruins the prosthetic and cuts the part above.
- Wallpaper (2026-10-17): `RoomDef::wallpaper` (`pattern`: Plain, Stripes, Checks, Dots, Tiles, Bricks, Planks,
  Diamonds; `base` and `accent` sRGB). The terrain mesher papers each wall face looking into a themed room
  (`MeshBuf::papered`): 8×8 vertex-coloured squares per cell (`Pattern::accent`, `Pattern::tone`), continuing
  along the wall, the bottom row a darker skirting board. Window walls stay plain.
- Shelves block the way (2026-10-17): `shelf` is not passable (storage may now be impassable; seats still
  must be passable). `bfs` reaches storage furniture beside a reached cell one step further but never
  through it (`Map::reach_neighbors`; `bfs_avoiding`, for battles, doesn't); `DistField::approach` never
  picks one to stand on and treats one as a target to reach from beside; `go_to` does the same. Shelves
  still take items (`Map::can_accept`).
- Beds can be given to a colonist (2026-10-17): click a bed, pick who sleeps there in the inspector
  ("Nobody (first come)" frees it). `World::assign_bed` moves `Pawn::bed`: the new owner's old bed is
  freed and whoever had this one looks for another (`find_bed` skips beds others own). With a leader
  as owner, the panel says whether it makes proper quarters (`has_proper_quarters`).
- Shelves (any buildable with `storage`) are storage by themselves (2026-10-17): `sync_containers`
  (each colony second, on load, and when storage furniture is built or taken down) gives one standing
  outside any stockpile a `Stockpile` of its own (`container: true`, "Shelf N", Preferred), shared with
  the same furniture beside it, so clicking it shows the usual settings. Clear and safe zones skip
  those cells, painted stockpiles don't join them, and when the shelf goes its storage goes and what no
  longer fits the cell spills over. A shelf inside a painted stockpile uses that stockpile, as before.
- Phase 8: table, chair, shelf, window, fence, torch, stone door, all as definitions. New definition
  fields: buildable `category` (toolbar menus), `beauty`, `daylight` (windows), `storage` (cell holds
  `stack_limit`, items drawn at `height`; `Map::capacity`), `along_walls` (doors and windows turn to their
  walls), `Special::Table` / `Seat`, part `link` (fence rails shown only towards matching neighbours),
  light `flicker`, and a `clear` palette for glass.
- Room quality (`src/rooms.rs`): per room size, beauty (contents plus bordering walls/windows/doors),
  dirt (blood, loose items outside stockpiles and shelves) and light (lamp, torch or window), scored
  into five tiers (Awful to Impressive). Recounted once a second and whenever rooms change. Memories
  `Bedroom(tier)` (replaces "slept in bedroom") and `DiningRoom(tier)`; memories of the same kind
  replace each other.
- Windows are a `BuildLook::Opening` (wall block with a hole) meshed with the walls by the terrain
  mesher, so they join walls without seams; only the frame and glass are props.
- Four-slot shelves (2026-10-18): `Storage { slots, height, lower }` (shelf: 4 slots of 50, two shown on top,
  two on the middle board). A cell's first stack stays in `Map::items` (save layout unchanged), stacks 2–4 in
  `Map::shelved`. All code goes through `Map::stacks`/`cell_stacks`/`set_stacks`/`count_in`/`room_in`/
  `add_to_cell`/`take_item`/`take_all`/`settle_cell`; hauling picks what to take from a mixed cell
  (`pick_up_kind`). Shelf sync settles over-full cells (old saves' one big stack becomes 4×50; a removed shelf
  spills). Props draw a shelf's slots small at x ±0.22.
- Furniture fronts (2026-10-18): `BuildDef::front` is the side something is used from (default +y as
  modelled; dresser, medical cabinet and home battery -y; stairs +x, the way up) and `any_way` marks things
  that look the same from every side (lamp, torch, table, fence, PV system: not turnable, no arrow).
  `map::turn_dir` turns a direction by a structure's `dir` (the front end's `props::turned` uses it,
  checked against the model rotation); `Map::front_cell` is the standable cell a structure faces.
  Crafters walk to a bench's front cell, or to beside it if the front is out of reach. The placement
  arrow (`input::front_arrow`) points at the front, also while moving furniture: `,`/`.` turn it there
  (`UiState::start_move` starts from how it stands, `UiState::turning`, `World::order_move_facing`;
  `Move::dir` keeps how it stood, for when a move is called off).
- Sleeping spots (2026-10-17): `sleeping_spot`, a free (`cost` count 0 is now allowed, `BuildDef::free`) bed
  with `comfort: 0.5` (beds default to 1). Rest per second is 0.8 + 0.4 × comfort (ground 0.8, spot 1.0, bed
  1.2; halved with the longer day, see below); sleeping there gives `Memory::SleptOnSpot` (-2, vs -5 on the ground) instead of a bedroom memory; it
  heals like lying anywhere (`in_bed` for healing needs `proper_bed`), but tending counts it as a bed and
  rescuers carry the downed to it. It counts towards no room theme and spoils none (`RoomDef::allows`).

### Colonists: needs, work and health

- Operations are ordered by the player (2026-10-17): `Pawn::operations` (`surgery::Operation { part, item }`),
  `World::order_operation` / `cancel_operation`, shown in the Health tab with Fit/Cancel buttons and what
  it's waiting for. Only ordered operations happen; an order ends when tried (fitted or botched) or when
  the part is no longer lost. Colonist cards show the equipped weapon (or "Unarmed").
- Prioritizing single jobs (2026-10-17, `src/prioritize.rs`): right-click a cell with a colonist selected for
  a menu (`front::JobMenu`) of `World::job_options`: equip, and every job there of any work type (enabled or
  not; `work_candidate(.., forced: true)` ignores others' reservations), doctoring a colonist lying there
  (`doctor_job`, extracted from `doctor_candidate`), operating, burying a body there or into a grave there.
  `prioritize` takes the job over (`take_over` ends whoever held its target or patient) and starts it; the
  colonist keeps working that cell (`Pawn::forced`, "(prioritized)" in their status) until `forced_job`
  finds nothing more there. `find_work`'s map scan is now `World::scan` (`Scan`), shared with the menu.
  Right-click on gear now opens the menu too (Equip is one of its choices).
- Fuller hauling trips (2026-10-17, `jobs.rs`): hauling to a stockpile takes as much as fits in the target
  cell (`room_for`, up to a stack), then picks up more of the same item lying within `NEARBY_STEPS` (8) that
  belongs there more than where it lies (`gather_next`, walking to each before setting off). Deliveries take
  enough for the blueprint plus the free blueprints within 8 steps needing the same material
  (`delivery_load`, up to a stack), then carry the rest on to the next one (`next_delivery`).
- Refused items (2026-10-17): things in a stockpile (or on a shelf) that no longer takes them were only moved
  if another stockpile took them. Now, if none does, haulers carry them out to open ground within 10 cells
  outside any zone with nothing built on it (`dump_spot`), where they're left.
- Starving colonies (2026-10-17, found in a save: 182 raw food on one shelf, cooks idle, colonists at 0):
  item reservations were per cell, so one colonist fetching from a big stack hid it from everyone; now a
  stack of more than one stays available (`World::item_free`) for eating, medicine and ingredients.
  Crafters with a bill fetch its missing ingredients themselves (before, only haulers did, so a busy
  colony never cooked), which also lets the stove's bill be prioritized from the right-click menu
  ("... (bringing raw food first)"). Sleepers wake, and colonists in a mental break eat, below 10 hunger.
- Trapped colonists (2026-10-17, found in a save: shelves made impassable shut Nell into the smithy):
  once a second `update_trapped` lists active colonists who can't walk to the map edge (`World::trapped`,
  not saved); each gets an urgent alert, and climbs out (`way_out`: nearest standable cell beyond the
  furniture from which the edge can be reached, as a `Goto`). Their paths may cross `Map::climbable`
  furniture (built, blocking, not a wall or door; `pathing::bfs_climbing`) at `CLIMB_PACE` (30%).
- Graves (2026-10-17, `src/graves.rs`): a `grave` buildable (`special: Grave`, 2 wood for the marker,
  8 s of digging) goes only on natural soil. Haulers carry a dead colonist to an empty grave
  (`Job::Bury`, corpse reserved in `res_patient`, grave in `res_target`); the colonist stays in the pawn
  list with `buried` set and its position at the grave, so the inspector says whose it is. Buried
  colonists aren't drawn, picked or listed in the colonist bar. While anyone lies unburied, colonists
  think "A colonist lies unburied" (-5) and an alert asks for a grave. Filled graves can't be
  deconstructed. Parts with `filled` show only while a grave is filled or open (pit and heap vs mound;
  the props' `open` flag means an empty grave).
- Eating: `Job::Eat` has an optional seat. Colonists take their food to the nearest free chair beside a
  table within 40 steps of the food, else eat on the spot and get "Ate without a table" (-3).
- Food: `raw` food gives "Ate raw food" (-5) and a `poison` chance of `HediffKind::FoodPoisoning`
  (pain, slower, gone in under a day, never dangerous); other food gives "Ate a cooked meal" (+2).
  Eating and doctors' feeding prefer cooked food.
- Longer days (2026-10-18): `DAY_LEN` 240 → 480 s so colonists get more work done per day at the same
  walking pace. Per-second need rates halved (`HUNGER_DRAIN` 0.2, `REST_DRAIN` 0.165, `SLEEP_GAIN` 0.4 on
  the ground, ×1.5 in a bed; also during battle turns), crop `grow_time` doubled and wild regrowth every 8 s,
  so all of these stay the same per day. Healing, illness, memories and the storyteller already count in
  days, so they take twice as long in real time (raids are spaced further apart).
- Recreation and schedules (2026-10-18): `src/recreation.rs`. `Pawn::joy` (saved, 70 for old saves)
  drains `JOY_DRAIN` (a full need lasts 1.5 days) while awake; thoughts Bored/Very bored/Entertained.
  `BuildDef::fun` (chess table 1.6, bookshelf 1.3, dartboard 1.2 as wall decor); `Job::Relax { at,
  stand }` (furniture from `approach_build`, decor from `decor_front`; reserves `at`), `Job::Stroll`
  (outdoors by day, 0.7). `RoomEffects::fun`: the Recreation room theme (two of the fun things) 1.4.
  `Pawn::schedule: [Block; 24]` (Anything/Work/Fun/Sleep; default sleep 22-6, fun 18-22): Sleep hours
  send them to bed below 90 rest (else below 30), Fun hours to fun until `JOY_FULL` and no work, Anything
  to fun below `JOY_BORED`. Fun ends when full or in Work/Sleep hours. UI: Fun bar, the Work window's
  Schedule tab (pick a block, click or drag to paint), Info Fun row. `testing::colonist` starts
  entertained with no fixed hours, so other tests time only the work.
- Warming up and sitting (2026-10-19): freezing colonists (`needs_warmth`: hypothermia ≥
  `WARM_UP_AT` 0.1 and `felt` below `FREEZING`) take `Job::WarmUp { to }` from `colonist_think` (with
  the urgent needs) to the nearest standable room cell that feels at least `FREEZING + 5`
  (`warm_spot`, with their clothes); they stay until hypothermia is 0, look again if it turns cold
  there, and otherwise carry on. `World::seated` (recreation.rs): settled on a `Seat` while eating
  there, crafting from a chair at a bench's `front_cell`, playing from a chair (`find_fun` prefers a
  free chair around fun furniture), or `Job::Sit { at, until }` (idle: 10% of idle thinks, a free chair
  within 12 steps, 15-30 s, a little fun). `seat_free` keeps chairs to one person. Leaving a seat sets
  `Pawn::getting_up` to `GET_UP` (1.2 s); `go_to_reach` doesn't step meanwhile.

### Items, production and gear

- Phase 6: `assets/defs.ron` (RON, built in with `include_str!`) holds items, buildables and plants;
  `src/defs.rs` loads it once into a global registry (`defs()`), checks it (unique ids, known items and
  colours, sensible numbers) and hands out `ItemId` / `BuildId` / `PlantId` (u16 indexes).
  `Feature` is now `None | Plant { kind, growth } | Built { kind, dir } | Blueprint { kind, delivered }`;
  tasks are `Harvest(plant) | Sow(plant) | Mine | Construct`, with skill and time from the definition.
  Code-handled behaviour is fields: `nutrition`, `medicine` (potency), `pods`, `special` (`Floor`,
  `Stairs`, `Bed`, `Door`), `passable` / `blocks_room` / `blocks_sight` / `holds_floors`, `light`,
  `drops` (default half the cost), `sow`, `wild` (density, regrow weight), `harvest` (designation,
  work type, skill, time, yield, verb). Looks are lists of unit shapes with palette colours; plants
  have growth stages with random variants. Blueprints now show a see-through copy of the real model.
  Toolbar entries and shortcuts for buildables come from the file too (`key`; letters/digits/`-`/`=`,
  fixed keys are refused).
- Saves store definition ids as text (`"wood"`, `"tree"`), so adding or reordering definitions keeps
  saves loadable; renaming an id doesn't. `SAVE_VERSION` is 2. Eating now holds the food item while
  eating (dropped again if interrupted), so its nutrition is known when done.
- Still in code: blocks (soil, rock, water; rock yields `rock_drops`), work types, skills, designations
  and zones. Floors are still one kind (`Map::floor` is a bool), so only one buildable may be the floor.
  Grow zones plant the first plant with `sow`; choosing per zone comes with phase 7's crops.
- Phase 7 (`src/production.rs`): a workbench is any buildable a recipe names (`BuildDef::recipes` is
  filled in at load). `World::benches` holds each bench's bills (`Count(n)`, `Until(n)`, `Forever`,
  suspendable, ordered), its current recipe and delivered ingredients. Once a second
  (`update_production`) benches are synced with the map and each picks the first wanted bill whose
  ingredients exist (`choose_recipes`); stock the new recipe doesn't need is dropped beside the bench.
  Haulers deliver ingredients (`Job::Haul { deliver: true }`, shared with blueprints); a colonist with
  the recipe's work type does `Task::Craft`; products drop beside the bench and get hauled normally.
- Work types Cooking and Crafting, skills Cooking and Crafting (arrays sized from `ALL`).
- New content: planks, stone blocks, iron ore (`Block::Ore` veins, ~5% of rock, richer deeper;
  `ore_drops`), steel, cotton, cloth, raw food / simple meal, healroot / herbal medicine, rifle, pistol,
  club, knife, simple clothes, steel armour; crops corn, healroot, cotton; six workbenches. Beds now
  cost planks; colonists land with planks, meals and raw food.
- Weapons are items (`weapon` stats + `held` look); `Pawn::weapon: Option<ItemId>` (none = fists),
  `Pawn::apparel` one item per slot (`Body`, `Outer`). Raider weapons and the colonists' starting weapon
  come from the definitions. Dead pawns drop their gear. Idle colonists fetch better gear first
  (`Job::Equip`; weapons must be 10% better). Armour scales damage in `Health::apply_damage`; blows
  below 1 point glance off.
- Stockpiles: `Zone::Stockpile(id)` with `World::stockpiles[id]` (name, priority 1-4, forbidden items).
  Painting next to a stockpile extends it, elsewhere starts a new one. Items are hauled to the
  highest-priority stockpile that takes them, and out of a stockpile only into a higher-priority one.
  `Zone::Grow(crop)`; the inspector changes a whole connected grow zone's crop.
- Left for later: per-cook meal quality, fuel, smithing/tailoring as separate skills, apparel visuals on
  the pawn figures, stockpile renaming, bill ingredient filters, ingredients from a radius only.
- Components (2026-10-18): three tiers. Materials: steel, cloth, copper (`Block::Copper` veins from their own
  noise, `copper_drops`, smelted), silicon (refinery: stone), rubber (refinery: latex from the new rubber
  plant). Six component items (category "Components", `ItemDef::about` names what they stand for); benches:
  smithy (structural), crafting bench (textile), machinist (mechanical), chemistry lab (chemical),
  electronics bench (electrical, optical), fabricator (powered armour, laser rifle), salvage bench. Existing
  gear reworked onto components; new assault rifle. `BuildDef::cost` is a list (single amount still loads),
  delivered in order; `Blueprint::delivered` counts across it (`BuildDef::missing`, `delivered_split`,
  `drops` per material). Benches cost components in a ladder (tested: every bench, component and endgame
  item reachable without salvage, fabricator last). `Defs::add_salvage` generates "Salvage X" for every
  component-made single item (half of each ingredient back); scrap metal (raiders drop 1–2, pods) and
  e-waste (pods) have fixed salvage recipes. `BuildDef::power` (shown only) and `open_sky` (PV system);
  Resources window sums power. `DamageKind::Burn` (laser: no bleeding, more pain). Generated "laser" sound.
- Equip order: select a colonist and right-click a weapon or piece of apparel lying on
  the map (`World::order_equip`, `Job::Equip { chosen: true }`). What they're told to use goes into
  `Pawn::chosen` and isn't swapped for better gear automatically; switching or dying clears it. Both new
  fields default when missing, so older saves still load. Not done yet: dropping or unequipping gear from
  the inspector.
- One loadout (2026-10-18): everyday gear and the combat loadout were two layers (the battle put the
  loadout on from storage at once and dropped it afterwards; right-click "kept" gear lived in
  `Pawn::chosen`). Now `Pawn::loadout` is the gear they use: `equip_candidate` sends them for loadout
  items they don't have on (walking, as any job), and only for slots the loadout leaves free do they go
  for upgrades (`wants_upgrade`); `run_equip` takes only what they came for. Right-click Equip sets the
  loadout slot (`World::set_loadout_item`; a squad member taking the other kind of weapon leaves the
  squad). Battles no longer swap gear (`gear_up` removed; `gear_down` stays for battles saved mid-way).
  Loading moves old `chosen` gear into free loadout slots (`chosen` is no longer saved). The Combat tab
  calls it Loadout and has a helmet slot.

- Body slots and cyberware (2026-10-20). Gear: `Pawn::weapon`, `sidearm` (`WeaponDef.sidearm`:
  pistol, knife, club; `sidearm_drawn` while in hand, `Pawn::held`), `utility: Option<Stack>` (belt items,
  `ItemDef.utility`: `Grenade`/`Medkit`, up to `carry`) and apparel by `Slot` (`Head`, `Face`, `Shirt`
  = the old `Body`, aliased in defs, `Outer` = jacket/vest, `Legs`). `ApparelDef.also` lets one item take
  several slots (powered armour: outer, legs, head, face); `Pawn::wear` returns everything it displaced.
  Loadouts keep one weapon, one belt item and non-overlapping apparel (`squads::conflicts`) plus a separate
  `loadout_sidearm`; auto-upgrades are apparel and main weapons only. Cyberware: `health::CyberSlot`
  (arms, legs, eyes, spine, heart, lungs replace natural parts; brain and dermal add on), stored as
  `HediffKind::Prosthetic { efficiency, item }` on the slot's parts (dermal: whole body), so losing the
  part loses the implant. `CyberwareDef` (`fits`, `efficiency`, `armor`, `aim`, `speed`) replaces
  `ProstheticDef` (`prosthetic:` still parses). Operations are `{ slot, item: Option }` in
  `Pawn::surgeries` (install, swap, or `None` = take out; heart, lungs and spine can't be taken out);
  pending orders from older saves are dropped, everything else loads. Battles: `World::choose_weapon`
  picks main or sidearm per strike by expected harm; `aim_bonus` (goggles, coprocessor) adds to the base
  hit chance, and sight/manipulation above 1 (bionics) count up to 1.2; `BattleOrder::Throw(t)` (6
  cells, everyone in the unit, +30 suppression) and `Medkit`; the AI uses both. Leg, face and cyberware
  items have stored props (`NEW_ITEMS` in `tools/make_items.py`) but nothing drawn on the character yet.

- Hit locations, raider tiers and backpacks (2026-10-20). `Health::apply_damage` takes armour per part:
  the outer part hit is picked first, and `Pawn::armor_at(part)` sums what protects it (`Slot::protects`,
  `ApparelDef.sealed` for everything, dermal implants everywhere; capped at 0.8); a deep blow is stopped
  where it came in. `Pawn::armor` is the hit-weighted average (for display). Armour values were raised to
  suit (helmet 0.45, Flak Jacket 0.45, leg guards 0.35). `Slot::Back` packs: `carry` (expeditions:
  `CARRY_EACH` 60 a member plus packs; `Expedition::take_in` leaves the rest behind), `tend` (added to
  tending and medkit quality), `ap` (`World::ap_bonus`: the best radio in a unit, added to its AP each
  round; under 40 so a pinned unit stays held down). Packs, sidearms and belt items are never auto-equipped.
  Raiders: `raider_tiers` replaces `raider_weapons`; `World::threat` (days + wealth/500, × raid size) picks
  the tier (`raid_tier`, or `Difficulty::raid_gear`), carried in `Parley::tier`; `equip_raider` rolls
  weapon, apparel, sidearm, belt and cyberware and adds 2 Shooting/Melee a tier. Expedition fights use
  the colony's tier. In 20 AI-vs-AI runs of the battle scenario (mercenaries and an elite pair) the
  colony won 18.

- Sidearms and squad styles (2026-10-20). Style stays the main weapon's (deploying, formation, AI).
  `Attack::long_gun` (ranged, not a sidearm) hits × `CLOSE_QUARTERS` (0.2) against adjacent targets, so
  `choose_weapon` picks a pistol up close (0.2 keeps even the assault rifle's three shots below one
  pistol shot). `World::attack_cost`: `AP_SIDEARM_ATTACK` (25) when everyone attacking would use their
  sidearm (`planned_weapons`), else `AP_ATTACK`; the AI checks it too. `squads::sidearm_fits_squad`:
  ranged squads only ranged sidearms (loadout and right-click refuse others; `World::sidearm_barred`
  keeps one given anyway holstered in battle), melee squads and lone fighters anything; raiders follow
  the same rule. AI-vs-AI battle scenario: the colony still won 18 of 20.
- Melee pace and shields (2026-10-20). `MELEE_PACE` 1.15: `step_ap` and the walk itself are faster for a
  unit whose members all fight in melee (`World::melee_pace`). `UtilityEffect::Shield { energy, recharge }`
  (`personal_shield`, belt, `ap` 0): `Pawn::shield_drain` (saved), `shield_energy`; `World::shot_damage`
  (shots and grenades, not blows) drains it first, the rest breaking through; recharged each round in
  `battle_tick`, reset in `end_battle`. `Pawn::can_use` bars ranged weapons while shielded, so `held`,
  `attack`, `main_attack`, `weapons` and `squads::fights_at_range` all treat the wearer as melee. Raiders
  only get shields with melee weapons. Balance (AI vs AI, battle scenario): melee raiders are strong
  against this colony whatever `CLOSE_QUARTERS` (0.2–1.0) or `MELEE_PACE` (1.0–1.15): eight raiders
  (4+2+2) won 16 of 20; the scenario now has three pairs (2+2+2), and the colony wins about 21 of 30.

### Seasons, temperature and power

- Seasons, step 1 (2026-10-18): `src/seasons.rs`. A year of four `Season`s of `SEASON_DAYS` (5) days from
  game time (no saved state). Sown crops grow at `Season::crop_growth` (1, 1, 0.6, 0) in `grow_plants`;
  wild regrowth stops in winter; `check_season` (each tick) logs the new season and, at winter, kills
  unripe sown crops (`frost`). `crops_frost_will_kill` feeds an alert the last day and a half of autumn.
  Front end: the season in the HUD, grass tinted by season (`terrain::Cut::season`, golden/snow), a
  Seasons row on crops' Info sheets. Next steps: temperature (rooms keep warmth by walls, roof, doors;
  mood and health), clothing warmth, then heaters and coolers using power.
- Seasons, step 2: temperature (2026-10-18): `src/temperature.rs`. `outdoor_at(t)`: yearly cosine
  (mean 11, ±16, warmest a quarter-season into summer so spring starts mild and winter starts cold) plus
  ±4 over the day (warmest 15:00). Rooms: `RoomStats::{heat, doors, temp}`; once a second
  `update_temperatures` moves each room's temperature towards outdoor + 3 * heat / sqrt(size) with a
  lag of 0.2 days * max(1, sqrt(size)/3) / (1 + doors/2); `World::cell_temp` (not saved) carries it
  over when rooms are recounted. `BuildDef::heat`: torch 3, stove 8, smithy 10, smelter 12, the new
  fireplace 16. Colonists: `Pawn::felt` each tick; thoughts Cold (< 10), Freezing (< 0), Hot (> 30);
  `Health::hypothermia` grows 0.08 a day per degree below 0, falls 1 a day from 5 (0.3 between),
  lowers consciousness, moving and manipulation, downs at 0.7, kills at 1 ("froze to death"). UI: outside
  temperature in the HUD, temperatures in the inspector, hypothermia in the Health tab, a freezing
  alert, a Heat row in Info. Not yet: colonists don't seek warmth by themselves; clothing warmth (step 3)
  and powered heaters and coolers (step 4).
- Seasons, step 3: clothing warmth (2026-10-18): `ApparelDef::warmth` (clothes 4, steel helmet 1, steel
  armour 2, powered armour 12, new parka 14 (Outer, 12 cloth) and wool hat 4 (Head, 3 cloth), both at
  the crafting bench). `temperature::felt(t, warmth)` adds it scaled by `chill(t)` (full at 10°C and
  below, none from 20°C), used for `Pawn::felt`. `ApparelDef::score_at(outdoors)` weighs warmth by the
  same chill (3 a degree), so colonists swap armour for a parka in the cold (slots their loadout leaves
  free). `WornPiece::cloth` draws pieces in plain colours. UI: Info Warmth row; the colonist inspector
  shows what they feel.
- Seasons, step 4: power for heating and cooling (2026-10-18): `src/power.rs`. One grid for the colony.
  `Power::draw` (watts while running); `electric_heater` (heat 20, 800 W) and `cooler` (heat -16, 800 W)
  run when their room (or the room beside, for one in a wall) is below `HEAT_BELOW` (18) / above
  `COOL_ABOVE` (24). Once a second, after the room temperatures, `update_power` gives PV output
  (`output` x daylight, none at night or covered) to wanting devices in turn, the batteries
  (`World::stored_wh`, saved; capacity = sum of `storage`) making up the rest and taking what's left;
  devices that can't be fed stay off. `World::running` (cells) decides whose `heat` counts in
  `update_room_stats` (fires always count). `World::power` (`PowerState`) feeds the Resources window and
  the "Not enough power" alert; `device_state` the inspector. Possible later: cables and separate grids,
  more consumers (lamps, benches).

### Battles and squads

- Phase 12 (turn-based battles):
  - **Squads (`src/squads.rs`):** `Pawn::role` is `Fighter` (default), `Civilian`, `Leader` or
    `Member(leader)`; `set_role` checks the leader and `SQUAD_MAX`, and stepping down or dying releases
    the members. `update_squads` runs once a second and sets the cached `Pawn::poor_quarters`: a leader
    needs a bed in a room of tier Decent or better that nobody else sleeps in. `Pawn::loadout` holds
    one weapon and one item per slot (`set_loadout`). `Zone::Safe` is painted from the Zones menu.
    - Interface (`src/front/squads_ui.rs`): the inspector's Combat tab (role, squad, loadout dropdowns
      with stored counts, "Use current gear") and the Squads window (HUD button).
  - **Parley and battles (`src/battle.rs`):**
    - **Parley:** `spawn_raid` opens `World::parley` (raider squads of up to 4, entry cell, a demand of
      `DEMAND_PER_RAIDER` value each, most valuable goods first; empty if the colony can't pay). Item
      values are a new `value` field in `defs.ron`. While a parley is open `tick` does nothing;
      `pay_tribute` / `refuse_tribute` answer it.
    - **Starting:** `start_battle` ends everyone's job, spawns the raiders in squads near the entry, and
      forms the colony's units: leaders' squads (members within 2 cells join at once; the rest are
      `Arrival`s after `ceil((path steps - 2) / CELLS_PER_ROUND)` rounds, 10 cells a round), lone
      fighters where they stand, and
      civilians moved to safe-zone cells as passive units. Fighters put on their loadout from storage;
      `Pawn::stash` remembers (item taken, what it replaced) and `gear_down` reverses it afterwards
      (the battle gear is left at their feet for haulers; death drops the stash too).
    - **Turns:** while `World::battle` is set, `tick` runs only `battle_tick`. The sides take turns
      activating one unit each (`BattleOrder::Activate`, switchable until AP are spent); every unit
      acts once per round and the colony goes first. AP: 100; moving 10 a step (so 10 cells, or 6 and
      an attack; the whole unit walks to the one cell); attacking
      40 (every member who can reach someone shoots or strikes); hunkering 20 (ends the turn, better
      cover until next activation).
      Each round passes `TURN_SECS` (6 s) for health and hunger, halves suppression, counts arrivals
      down and restores AP (60 if pinned).
    - **Hit chance:** `hit_chance` uses the old formula (skill, distance, sight, manipulation) and
      then: cover on the side facing the shooter (`cover` / `cover_at`: rock, walls and doors full
      ×0.45; other structures and trees half ×0.7; weaker when shot from above), hunkered ×0.75,
      higher ground ×1.15, a pinned shooter ×0.7. `battle_sight` is line of sight that ignores cover
      next to either end (you lean out to shoot).
    - **Suppression and morale:** suppression +8 per shot at a unit (+15 if it hits); 50 or more pins
      it. Morale starts from mood (raiders 60); -20 per member lost, -20 more for the leader. Below 25
      raiders flee (to the nearest edge, gone there) and colonists cower (their turn is skipped,
      hunkered).
    - **Ending:** won when no raider unit is left; lost when no colony unit can act (the raiders take
      1.5× the demand and leave); after `STALEMATE` (10) rounds with no attack and the sides getting no
      closer than they have been (`Battle::closest`), the raiders give up. Then gear comes off, everyone gets "Won a battle" (+6) or "Lost a battle" (-10), standing
      raiders leave, and the downed stay.
  - **AI (`src/battle_ai.rs`):** activates the unit nearest the enemy and attacks when its expected
    hits reach 0.45. Otherwise it moves first: to a spot it can attack from with AP to spare (+10, plus
    cover), else one to attack from next round (+3), else as far along the walking path towards the
    nearest enemy as it can (-0.3 per step; it measures walking distance, so it goes round obstacles).
    Then it attacks, hunkers, or ends. `Battle::auto_colony` lets it play the colony too (tests and
    `--example profile`).
  - **Interface (`src/front/battle_ui.rs`):**
    - the parley dialog;
    - the battle panel: whose turn it is, the active unit's AP, Hunker / End turn, your units, arrivals
      and events;
    - hit chances over enemies;
    - reachable cells, green while there are AP left to attack;
    - the path preview, cover markers on the hovered cell's sides, and unit rings with squad lines;
    - clicks: your unit to activate it, an enemy to attack, a cell to move. Space ends the turn; Tab
      goes to the next unit;
    - the camera eases to the acting unit.
  - During a battle the normal clicks, tools, Space and Tab are off, and the build menu is hidden (also
    during the parley). Hunkered pawns crouch; fighters hold their weapons and face the nearest enemy.
  - **Movement area:** a see-through mesh (`ReachArea`, rebuilt only when the unit, its cell, AP or
    the view change): blue where the unit can still attack after moving, yellow for a full run.
    Bold outlines in a gizmo group with 4 px lines (`BattleGizmos`). The hovered destination gets a
    bold ring, the path, and a label with the AP cost and whether an attack is still possible.
  - **Walking and running:** colonists walk around the colony at `WALK_SPEED` 1.0 cells/s (about
    1.8 m/s; it was 4, a sprint) and run in battle at `RUN_SPEED` 3.0 (`Pawn::running`, set for
    everyone while a battle lasts). The clip follows: walk out of battle (about 1.9× its own pace),
    jog in battle (its own pace), sprint only if very fast; a faster game plays the same clip faster.
    Footsteps come once per stride of the clip (0.36 cells walking, 1.37 running); running uses the
    louder, deeper `run_*` sounds (same files). To make up a little for the walking, `work_speed` is
    ×1.25. A two-day `--example profile` colony now makes fewer things (17 meals, 82 planks and 9
    clubs, against 22, 146 and 30 before).
  - **Removed:** drafting (`Pawn::drafted`, F, the Draft button, right-click orders), `Job::Attack` and
    `Job::Leave`, `auto_fire` and `raider_think`. Raiders exist only in battles. `SAVE_VERSION` is 5.
  - **Left for 12d (optional, 2026-10-19: battles are fine for now):** tuning from real play (AP costs, odds, morale, raid sizes and demand), a proper
    pick-up for loadouts that aren't in storage, prisoners, explosives and vehicles, overwatch-like
    reactions (MENACE has none), and showing the hit chance per member.
- "End all turns" (battle panel): `World::end_colony_turns` sets `Battle::colony_passes`, so each of
  the colony's remaining units ends its turn as it comes up (the raiders still alternate). It
  resets each round. Clicking a unit's square again cycles the selection through its roster (the
  fallen too), to inspect every member of a squad, raiders' included.
- "Round N" fades in and out in the middle of the screen when a round starts (`BANNER_SECS`, 1.8 s).
- Cover tiers (2026-10-16), like MENACE: `defs::CoverKind` None / Light / Medium / Heavy
  (`battle::Cover`), set per buildable and plant with `cover` in `defs.ron`. Defaults: heavy if it
  blocks sight, medium if it can't be walked through, else light; plants medium if impassable.
  Rock and soil are heavy. Hit factor: light ×0.8, medium ×0.6, heavy ×0.45 (only for shots that get
  past it at an angle). From above, cover counts one step less. Heavy cover blocks the view: there's
  no peeking any more (`battle_sight` is just `clear_line`). Markers on the hovered cell are drawn
  at three heights.
- The battle camera eases to a unit once, as its turn starts, and stops as soon as the player moves
  the camera (keys, right or middle drag, wheel). It used to keep pulling back during every move.
  The acting unit is put in the inspector once per turn, not every frame.
- Battle additions (2026-10-16):
  - **Suppression:** counted per attack (+15 for coming under attack, +10 per hit), not per shot.
    With rifles firing twice it pinned squads nearly every round, leaving 60 AP and so one attack.
    The panel says "pinned down: N AP this round".
  - **Hit chance from a cell:** `hit_chance_from` / `can_attack_from`. While a destination is hovered,
    each enemy group's label shows the chance from there ("no AP left" if the move uses the attack's
    AP; "out of reach from there").
  - **Deploying:** squads only (`BattleOrder::Deploy`, `Unit::deployed`, `AP_DEPLOY` 30 each way).
    The squad goes down on one knee: its shots ×1.25, shots at it ×0.8, and it can't move until it
    stands up. It stays deployed across rounds. It kneels (crouch clip) and fires without standing.
    The movement area hides while deployed. The AI deploys a squad that's staying put to fire at the
    start of its activation, and stands up to move or flee.
- Battle line of sight (2026-10-16): `battle_sight` used to ignore every cell next to either end (to
  allow leaning out), which let people shoot through a wall they both stood against. It also sampled
  points a third of a cell apart, so lines grazing a corner were blocked. Now:
  - **The line:** `clear_line` traces every column of cells the line crosses at chest height. Rock,
    walls and doors block it, and so does a built floor where the line passes from one level to
    another. A corner blocks only if both cells beside it do.
  - **Leaning out:** a unit with cover next to it may see, and be seen, from the open cells beside
    it (`peek_spots`), so it can shoot past a wall's end or round a corner, never through a wall
    between the two.
- Battle fixes (2026-10-16):
  - **Targets:** clicking and hovering prefer someone still in a unit (`battle_ui::fighter_at`), so
    the downed lying on a cell don't block attacks on the living there.
  - **Shots:** weapons have `shots` per attack (`defs.ron`, default 1; rifles 2), taken in turn: everyone's
    first shot, then the second ones.
  - **Picking who is hit:** the target is picked when each shot fires (`pick_target`, from the unit in
    `Strike::unit`), and a squad's leader is only picked once nobody else in it is standing.
  - **Showing losses:** `Unit::roster` keeps everyone who fought in a unit. Over each squad a status
    row shows a bar per member (health, dark red when downed, grey crossed out when dead; the leader's
    outlined) and "Name (standing/total)". The panel lists "3/4 standing". Per-person overlay labels
    are left out for squad members in battle (they stacked on one square).
- Battle running (2026-10-16): `RUN_SPEED` is 4.5 cells/s, with strides 60% longer than the jog
  clip's (`characters::RUN_STRIDE`), so the clip plays at about 0.95x: a steady step, with the feet
  sliding a little. The jog is kept up to 6 cells/s before the sprint takes over. Running footsteps
  are spaced by the longer stride.
- Squads hold one cell (2026-10-16): a squad stands in for a single colonist in battle. It has one
  cell (its leader's; `World::unit_cell`), one cover check, one line of sight, and no other unit may
  share it. At most 4 people (`SQUAD_MAX`), because a 1.8 m cell fits 4 shoulder to shoulder.
  Members start the battle and arrive on the leader's cell. A move sends everyone to the same cell
  together (`Battle::moving: Vec<Mover>`), at the pace of the slowest member, keeping the huddle
  shape while walking (a staggered column made moves about a second longer and was dropped). Shots at a squad hit a random member; area attacks could later hit all of them. The front
  end only draws the members apart (`pawns.rs::squad_slot`): standing still, in a row against the
  cover facing the nearest enemy, else (and while walking) in a huddle, overlapping the neighbouring
  cells a little. There's one ring per
  unit, an "xN" count beside a squad's ring, one hit-chance label per enemy unit, and a single aiming
  line in the attack preview. `SAVE_VERSION` is 6.
- Battle pacing (after the first play test): battles always run at 1x game speed (`Sim::rate`);
  running is `RUN_SPEED` 2.2 cells/s (an easy jog). An attack is a sequence of `Strike`s
  (`Battle::strikes`): the unit takes aim (0.6 s), then each member fires 0.45 s (blows 0.8 s) after
  the last, and the attack ends 0.7 s after the last. Each one is resolved as it happens, so a target
  who went down is swapped for another one in reach. Pawns face their `aim_target`. Rifle range is
  now 16. Hovering an enemy switches to an attack preview: the move marker hides; aiming lines run from
  each member who can reach, bold rings mark the targets, a label gives the best chance, how many can
  reach and the AP cost (or why not), and the pointer becomes a crosshair.
- Enemies block the way in battles (2026-10-17): `World::battle_field(side, from)` is a BFS
  (`pathing::bfs_avoiding`) that can't enter cells where an active enemy of `side` stands. Move costs, the AI
  (`best_spot`, `flee_order`), the reach area and path preview use it, and `start_move` plans each walker's
  path with it (so `go_to` follows it). Allies can still pass each other.
- Squads fight one way (2026-10-17): everyone in a squad, the leader too, is ranged or melee
  (`squads::fights_at_range`: the loadout weapon, else the one carried; bare hands are melee). Joining a
  squad of the other kind and a member's loadout switching kind are refused; a leader switching kind
  drops the members who no longer fit. At battle start everyone gears up first, and a member armed the
  other way (gun not in storage, picked up a club) fights alone that battle. Raider squads are armed
  alike (`Pawn::raider_weapon` with a kind).
- Battle readability (2026-10-17): every unit (lone fighters and raiders too) has a plate: state chips
  (PINNED, SHAKEN / FLEEING, HUNKERED, DEPLOYED, SHELTERING), name, health per member with a red dot when
  bleeding, and morale and suppression bars with a tick at `BROKEN` / `PINNED`. Changes pop up and rise
  ("+25 suppression", "-20 morale", "PINNED!", "SHAKEN!", "RALLIES"; quick hits add up), from
  `BattleView::last`. The panel shows labelled bars and chips with explanations on hover, for the acting
  unit and the hovered enemy, and states in the unit list.
- Hit chances in the Combat tab (2026-10-18): `battle::base_hit_chance` (skill, distance, sight and
  manipulation) is shared by `hit_chance_from` and the tab's new Fighting section (Shooting and Melee
  levels, chances with the weapon they have: shots at 3 cells and at its range, or blows).
- Movement in battle (2026-10-18): `World::step_ap` is what a unit pays per cell: `AP_MOVE` divided by
  the slowest member's `Pawn::pace` (Moving capacity, 15% at least, times `gear_speed`, the product of
  `ApparelDef::speed`; powered armour 1.25, so 8; a limp at 50% makes it 20),
  times `PINNED_MOVE` (2) for a unit that started the round pinned (`Unit::held_down`: its AP were cut,
  even if its suppression has halved since). Used by orders, the AI and the move preview. Everyday
  walking (`Pawn::move_speed`) uses `gear_speed` too.
- Only ranged squads deploy (2026-10-19): `World::can_deploy(u)` (a led unit whose leader fights at
  range, or one already deployed so it can stand up); the order, the AI and the battle panel's button
  use it.

### Storyteller, raids and expeditions

- Difficulty (2026-10-17, `story.rs`): `World::difficulty` (`Difficulty { raids, raid_size }`, saved, carried over
  to a new game). With raids off the raid timer runs on but nobody comes. `raid_size()` scales the usual size
  (1 + day/3 + wealth/800, up to 8) by `raid_size` (0.25–3, at least 1, up to 8 × the multiplier when above 1).
  `raid_now` calls one in (not during another). The Menu has a Difficulty section with all three.
- Expeditions (2026-10-18): `src/expeditions.rs`. `World::factions` (three, `starting_factions`;
  goodwill -100..100 sets `sell_factor`/`buy_factor`), `offers` (up to 4, about one a day, open 3 days)
  and `expeditions`, all saved (defaults for old saves). `send_expedition` takes the pawns out of
  `World::pawns` into `Expedition::members` and the packed goods off the map. Stages: `Out` (halfway:
  `waylay_event`, toll for Trade or ambush for Escort; then `main_event`), `Decide` (waits for `choose`),
  `Trading` (`trade(sell, buy)` needs worth*sell_factor >= worth*buy_factor; `leave_market`), `Fighting`,
  and `Back` (then `come_home` at a map edge, with goods and any recruit). `Effect`s: Goodwill, Gain,
  Give (out of the cargo; `affords` greys out choices they can't pay), GiveAll (gifts → goodwill by
  worth), Delay, Injure, Recruit, Trade, Truce (pushes `story.next_raid`), Fight, Chance. Fights:
  `start_site_fight` builds a 34×24 `World` (road/camp/crash cover), puts the squad on the west edge and
  calls `start_battle` with raiders from the east; it waits in `Expedition::site`. The front end's
  `Sim::swap_sites` swaps it in (the colony waits in `Sim::home`) and back when the battle ends, and
  `finish_site` returns the survivors and the loot (if won). Saves go through `Sim::saveable` (site put
  back). `raid_spacing` scales the raid interval by the bandits' goodwill (0.7-2x); allies send gifts.
  `colony_lost` counts colonists away. UI: `front/expeditions_ui.rs` (Offers with planning and packing,
  Under way with decisions, the market and the journal, Factions); alerts for waiting decisions; the
  camera re-centres on a site. Possible later: faction raids by other factions, caravans visiting the
  colony, more varied site maps, a world map.
- Reachable edge entries (2026-10-19): `World::random_edge_tile` (raids, wanderers, returning
  expeditions and gifts) only picks a passable edge cell from which a living colonist can be reached on
  foot (`bfs`, up to 40 tries), so nobody arrives in a pocket closed off by trees, cliffs or water.
  Falls back to any edge cell if nobody can be reached. Trees stay impassable.

### Graphics and sound

- Phase 10b (`src/front/characters.rs`, `src/front/pawns.rs`): every pawn spawns a Quaternius character
  (outfit + cut head + hair, beard for some men) under its root, scaled `RIG_SCALE` 0.55. Colonists wear
  the peasant outfit (half with the second colour scheme), tinted halfway towards the pawn's colour;
  raiders wear the hooded ranger outfit tinted red. Sex and hair come from a hash of the pawn id (front
  end only, not saved). Skin materials are tinted relative to the texture's own tone, hair textures
  (grey) by hair colour. Each character's materials are its own copies, greyed on death.
  - The code-built figure stays as the fallback: shown until all parts are spawned and the animation
    graph is attached, and for good if the models don't load (headless tests, no GPU).
  - Animations (`Anim`): idle, walk/jog/sprint by drawn speed (thresholds 1.2 and 4.3 cells/s; the clip
    rate is speed ÷ the clip's root-motion speed, so feet don't slide; colonists' 4 cells/s is a quick
    jog), Sword_Attack looped for chopping and mining, Fixing_Kneeling for sowing, crop harvests,
    floors and tending, Interact for walls and workbenches, Sitting_Idle on a chair (moved onto the
    seat, facing the table), Crouch_Idle eating without one, Pistol_Idle / Sword_Idle when armed, a
    one-shot Pistol_Shoot / Sword_Attack / Punch_Cross when an attack starts the weapon's cooldown,
    Death01 held when downed or dead off a bed, Jump_Loop while falling. In bed the character is tucked in: the outfit
    (everything below the head) is hidden under a blanket lump in the pawn's colour, the back of the head
    rests on the pillow. Asleep off a bed, the body is laid flat with a slow idle. Sleeping, downed and
    dead characters close their eyes (eye meshes painted in a skin-coloured eyelid material). Cross-fades of 0.25 s via `AnimationTransitions`.
    Non-walking clips run at the simulation speed (stopped when paused).
  - Movement: the drawn position trails the simulated one (`FOLLOW`, scaled with game speed), which
    rounds path corners and eases starts and stops; turning is rate-limited (`TURN_RATE`), facing the
    walking direction, else the work target, enemy or table.
  - Weapons hang off the `hand_r` bone: the definitions' `held` size and turn, rotated by the inverse of
    the hand's measured rotation in the aiming pose (`AIM_HAND`) so they point forward when aiming,
    gripped near one end. Carried items are still a box in front of the chest (no carry clip).
  - Tests: walk-speed matching and weapon orientation run normally; `characters_animate` (whole game
    until every character is ready, has a hand and plays) needs a GPU: `cargo test --release -- --ignored`.
  - Left for later: a real carry pose (the library has none; an upper-body mask over walking), pick-up
    and put-down clips (the simulation doesn't pause for them), hit reactions (Hit_Chest), a proper
    pose for sleeping on the ground, walking on stairs/slopes beyond the hop, foot IK, apparel shown on the body,
    tuning everything by eye.
- Armour shows (2026-10-17): `ApparelDef::pieces` (`WornPiece { bone, parts, hides_hair }`): shapes on a bone
  of the character, placed from the bone in the model's own frame (metres, +z forward). `Rig::bones` records
  each bone's turn at rest (composed down from the `Armature` as parts load), and `props::spawn_worn` hangs
  the parts under the bone turned back by it, in polished metal (`PropAssets::metals`), so they follow the
  animations. Steel armour: a rounded breastplate (spine_03) with iron collar and belt, shoulder domes
  (upperarm_l/_r); new steel helmet (slot `Head`, smithy): dome, brim and nose guard on `Head`, hiding the hair
  (`Rig::show_hair`). Sizes were checked against the body and head meshes (no skull vertex pokes through the
  helmet; the breastplate covers the torso except where the arms join). The figure shows a metal shell and cap.
  (An earlier outfit tint, `worn`, was dropped.)
- Notifications (2026-10-20): a prominent framed panel at the top right replaces the small faded
  event lines in the bottom toolbar. Urgent alerts sort first, with red cards and explicit urgency
  labels; warnings have amber borders. The latest three events stay visible in separate cards,
  with the newest in larger type and an amber frame. Alerts and events scroll independently to
  keep long lists within the viewport. Gameplay, event storage and sounds are unchanged.
- Hide roofs (Shift+R or the HUD button; like Going Medieval's roof toggle): every column is cut off
  just under its topmost roof (built or blueprint, finished room or not), or above the topmost roofed
  room whose ceiling is a built floor (`input::roof_cut`)
  (`UiState::roof_cut`, recomputed each frame in `update_roof_cut`), so the top room of every building
  is visible at once. Rock over caves and tunnels stays. Terrain (`terrain::Cut`), props, items,
  pawns, labels, markers and picking all use `UiState::top_at` / `shows_at`; combined with the layer
  view it takes the lower of the two. Tool shortcuts ignore keys pressed with Shift.
- Furniture against thin walls (2026-10-18): drawing only. `props::snug` pushes turnable furniture
  (not stairs, ladders or seats) back until it touches a wall, window or rock right behind it (the side
  opposite its `front`), from how far its parts reach; kept in the prop keys so it updates when a wall
  goes up or comes down. Items on such shelves, sleepers in such beds and crafters at their front move
  along (`built_snug`, `pawns::snugged`). Sleepers now lie along the bed as it's turned (`bed_yaw`).
- Doors drawn like windows (2026-10-18): doors use `BuildLook::Opening` (a hole from the floor to 0.94,
  no sill when it starts at the floor; passable openings are allowed only for doors), so the terrain
  draws their wall parts thin and papered per side and `under_wall` gives the doorway the floor of
  the room on one side and the outside on the other. The door leaf (and a stone door's stone frame)
  stay props. The stone door now holds floors like the wooden one.
- Blueprint preview (2026-10-18): `props::preview_blueprints` shows ghosts (the blueprint look,
  turned and pushed back as they'd be) where the picked buildable would go: under the cursor, or over
  the dragged box (`input::box_cells`, shared with `paint_box`, so the preview is what a click
  places; at most 400), and for wall decor in the slot under the cursor. Rebuilt only when that changes.
- No glTF loader warnings (2026-10-18): the character models carried vertex attributes Bevy doesn't load
  (COLOR_1-3, TEXCOORD_2-4, JOINTS_1/WEIGHTS_1), so it warned on every load. They're stripped from the
  meshes' attribute lists (the data stays in the .bin files, unused); a test
  (`characters::models_only_use_vertex_attributes_bevy_loads`) keeps new models clean.
- Phase 9 (sound): `src/front/audio.rs` plays everything; `assets/sounds.ron` maps sound names to files
  (random variant, volume, pitch range, `range` in cells, `max` at once, `every` for work strokes, `kind`
  for the volume slider) plus three ambience loops. The files are synthesised placeholders from
  `tools/make_sounds.py` (stdlib Python, deterministic): replace them by editing `sounds.ron`. Bevy's
  `wav` feature is on for them (OGG works by default).
  - The simulation notes `SoundEvent`s (`World::sounds`, capped, not saved; `take_sounds`): shots
    (weapon `sound`), hits / armour glancing off, deaths, falls and collapses, trees falling
    (`done_sound`), rock breaking, digging, finished buildings, raids, newcomers, cargo pods, skill-ups.
    `SIM_SOUNDS` lists the names it uses.
  - The front end adds footsteps (by floor / rock / grass underfoot), work strokes while a pawn's work
    progresses (from the harvest / sow / buildable / recipe `sound`, mining by block), doors opening,
    and interface sounds (tool change, windows, placing orders, inspector buttons).
  - Volume comes from distance to the camera's focus and zoom, set by us; Bevy's spatial audio is used
    only for left/right panning (the emitter goes one unit from the listener in the sound's direction),
    because its own falloff is steep inverse-square. At most 32 effects at once and `max` per sound.
  - Ambience loops are sparse sounds over silence (birds by day, crickets at night, drips in caves); the
    first version's wind / rumble noise bed sounded like a waterfall and was removed.
  - Ambience: day / night by daylight, cave when the focus is under cover or the layer view is below
    the surface. Sound window: master, effects, alerts, interface and ambience sliders, mute (not saved).
  - Tests check every sound name the game and the definitions use exists, every file is there, WAVs
    load, and the simulation makes the expected sounds.
- Real sounds (2026-10-16):
  - **Kenney (CC0):** Impact Sounds, RPG Audio and UI Audio, copied into `assets/sounds/kenney/`.
    They cover footsteps (grass, concrete, wood; snow for digging), mining and rock, planks for
    building, metal for the anvil, chisel and glancing blows, punches for hits and thuds, chopping,
    cloth rustles, crafting clicks, knife draws for swings, doors, creaks for falling trees, and UI
    clicks.
  - **Music:** "Dream Raid" by Matthew Pablo (CC-BY 3.0, credit in `assets/CREDITS.md`) at
    `sounds/music/dream_raid.mp3`, with Bevy's `mp3` feature on. It plays from the start while
    raiders are at the gate and through the battle (`ambience.battle` in `sounds.ron`,
    `audio::battle_music`), replacing the ambience, then fades out. It's the alarm too, so the
    "raid" horn is gone. There's a Music volume slider.
  - **Still placeholders:** guns, saw, sizzle, furnace, death, pods, joined, level up, and the
    ambience loops. Originals are in `asset-sources/kenney/` and `asset-sources/matthew-pablo/`.
- No footsteps at 5x and 10x speed (2026-10-18): they ran together into a constant noise
  (`audio::steps_heard`, `SILENT_STEPS_SPEED`); work strokes and other sounds still play.
- Layered poses (2026-10-19, `front/characters.rs`): the library graph holds each clip three times,
  whole body, legs only and upper body only (bone mask groups: everything under `spine_01` is upper).
  `Pose { body, upper }` plays one clip, or the legs of `body` with the upper body of `upper`. `play`
  does its own cross-fading (`Rig::layers`, replacing `AnimationTransitions`); a node starting while
  its clip plays elsewhere starts at the same time. Deployed: `Kneel` legs + `Aim` (or `Shoot` when
  firing). Carrying: `Carry` (`Driving_Loop`'s arms) over walking or standing. Seated (`World::seated`):
  `Sit`, `SitTalk` at chess, `Sit` + `Interact` crafting; `SitEnter` plays on sitting down and
  `SitExit` while `getting_up`; the seat offset glides. Idle sitters face the chair's front. The clips
  are constants in `Anim::clip` (to swap after a look).

### Saves, performance and history

- Saves carry a format version (`save.rs`, `SAVE_VERSION`) and refuse older or newer formats with a
  readable message. There are no migrations (removed 2026-10-20, when they could only run on current
  saves and some changed them on load): change the format, bump the version. Loading checks per-cell
  sizes and references (`save::check`) and only rebuilds derived state, so save, load and save again
  gives the same file; room temperatures and the last once-a-second pass are saved for that. Per-cell arrays are stored as runs, which took a fresh colony's save from 5.2 MB to
  0.1 MB and save/load from 4/8 ms to about 1 ms each.
- Pinned to Bevy 0.20 and bevy_egui 0.43.
- Old save games don't load since Phase 2 (the format changed).
- `SAVE_VERSION` 4. Sitting isn't animated yet (phase 10b).
- Saves: `SAVE_VERSION` 3.
- Phase 5 measurements (`cargo run --release --example profile`: 10 colonists, 240 work orders, two
  in-game days on an M4 Max): about 0.05 ms per simulation tick on average (under 0.5% of a 60 fps frame
  at 3x speed), 6 ticks over 2 ms in two days. The front end's own systems take about 1.3 ms per frame
  (headless, without GPU work). A* or cached regions weren't needed. Changes made: job search checks each
  work type once instead of once per priority level, and pathfinding searches use zero-initialised
  32-bit arrays.
- Phase 6 performance: unchanged (0.051 ms per tick in `--example profile`, headless frame 1.4 ms).
- Phase 7 performance (`--example profile`, now with three busy workbenches): 0.067 ms per tick on
  average (0.6% of a 60 fps frame at 3x speed); two days made 15 meals, ~100 planks and 31 clubs.
- Phase 8 performance: 0.074 ms per tick in `--example profile` (0.7% of a frame at 3x speed).
