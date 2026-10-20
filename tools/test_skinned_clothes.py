"""Geometry regressions for worn clothing; run with python3 -B tools/test_skinned_clothes.py."""
from collections import defaultdict
import math
from pathlib import Path
import unittest

from gltf_pose import Gltf, deform

ROOT = Path(__file__).resolve().parents[1] / "assets/models"


class ClothingGeometry(unittest.TestCase):
    def test_bare_arms_are_connected_without_detached_wraps(self):
        for sex in ("Male", "Female"):
            for coat in ("SimpleClothes", "Parka"):
                with self.subTest(sex=sex, coat=coat):
                    model = Gltf(ROOT / f"items/{coat}{sex}.gltf")
                    groups = defaultdict(set)
                    for primitive in model.data["meshes"][1]["primitives"]:
                        positions = model.values(primitive["attributes"]["POSITION"])
                        indices = [i[0] for i in model.values(primitive["indices"])]
                        keys = [tuple(round(v, 5) for v in p) for p in positions]
                        for k in range(0, len(indices), 3):
                            triangle = [keys[i] for i in indices[k:k+3]]
                            for point in triangle:
                                groups[point].update(triangle)
                    remaining = set(groups)
                    components = []
                    while remaining:
                        stack = [remaining.pop()]
                        component = set(stack)
                        while stack:
                            for neighbor in groups[stack.pop()] & remaining:
                                remaining.remove(neighbor)
                                component.add(neighbor)
                                stack.append(neighbor)
                        components.append(component)
                    self.assertEqual(len(components), 2, "each bare arm is one continuous surface")
                    self.assertTrue(all(len(c) > 500 for c in components))

    def test_hands_preserve_source_geometry_and_animation(self):
        animation = Gltf(ROOT / "characters/animations/UAL1_Standard.glb")
        for sex in ("Male", "Female"):
            path = f"outfits/{sex}_Peasant" if sex == "Male" else f"base/Superhero_{sex}_FullBody"
            source = Gltf(ROOT / f"characters/{path}.gltf")
            names = [source.data["nodes"][j]["name"] for j in source.data["skins"][0]["joints"]]
            hands = {i for i, name in enumerate(names) if name.startswith(
                ("hand_", "thumb_", "index_", "middle_", "ring_", "pinky_"))}
            primitives = [p for m in source.data["meshes"] for p in m["primitives"]
                          if source.data["materials"][p["material"]]["name"].startswith(("MI_Regular", "MI_Superhero"))]
            for coat in ("SimpleClothes", "Parka"):
                garment = Gltf(ROOT / f"items/{coat}{sex}.gltf")
                for clip, time in [("Idle_Loop", .5), ("Pistol_Idle_Loop", .35), ("Pistol_Shoot", .12)]:
                    with self.subTest(sex=sex, coat=coat, clip=clip):
                        pose = animation.sample(clip, time)
                        actual = defaultdict(list)
                        for primitive in garment.data["meshes"][1]["primitives"]:
                            attrs = {k: garment.values(v) for k, v in primitive["attributes"].items()}
                            positions, _ = deform(attrs["POSITION"], attrs["NORMAL"], attrs["JOINTS_0"], attrs["WEIGHTS_0"], garment.skin_matrices(pose=pose))
                            for uv, p in zip(attrs["TEXCOORD_0"], positions):
                                actual[uv].append(p)
                        checked = 0
                        for primitive in primitives:
                            attrs = {k: source.values(v) for k, v in primitive["attributes"].items()}
                            positions, _ = deform(attrs["POSITION"], attrs["NORMAL"], attrs["JOINTS_0"], attrs["WEIGHTS_0"], source.skin_matrices(pose=pose))
                            used = {i[0] for i in source.values(primitive["indices"])}
                            for i in used:
                                if sum(w for j, w in zip(attrs["JOINTS_0"][i], attrs["WEIGHTS_0"][i]) if j in hands) <= .5:
                                    continue
                                matches = actual[attrs["TEXCOORD_0"][i]]
                                self.assertTrue(matches, "a source hand vertex was removed")
                                self.assertLess(min(math.dist(positions[i], p) for p in matches), 1e-5)
                                checked += 1
                        self.assertGreater(checked, 500)


if __name__ == "__main__":
    unittest.main()
