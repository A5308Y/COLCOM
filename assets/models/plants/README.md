# Original crop stages

The eleven `Healroot1–4`, `Cotton1–4` and `RubberPlant1–3` glTF/bin pairs are original
project artwork from `tools/make_plants.py`. Shared mesh helpers generate faceted
leaf surfaces, purple medicinal flowers, pale cotton bolls and a mature rubber
plant's small latex collection cup. Five opaque, untextured materials per asset;
all stages stay below 1,000 triangles per stalk.

```sh
python3 -B tools/make_plants.py
python3 -B tools/preview_world_props.py --out target/world-props-review
cargo run --release --example look -- --world-props
```

The definitions also select existing CC0 flat-colour Quaternius models:

- Trees: `nature/CommonTree_1`, `CommonTree_3`, `BirchTree_1`, `PineTree_1`, `PineTree_2`.
  Heights are normalized to 1.46 cells for broadleaf trees and 1.6 for pines, before
  the existing 0.85–1.2 per-cell size variation. Trunk ground contacts are centered;
  crowns may overhang adjacent cells naturally.
- Berry bushes: `nature/BushBerries_1`.
- Corn: `crops/Corn_1` through `Corn_4`, aligned to the existing four growth stages.

These choices retain the 2026-10-12 flat-colour art decision; the textured tree pack
remains available in the source-pack viewer but is not selected for gameplay.
See [credits](../../CREDITS.md) for the Quaternius source licenses.

`Stage.variants` pairs each variant's `parts` fallback with its optional `model`:
asset path plus nonempty `placements` using `ModelPose`. Corn, healroot and cotton
repeat one shared stalk scene at the existing four plot positions; rubber and wild
plants use one scene. Growth thresholds, yields, sowing, collision/cover, variant
counts and deterministic per-cell variation are unchanged. No save version change.

A multi-stalk plot switches as a whole once all scene instances are ready. While
any instance is delayed or fails, all of that plot's original fallback stays visible.
Removed/harvested plots and stage changes clean up the complete hierarchy. Idle
frames retain the existing props and still use the map revision cache.

Geometry/regeneration and CPU pose-scale review pass; actual GPU lighting and dense
vegetation rendering remain pending. Wind and new seasonal foliage behaviour belong
to the later atmosphere work.
