# Furniture: faceted frontier sci-fi

Original project artwork extending the [items](../items/README.md) and
[weapons](../weapons/README.md) style to room furnishings. Warm timber, worn gunmetal,
olive panels, soft fabric and broad chamfers keep the silhouettes readable at gameplay zoom.
Medical furniture uses pale enamel and muted amber markings; lights use warm emissive diffusers.

```sh
python3 tools/make_furniture.py
python3 tools/preview_furniture.py --out target/furniture-review
cargo test --offline --release
cargo test --release character_models_load -- --ignored
cargo run --release
```

The generator and CPU preview use only Python's standard library. The standalone gallery at
`target/furniture-review/index.html` shows geometry and base colors, not Bevy lighting or shadows.
Final in-game review needs a GPU: check normal/close zoom, daylight/night, rotated placement,
blueprints, owner changes, seated/sleeping colonists and stocked shelves.

| Furniture | Triangles | Furniture | Triangles |
|---|---:|---|---:|
| Bed | 572 | Sleeping spot | 140 |
| Table | 420 | Chair | 476 |
| Chess table | 896 | Bookshelf | 1,204 |
| Shelf | 392 | Dresser | 336 |
| Medical cabinet | 252 | Operating table | 224 |
| Painting | 170 | Poster | 77 |
| Calendar | 392 | Dartboard | 112 |
| Wall shelf | 44 | Wall plant | 194 |
| Lamp | 252 | Wall lamp | 168 |
| Torch | 118 | Fireplace | 460 |

These 20 models cover the Furniture, Wall decor and Light menus. Keep future furniture below
2,000 triangles and five materials per asset, with flat normals and a few large details.
There are no textures, transparent surfaces, custom shaders, animated meshes or added light sources.
The existing point lights and flame flicker remain responsible for room illumination.

Each `.gltf` references its matching `.bin`; regenerate and commit both. Scene 0 uses cell units,
+Y up and the same origin/orientation as the definition's shape fallback. Wall decor faces +Z and
fits a 0.2-cell wall slot. The dresser and medical cabinet face -Z. A build definition's optional
`model` supplies `path` and `pose` (`at`, Euler-degree `turn`, uniform `scale`).

Models preserve the fallback's back reach for wall placement and fit below the ceiling. Shelf tops
are at 0.32/0.62 cells, the wall shelf at 0.56, the chair seat at 0.325, the bed mattress at 0.33,
the table at 0.53 and operating-table padding at 0.475. Geometry tests guard these interaction surfaces.

Scene meshes/material handles are shared across instances. Once a scene is ready, construction
previews replace its surface handles with the existing blue ghost material. `owner_material` on
beds selects only the `Owner fabric` material for the owner's blanket color; the pillow keeps its
linen material. Overrides are per instance and never edit the shared source material. Shape parts
remain visible until loading succeeds, and stay visible if it fails. Costs, room effects, storage
capacity, rotation rules and saves retain their existing behavior.
