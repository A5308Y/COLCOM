# My World

A small RimWorld-style colony sim in Rust, shown in 3D with [Bevy](https://bevy.org).

## Run

```sh
cargo run --release            # first build takes several minutes (Bevy)
cargo run --features dev       # faster rebuilds while developing (dynamic linking)
cargo test                     # simulation tests + a headless run of the whole app
```

(No Rust yet? `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

## How to play

You don't control colonists directly. You give orders, and they pick up the work
according to their **work priorities** (press `P`).

| Key | Tool / action |
|---|---|
| `1` / `2` / `3` | Designate trees to chop / rock or structures to mine / berry bushes to forage |
| `4` / `5` | Stockpile zone (loose items get hauled there) / grow zone (crops get planted) |
| `6` / `7` / `8` | Blueprint a wall (4 stone) / door (4 wood) / bed (8 wood) |
| `9` / `0` | Clear zones / cancel designations and blueprints (refunds materials) |
| `Esc` | Select tool: click colonists or tiles to inspect them |
| Left drag | Paint the current tool over a rectangle |
| `F` | Draft the selected colonist (or everyone if nothing is selected) |
| Right click | With a drafted colonist: move there, or attack the clicked raider |
| `WASD` / arrows, middle-drag | Pan the camera |
| `Q` / `E`, right-drag | Rotate and tilt the camera |
| Mouse wheel | Zoom |
| `Space` / `Tab` | Pause / cycle speed 1x-2x-3x |
| `P` / `H` / `C` | Work priorities / help / center camera |
| `K` / `L` | Save / load (`savegame.json`) |
| `Shift+N` | New colony |

### Systems

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
  rocky mountains. Pawns walk up and down one-level slopes but can't climb cliffs or walls. Mining rock
  digs tunnels into mountains.
- **Rooms:** areas fully enclosed by walls, doors or rock get a roof (shown as a wooden floor). Sleeping
  in a bed indoors gives a mood bonus.
- **Mood:** built from needs and memories (see the inspector). Low mood can cause mental breaks.
- **Storyteller:** raids that grow with colony wealth and age, wanderers who join, cargo pods (sometimes
  medicine), flu outbreaks and crop blight.
- **Combat:** drafted colonists shoot raiders in range with line of sight. Wounds land on body parts (see
  Health). Hurt raiders flee, and downed ones die if left lying.

## Code layout

The simulation is a library with no rendering code, so it can be tested headlessly:

- `src/lib.rs`: module list and constants (day length, tick length)
- `src/map.rs`: 3D cells (blocks, features, items, zones), walking rules, rooms, terrain generation
- `src/pathing.rs`: BFS distance fields over the walking graph, 3D line of sight
- `src/pawn.rs`: pawns, skills, mood, jobs
- `src/health.rs`: body parts, injuries, diseases, capacities, bleeding, healing
- `src/medical.rs`: doctors (rescue, tend, feed) and patients
- `src/world.rs`: world state, fixed-step tick, needs, movement, orders, save/load
- `src/jobs.rs`: finding work by priority and carrying out jobs
- `src/combat.rs`: shooting, injuries, raider AI
- `src/story.rs`: storyteller events

The binary (`src/main.rs` and `src/front/`) is the Bevy app: `terrain.rs` (chunked ground, rock and wall
meshes), `props.rs` (trees, plants, furniture, doors, items), `pawns.rs` (3D figures), `scene.rs`
(sun, moon, sky), `camera.rs`, `input.rs` (picking, tools, gizmo markers), `ui.rs` and `overlay.rs`
(egui interface and labels over the 3D view).

See `PLAN.md` for the roadmap to a multi-level 3D world.
