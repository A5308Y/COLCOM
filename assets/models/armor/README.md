# Armor: faceted military sci-fi

Original project artwork, generated alongside the [weapons](../weapons/README.md):

```sh
python3 tools/make_gear.py
python3 tools/preview_gear.py --out target/gear-review
```

The armor uses the Rifle's five materials: olive ceramic, parkerized steel, charcoal polymer,
exposed edges, and restrained amber indicators. Flat faces, broad chamfers and a few lighter edges
provide detail without textures, transparency, custom shaders or extra lights.

| Item | Triangles | Appearance |
|---|---:|---|
| Steel helmet | 306 | Open-face combat shell, cheek plates, padded ear protection |
| Steel armour | 700 | Chest/back plates, collar straps, articulated shoulder caps, utility pouches |
| Powered armour | 1,538 | Heavier plates and arm sleeves, vented power pack, sealed helmet and amber visor |

These are cosmetic replacements. Item IDs, prices, protection, warmth, speed, recipes and saves
retain their existing behavior. Simple clothes, parkas and wool hats retain their existing artwork.

## Shared stored and worn geometry

Each `.gltf` references its matching `.bin`. **Scene 0** assembles the stored item for ground and shelf
displays. Other scenes expose the very same meshes at the origins of their character bones:

| File | Scene | Bone |
|---|---:|---|
| SteelHelmet.gltf | 1 | Head |
| SteelArmor.gltf | 1 / 2 / 3 | spine_03 / upperarm_l / upperarm_r |
| PoweredArmor.gltf | 1 / 2 / 3 / 4 | spine_03 / upperarm_l / upperarm_r / Head |

The glTF file owns one set of materials shared by all its scenes. No second copy of the wearable
geometry is generated for inventory displays. Assembly translations approximate the character's
resting bone positions, in metres, +Y up and +Z forward.

`WornPiece.model` in `assets/defs.ron` selects a `path`, `scene`, and `pose` (`at`, Euler-degree `turn`,
uniform `scale`). Runtime attachment cancels the bone's rest rotation so each piece moves with the
correct bone. The fallback figures use the same models with scale and arm-orientation adjustments.
Existing shape pieces stay visible until each model instance loads, and remain if loading fails.

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

Final in-game review should include standing, kneeling, walking, switching armor/headgear,
daylight/dusk lighting, and ground/shelf placement. A GPU is required for that review.
