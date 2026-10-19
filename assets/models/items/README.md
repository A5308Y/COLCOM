# Inventory items: faceted frontier sci-fi

Original artwork for all 31 inventory items outside the weapons and armor sets:

```sh
python3 tools/make_items.py
python3 tools/preview_items.py --out target/item-review
```

The standalone gallery is `target/item-review/index.html`. These are CPU geometry previews, not
game screenshots; garments are shown assembled upright, while their ground poses lay them down.

## Art budget and material choices

Each item uses **102–636 triangles** and at most **five material groups**, with no textures,
transparency, emission, custom shaders or extra lights. The intended ceiling for new small supplies
is 1,000 triangles; use fewer faces for small resources. Broad facets, chamfers and rough finishes
follow the [Rifle benchmark](../weapons/README.md).

Resources keep their natural identity instead of receiving olive paint. Bark and end grain distinguish
logs from planks; irregular stone and mineral faces distinguish ores; stacked ingots distinguish steel
and copper. Manufactured parts use charcoal, steel, olive, copper traces and muted identification
colors. Organic materials and fabric stay matte.

| Group | Items | Main forms |
|---|---:|---|
| Materials | 13 | Banded logs, planks, cut stone, ore piles, ingots, latex can, rubber sheets, silicon wafers, cotton and folded cloth |
| Scrap | 2 | Bent plates and hollow pipe; discarded boards and broken electronics |
| Components | 6 | Gear and motor, circuit board, camera assembly, plate/frame bundle, chemical cells, padded fabric |
| Food | 2 | Produce crate and an open field mess tin |
| Medicine | 5 | Tied roots, herbal pouch, medical case, articulated prosthetic arm and leg |
| Clothing | 3 | Utility jacket, insulated coat with folded hood, cuffed wool cap |

Geometry and palettes are grouped by item in `tools/make_items.py`. The shared writer in
`tools/make_rifle.py` accepts an item palette; only the gear's original default palette emits light.
Commit each generated `.gltf` and matching `.bin` together.

## Placement and wearing

Every item definition now names a model. Ground poses center each stack inside a tile and place its
lowest face just above the surface. Shelves apply their existing display scale. Hauling uses the same
item scene at half scale in front of the colonist, and removes the previous scene when a stack changes
or is dropped. Meshes/materials are shared with ground and shelf instances.

Clothing follows the same modular system as armor: scene 0 assembles the item; scenes 1/2/3 of
`SimpleClothes.gltf` and `Parka.gltf` attach Body/SleeveLeft/SleeveRight to `spine_03` and
`upperarm_l`/`upperarm_r`. Scene 1 of `WoolHat.gltf` attaches the cap to `Head` and hides hair.
Outer garments cover the utility jacket visually so duplicate collars and sleeves cannot flicker.
The existing character outfit remains underneath these pieces.

Original shape definitions remain visible during loading or if loading fails. Gameplay properties,
item IDs, crafting, nutrition, medical effects, clothing stats, and save formats are unchanged.
Prosthetics are improved as inventory items; this does not add replacement-limb animations.

## Validation

The tests cover every inventory model's floor clearance, tile bounds, material/triangle budget,
stored/worn mesh reuse, both skeletons' bone names, carried stack replacement, clothing layers,
and cleanup. The existing GPU asset test includes all 40 inventory models:

```sh
cargo test --release
cargo test --release -- --ignored
```

For in-game review, use Menu → Spawn items, place a few stacks on the ground and in storage, and
order a colonist to haul or equip them. Review normal/close zoom, daylight/dusk, carrying and walking.
Final lighting and animation review requires GPU access.
