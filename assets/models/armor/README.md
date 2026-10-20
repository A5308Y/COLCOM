# Armor: faceted military sci-fi

Original project artwork, generated alongside the [weapons](../weapons/README.md):

```sh
python3 tools/make_gear.py
python3 tools/preview_gear.py --out target/gear-review
```

The steel helmet retains the Rifle's olive, steel and amber materials. The Flak Jacket uses matte
ballistic fabric, padded olive panels, dark webbing and small fasteners. Its tapered body and light
shoulder pads replace the old bulky metal breastplate.

Powered armor deliberately breaks from that palette: blue-gray titanium ceramic, graphite framing,
dark flexible joints, machined edges and emissive ion cyan. A split visor, diamond chest reactor,
layered abdominal plates, angular shoulder shells and powered forearms distinguish it at gameplay
zoom. Separate hips, thighs, knee/shin guards and boots follow the character's leg joints. Shape and
layering take inspiration from the supplied futuristic exosuit references; the geometry is original.

All three use five materials, flat faces and tapered sections, without textures, transparency,
custom shaders or extra lights. The cyan surfaces are emissive materials, not point lights.

| Item | Triangles | Appearance |
|---|---:|---|
| Steel helmet | 306 | Open-face combat shell, cheek plates, padded ear protection |
| Flak Jacket | 564 | Fitted padded vest, webbing, shallow pockets, small shoulder pads |
| Powered armour | 1,986 | Articulated exosuit, sealed split visor, cyan reactor, armored forearms and legs |

These are cosmetic replacements. Item IDs, prices, protection, warmth, speed, recipes and saves
retain their existing behavior. The Flak Jacket keeps the legacy `steel_armor` item ID and
`make_armor` recipe ID; its item name and crafting label both say Flak Jacket.
[Simple clothes, parkas and wool hats](../items/README.md) use
matching faceted models with softer materials.

## Shared stored and worn geometry

Each `.gltf` references its matching `.bin`. **Scene 0** assembles the stored item for ground and shelf
displays. Other scenes expose the very same meshes at the origins of their character bones:

| File | Scene | Bone |
|---|---:|---|
| SteelHelmet.gltf | 1 | Head |
| FlakJacket.gltf | 1 / 2 / 3 | spine_03 / upperarm_l / upperarm_r |
| PoweredArmor.gltf | 1 / 2 / 3 / 4 | spine_03 / upperarm_l / upperarm_r / Head |
| PoweredArmor.gltf | 5 / 6 / 7 | lowerarm_l / lowerarm_r / pelvis |
| PoweredArmor.gltf | 8 / 9 | thigh_l / thigh_r |
| PoweredArmor.gltf | 10 / 11 | calf_l / calf_r |
| PoweredArmor.gltf | 12 / 13 | foot_l / foot_r |

The glTF file owns one set of materials shared by all its scenes. No second copy of the wearable
geometry is generated for inventory displays. Assembly translations approximate the character's
resting bone positions, in metres, +Y up and +Z forward.

`WornPiece.model` in `assets/defs.ron` selects a `path`, `scene`, and `pose` (`at`, Euler-degree `turn`,
uniform `scale`). Runtime attachment cancels the bone's rest rotation so each piece moves with the
correct bone. The fallback figures use the same models with scale and arm-orientation adjustments.
Existing shape pieces stay visible until each model instance loads, and remain if loading fails.
The fallback figures also attach the forearm and leg pieces, with shorter leg proportions.
The stored powered suit lies on its back at a smaller scale to fit within a tile and on shelves;
the worn scenes retain full-size geometry.

The most protective head piece takes visual precedence when a suit's integrated helmet overlaps
separate headgear. This prevents duplicate shells from flickering; protection calculations are unchanged.
Head pieces keep the existing hair-hiding behavior.

## Review

`target/gear-review/index.html` is a CPU geometry gallery; it does not reproduce the game's PBR,
shadows, bodies or animation. All item meshes fit the 2,000-triangle/five-material budget. Checks
verify scene/mesh reuse, bone names in both character skeletons, floor clearance and tile bounds,
weapon/armor cleanup, and loading fallbacks. The ignored GPU tests load every gear asset and check
weapons, armor and helmets on animated characters:

```sh
cargo test --release -- --ignored
cargo run --release -- --scenario battle
```

For a CPU fit review on both actual base character meshes:

```sh
python3 tools/preview_armor.py --out target/armor-fit-review
```

This produces a standalone `index.html` with male/female front and powered-suit rear views.
The base bodies use neutral colors without their textures, and a simple weighted arm pose lowers
their arms. Armor uses each skeleton's actual bone positions. These are fit previews, not game
screenshots. In the battle scenario Eve wears powered armor and holds a laser rifle from round 1;
Finn wears the Flak Jacket and steel helmet.

Final in-game review should include standing, kneeling, walking, switching armor/headgear,
daylight/dusk lighting, and ground/shelf placement. A GPU is required for that review.
