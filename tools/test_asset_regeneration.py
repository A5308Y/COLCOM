"""Byte-for-byte asset regeneration: python3 -B tools/test_asset_regeneration.py.

Generators write only into temporary directories. OBJ pack checks require the
optional original downloads in asset-sources/ (see assets/CREDITS.md); all other
inputs are committed. No renderer, third-party Python modules or network needed.
"""
from contextlib import redirect_stdout
import difflib
import io
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

import make_furniture
import make_gear
import make_heads
import make_items
import make_plants
import make_world_props
import make_skinned_clothes
import make_skinned_legs
import make_skinned_power_armor
import obj_to_gltf

ROOT = Path(__file__).resolve().parents[1]
MODELS = ROOT / "assets/models"


class AssetRegeneration(unittest.TestCase):
    def compare(self, actual, expected):
        self.assertTrue(expected.is_file(), f"generated file has no committed counterpart: {expected}")
        got, want = actual.read_bytes(), expected.read_bytes()
        if got != want:
            if expected.suffix == ".gltf":
                diff = "\n".join(difflib.unified_diff(want.decode().splitlines(), got.decode().splitlines(),
                                                   fromfile=str(expected), tofile=str(actual)))
                self.fail(diff[:8000])
            at = next((i for i, (a, b) in enumerate(zip(got, want)) if a != b), min(len(got), len(want)))
            self.fail(f"{expected}: first binary difference at byte {at}; regenerated {len(got)} vs committed {len(want)} bytes")

    def compare_generated(self, out):
        files = sorted(p for p in out.rglob("*") if p.suffix in (".gltf", ".bin"))
        self.assertTrue(files, "generator produced no files")
        for path in files:
            with self.subTest(file=str(path.relative_to(out))):
                self.compare(path, MODELS / path.relative_to(out))
        return {p.relative_to(out) for p in files}

    def test_procedural_models_match_committed_files(self):
        with TemporaryDirectory() as tmp, redirect_stdout(io.StringIO()):
            out = Path(tmp)
            expected = set()
            for module, fixed_folder in [(make_gear, None), (make_items, "items"), (make_furniture, "furniture")]:
                for folder, name, geometry, pieces in module.models():
                    folder = fixed_folder or folder
                    geometry.write(name, out / folder, pieces)
                    expected.update(Path(folder) / (name + ext) for ext in (".gltf", ".bin"))
            self.assertEqual(self.compare_generated(out), expected)
            # These directories contain only procedural and skinned generated pairs.
            committed = {p.relative_to(MODELS) for folder in ("weapons", "armor", "items", "furniture")
                         for p in (MODELS / folder).iterdir() if p.suffix in (".gltf", ".bin")
                         and not p.stem.endswith(("Male", "Female"))}
            self.assertEqual(expected, committed, "update generator coverage when adding or removing models")

    def test_world_props_and_plants_match_committed_files(self):
        with TemporaryDirectory() as tmp, redirect_stdout(io.StringIO()):
            out = Path(tmp)
            make_world_props.write_all(out / "world")
            make_plants.write_all(out / "plants")
            expected = {p.relative_to(MODELS) for folder in ("world", "plants")
                        for p in (MODELS / folder).iterdir() if p.suffix in (".gltf", ".bin")}
            self.assertEqual(self.compare_generated(out), expected)

    def test_skinned_models_match_committed_files(self):
        with TemporaryDirectory() as tmp, redirect_stdout(io.StringIO()):
            out = Path(tmp)
            for sex in ("Male", "Female"):
                for parka in (False, True):
                    make_skinned_clothes.write(sex, parka, out / "items")
                for guarded in (False, True):
                    make_skinned_legs.write(sex, guarded, out / "items")
                make_skinned_power_armor.write(sex, out / "armor")
            expected = {p.relative_to(MODELS) for folder in ("armor", "items")
                        for p in (MODELS / folder).iterdir() if p.suffix in (".gltf", ".bin")
                        and p.stem.endswith(("Male", "Female"))}
            self.assertEqual(self.compare_generated(out), expected)

    def test_heads_match_committed_files(self):
        with TemporaryDirectory() as tmp, redirect_stdout(io.StringIO()):
            out = Path(tmp)
            for name in make_heads.CHARACTERS:
                make_heads.make_head(name, out / "characters/base")
            expected = {p.relative_to(MODELS) for p in (MODELS / "characters/base").glob("*_Head.*")
                        if p.suffix in (".gltf", ".bin")}
            self.assertEqual(self.compare_generated(out), expected)

    def test_obj_packs_match_committed_files(self):
        for source, folder, textures in obj_to_gltf.PACKS:
            with self.subTest(pack=folder):
                inputs = Path(obj_to_gltf.SRC) / source
                if not inputs.is_dir():
                    self.skipTest(f"optional original OBJ pack missing: {inputs}")
                objects = sorted(p for p in inputs.iterdir() if p.suffix.lower() == ".obj")
                self.assertTrue(objects, f"no OBJ files in {inputs}")
                with TemporaryDirectory() as tmp, redirect_stdout(io.StringIO()):
                    out = Path(tmp)
                    for obj in objects:
                        obj_to_gltf.convert(str(obj), str(out / folder), textures)
                    expected = {p.relative_to(MODELS) for p in (MODELS / folder).iterdir()
                                if p.suffix in (".gltf", ".bin")}
                    self.assertEqual(self.compare_generated(out), expected)


if __name__ == "__main__":
    unittest.main()
