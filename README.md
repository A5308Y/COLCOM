# My World

A small RimWorld-style colony sim in Rust, shown in 3D with [Bevy](https://bevy.org).

## Run

```sh
cargo run --release            # first build takes several minutes (Bevy)
cargo run --features dev       # faster rebuilds while developing (dynamic linking)
cargo run --release -- --scenario battle   # start in a prepared battle (Shift+N restarts it)
cargo test                     # simulation tests + a headless run of the whole app
cargo run --release --example profile   # time the simulation and saving on a busy colony
cargo run --release --example look      # the downloaded models in the game's lighting (art direction test)
```

Scenarios are fixed worlds for trying things out (`src/scenarios.rs`). `--scenario <name>` or
`MY_WORLD_SCENARIO=<name>` picks one; an unknown name lists the ones there are. `battle` starts in
round 1 of a battle on a 48×32 yard: Ada's squad (one member arrives two rounds late), two lone
fighters (one gears up from the stockpile), a civilian in the house, and six raiders from the east.

(No Rust yet? `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

## How to play

You don't control colonists directly. You give orders, and they pick up the work
according to their **work priorities** (press `P`).

| Key | Tool / action |
|---|---|
| `1` / `2` / `3` | Designate trees to chop / rock or structures to mine / berry bushes to forage |
| `4` / `5` | Stockpile zone (loose items get hauled there) / grow zone (crops get planted) |
| `V` `J` `T` `M` `U` | Workbenches: stove, sawing bench, stonecutter, smelter, crafting bench (the smithy is in the build menu) |
| `O` `G` / `Y` `R` `X` / `Z` | Window, fence / table, chair, shelf / torch (all buildables are also in the toolbar's category menus) |
| `6` / `7` / `8` | Blueprint a wall (4 stone) / door (4 wood) / bed (8 wood) |
| `-` / `=` | Blueprint a floor (2 wood) / stairs (6 wood) |
| `B` | Blueprint a lamp (3 wood) |

Everything is also in the **build menu** docked on the left: grouped (orders, zones, structure,
furniture, light, production), searchable, with costs (red if the colony is short) and details on hover.
`` ` `` (the key left of `1`) hides and shows it. Build shortcuts and costs come from `assets/defs.ron`.
| `PageDown` / `PageUp` (or `[` / `]`) | Layer view: hide everything above a level, and build on that level |
| `9` / `0` | Clear zones / cancel designations and blueprints (refunds materials) |
| `Esc` | Select tool: click colonists or tiles to inspect them |
| Left drag | Paint the current tool over a rectangle |
| `F` | Draft the selected colonist (or everyone if nothing is selected) |
| Right click | With a drafted colonist: move there, or attack the clicked raider |
| `WASD` / arrows, middle-drag | Pan the camera |
| `Q` / `E`, right-drag | Rotate and tilt the camera |
| Mouse wheel | Zoom |
| `Space` / `Tab` | Pause / cycle speed 1x-2x-3x |
| `P` / `I` / `H` / `C` | Work priorities / resources (what the colony has) / help / center camera |
| `K` / `L` | Save / load (`savegame.json`; saves from other versions are refused with a message) |
| `Shift+N` | New colony |

### Systems

- **Production:** click a workbench to give it bills ("make 5", "make until you have 20", "forever").
  Haulers bring the ingredients, colonists with Cook or Craft work make it, and products get hauled to
  stockpiles. Chains: crops → meals (stove); wood → planks; stone → blocks; healroot → herbal medicine;
  cotton → cloth → clothes; iron ore (veins in the rock, more of it deeper down) → steel (smelter) →
  knives, pistols, rifles and steel armour (smithy). Colonists pick up better weapons and apparel by
  themselves; armour stops part of every blow, and clothes cheer colonists up.
- **Stockpiles:** each has a priority (low to critical) and a list of item kinds it takes; things move
  to better stockpiles. Grow zones plant the crop chosen for them (corn, healroot, cotton).
- **Sound:** footsteps that depend on the ground, chopping, mining, hammering, sawing, cooking and
  smithing while colonists work, trees crashing down, rock breaking, doors, gunshots, hits and armour
  glancing off, falls, alerts for raids and newcomers, interface clicks, and ambience that fades between
  day, night and caves. Sounds are placed in the world: quieter further from the camera's focus and when
  zoomed out, panned left and right. The **Sound** button opens volume sliders.
- **Furniture and rooms:** tables and chairs (colonists eat sitting at a table, and dislike eating
  without one), shelves (three stacks per cell), windows, fences that join up, flickering torches, stone
  doors. Each room has a quality from its size, the beauty of its furniture, doors and windows, its
  cleanliness (blood, clutter) and light; sleeping and eating in better rooms lifts mood more. Click a
  cell in a room to see the breakdown.
- **Food:** raw food can cause food poisoning and upsets colonists; cooked meals are better and fill
  more. Colonists pick meals first.

- **Items and hauling:** chopping, mining, foraging and harvesting drop items on the map. Haulers carry
  them to stockpiles, and builders carry materials to blueprints. Reservations stop two colonists from
  grabbing the same thing.
- **Work priorities and skills:** each colonist has 1–4 (or off) per work type. Skills make work faster
  and improve with practice.
- **Needs:** hunger (eat food from the ground or stockpiles) and rest (sleep in a bed, or on the ground if
  there isn't one). Going hungry leads to malnutrition.
- **Health (RimWorld-style):** the body is 29 parts (organs inside the torso, brain inside the skull, limbs
  down to hands and feet), each with its own hit points. Hits land on outer parts and can go deeper. Wounds
  (cuts, gunshots, bruises) bleed and hurt; destroyed parts are gone for good. Untended wounds don't heal and
  can get infected; healed ones sometimes scar. Flu outbreaks happen. Capacities like consciousness,
  moving, manipulation and sight follow from all this and affect work speed, walking and aim. Too much pain,
  blood loss or a crippled body knocks pawns down; losing a vital organ, bleeding out, an infection or the
  flu winning can kill.
- **Doctors and medicine:** colonists with the Doctor work type rescue downed colonists to a bed, tend
  wounds and diseases (quality depends on Medicine skill, medicine and bed rest), and feed bedridden
  patients. Wounded or sick colonists go to bed on their own. A lone doctor tends their own wounds. Click a
  colonist and open the **Health** tab for the full picture.
- **Terrain:** the world is a 3D grid of soil, rock and water, 16 levels deep, with rolling hills, lakes and
  rocky mountains. Pawns walk up and down one-level slopes of natural ground but can't climb cliffs,
  walls or floors. Mining digs tunnels into rock and shafts down through soil.
- **Building upwards:** floors make upper storeys and roofs; each needs a wall, stairs or ground beneath
  some cell within 3 steps. Stairs connect a cell to the one above; you enter them from their low end
  and can't walk across them. Use the layer view to place floors in mid-air (a faint grid shows the
  cells). Pawns and items with nothing underneath fall, and falls of two levels or more hurt.
- **Looks:** sun and moon with shadows, dark nights, lamps that light rooms and caves, see-through
  glossy water, distance haze, ambient occlusion and bloom. Pawns swing their arms and legs as they walk,
  hop when they step up or down a level, and drop when they fall. Gunfire flashes and kicks up dust.
- **Rooms:** areas enclosed by walls, doors or rock *and* covered overhead (by floors or rock) are rooms
  (shown with a wooden floor). Sleeping in a bed in a room gives a mood bonus.
- **Mood:** built from needs and memories (see the inspector). Low mood can cause mental breaks.
- **Storyteller:** raids that grow with colony wealth and age, wanderers who join, cargo pods (sometimes
  medicine), flu outbreaks and crop blight.
- **Combat:** drafted colonists shoot raiders in range with line of sight. Wounds land on body parts (see
  Health). Hurt raiders flee, and downed ones die if left lying.

## Code layout

The simulation is a library with no rendering code, so it can be tested headlessly:

- `assets/defs.ron`: items, buildables and plants (costs, work, behaviour flags, looks)
- `src/lib.rs`: module list and constants (day length, tick length)
- `src/defs.rs`: loads and checks the definitions; `ItemId`, `BuildId`, `PlantId`, `RecipeId`
- `src/production.rs`: workbenches and bills, stockpile settings, picking up better gear
- `src/rooms.rs`: room quality (space, beauty, cleanliness, light)
- `assets/sounds.ron`: sound names → files, volume, pitch variation, range; `assets/sounds/` holds
  generated placeholders (`python3 tools/make_sounds.py` remakes them)
- `src/map.rs`: 3D cells (blocks, features, items, zones), walking rules, rooms, terrain generation
- `src/pathing.rs`: BFS distance fields over the walking graph, 3D line of sight
- `src/pawn.rs`: pawns, skills, mood, jobs
- `src/health.rs`: body parts, injuries, diseases, capacities, bleeding, healing
- `src/medical.rs`: doctors (rescue, tend, feed) and patients
- `src/world.rs`: world state, fixed-step tick, needs, movement, orders, save/load
- `src/jobs.rs`: finding work by priority and carrying out jobs
- `src/combat.rs`: shooting, injuries, raider AI
- `src/story.rs`: storyteller events
- `src/save.rs`: versioned save format, run-length encoding of per-cell arrays
- `src/scenarios.rs`: prepared worlds to start in (`--scenario battle`)

The binary (`src/main.rs` and `src/front/`) is the Bevy app: `terrain.rs` (chunked ground, rock and wall
meshes), `props.rs` (trees, plants, furniture, doors, items), `pawns.rs` (3D figures), `scene.rs`
(sun, moon, sky), `camera.rs`, `input.rs` (picking, tools, gizmo markers), `audio.rs` (sound effects,
footsteps, ambience), `build_menu.rs`, `ui.rs` and `overlay.rs`
(egui interface and labels over the 3D view).

### Adding content

Items, buildables, plants and recipes are described in `assets/defs.ron` (built into the game, so rebuild after
editing). A new definition there gets a toolbar button, a shortcut (if it has a `key`), a look made of
simple shapes, and works in hauling, building, mining, harvesting and saves without code changes.
Behaviour the code has to know about is a field: `nutrition` (food), `medicine`, `special` (`Floor`,
`Stairs`, `Bed`, `Door`), `blocks_room`, `holds_floors`, `light`, `sow`, `wild`, `harvest`, `raw`,
`poison`, `weapon`, `apparel`, `beauty`, `daylight`, `storage`, `along_walls`, and `link` on parts. A buildable that a recipe names becomes a workbench.
`cargo test` checks that the file loads and that every buildable can be built and removed, every plant
harvested and every recipe made.

### Assets

- `assets/defs.ron`, `assets/sounds.ron`: definitions and the sound table (built into the game)
- `assets/sounds/`: placeholder sounds
- `assets/models/characters/`: Quaternius characters, hair, outfits and animation library (glTF, CC0)
- `assets/models/nature/`, `trees/`, `crops/`, `village/`: Quaternius packs converted from OBJ by
  `python3 tools/obj_to_gltf.py` (from `asset-sources/`)
- `asset-sources/`: the original downloads (FBX, OBJ, Blender files). Not used by the game and not in
  git (see `.gitignore`); `assets/CREDITS.md` says where everything comes from.

`cargo test --release -- --ignored` loads the character models and animations with the GPU (it can't run
in a sandbox without one).

### Replacing the sounds

The sounds in `assets/sounds/` are simple synthesised placeholders. To use real ones (e.g. a CC0 pack),
put the files in `assets/` and point the entries in `assets/sounds.ron` at them (WAV or OGG). Several
files per sound are picked at random. Definitions name the sounds they make (`sound` on harvests,
buildables, recipes and weapons; `done_sound` on harvests), and `cargo test` checks that every name used
exists and every file is there.

See `PLAN.md` for the roadmap.
