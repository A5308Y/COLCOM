# Weapons: faceted military sci-fi

Original project artwork using only Python's standard library. The Rifle is the reference asset;
the other weapons share its material palette and construction style.

```sh
python3 tools/make_rifle.py
python3 tools/make_gear.py                 # all weapons and armor
python3 tools/preview_rifle.py --out target/rifle-review
python3 tools/preview_gear.py --out target/gear-review
cargo run --release -- --scenario battle
```

The previews are CPU geometry renders, not game screenshots. Use the battle scenario to judge the
actual lighting and grip: wheel to zoom, right-drag to orbit. A normal colony also starts with a Rifle.
`target/gear-review/index.html` is a standalone gallery of every weapon and armor item.

| Weapon | Triangles | Design |
|---|---:|---|
| Rifle | 1,620 | Original frontier rifle with conventional magazine and reflex optic |
| Assault rifle | 1,408 | Short bullpup, magazine behind the grip, full-length rail |
| Laser rifle | 1,108 | Square emitter, external heat sinks, removable power cell |
| Pistol | 624 | Compact service pistol with olive slide panel and grip inserts |
| Club | 588 | Weighted industrial baton with a wrapped polymer grip |
| Knife | 210 | Clipped, faceted steel blade with a crossguard and ribbed grip |

## Repeatable quality budget

Military sci-fi with a weathered, low-poly feel: believable stock, receiver, magazine and grip;
broad chamfers; eight-sided barrel; restrained technology. Detail should read at gameplay zoom.
The Rifle has **1,620 triangles**, one mesh, five material primitives, and an **86,592-byte buffer**.
Keep similar props below 2,000 triangles and five shared materials; small weapons need fewer faces.
No textures, normal maps,
transparency, new lights, moving weapon parts or custom shaders are needed.

The five swatches are olive ceramic, parkerized steel, charcoal polymer, exposed steel edges, and
amber optic indicators. Colors and roughness/metal values are grouped in `PALETTE`; dimensions are
in `Rifle.build()` and `tools/make_gear.py`. Edge wear comes from a few lighter facets rather than scratch textures.
The shared exporter skips empty material groups and can also export the [modular armor](../armor/README.md).

## Placement and integration

`Rifle.gltf` references `Rifle.bin`; commit both after regenerating. Geometry uses metres, +Y up,
+Z toward the muzzle, and the right-hand grip at the origin. The model is approximately one metre long.
The other weapons use the same grip convention, except the Club's head extends along +Y; its held
pose supplies the existing forward lean. Generated file stems match the asset names in the table
without spaces (`AssaultRifle`, `LaserRifle`, etc.).

The optional `model` entry on an item in `assets/defs.ron` supplies the asset path and two poses:
`ground` lays it on its side within a tile; `held` adjusts it relative to the hand's grip. Each pose
has `at`, Euler-degree `turn`, and uniform `scale`. Shelves apply their normal display scale around
the ground pose. Animated hands cancel the aiming hand's rotation; fallback figures convert metres
to their own scale. The asset server shares the scene's mesh and material handles across instances.

Existing `look` and weapon `held` shapes are retained as fallbacks. They stay visible until each
glTF instance is ready, and remain visible when loading fails. Hauling uses a smaller instance of
the same item scene, as it does for the [other inventory items](../items/README.md). Gameplay data
and saves are unaffected.

Review standing and deployed aim poses, switching weapons, dropped items and shelf displays, at
normal and close zoom in daylight and dusk. GPU validation is required for final lighting and animation
review; geometry previews do not reproduce Bevy's PBR or shadows.
