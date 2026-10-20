# Asset tools

The generators and CPU previews share mesh primitives, glTF buffer writing and
vector/matrix operations in [asset_helpers.py](asset_helpers.py). Keep geometry,
materials and skeleton selection in the individual generators. For apparel changes,
follow [APPAREL_GUIDE.md](APPAREL_GUIDE.md).

Run all Python checks from the repository root:

```sh
python3 -B -W error::ResourceWarning -m unittest discover -s tools -p 'test_*.py'
```

The regeneration check can also run on its own:

```sh
python3 -B tools/test_asset_regeneration.py
```

It regenerates into temporary directories and compares both JSON and binary bytes
against the committed files, reporting a text diff or the first binary difference.
Coverage includes 77 static models, 10 skinned garments/suits and two extracted heads, plus 31 world-prop and plant-stage pairs.
With the original Quaternius OBJ packs in `asset-sources/`, it also checks 341 converted
models: 461 pairs in total. Missing optional OBJ packs are reported as skips; the
remaining checks use committed inputs. A small OBJ fixture exercises the converter
without those downloads. Download sources are listed in [CREDITS.md](../assets/CREDITS.md).
Imported character bodies/outfits and animation files are inputs, not regenerated.

On the development machine, the four regeneration tests took about 3.2 seconds with
all OBJ packs present; the original 17 Python tests, including CPU skinning/pose checks, took
about 11 seconds. These are separate from `cargo test` and add no wait to that command.
Byte-identical assets and CPU pose tests do not constitute in-game GPU verification.

Generator command-line defaults still write to `assets/models/`. For isolated builds,
static meshes accept `geometry.write(name, out, pieces)`; skinned generator `write`
functions and `make_heads.make_head` accept an optional output directory. Input paths
remain anchored to the repository.

## World models (10d)

Regenerate the new models with `python3 -B tools/make_world_props.py` and
`python3 -B tools/make_plants.py`. Each accepts `write_all(out)` from Python for
isolated output. The shared writer accepts named composite scenes referencing the
same piece meshes; existing inventory/worn scene layouts retain their exact bytes.

`python3 -B tools/preview_world_props.py` creates front/rear CPU views at a common
cell scale in `target/world-props-review/index.html`. It obtains paths, scene indices
and exact placement matrices from `cargo run --example look -- --world-manifest`;
there is no second copy of the plant-to-model mappings in Python. GPU review uses
`cargo run --release --example look -- --world-props`, with the existing camera,
lighting and time-of-day controls. Original source-pack browsing remains the default.

The Python suite now has 23 tests (about 12 seconds here). The five geometry checks
add under 0.1 seconds, and regeneration covers all 31 new pairs. The opt-in CPU cache
benchmark does not add time to ordinary `cargo test`:

```sh
cargo test --release --bin my-world benchmark_dense_world_prop_sync -- --ignored --nocapture
```

It compares identical 100x70 colonies with 6,664 props, using model roots or the
original shape fallbacks, then checks 1,000 cached updates do not rebuild entities.
It uses empty scene assets: timings exclude glTF decoding, loaded mesh propagation
and GPU rendering. GPU frame time and occupied-station visual acceptance are still
pending on a machine/session with a GPU.
