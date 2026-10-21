# My World

A small RimWorld-style colony sim in Rust, shown in 3D with [Bevy](https://bevy.org).

## Run

```sh
cargo run --release            # first build takes several minutes (Bevy); continues the last game
cargo run --release -- --new   # a new colony instead of continuing
cargo run --features dev       # faster rebuilds while developing (dynamic linking)
cargo run --release -- --scenario battle   # start in a prepared battle (Shift+N restarts it)
cargo test                     # simulation tests + a headless run of the whole app
cargo run --release --example profile   # time the simulation and saving on a busy colony
cargo run --release --example look      # downloaded source packs in the game's lighting
cargo run --release --example look -- --world-props  # selected plants, machinery and state scenes at gameplay scale
cargo run --release -- --new --screenshot           # save every interface screen to target/screenshots/ and quit
cargo run --release -- --screenshot help,menu       # only these shots (an unknown name lists them all)
MY_WORLD_NO_GPU=1 cargo run --release -- --new --screenshot   # no GPU: the interface alone, drawn on the CPU (*-cpu.png)
```

Screenshot mode pauses the game, opens one screen after another (HUD, handbook pages, menu, work,
resources, info, rooms, squads, expeditions, inspectors, build search), saves each as a PNG and
quits without autosaving. With `--scenario <name>` the files are prefixed with the scenario's name.
The CPU shots have no 3D scene behind the panels (a plain field-green backdrop); they're for
reviewing layout and colours, not lighting.

The interface follows a "frontier field manual" look (`src/front/theme.rs`): warm charcoal panels in
olive frames with chamfered corners, cream text, amber/teal/red/green accents, and condensed uppercase
headings in Barlow (SIL OFL, `assets/fonts/`). The handbook (`H`) is split into topics with a search.
Notifications occupy a framed panel at the top right: urgent colony alerts come first in red,
followed by amber warnings and the three most recent events. The latest event has a larger amber
card, and messages stay visible until newer events replace them. Long lists can be scrolled.
Idle warnings appear after three real seconds of sustained idleness and clear after two seconds
of sustained work, so brief gaps between jobs don't flash notices. The alert section reserves its
space to keep recent events stationary as warnings appear and clear; urgent alerts appear immediately.

Scenarios are fixed worlds for trying things out (`src/scenarios.rs`). `--scenario <name>` or
`MY_WORLD_SCENARIO=<name>` picks one; an unknown name lists the ones there are. `battle` starts in
round 1 of a battle on a 48×32 yard: Ada's squad (one member arrives two rounds late; Ada carries the field
radio), two lone fighters (one gears up from the stockpile), a civilian in the house, and six raiders from
the east in three pairs: mercenaries (assault rifles, flak armour, grenades, a medic), elite raiders with
laser rifles, powered armour and cyberware, and a melee pair (knife, club) behind personal shields.
Every weapon, apparel and belt item appears on a fighter (the assault rifleman carries a pistol sidearm
and frag grenades; the geared fighter cargo trousers, tactical goggles, a field medkit and a club sidearm).
A ranged squad's sidearms are pistols; only melee squads and lone fighters (Eve's knife) carry other kinds. The stockpile
also holds a spare of every weapon, apparel and belt item, medicine and cyberware for inspecting models
and trying loadouts. Eve, the lone fighter north of the house at (14, 9), starts wearing powered armor
(with bionic eyes and subdermal plating under it) and holding the laser rifle, ready to inspect from round 1. Restart the scenario with Shift+N to refresh an existing battle.

(No Rust yet? `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

## How to play

You don't control colonists directly. You give orders, and they pick up the work
according to their **work priorities** (press `P`).

| Key | Tool / action |
|---|---|
| `1` / `2` / `3` / `4` | Game speed 1x / 2x / 5x / 10x (also carries on if paused) |
| `Space` | Pause |
| Build menu | Orders (chop, mine, forage, cancel), zones (stockpile, grow, safe, clear), and every buildable |
| `Backspace` | Deconstruct |
| `V` `J` `T` `M` `U` | Workbenches: stove, sawing bench, stonecutter, smelter, crafting bench (the smithy is in the build menu) |
| `O` `G` / `Y` `R` `X` / `Z` | Window, fence / table, chair, shelf / torch (all buildables are also in the toolbar's category menus) |
| `-` / `=` | Blueprint a floor (2 wood) / stairs (6 wood) |
| `B` | Blueprint a lamp (3 wood) |

Everything is also in the **build menu** docked on the left: grouped (orders, zones, structure,
furniture, light, production; the groups start closed), searchable, with costs (red if the colony is
short) and details on hover; right-click an entry for its full info. `` ` `` (the key left of `1`) hides
and shows it. Build shortcuts and costs come from `assets/defs.ron`.

The **Info** window (top bar) shows everything about an item, building or plant: a weapon's damage,
range and rate of fire; armour; food and medicine; what a workbench makes; what a building costs, gives
back, and does (cover, light, storage, rooms it counts for); how long a crop takes. Search it for
anything, or open it with the Info buttons in the cell inspector, from a colonist's gear, or by clicking
an item in the Resources window. "See also" links lead on (to the bench that makes it, what it's used
for), and `<` goes back.
| `PageDown` / `PageUp` (or `[` / `]`) | Layer view: hide everything above a level, and build on that level |
| `Esc` | Select tool: click colonists or tiles to inspect them |
| Left drag | Paint the current tool over a rectangle (with something to build picked, see-through ghosts show where it'll go, before and while dragging) |
| Left drag with walls | Wall in a rectangle: only its outline (`Shift` fills it) |
| `,` / `.` while placing or moving | Turn furniture or stairs a quarter turn. The arrow points to the front, where it's used from: crafters stand there at workbenches (beside it if that's blocked). For stairs it shows the way up, orange if they couldn't be climbed that way (yet); they're built exactly as placed. Chairs next to a table face it by themselves; things that look the same from every side (tables, lamps) don't turn |
| `F` | Draft the selected colonist (or everyone if nothing is selected) |
| Right click | With a drafted colonist: move there, or attack the clicked raider |
| Right click with a colonist selected | Menu of what they could do there: prioritize building this grave, tending this colonist, hauling this stack, equipping this rifle... They do it now (even work they're not assigned to), then finish everything there |
| Left click while a unit walks (battle) | Hurry the walk along: 5x until it arrives |
| `WASD` / arrows, middle-drag | Pan the camera |
| `Q` / `E`, right-drag | Rotate and tilt the camera |
| Mouse wheel | Zoom |
| `P` / `I` / `H` / `C` | Work priorities / resources (what the colony has) / handbook (help by topic, searchable) / center camera |
| Double click a colonist card | Select that colonist and center the camera on them |
| `K` / `L` | Save / load (`savegame.json`; saves from other versions are refused with a message) |
| Menu (`Esc`) → Save as | Save under a name (in `saves/`); the list below loads any of them |
| Menu → Spawn items (testing) | Pick an item and an amount, then click on the map to drop it there |
| `Shift+N` | New colony |

### Systems

- **Production:** click a workbench to give it bills ("make 5", "make until you have 20", "forever"; Shift+click on + or - changes the number by 5). The
  colonist doing a bill fetches what it lacks (haulers help too); right-click a bench with a colonist
  selected to have them do its bill now. Colonists with Cook or Craft work make it, and products get hauled to
  stockpiles. Chains: crops → meals (stove); wood → planks; stone → blocks; healroot → herbal medicine;
  cotton → cloth → clothes; iron ore (veins in the rock, more of it deeper down) → steel (smelter); copper
  ore (green veins) → copper (smelter); stone → silicon and rubber-plant latex → rubber (refinery).
  Each colonist has a loadout (Combat tab; right-clicking a weapon or clothes with them selected sets it
  too) with a slot for each piece of gear: **Weapon**, **Sidearm** (a light weapon: pistol, knife, club),
  **Utility** (belt items: frag grenades, field medkits), and apparel for **Head**, **Face** (goggles),
  **Shirt** (inner torso), **Jacket / vest** (outer torso: Flak Jacket, parka), **Legs** (cargo
  trousers, flak leg guards) and **Back** (a field pack to carry more on expeditions, a medic pack for
  better tending in and out of battle, a field radio that gives its unit +10 action points a round).
  One item can take several slots: powered armour also covers legs, head and face. They fetch it from
  storage and keep it on, in and out of battle; for apparel slots left empty they pick up the best they
  find (sidearms, belt items and backpacks only when told to: loadout, or right-click). Leg, face and
  back gear has no worn model yet (the trousers under it show).
  Armour protects where it's worn: a blow lands on a body part, and only what covers that part stops
  it (helmets the head and ears, goggles the eyes, nose and jaw, shirts and jackets the torso, neck,
  shoulders and arms, legwear the legs and feet; hands are bare). Sealed powered armour and subdermal
  plating cover everything. The Info window says what each piece protects. Armour stops part of every
  blow (and shows: a fitted Flak Jacket, a steel helmet, powered armour's articulated suit), and
  clothes cheer them up. Powered armour's motors make its wearer a quarter faster, on foot and in
  battle; a limp (low Moving) slows them in both (a squad moves at its slowest member's pace). Pinned down in battle, a unit has 60% of its
  action points and each step costs twice as much. Ranged squads can deploy: down on one knee, aiming
  (better aim, harder to hit, can't move until they stand up). Melee squads and lone fighters can't.
- **Components:** six kinds (mechanical, electrical, optical, structural, chemical, textile; their
  descriptions say what they stand for: gears, wiring, lenses ...) made at fitting benches: structural at
  the smithy, textile at the crafting bench, mechanical at the machinist's bench, chemical at the chemistry
  lab, electrical and optical at the electronics bench. Gear needs them: knives, pistols, rifles, assault
  rifles, Flak Jackets, leg guards and steel helmets (smithy), prosthetics, bionic limbs, spines, plating
  and grenades (machinist), goggles, bionic eyes and organs, neural coprocessors (electronics bench), field
  medkits (chemistry lab), and at the **fabricator** powered
  armour and the laser rifle (its burns don't bleed but hurt more). The benches themselves cost components
  and climb a ladder (machinist → refinery → chemistry lab → electronics bench → fabricator), each built
  from what the ones before make. Buildings can cost several materials, brought in order. The **salvage
  bench** takes component-made gear apart for half its components, and scrap metal (raiders drop it) and
  e-waste (cargo pods) for materials and components. A **PV system** (open sky only) and a **home battery**
  can be built from components; the Resources window sums their power, which nothing uses yet.
- **Stockpiles:** each has a priority (low to critical) and a list of item kinds it takes; things move
  to better stockpiles. Grow zones plant the crop chosen for them (corn, healroot, cotton).
- **Recreation and schedules:** colonists need fun (Overview tab), which drops while they're awake:
  bored (-6) when low, very bored (-12) when nearly empty, entertained (+3) when high. They have fun at
  a chess table, a bookshelf or a dartboard (wall decor), or strolling outdoors by day; a Recreation
  room (two of those) makes fun come 40% faster. Each colonist's day is 24 hours painted in the Work
  window's Schedule tab: Sleep (to bed even a little tired), Fun (fun until full, no work), Work (no
  stopping for fun) or Anything (work, and fun when bored); by default sleep 22-6, fun 18-22. Hunger,
  exhaustion and health come first. In the Priorities tab, left click raises a priority, right click lowers it.
- **Expeditions** (Expeditions button): about once a day a mission is offered by, or about, one of three
  neighbours: Riverford Traders, the Iron Pact (settlers) and the Ash Raiders (bandits). The ten
  missions are trade caravan, envoy, bandit camp, salvaging ruins, rescue, hunting, prospecting, escort
  duty, peace talks and a strange signal. Plan one: pick who goes (whole squads with one click, or one by one from a table of weapon,
  combat skills, armour, movement and health; hover a name for all skills) and pack goods from the colony (to
  sell, give or pay). The squad leaves the map for real game days. When something comes up, such as a
  toll on the road, an ambush or the mission itself, the journey waits for your choice (an alert and
  the button show it). Choices give or gain goods, change goodwill, cost days, risk injuries, bring
  back a recruit, buy a truce from the raiders, or lead to a fight. A fight is a turn-based battle on a
  small map of its own; afterwards the squad carries on with its wounds and whatever the enemy dropped.
  At a market you barter what you carry for their stock at their prices (better with more goodwill).
  The raiders' goodwill spaces out raids, and allies (goodwill 50+) send gifts now and then.
  For testing, **Offer now** (below the offers) brings any mission, or a chosen one, straight away, and
  **Skip travel** (under way) ends the current leg of a journey.
- **Seasons:** a year has four seasons of 5 days, starting in spring (the top bar shows the season and
  its day). Crops grow in spring and summer, slower in autumn and not at all in winter, and winter's
  first frost kills the ones not yet ripe (an alert warns the day before); wild plants don't grow back
  in winter. Grass turns golden in autumn and snowy in winter. So: sow in time, and stock up for winter.
- **Temperature:** outdoors it follows the year (spring starts mild, warmest around the start of
  summer, coldest around the start of winter, below freezing at night then) and the day (top bar).
  Rooms keep warmth: they follow the outdoors slowly (bigger rooms slower, doors faster) and are warmed
  by what burns in them: torches, stoves, smelters, smithies and the fireplace (8 stone blocks, warms a
  small room by about 16°C). Colonists feel where they are: below 10°C cold, below 0°C freezing and
  hypothermia builds up (they slow down, collapse at 70%, die at 100%; it wears off when warm), above
  30°C hot. Clothes keep them warm in the cold (not in summer heat): simple clothes +4°C, a parka
  +14 (12 cloth, worn instead of armour), a wool hat +4 (3 cloth, instead of a helmet), powered armour
  +12. Colonists without those slots in their loadout put on a parka and hat when it gets cold and
  go back to armour when it's warm. The cell inspector shows a room's temperature; the colonist
  inspector how warm they feel; the Health tab hypothermia; an alert says who's freezing. Freezing
  colonists (hypothermia from 10%) drop what they're doing and go to the nearest room warm enough to
  recover, stay until it's gone, then go back to work; with no warm room they carry on.
- **Power:** one grid for the whole colony (no cables). PV systems make up to 1 kW in daylight (about
  10.8 kWh a day), home batteries store 10 kWh. Electric heaters (+20°C) and coolers (-16°C) are
  thermostats for their room: a heater runs below 18°C, a cooler above 24°C, each using 800 W while it
  runs. Panels feed them first, batteries make up the rest and take what's spare; without enough, they
  stay off (an alert says so). A heater running all winter day and night needs about two panels and a
  battery. The Resources window shows the grid; a device's inspector whether it's running.
- **Quarries:** paint a Quarry zone over the ground (Zones) and miners dig it out level by level from
  the top, down to the depth set in its inspector (5 levels below the ground at its entrance to start,
  up to 12). The way down is a ladder in one edge column, on the side your colonists come from
  (marked while the tool is out or a quarry cell is selected): each layer starts with the shaft cell
  and its piece of ladder (3 wood), and the rest of the layer is only dug once that's in, so nobody
  gets shut in. Ladders (Structure) can be built anywhere: like stairs, but climbed straight up,
  leaning on the side the arrow points to. Everything in the zone is dug, ore included, strictly one
  layer at a time across the whole zone, starting with any ground higher than the entrance, so no
  part is ever dug deeper than the rest. Cells nobody can reach (the top of a pillar too tall to climb)
  are left standing and the column dug from below them. Columns with water or buildings are left alone,
  a rim stays beside water, and trees in the way are cut first. The inspector tells you when it waits for
  the ladder, and about cells nobody can reach. Clear zone takes columns out again. Anyone who still
  ends up in a pit with no way out climbs up its rock wall and over the rim.
- **Sound:** footsteps that depend on the ground (silent at 5x and 10x speed), chopping, mining, hammering, sawing, cooking and
  smithing while colonists work, trees crashing down, rock breaking, doors, gunshots, hits and armour
  glancing off, falls, alerts for raids and newcomers, interface clicks, and ambience that fades between
  day, night and caves. Sounds are placed in the world: quieter further from the camera's focus and when
  zoomed out, panned left and right. The **Sound** button opens volume sliders.
- **Furniture and rooms:** tables and chairs (colonists eat sitting at a table, and dislike eating
  without one; they also sit to play chess, to craft at a bench with a chair at its front, and now and
  then just to sit down for a bit; they sit down and get up properly before walking off), shelves (four different things per shelf, 50 of each, shown two on top and two on the middle board; nobody walks through them, they're stocked from beside; a colonist shut in by furniture gets an alert and climbs out over it), windows, fences that join up, flickering torches, stone
  doors. Each room has a quality from its size, the beauty of its furniture, doors and windows, its
  cleanliness (blood, clutter) and light; sleeping and eating in better rooms lifts mood more. Click a
  cell in a room to see the breakdown.
- **Moving furniture:** click a piece of furniture and press **Move** (or use *Move furniture* in the
  build menu's Orders), then click where it should go. A builder packs it up and sets it down there at no
  cost; a workbench keeps its bills and a bed its owner. Cancel (or the inspector) calls a move off.
- **Themed rooms:** a room with everything a theme needs becomes a hospital, surgery room, bedroom,
  barracks, kitchen, smithy or workshop: its floor changes, its walls get a wallpaper to match (tiles,
  stripes, dots, checks, bricks, planks) and its name shows over it. The **Rooms** button lists what each needs;
  clicking a cell in a room shows a checklist towards every theme (themes are defined in
  `assets/defs.ron`). Themes pay off: bedrooms and barracks rest better and cheer sleepers up; kitchens,
  smithies and workshops work faster at their benches, and may turn out a spare product or teach faster.
  Hospitals (beds, a medical cabinet, two windows, a lamp) heal patients faster and improve tending;
  surgery rooms (an operating table, a medical cabinet, a lamp) make operations safer.
- **Food:** raw food can cause food poisoning and upsets colonists; cooked meals are better and fill
  more. Colonists pick meals first.

- **Items and hauling:** chopping, mining, foraging and harvesting drop items on the map. Haulers carry
  them to stockpiles (picking up nearby piles of the same thing on the way, up to 50), and builders carry
  materials to blueprints (enough for the ones nearby too, dropping them off one after another). Things a
  stockpile or shelf no longer takes go to one that does, or out onto open ground nearby. Reservations
  stop two colonists from grabbing the same thing.
- **Work priorities and skills:** each colonist has 1–4 (or off) per work type. Skills make work faster
  and improve with practice.
- **Needs:** hunger (eat food from the ground or stockpiles) and rest (sleep in a bed, or on the ground if
  there isn't one). A **sleeping spot** (Furniture, free) is a place on the ground to sleep: better than
  nothing, worse than a bed, and rescuers bring the wounded there. Going hungry leads to malnutrition.
  A day lasts 8 minutes; a full stomach lasts a little over a day.
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
  colonist and open the **Health** tab for the full picture. Patients take hospital beds first; nobody
  claims a hospital bed or sleeps on an operating table.
- **Cyberware:** every colonist has cyberware slots: **left/right arm**, **left/right leg**, **eyes**,
  **spine**, **heart** and **lungs** (these replace the natural parts, lost or healthy) and the **brain** and
  **dermal** (skin) slots, which add to the body. Pick what goes in a slot in the colonist's **Health**
  tab (*Cancel* calls it off; *Take it out* removes arms, legs, eyes, brain and skin implants; organs and
  the spine can only be swapped). Once the item is made, an operating table is free and another colonist
  can doctor, the patient lies down and a doctor installs it; what it replaces is left at the table.
  Prosthetic limbs work 75% as well as flesh; bionic arms, legs, eyes, spine, heart and lungs better
  (sharper aim, quicker steps, more resilience), a neural coprocessor helps aim and subdermal plating
  is armour that's always on. It can go wrong (more likely with a poor doctor, less in a surgery room):
  the item is ruined and the patient wounded. Cyberware has no visible model on the character yet.
- **Terrain:** the world is a 3D grid of soil, rock and water, 16 levels deep, with rolling hills, lakes and
  rocky mountains. Pawns walk up and down one-level slopes of natural ground but can't climb cliffs,
  walls or floors. Mining digs tunnels into rock and shafts down through soil; **Fill** (Orders) fills
  holes and tunnels back in with soil, for free, bottom up (mark a shaft again to mark the level above),
  and on open ground raises it a level.
- **Building:** builders work from any cell around what they build, diagonals included (so corners
  behind furniture can be built), on its level or one up or down, but not from straight above or below;
  floors and roofs are put up from underneath. Materials are brought the same way.
- **Building upwards:** floors make upper storeys and roofs; each needs a wall, door, stairs or ground
  beneath some cell within 3 steps. Walls, doors and windows stack onto walls, doors and windows below,
  even ones still planned: plan a wall several storeys high and it goes up one level at a time
  (cancelling the bottom takes the rest along). Doors and windows can be built into a standing wall
  (and walls, doors and windows over each other): the old one stays until the new one is built, then
  gives back its materials; cancelling leaves it as it was. Stairs, even planned ones, and the cell in front of them get no roof. Stairs work like a slope: walk on at the low end and off
  the high end onto the level above, beside them (keep the space over them open for headroom); they
  can't be walked across. Ladders are climbed straight up into the cell above, and stack. Use the layer view to place floors in mid-air (a faint grid shows the
  cells). Pawns and items with nothing underneath fall, and falls of two levels or more hurt.
- **No roof areas:** in Zones, paint **No roof** on the room's floor to prevent automatic roofing
  immediately above it. Unfinished roofs are cancelled and delivered materials returned. Built roofs
  stay until you use **Deconstruct**. Use **Allow roofs** to restore automatic roofing. These areas
  coexist with grow, stockpile and safe zones, apply only to the painted level, and survive saving.
  Hide roofs with **Shift+R** to paint inside; clicking a visible roof also targets the room below.
  Existing saves retain automatic roofing until you paint an area. Floors remain buildable there.
- **Wall decor:** walls are drawn thin (about 35 cm) through the middle of their cell, so rooms show
  more floor. Furniture with its back to a wall (beds, workbenches, shelves) is drawn pushed back
  against it, and whoever sleeps or works there with it. Windows and doors are drawn as part of their wall: papered
  like it in themed rooms, with the room's floor running into the doorway. Each side of a wall has five places for a painting, poster, calendar (shows the date),
  wall lamp (lights the room), wall shelf (holds one stack; storage like a shelf) or hanging plant: pick
  one in the build menu's Wall decor, point at a wall (the place under the cursor is marked green, or red
  if it doesn't fit) and click. Builders bring the materials and put it up from in front. It counts
  for the room it faces (beauty, light). Click the wall to see what's on it, call plans off or have
  things taken down; anything on a wall comes down with the wall.
- **Looks:** sun and moon with shadows, dark nights, lamps that light rooms and caves, see-through
  glossy water, distance haze, ambient occlusion and bloom. Pawns swing their arms and legs as they walk,
  hop when they step up or down a level, and drop when they fall. Gunfire flashes and kicks up dust.
- **Rooms:** areas enclosed by walls, doors or rock *and* covered overhead (by floors or rock) are rooms
  (shown with a wooden floor). Sleeping in a bed in a room gives a mood bonus.
- **Mood:** built from needs and memories (see the inspector). Low mood can cause mental breaks.
- **Storyteller:** raids that grow with colony wealth and age, wanderers who join, cargo pods (sometimes
  medicine), flu outbreaks and crop blight. **Difficulty** (in the Menu, Esc): switch raids off, scale their
  size (×0.25 to ×3), or call one in now for testing; saved with the game.
- **Raiders' equipment** grows with the colony's threat (a point a day, one per 500 of wealth, scaled by
  the raid size setting), in tiers from `raider_tiers` in `assets/defs.ron`: *Scavengers* with pistols and
  clubs, *Gangers*, *Raiders* (rifles, some flak jackets and grenades), *Mercenaries* (assault rifles, flak
  armour, goggles, grenades and medkits, some cyberware) and, from threat 24, the *Elite*: laser rifles,
  powered armour, field radios and cyberware (neural coprocessors, plating, bionic eyes and limbs). The
  parley names the tier (better equipped raiders demand more), expedition fights face the same tier, and
  Menu → Difficulty shows the next raid's tier and can fix it (*Raider gear*, for testing). What raiders
  wear and carry drops when they fall.
- **Expeditions carry** 60 things a member (packed goods and what they gain or loot); backpacks carry more.
  Loot that doesn't fit is left behind (the most valuable is taken first), and the journal says so.
- **Combat:** drafted colonists shoot raiders in range with line of sight. Wounds land on body parts (see
  Health). Hurt raiders flee, and downed ones die if left lying. In battles nobody walks through an
  enemy: units go round them, or can't get past. Members with a sidearm draw it when it does more harm.
  Rifles, assault rifles and laser rifles hit only a fifth as often at someone right next to them, so
  riflemen draw their pistols in close quarters; an attack in which everyone uses their sidearm costs 25
  AP instead of 40. A squad's style comes from main weapons only: a ranged squad's sidearms must shoot
  (pistols; a melee sidearm stays holstered), while a melee squad may carry pistols to fire on the way in
  (it still can't deploy). Lone fighters carry anything. Melee units move 15% faster (fewer AP a
  step). A **personal shield** on the belt takes up to 40 damage from shots and grenade blasts (not
  blows), recharging 10 a round and full again after the battle; its field stops the wearer's own shots,
  so they fight (and count for squads) as melee. Mercenary and Raiders-tier melee raiders may wear one. A unit carrying grenades can throw one (*Grenade* button,
  then click an enemy unit within 6 cells and in sight: everyone in it is caught, whatever their cover);
  a unit with a medkit can tend everyone's wounds mid-battle (*Medkit*).

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
- `src/expeditions.rs`: factions, missions, journeys, choices, trading, fights on site maps
- `src/save.rs`: versioned save format, run-length encoding of per-cell arrays
- `src/scenarios.rs`: prepared worlds to start in (`--scenario battle`)

The binary (`src/main.rs` and `src/front/`) is the Bevy app: `terrain.rs` (chunked ground, rock and wall
meshes), `props.rs` (trees, plants, furniture, doors, items), `pawns.rs` (3D figures), `scene.rs`
(sun, moon, sky), `camera.rs`, `input.rs` (picking, tools, gizmo markers), `audio.rs` (sound effects,
footsteps, ambience), `build_menu.rs`, `expeditions_ui.rs`, `ui.rs` and `overlay.rs`
(egui interface and labels over the 3D view).

### Adding content

Items, buildables, plants and recipes are described in `assets/defs.ron` (built into the game, so rebuild after
editing). A new definition there gets a toolbar button, a shortcut (if it has a `key`), a look made of
simple shapes, and works in hauling, building, mining, harvesting and saves without code changes.
Behaviour the code has to know about is a field: `nutrition` (food), `medicine`, `special` (`Floor`,
`Stairs`, `Bed`, `Door`), `blocks_room`, `holds_floors`, `light`, `sow`, `wild`, `harvest`, `raw`,
`poison`, `weapon` (`sidearm`), `apparel` (`slot`, `also`, `aim`, `sealed`, `carry`, `tend`, `ap`), `cyberware`,
`raider_tiers`, `utility`, `beauty`, `daylight`, `storage`, `along_walls`, and `link` on parts. A buildable that a recipe names becomes a workbench.
`cargo test` checks that the file loads and that every buildable can be built and removed, every plant
harvested and every recipe made.

### Assets

- `assets/defs.ron`, `assets/sounds.ron`: definitions and the sound table (built into the game)
- `assets/sounds/`: placeholder sounds
- `assets/models/characters/`: Quaternius characters, hair, outfits and animation library (glTF, CC0)
- `assets/models/weapons/`: all six weapons in a shared faceted sci-fi style; [asset notes](assets/models/weapons/README.md)
  include quality budgets and regeneration/preview commands. Models appear in hands, on the ground
  and on shelves; shape definitions remain the loading fallback.
- `assets/models/armor/`: steel helmet, fitted Flak Jacket and cyan-lit powered armor; [asset notes](assets/models/armor/README.md).
  Stored displays and pieces attached to character bones share the same meshes and materials.
- `assets/models/items/`: all remaining resources, components, food, medical supplies, cyberware, belt items
  and clothing in the same faceted style; [asset notes](assets/models/items/README.md). Ground,
  shelf and carried stacks share models; clothing also has pieces attached to character bones.
  Simple clothes and parkas use body-specific deforming skins for aiming and movement;
  [gun-pose review command](assets/models/items/README.md#validation).
- `assets/models/furniture/`: 20 matching beds, tables, seats, storage pieces, lights and wall
  furnishings; [asset notes](assets/models/furniture/README.md). Models preserve interaction heights,
  owner-colored blankets and blue construction previews, with shape parts as loading fallbacks.
- `assets/models/world/`: the remaining 20 production, power and structure props; [asset notes](assets/models/world/README.md).
  Grave and fence scenes share geometry and follow fill state and neighbouring connections.
- `assets/models/plants/`: original healroot, cotton and rubber growth stages; [asset notes](assets/models/plants/README.md).
  Trees, berry bushes and corn use the flat-colour `nature/` and `crops/` packs. Models are chosen per
  growth-stage variant, with cell-stable rotation/size and shape fallbacks while loading.
- `assets/models/nature/`, `trees/`, `crops/`, `village/`: Quaternius packs converted from OBJ by
  `python3 tools/obj_to_gltf.py` (from `asset-sources/`)
- `asset-sources/`: the original downloads (FBX, OBJ, Blender files). Not used by the game and not in
  git (see `.gitignore`); `assets/CREDITS.md` says where everything comes from.

`cargo test --release character_models_load -- --ignored` loads all referenced models and animations with the GPU (it can't run
in a sandbox without one).

### Replacing the sounds

The sounds in `assets/sounds/` are simple synthesised placeholders. To use real ones (e.g. a CC0 pack),
put the files in `assets/` and point the entries in `assets/sounds.ron` at them (WAV or OGG). Several
files per sound are picked at random. Definitions name the sounds they make (`sound` on harvests,
buildables, recipes and weapons; `done_sound` on harvests), and `cargo test` checks that every name used
exists and every file is there.

See `PLAN.md` for the roadmap.
