#!/usr/bin/env python3
"""Review clothing on the actual outfit in sampled idle/aim/fire animations.

python3 tools/preview_clothes_poses.py --out target/clothes-poses
CPU skinning with the game's animation library; textures/lighting are omitted.
"""
import argparse
import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from gltf_pose import Gltf, deform, matrix, multiply, point, unit
from make_gear import Gear
from make_rifle import PALETTE, Rifle
from preview_gear import write_gallery

ROOT = Path(__file__).resolve().parents[1]


def posed(sex, coat, clip, time):
    outfit = Gltf(ROOT / f"assets/models/characters/outfits/{sex}_Peasant.gltf")
    garment = Gltf(ROOT / f"assets/models/items/{coat}{sex}.gltf")
    head = Gltf(ROOT / f"assets/models/characters/base/Superhero_{sex}_Head.gltf")
    animation = Gltf(ROOT / "assets/models/characters/animations/UAL1_Standard.glb")
    pose = animation.sample(clip, time)
    colors = [tuple(pow(v, 1/2.2) for v in m["pbrMetallicRoughness"].get("baseColorFactor", [1, 1, 1])[:3])
              for m in garment.data["materials"][:3]]
    palette = [("Fabric", colors[0], 0, .9), ("Trim", colors[1], 0, .9), ("Seams", colors[2], 0, .9),
               ("Skin without textures", (.52, .40, .32), 0, .9), ("Trousers and boots", (.23, .26, .27), 0, .9)] + list(PALETTE)
    result = Gear(palette)

    def add(source, mesh, material):
        transforms = source.skin_matrices(pose=pose)
        for primitive in source.data["meshes"][mesh]["primitives"]:
            attrs = {key: source.values(value) for key, value in primitive["attributes"].items()}
            positions, normals = deform(attrs["POSITION"], attrs["NORMAL"], attrs["JOINTS_0"], attrs["WEIGHTS_0"], transforms)
            indices = [i[0] for i in source.values(primitive["indices"])]
            used = sorted(set(indices))
            remap = {j: i for i, j in enumerate(used)}
            mat = primitive["material"] if material is None else material
            dst = result.groups[mat]
            offset = len(dst[0])
            dst[0].extend(positions[i] for i in used)
            dst[1].extend(normals[i] for i in used)
            dst[2].extend(offset + remap[i] for i in indices)

    add(garment, 0, None)
    add(garment, 1, 3)
    for i, mesh in enumerate(outfit.data["meshes"]):
        if mesh["name"].endswith(("_Legs", "_Feet")):
            add(outfit, i, 4)
    for i, mesh in enumerate(head.data["meshes"]):
        add(head, i, 2 if "Face" in mesh.get("name", "") else 3)

    if clip != "Idle_Loop":
        globals = outfit.globals(pose)
        hand = next(i for i, n in enumerate(outfit.data["nodes"]) if n.get("name") == "hand_r")
        aim = unit((-.007, .77, .638, -.025))
        grip = matrix({"rotation": [-aim[0], -aim[1], -aim[2], aim[3]]})
        transform = multiply(globals[hand], grip)
        for dst, (positions, normals, indices) in zip(result.groups[5:], Rifle().build().groups):
            offset = len(dst[0])
            dst[0].extend(point(transform, p) for p in positions)
            dst[1].extend(unit(point(transform, n, 0)) for n in normals)
            dst[2].extend(i + offset for i in indices)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/clothes-poses"))
    args = parser.parse_args()
    entries = []
    for sex in ("Male", "Female"):
        for coat in ("SimpleClothes", "Parka"):
            for label, clip, time in [("Idle", "Idle_Loop", .5), ("Aim", "Pistol_Idle_Loop", .35), ("Fire", "Pistol_Shoot", .12)]:
                geometry = posed(sex, coat, clip, time)
                entries.append(("armor", f"{coat}-{sex}-{label}", geometry, None))
                if label == "Aim":
                    entries.append(("armor", f"{coat}-{sex}-Aim-Side", Gear(geometry.palette).add(geometry, turn=(0, -70, 0)), None))
    write_gallery(entries, args.out, "clothing in gun poses", "Actual Peasant outfit, character head, shared skeleton animation and rifle grip. CPU skinning; textures and game lighting omitted. Counts include native body parts and rifle.")
