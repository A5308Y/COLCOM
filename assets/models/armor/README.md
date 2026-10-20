# Armor: faceted military sci-fi

For worn armor changes, start with the [apparel authoring guide](../../../tools/APPAREL_GUIDE.md),
especially the continuous-shell recipe and the problems it replaces.

Original project artwork, generated alongside the [weapons](../weapons/README.md):

```sh
python3 tools/make_gear.py
python3 tools/make_skinned_power_armor.py
python3 tools/preview_gear.py --out target/gear-review
```

The steel helmet retains the Rifle's olive, steel and amber materials. The Flak Jacket uses matte
ballistic fabric, thick olive front/back inserts, dark webbing and small fasteners. Its tapered body
and reinforced shoulder pads provide more protective bulk than everyday clothes, without the old
rectangular metal breastplate.

Powered armor deliberately breaks from that palette: blue-gray titanium ceramic, graphite metal,
machined edges and emissive ion cyan. The live suit's joints are fully covered by metal.
A split visor, diamond chest reactor,
layered abdominal plates, angular shoulder shells and powered forearms distinguish it at gameplay
zoom. Separate hips, thighs, knee/shin guards and boots follow the character's leg joints. Shape and
layering take inspiration from the supplied futuristic exosuit references; the geometry is original.

All three use five materials, flat faces and tapered sections, without textures, transparency,
custom shaders or extra lights. The cyan surfaces are emissive materials, not point lights.

| Item | Triangles | Appearance |
|---|---:|---|
| Steel helmet | 306 | Open-face combat shell, cheek plates, padded ear protection |
| Flak Jacket | 624 | Thick front/back padding, webbing, utility pockets, reinforced shoulder pads |
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

## Sealed, articulated powered suit

Live colonists use `PoweredArmorMale.gltf` or `PoweredArmorFemale.gltf` through the same
`ApparelDef.skinned` loader, outfit-bone binding and cleanup as clothes and parkas.
Each fit uses the matching outfit's bind matrices and actual limb lengths. A continuous titanium
shell encloses the entire body, including the fingers, wrists, armpits, hips and neck beneath
the helmet. There are no exposed fabric inserts. The foundation derives from the matching
CC0 base character, retargeted and simplified using the clothing CPU skinning helpers.

The shell is not an offset copy of the body: that read as a painted bodysuit. Each segment
between joints is projected onto low-poly faceted plates (12-sided cuirass with a chest keel and
fauld lames, 8-sided vambraces, thigh plates and ridged greaves) sized to enclose the body rather
than trace its muscles, breasts or seat. Faceted domes form pauldrons and knee cops. Boots have
a faceted shaft with a cuff, a hulled foot with toe cap and heel, a flat sole and a graphite sole
rim. Plates blend into narrow flexible bands at the neck, shoulders, elbows, wrists, hips, knees
and ankles, where skin weights blend. Plates stand about 2–5 cm off the body; joints stay close.
The shell is flat-shaded: vertices split only between facets, and the copies retain position
and weights, so the enclosure stays closed and bends with the pawn without overlapping layers.
Cyan lights sit in shallow relief on the finished plates with interpolated skin weights. A full
suit takes visual precedence over separately equipped leg apparel.

The suit has no skin textures or transparent materials. `full_body: true` hides the original
torso, limbs, head and all native outfit accessories only after the suit is ready.
Weapons and helmets always attach to the outfit skeleton, so hidden hair cannot hide them.
Unequipping restores the original body; failed
loading retains fallbacks. The helmet remains the shared rigid attachment, and the stored
1,986-triangle prop remains unchanged. Worn suits use 8,551 male / 7,818 female triangles,
including the complete shell and shallow details, plus the existing helmet. These deforming
full-body meshes have a higher budget than the static item props.

## Review

`target/gear-review/index.html` is a CPU geometry gallery; it does not reproduce the game's PBR,
shadows, bodies or animation. Static item props fit the 2,000-triangle/five-material budget. Checks
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
python3 -B tools/preview_clothes_poses.py --garment PoweredArmor --out target/powered-armor-poses
python3 -B tools/test_power_armor.py
```

The older `armor-fit-review` shows rigid inventory/fallback pieces. The `powered-armor-poses`
gallery samples idle, walking, carrying, aiming, firing, kneeling and sitting clips on both
outfit skeletons, with front/side/back views and the laser rifle. Geometry checks verify
complete body-surface coverage, opaque metal materials, matching bind poses and close fitting
through combat and movement poses.

The older gallery produces a standalone `index.html` with male/female front and powered-suit rear views.
The base bodies use neutral colors without their textures, and a simple weighted arm pose lowers
their arms. Armor uses each skeleton's actual bone positions. These are fit previews, not game
screenshots. In the battle scenario Eve wears powered armor and holds a laser rifle from round 1;
Finn wears the Flak Jacket and steel helmet.

Final in-game review should include standing, kneeling, walking, switching armor/headgear,
daylight/dusk lighting, and ground/shelf placement. A GPU is required for that review.
