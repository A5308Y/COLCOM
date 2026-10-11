# My World

A small RimWorld-style colony sim in Rust ([macroquad](https://macroquad.rs)).

## Run

```sh
cargo run --release
cargo test            # headless simulation tests
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
| `WASD` / arrows / middle or right drag / wheel | Pan and zoom |
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
- **Needs:** hunger (eat food from the ground or stockpiles), rest (sleep in a bed, or on the ground if
  there isn't one) and health.
- **Rooms:** areas fully enclosed by walls, doors or rock get a roof (shown as a wooden floor). Sleeping
  in a bed indoors gives a mood bonus.
- **Mood:** built from needs and memories (see the inspector). Low mood can cause mental breaks.
- **Storyteller:** raids that grow with colony wealth and age, wanderers who join, cargo pods and crop
  blight.
- **Combat:** drafted colonists shoot raiders in range with line of sight. Badly hurt colonists go down
  and recover over time. Badly hurt raiders flee.

## Code layout

The simulation is a library with no rendering code, so it can be tested headlessly:

- `src/lib.rs`: module list and constants (day length, tick length)
- `src/map.rs`: tiles, items, zones, room detection, map generation
- `src/pathing.rs`: BFS distance fields, line of sight
- `src/pawn.rs`: pawns, skills, mood, jobs
- `src/world.rs`: world state, fixed-step tick, needs, movement, orders, save/load
- `src/jobs.rs`: finding work by priority and carrying out jobs
- `src/combat.rs`: shooting, injuries, raider AI
- `src/story.rs`: storyteller events

The binary (`main.rs`, `camera.rs`, `render.rs`, `ui.rs`) handles the window, drawing and input.
