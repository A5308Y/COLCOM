"""Contracts shared by generators and CPU previews: python3 -B tools/test_asset_helpers.py."""
import math
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from asset_helpers import IDENTITY, matrix, multiply, point, unit
from gltf_pose import Gltf
from obj_to_gltf import convert


class SharedAssetHelpers(unittest.TestCase):
    def test_trs_and_column_major_matrices_transform_points_and_directions(self):
        # Scale (1, 0, 0) by 2, rotate 90 degrees around Z, then translate.
        trs = {"translation": (3, 4, 5), "rotation": (0, 0, math.sqrt(.5), math.sqrt(.5)), "scale": (2, 3, 4)}
        transform = matrix(trs)
        for actual, expected in zip(point(transform, (1, 0, 0)), (3, 6, 5)):
            self.assertAlmostEqual(actual, expected)
        for actual, expected in zip(point(transform, (1, 0, 0), w=0), (0, 2, 0)):
            self.assertAlmostEqual(actual, expected)
        column_major = [transform[r][c] for c in range(4) for r in range(4)]
        self.assertEqual(matrix({"matrix": column_major}), transform)
        self.assertEqual(multiply(IDENTITY, transform), transform)
        # Composition must apply the child before its parent.
        parent = matrix({"translation": (-1, 2, 0)})
        self.assertEqual(point(multiply(parent, transform), (1, 0, 0)), point(parent, point(transform, (1, 0, 0))))

    def test_unit_handles_zero_normals_and_four_component_rotations(self):
        self.assertEqual(unit((0, 0, 0)), (0, 1, 0))
        self.assertEqual(unit((3, 0, 4)), (.6, 0, .8))
        self.assertEqual(unit((0, 0, 0, 2)), (0, 0, 0, 1))

    def test_obj_writer_preserves_mesh_attributes_without_original_downloads(self):
        with TemporaryDirectory() as tmp:
            out = Path(tmp)
            obj = out / "Triangle.obj"
            obj.write_text("v 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl Bark\nf 1/1 2/2 3/3\n")
            convert(str(obj), str(out), textures=True)
            model = Gltf(out / "Triangle.gltf")
            primitive = model.data["meshes"][0]["primitives"][0]
            attrs = primitive["attributes"]
            self.assertEqual(model.values(attrs["POSITION"]), [(0., 0., 0.), (1., 0., 0.), (0., 1., 0.)])
            self.assertEqual(model.values(attrs["NORMAL"]), [(0., 0., 1.)] * 3)
            self.assertEqual(model.values(attrs["TEXCOORD_0"]), [(0., 1.), (1., 1.), (0., 0.)])
            self.assertEqual(model.values(primitive["indices"]), [(0,), (1,), (2,)])
            self.assertEqual(model.data["images"], [{"uri": "Tree_Bark.png"}])
            self.assertEqual(model.data["buffers"][0]["byteLength"], len((out / "Triangle.bin").read_bytes()))
            self.assertTrue(all(v["byteOffset"] % 4 == 0 for v in model.data["bufferViews"]))


if __name__ == "__main__":
    unittest.main()
