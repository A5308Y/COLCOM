# Authoring clothing and worn armor

Read this before implementing new apparel or changing its fit. Run commands from the
repository root. The existing generators are the working examples; extend their helpers
instead of inventing another skeleton, glTF writer or runtime attachment system.

## Start with the right example

| Task | Start here | Reuse |
|---|---|---|
| Shirt, jacket, coat or exposed hands | [make_skinned_clothes.py](make_skinned_clothes.py) | `build`, `bare_arms`, torso/sleeve weights, trouser-weight transfer for long hems, `attachment` for details |
| Trousers, leg guards and included boots | [make_skinned_legs.py](make_skinned_legs.py) | `lower_body`, native leg weights, continuous raised guards and fitted cargo pockets |
| Enclosed, close-fitting powered suit | [make_skinned_power_armor.py](make_skinned_power_armor.py) | `sealed_body`, `BodySurface`, one continuous shell, shallow skinned details |
| Writing skinned glTF | [skinned_asset.py](skinned_asset.py) | `SkinnedAsset`; preserve the source skeleton and inverse bind matrices |
| Sampling actual animation | [gltf_pose.py](gltf_pose.py), [preview_clothes_poses.py](preview_clothes_poses.py) | Existing animation clips, layered poses and weapon grip |
| Ground/shelf prop or loading fallback | [make_items.py](make_items.py), [make_gear.py](make_gear.py) | Static scene assembly and bone-local fallback pieces |
| Equipping and body visibility | [characters.rs](../src/front/characters.rs), [pawns.rs](../src/front/pawns.rs) | `spawn_garment`, live bone rebinding, cleanup and outer-layer precedence |

The live body uses `assets/models/characters/outfits/{Male,Female}_Peasant.gltf`.
The base character and outfit can have matching bone names but different bind poses.
Do not copy vertices or inverse bind matrices between them without retargeting.
Generate a separate fit for each body type; scaling the male file is insufficient.

## Work in this order

1. **Choose coverage.** Decide which body regions the garment replaces. Ordinary clothes
   replace the torso/arms and retain native trousers, boots and head. A full suit needs
   its own coverage of legs, feet, hands, fingers, neck and head beneath its helmet.
   `full_body: true` hides native head and lower-body surfaces too; it does not generate
   missing geometry. `Slot::Legs` has its own skinned garment root alongside the upper
   garment and replaces native trousers **and boots**. Include both in leg assets.
   A full suit suppresses the separate leg garment. Other arbitrary per-region coverage
   is not supported.
2. **Make the undecorated fit first.** For clothes, start with the tapered torso and sleeve
   sections in `build`. Its `details=False` option helps isolate shape and skinning.
   Use the matching outfit's bone positions to locate shoulders, elbows, wrists and hips.
   Do not model everything as rectangular boxes or inflate the old tunic.
3. **Bind the surface to motion.** Blend adjacent spine bones along the torso and back,
   upper/lower arm weights around elbows, and trouser/thigh weights around long hems.
   Keep enough longitudinal sections near bends. Normalize weights and retain at most
   four influences per vertex. Narrow cuffs should end at the wrists.
4. **Keep joins covered.** Overlap clothing sleeve roots inside the torso and sleeves
   over the underlying arms. Preserve the hand geometry and finger weights through
   `bare_arms`: male hands come from the outfit; female bare arms are retargeted from
   the base character. Do not replace these with approximate mittens or detached wraps.
5. **Review the hard poses immediately.** Generate both variants and inspect Aim and
   Deployed (kneeling with an aiming upper body), including side and rear views.
   Resolve shoulder, elbow, back and hip fit before adding pockets, trim or lights.
6. **Fit details to the finished surface.** Small pockets, seams and plackets must use
   the underlying surface position and weights. Start with clothing's `attachment`
   helper. Attaching a detail to the nearest single bone can still make it float.
7. **Integrate and review all poses.** Add the variant paths to `ApparelDef.skinned` in
   [defs.ron](../assets/defs.ron), retain appropriate stored/fallback scenes, and reuse
   the existing loading and visibility lifecycle. Then run the review below.

For a new item, also extend the preview tool's `--garment` choices and loading/material
logic as needed: it currently recognizes `SimpleClothes`, `Parka`, `PoweredArmor`,
`CargoTrousers` and `FlakLegGuards`. Use `--leg-top Parka` to check leg apparel under a coat.
Extend the relevant test item lists too. Running unchanged tests does not cover a new item.
Do not silently make an incompatible new garment behave like one of those existing names.

## Powered armor: use the final continuous-shell approach

The user wants fully enclosing, close-fitting armor, and explicitly accepts bending metal.
Use one continuous titanium surface with the body's weights, including all joints.
It must still read as armor in a low-poly, Valheim-like style, not a colored skinsuit:
`armor_form` projects each segment onto faceted plates that enclose the body, adds
pauldrons, knee cops and boots, and thins to flexible bands at joints. `facet_normals`
flat-shades the result. Cyan lights have shallow relief and interpolated body weights.
Graphite remains on the boot soles and helmet, not in broad body patches.

Do not go back to either failed approach:

- Rigid plates on individual spine bones: the back pulls away in gun and kneeling poses.
- Many overlapping shells projected onto the body: coarse faces and differing weights
  cut through each other, producing jagged patches around shoulders and knees. Merely
  subdividing more or increasing offsets adds cost without reliably solving this.
- Painting projected dark panels onto whole coarse triangles: the ragged boundaries
  look like exposed undersuit. Keep continuous metal coverage instead.
- Offsetting the body surface by a uniform thickness: muscles, breasts, buttocks and
  toes show through, and smooth shading makes it a painted bodysuit. Convex-hull filling
  alone keeps the convex muscle bulges. Use designed faceted sections that enclose the
  body, fitted only to vertices the plate covers, with generous edge snapping so most
  triangles lie on one facet plane.

Shared shell vertices prevent those layer intersections. For raised flak pads, reuse
`guard_panels` in `make_skinned_legs.py`: clip pad outlines into the native surface and
move the shared vertices for padding. Classifying whole coarse triangles by their centers
creates ragged edges; independent projected shin plates can stretch across a bent knee. Keep a closed metal surface
under details, including at joints; a visible dark fabric undersuit does not satisfy full
armor coverage. Graphite metal is fine. Do not change the whole armor palette to hide gaps.
The helmet can remain rigid on the head. Preserve the distinctive cyan visor/reactor.

## Style and budget

Aim for maintainable, modest-detail frontier sci-fi: readable silhouettes, tapered forms,
restrained trim and rough materials. Clothing uses smooth normals on rounded fabric,
matte materials, a fitted waist and narrow cuffs. Parkas get modest insulation and a close
folded hood, not oversized shoulders. Wool hats should read as knit caps, not helmets.
The Flak Jacket needs visible protective padding; do not thin it into a shirt.
Powered armor may use its distinct blue-gray, graphite and emissive cyan palette.

Existing ordinary fabric meshes are 684 triangles for a shirt and 960 for a parka,
with an intended fabric ceiling of 1,800 per variant. Retained arms/hands are additional.
The complete powered suits are about 7,800–8,600 triangles plus the helmet.
Cargo trousers and flak guards use about 3,988–5,708 triangles including retained boots.
Static inventory props have separate budgets; see the
[item notes](../assets/models/items/README.md) and [armor notes](../assets/models/armor/README.md).
Do not introduce textures, shaders, lights or dense meshes merely to compensate for poor fit.

## Diagnose the cause before changing dimensions

| Symptom | First check | Preferred fix |
|---|---|---|
| Looks fine standing but stretches while aiming | Torso/sleeve weights, elbow sections and bind matrices | Fix skinning and fit to the correct outfit; do not enlarge the whole garment |
| Back floats when bending | Back/details still bound to one spine bone | Share the underlying surface's blended weights; use a continuous shell for full armor |
| Sleeves look like bulky blocks | Box geometry, large shoulder radius, detached forearm wraps | Taper rounded sections, narrow cuffs, reuse continuous arms |
| Parka clips a raised thigh or seat | Too few hip sections or a spine-only hem | Reuse the parka's extra hip cuts and trouser-weight transfer |
| Leg equipment never appears when worn | Item has only an inventory model, or upper and lower garments share one root | Supply male/female leg skins, select the independent leg garment, include boots, test both load orders and full-suit precedence |
| Powered legs look like plain trousers | Most side/rear faces are dark frame material | Preserve enclosing titanium coverage around thighs, shins and feet with narrow joint bands |
| Powered armor looks like a skinsuit | Shell follows anatomy; smooth normals | Faceted `armor_form` plates with volume, flat shading; keep the volume and boot tests passing |
| Neck, wrists or armpits show unwanted gaps | Missing coverage or mismatched joins | Add continuous coverage or local overlap; test both body types |
| Speckled/jagged patches during motion | Intersecting layers, duplicate faces, mismatched detail weights | Remove buried faces and merge broad surfaces; keep only shallow details |
| Gun appears below/outside the hands | Source hand geometry, hand weights and live grip transform | Preserve the existing hands and inspect the actual grip; do not move the gun to conceal a sleeve error |
| Weapon or helmet disappears on some pawns | Attachment selected from the first-loaded head/hair skeleton, whose root can be hidden | Always attach to outfit bones; verify inherited visibility in every part load order |
| Only one body type is wrong | Shared fit or wrong source bind pose | Generate against that body's own outfit; retarget any borrowed anatomy |
| Old body, duplicate helmet or fallback remains visible | Loading, rebinding and layer selection | Fix the existing garment lifecycle; do not hide bones or native surfaces before successful binding |

## Review and completion

Regenerate only the assets affected by the change:

```sh
python3 -B tools/make_skinned_clothes.py
python3 -B tools/make_skinned_power_armor.py
python3 -B tools/make_skinned_legs.py
```

During fitting, use a small gallery, then open the actual PNGs or HTML:

```sh
python3 -B tools/preview_clothes_poses.py --garment SimpleClothes Parka --pose Aim Deployed --out target/clothes-fit-check
python3 -B tools/preview_clothes_poses.py --garment PoweredArmor --pose Aim Deployed --out target/armor-fit-check
```

Before finishing, render the full pose set for the affected garments:

```sh
python3 -B tools/preview_clothes_poses.py --garment SimpleClothes Parka --out target/clothes-poses
python3 -B tools/preview_clothes_poses.py --garment PoweredArmor --out target/powered-armor-poses
python3 -B tools/test_skinned_clothes.py
python3 -B tools/test_power_armor.py
python3 -B tools/test_skinned_legs.py
python3 -B tools/preview_clothes_poses.py --garment CargoTrousers FlakLegGuards --out target/leg-apparel-poses
python3 -B tools/preview_clothes_poses.py --garment CargoTrousers FlakLegGuards --leg-top Parka --out target/leg-apparel-parka-poses
```

Inspect both body types, front/side/back, across Idle, Walk, Carry, Aim, Fire, Kneel,
Sit and Deployed. Reject floating backs/details, unintended skin/fabric exposure,
broken silhouettes, detached sleeves and misplaced hands. Inspect at gameplay scale
as well as close up. For leg apparel, check visible thighs, knees, shins and feet; a shirt or coat must remain
visible too. Switching to a full suit must remove the previous trousers/guards. Passing geometry tests alone does not establish that an item looks good.
The older static `preview_armor.py` galleries do not validate live garment deformation.

For runtime/definition changes, run appropriate Rust tests (`cargo test --release` for
broader changes). The garment lifecycle tests live in `src/front/characters.rs`.
`battle_models_keep_finn_and_bram_armed` in `src/front/pawns.rs` loads the actual
battle character/weapon scenes without a GPU, checks visible weapons on active fighters,
and checks that full suits hide every native material surface, including Ranger accessories.
Asset-loading tests must call `app.finish()` and `app.cleanup()` before `app.update()`;
otherwise glTF loaders remain unregistered and a test may exercise only fallback figures.
Where GPU access exists, also run `cargo test --release characters_animate -- --ignored`
and inspect `cargo run --release -- --scenario battle` for lighting, animation, equipping,
unequipping, bedding and headgear overlap. Equip a new item explicitly; the existing scenario
cannot automatically demonstrate a newly added asset.

Deliver the generator edits and matching generated `.gltf`/`.bin` pairs, update credits
if reusing source artwork, and link the reviewed gallery in the completion report.
State which checks ran and distinguish CPU geometry/animation previews from in-game
GPU review. If GPU access is unavailable, complete CPU validation and state that limitation.
