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
Coverage includes 77 static models, 10 skinned garments/suits and two extracted heads.
With the original Quaternius OBJ packs in `asset-sources/`, it also checks 341 converted
models: 430 pairs in total. Missing optional OBJ packs are reported as skips; the
remaining checks use committed inputs. A small OBJ fixture exercises the converter
without those downloads. Download sources are listed in [CREDITS.md](../assets/CREDITS.md).
Imported character bodies/outfits and animation files are inputs, not regenerated.

On the development machine, the four regeneration tests took about 3.2 seconds with
all OBJ packs present; all 17 Python tests, including CPU skinning/pose checks, took
about 11 seconds. These are separate from `cargo test` and add no wait to that command.
Byte-identical assets and CPU pose tests do not constitute in-game GPU verification.

Generator command-line defaults still write to `assets/models/`. For isolated builds,
static meshes accept `geometry.write(name, out, pieces)`; skinned generator `write`
functions and `make_heads.make_head` accept an optional output directory. Input paths
remain anchored to the repository.
