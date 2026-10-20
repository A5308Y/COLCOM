#!/usr/bin/env python3
"""CPU garment fit previews on both base character meshes; no textures or game lighting.

    python3 tools/preview_armor.py --out target/armor-fit-review
    python3 tools/preview_armor.py --clothing --out target/clothing-review
"""

import argparse
import json
import math
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
from make_gear import Gear, armor
from preview_gear import write_gallery

ROOT = Path(__file__).resolve().parents[1]
BONES = ["spine_03", "upperarm_l", "upperarm_r", "Head", "lowerarm_l", "lowerarm_r",
         "pelvis", "thigh_l", "thigh_r", "calf_l", "calf_r", "foot_l", "foot_r"]


def multiply(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def matrix(node):
    x, y, z, w = node.get("rotation", (0, 0, 0, 1))
    t, s = node.get("translation", (0, 0, 0)), node.get("scale", (1, 1, 1))
    m = [[1-2*y*y-2*z*z, 2*x*y-2*z*w, 2*x*z+2*y*w, t[0]],
         [2*x*y+2*z*w, 1-2*x*x-2*z*z, 2*y*z-2*x*w, t[1]],
         [2*x*z-2*y*w, 2*y*z+2*x*w, 1-2*x*x-2*y*y, t[2]], [0, 0, 0, 1]]
    for i in range(3):
        for j in range(3):
            m[i][j] *= s[j]
    return m


def fit_preview(body, powered=False, clothing=None):
    path = ROOT / f"assets/models/characters/base/Superhero_{body}_FullBody.gltf"
    source = json.loads(path.read_text())
    buffers = [path.with_name(b["uri"]).read_bytes() for b in source["buffers"]]
    transforms, names, arm_side = {}, {}, {}

    def visit(index, parent, side=0):
        node = source["nodes"][index]
        name = node.get("name", "")
        side = {"upperarm_l": 1, "upperarm_r": -1}.get(name, side)
        arm_side[index] = side
        transforms[index] = multiply(parent, matrix(node))
        names[name] = index
        for child in node.get("children", []):
            visit(child, transforms[index], side)

    identity = [[int(i == j) for j in range(4)] for i in range(4)]
    for node in source["scenes"][0]["nodes"]:
        visit(node, identity)

    def origin(bone):
        return tuple(transforms[names[bone]][i][3] for i in range(3))

    def pose(v, side, normal=False):
        # Lower the arms 70 degrees from the bind pose, keeping the shoulders fixed.
        if not side:
            return v
        at = (0, 0, 0) if normal else origin("upperarm_l" if side > 0 else "upperarm_r")
        a = math.radians(-70 * side)
        x, y, z = [v[i] - at[i] for i in range(3)]
        return (at[0] + x * math.cos(a) - y * math.sin(a),
                at[1] + x * math.sin(a) + y * math.cos(a), at[2] + z)

    def values(index):
        a = source["accessors"][index]
        view = source["bufferViews"][a["bufferView"]]
        fmt = "<" + {"SCALAR": 1, "VEC3": 3, "VEC4": 4}[a["type"]] * {5121: "B", 5123: "H", 5125: "I", 5126: "f"}[a["componentType"]]
        offset = view.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = view.get("byteStride", struct.calcsize(fmt))
        return [struct.unpack_from(fmt, buffers[view["buffer"]], offset + i * stride) for i in range(a["count"])]

    bones = BONES
    if clothing:
        from make_items import jacket, wool_hat
        if clothing == "wool_hat":
            assembled = wool_hat()
            pieces, bones = [("Hat", assembled, (0, 0, 0))], ["Head"]
        else:
            assembled, pieces = jacket(clothing == "parka")
            bones = ["spine_03", "upperarm_l", "upperarm_r", "lowerarm_l", "lowerarm_r"]
    else:
        assembled, pieces = armor(powered)
    palette = list(assembled.palette) + [("Neutral base mesh", (.28, .29, .30), 0, .9)]
    result = Gear(palette)
    for bone, (_, geometry, _) in zip(bones, pieces):
        at = origin(bone)
        side = arm_side[names[bone]]
        placed = Gear(palette)
        for dst, (positions, normals, indices) in zip(placed.groups, geometry.groups):
            dst[0].extend(pose(tuple(p[i] + at[i] for i in range(3)), side) for p in positions)
            dst[1].extend(pose(n, side, True) for n in normals)
            dst[2].extend(indices)
        result.add(placed)

    # Mesh vertices are already authored in their skin's bind frame. Use skin
    # weights only for the simple arm pose; armor uses the actual bone origins.
    for node in source["nodes"]:
        if "mesh" not in node:
            continue
        joints = source["skins"][node["skin"]]["joints"]
        for primitive in source["meshes"][node["mesh"]]["primitives"]:
            attributes = primitive["attributes"]
            positions, normals = values(attributes["POSITION"]), values(attributes["NORMAL"])
            weights, bindings = values(attributes["WEIGHTS_0"]), values(attributes["JOINTS_0"])
            dst = result.groups[-1]
            offset = len(dst[0])
            for p, n, weight, binding in zip(positions, normals, weights, bindings):
                posed = [pose(p, arm_side[joints[j]]) for j in binding]
                turned = [pose(n, arm_side[joints[j]], True) for j in binding]
                dst[0].append(tuple(sum(v[i] * w for v, w in zip(posed, weight)) for i in range(3)))
                dst[1].append(tuple(sum(v[i] * w for v, w in zip(turned, weight)) for i in range(3)))
            dst[2].extend(v[0] + offset for v in values(primitive["indices"]))
    if clothing == "wool_hat":
        # A close view makes the cap's fit and silhouette readable.
        positions, normals, indices = result.groups[-1]
        kept = [indices[i:i+3] for i in range(0, len(indices), 3)
                if all(positions[j][1] > origin("Head")[1] - .09 for j in indices[i:i+3])]
        used = sorted({j for triangle in kept for j in triangle})
        remap = {j: i for i, j in enumerate(used)}
        result.groups[-1] = [[positions[j] for j in used], [normals[j] for j in used],
                             [remap[j] for triangle in kept for j in triangle]]
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/armor-fit-review"))
    parser.add_argument("--clothing", action="store_true", help="Review everyday garments and the flak jacket")
    args = parser.parse_args()
    entries = []
    for body in ("Male", "Female"):
        if args.clothing:
            for garment, label in [("clothes", "SimpleClothes"), ("parka", "Parka"), ("wool_hat", "WoolHat"), (None, "FlakJacket")]:
                geometry = fit_preview(body, clothing=garment)
                entries.append(("armor", f"{label}-{body}", geometry, None))
                if garment != "wool_hat":
                    entries.append(("armor", f"{label}-{body}-Back", Gear(geometry.palette).add(geometry, turn=(0, 180, 0)), None))
            continue
        for powered, label in [(False, "FlakJacket"), (True, "PoweredArmor")]:
            geometry = fit_preview(body, powered)
            entries.append(("armor", f"{label}-{body}", geometry, None))
            if powered:
                entries.append(("armor", f"{label}-{body}-Back", Gear(geometry.palette).add(geometry, turn=(0, 180, 0)), None))
    title = "clothing &amp; flak jacket fit review" if args.clothing else "armor fit review"
    write_gallery(entries, args.out, title, "Actual male and female base meshes with neutral colors and lowered arms; textures omitted. Counts include the base body.")
