#!/usr/bin/env python3
"""Cuts head-only versions of the Quaternius base characters.

The free character pack has the head and body as one mesh, so wearing an
outfit over the base character puts two bodies inside each other. This keeps
only the body triangles bound mostly to the head and neck bones, writing
`<name>_Head.gltf` next to each base character. The new file shares the
original `.bin` (vertex data, skin) and adds a small `.bin` with the new
triangle list; eyes and eyebrows are kept as they are.

    python3 tools/make_heads.py
"""

import json
import os
import struct

BASE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "models", "characters", "base")
CHARACTERS = ["Superhero_Male_FullBody", "Superhero_Female_FullBody"]
# Bones whose vertices make up the head (and the neck, which collars cover from below).
HEAD_BONES = {"Head", "neck_01"}
# A vertex belongs to the head if at least this share of its weight is on head bones.
THRESHOLD = 0.5

SIZES = {5121: 1, 5123: 2, 5125: 4, 5126: 4}
FORMATS = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}
COUNTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def read_accessor(g, blob, index):
    a = g["accessors"][index]
    view = g["bufferViews"][a["bufferView"]]
    n = COUNTS[a["type"]]
    ctype = a["componentType"]
    stride = view.get("byteStride", SIZES[ctype] * n)
    start = view.get("byteOffset", 0) + a.get("byteOffset", 0)
    out = []
    for i in range(a["count"]):
        values = struct.unpack_from("<%d%s" % (n, FORMATS[ctype]), blob, start + i * stride)
        out.append(values if n > 1 else values[0])
    return out


def make_head(name):
    path = os.path.join(BASE, name + ".gltf")
    g = json.load(open(path))
    assert len(g["buffers"]) == 1, "expects one buffer"
    blob = open(os.path.join(BASE, g["buffers"][0]["uri"]), "rb").read()
    joint_names = [g["nodes"][j]["name"] for j in g["skins"][0]["joints"]]
    head_joints = {k for k, n in enumerate(joint_names) if n in HEAD_BONES}

    # The body is the mesh with the most triangles (eyes and eyebrows are small).
    body = max(range(len(g["meshes"])), key=lambda m: sum(g["accessors"][p["indices"]]["count"] for p in g["meshes"][m]["primitives"]))
    extra = bytearray()
    kept_total = 0
    for prim in g["meshes"][body]["primitives"]:
        joints = read_accessor(g, blob, prim["attributes"]["JOINTS_0"])
        weights = read_accessor(g, blob, prim["attributes"]["WEIGHTS_0"])
        head = [sum(w for j, w in zip(js, ws) if j in head_joints) >= THRESHOLD for js, ws in zip(joints, weights)]
        indices = read_accessor(g, blob, prim["indices"])
        kept = []
        for t in range(0, len(indices), 3):
            tri = indices[t : t + 3]
            if all(head[i] for i in tri):
                kept.extend(tri)
        kept_total += len(kept) // 3
        while len(extra) % 4:
            extra.append(0)
        g["bufferViews"].append({"buffer": 1, "byteOffset": len(extra), "byteLength": len(kept) * 4, "target": 34963})
        extra.extend(struct.pack("<%dI" % len(kept), *kept))
        g["accessors"].append({"bufferView": len(g["bufferViews"]) - 1, "componentType": 5125, "count": len(kept), "type": "SCALAR"})
        prim["indices"] = len(g["accessors"]) - 1

    head_name = name.replace("_FullBody", "") + "_Head"
    g["buffers"].append({"uri": head_name + ".bin", "byteLength": len(extra)})
    g["meshes"][body]["name"] = head_name
    with open(os.path.join(BASE, head_name + ".bin"), "wb") as f:
        f.write(extra)
    with open(os.path.join(BASE, head_name + ".gltf"), "w") as f:
        json.dump(g, f, separators=(",", ":"))
    print(f"{head_name}: kept {kept_total} of the body's triangles")


if __name__ == "__main__":
    for c in CHARACTERS:
        make_head(c)
