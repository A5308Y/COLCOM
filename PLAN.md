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
| 6 | Data-driven definitions: items, buildables, plants and recipes described in a data file, not code | done (recipes come with phase 7) |
| 7 | Production chains: workbenches, job queues, Cooking/Crafting, new resources, stockpile filters | done (needs a visual check) |
| 8 | More buildables: furniture, shelves, windows, torches, fences, room quality | done (needs a visual check) |
| 9 | Sound: actions, ambience, interface | done with placeholder sounds (needs a listen) |
| 10 | Graphics, Valheim-like (low poly, realistic movement): 10a art direction, 10b animated characters, 10c smoothed natural terrain, 10d glTF models, 10e atmosphere | 10a done; 10b done (needs a visual check) |
| 11 | Follow camera: optional close third-person view of one colonist | later |
| – | Keyboard shortcuts: a thought-out scheme (all letters are taken; categories, rebinding?) | later |

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

### 10. Graphics (style agreed 2026-10-12)
Target look: like Valheim. Low-poly models with low-resolution textures, realistic movement, and most
of the mood from light and atmosphere. The simulation stays on its grid; all of this is drawing.

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
- **10b. Animated characters.** Rigged glTF pawns with an animation library: walk, idle, chop, mine,
  hammer, carry, eat, lie down, downed, aim and shoot, melee, fall. Pick the animation from what the
  pawn is doing (`Job` / `Task`) and blend between them. Smooth movement for display: curved paths,
  easing into and out of walking, turning on the spot, walk animation speed matched to the simulation's
  speed so feet don't slide. Foot placement on slopes (IK) is optional and later: Bevy has no built-in
  IK.
- **10c. Smoothed natural terrain.** Naturally generated ground (soil, rock, ore) is drawn as a smooth,
  rolling surface over the grid instead of cubes. Cells that have been mined, and anything built, stay
  blocky, so dug tunnels and shafts keep crisp edges. Needs: a flag per cell for "dug" (or natural vs.
  changed), a new mesher (e.g. smoothed heights or surface nets over natural cells only, blending into
  the blocky ones), and picking that still lands on exact cells. Replaces the old "ramps on slopes" idea;
  pawns stepping a level follow the smooth surface instead of hopping.
- **10d. Models.** Replace code-built shapes with glTF models category by category, through a `model`
  field in the definitions (shapes stay as a fallback).
- **10e. Atmosphere.** Wind on grass and trees (custom shader), a water shader with gentle waves,
  volumetric fog and light shafts, weather (rain, snow, fog) and seasons (colours, crop growth), plus the
  phase 4 tuning pass from real play: lamp brightness, bloom/ambient occlusion strength, water opacity,
  fog distance, hop height.

### 11. Follow camera (later, optional)
- A close third-person camera that follows one colonist, to show off the animations; toggled from the
  normal top-down camera.

- **10b done (2026-10-12, needs a visual check):** see the notes below.

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

- Multi-selection: drag a box with the Select tool (colonists inside it; Shift adds) or Shift+click on
  the map or in the colonist bar. `UiState::group` holds them (the first is `selected`, shown in the
  inspector); use `selected_pawns` / `select_pawns` / `toggle_pawn` / `select`. Right-click sends every
  selected drafted colonist: all attack an enemy, or spread over the walkable cells nearest the click
  (`spread_targets`, each taking the closest). F drafts or undrafts the selection; with nothing
  selected it drafts everyone and selects them. Equipping still goes to the first selected.

- Hide roofs (Shift+R or the HUD button; like Going Medieval's roof toggle): every column over a roofed
  room whose ceiling is a built floor is cut off just above the topmost such room
  (`UiState::roof_cut`, recomputed each frame in `update_roof_cut`), so the top room of every building
  is visible at once. Rock over caves and tunnels stays. Terrain (`terrain::Cut`), props, items,
  pawns, labels, markers and picking all use `UiState::top_at` / `shows_at`; combined with the layer
  view it takes the lower of the two. Tool shortcuts ignore keys pressed with Shift.

- Buildables must fit their cell (1.0 high): parts on a built floor are lifted by the slab (0.08), so
  furniture stays below 0.92; doors and windows aren't lifted (they fill the cell like walls) and may
  reach 1.0; stairs reach the level above. `defs::tests::buildables_fit_under_the_ceiling` checks it.
  (The door leaf used to be 1.1 tall and poked through the lintel and the floor above.)

- Equip order: select a colonist (drafted or not) and right-click a weapon or piece of apparel lying on
  the map (`World::order_equip`, `Job::Equip { chosen: true }`). What they're told to use goes into
  `Pawn::chosen` and isn't swapped for better gear automatically; switching or dying clears it. Both new
  fields default when missing, so older saves still load. Not done yet: dropping or unequipping gear from
  the inspector.

- Deconstruct (Orders menu, Backspace): marks structures and floors to be taken down by builders
  (Construction work and skill) in half their work time (at least 1 s), dropping `drops` (half the cost
  by default). Mine is for rock, ore and soil only now; old saves' Mine marks on structures and floors
  load as Deconstruct. Marked cells are outlined in red.

- Walls, doors and windows (`blocks_room`) can be placed straight on top of a wall or window (anything
  `holds_floors`) without a floor first: click the wall's top, or the cell above it in the layer view.
  Builders work up to one level above where they stand, so the second level is built from the ground;
  higher ones need a floor or stairs beside them to stand on.

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
- Eating: `Job::Eat` has an optional seat. Colonists take their food to the nearest free chair beside a
  table within 40 steps of the food, else eat on the spot and get "Ate without a table" (-3).
- `SAVE_VERSION` 4. Sitting isn't animated yet (phase 10b).
- Build menu (after phase 8): a full-height dock on the left (`src/front/build_menu.rs`) replaces the
  toolbar: groups (Orders, Zones, then buildable categories), search, cost per entry (red when short),
  shortcut keys, details on hover, a hint for the current tool at the bottom; `` ` `` toggles it.
  Tools are `input::ToolEntry` (name, key, group, hint). egui 0.36 panels are shown inside a root `Ui`;
  they don't count as "pointer over egui", so `UiState::dock_width` / `over_dock` keep clicks and
  scrolling on the dock out of the world. Windows and labels keep to the right of it.
- Windows are a `BuildLook::Opening` (wall block with a hole) meshed with the walls by the terrain
  mesher, so they join walls without seams; only the frame and glass are props.
- Phase 8 performance: 0.074 ms per tick in `--example profile` (0.7% of a frame at 3x speed).
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
- Food: `raw` food gives "Ate raw food" (-5) and a `poison` chance of `HediffKind::FoodPoisoning`
  (pain, slower, gone in under a day, never dangerous); other food gives "Ate a cooked meal" (+2).
  Eating and doctors' feeding prefer cooked food.
- Stockpiles: `Zone::Stockpile(id)` with `World::stockpiles[id]` (name, priority 1-4, forbidden items).
  Painting next to a stockpile extends it, elsewhere starts a new one. Items are hauled to the
  highest-priority stockpile that takes them, and out of a stockpile only into a higher-priority one.
  `Zone::Grow(crop)`; the inspector changes a whole connected grow zone's crop.
- Saves: `SAVE_VERSION` 3.
- Left for later: per-cook meal quality, fuel, smithing/tailoring as separate skills, apparel visuals on
  the pawn figures, stockpile renaming, bill ingredient filters, ingredients from a radius only.
- Phase 7 performance (`--example profile`, now with three busy workbenches): 0.067 ms per tick on
  average (0.6% of a 60 fps frame at 3x speed); two days made 15 meals, ~100 planks and 31 clubs.
- Phase 6 performance: unchanged (0.051 ms per tick in `--example profile`, headless frame 1.4 ms).

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
