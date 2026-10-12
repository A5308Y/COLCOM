#!/usr/bin/env python3
"""Converts the Quaternius OBJ packs in asset-sources/ to glTF in assets/models/.

    python3 tools/obj_to_gltf.py

Each OBJ becomes a .gltf with a .bin next to it: one mesh, one primitive per
material. Materials take their colour from the MTL's diffuse colour (Blender
writes it linear, as glTF wants). The textured trees' MTL files don't name
their textures, so materials are matched to texture files by name
(`TEXTURES`); leaves are cut out by their alpha channel and drawn from both
sides. Faces with more than three corners are split into triangle fans.

Only the standard library is used (and macOS `sips` to turn the one JPEG
texture into PNG, which Bevy reads without extra features).
"""

import json
import os
import shutil
import struct
import subprocess
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
SRC = os.path.join(ROOT, "asset-sources", "quaternius")
DST = os.path.join(ROOT, "assets", "models")

# (OBJ folder in the sources, output folder, texture folder or None)
PACKS = [
    ("Ultimate Nature Pack - Jun 2019/OBJ", "nature", None),
    ("Textured Stylized Trees - May 2020/OBJ", "trees", "Textured Stylized Trees - May 2020/Textures"),
    ("Nature Crops Pack - Jan 2020/OBJ", "crops", None),
    ("Medieval Village Pack - Dec 2020/Buildings/OBJ", "village/buildings", None),
    ("Medieval Village Pack - Dec 2020/Props/OBJ", "village/props", None),
]

# Textured trees: material name -> (texture file, cut out by alpha)
TEXTURES = {
    "Bark": ("Tree_Bark.png", False),
    "Birch_Bark": ("Birch_Bark.png", False),
    "Birch_Leaves": ("Birch_Leaves_Green.png", True),
    "Pine_Leaves": ("Pine_Leaves.png", True),
    "Tree_Leaves": ("Tree_Leaves.png", True),
}

FLOAT, UINT32 = 5126, 5125
ARRAY_BUFFER, ELEMENT_ARRAY_BUFFER = 34962, 34963


def read_mtl(path):
    mats, name = {}, None
    if not os.path.exists(path):
        return mats
    for line in open(path):
        parts = line.split()
        if not parts:
            continue
        if parts[0] == "newmtl":
            name = " ".join(parts[1:])
            mats[name] = {"kd": [0.8, 0.8, 0.8], "d": 1.0}
        elif name and parts[0] == "Kd":
            mats[name]["kd"] = [float(v) for v in parts[1:4]]
        elif name and parts[0] == "d":
            mats[name]["d"] = float(parts[1])
    return mats


def read_obj(path):
    """Vertex data and faces grouped by material: {material: [[(v, vt, vn), ...], ...]}."""
    v, vt, vn = [], [], []
    faces, mtllib, mat = {}, None, "Default"
    for line in open(path):
        parts = line.split()
        if not parts:
            continue
        tag = parts[0]
        if tag == "v":
            v.append([float(x) for x in parts[1:4]])
        elif tag == "vt":
            vt.append([float(x) for x in parts[1:3]])
        elif tag == "vn":
            vn.append([float(x) for x in parts[1:4]])
        elif tag == "usemtl":
            mat = " ".join(parts[1:])
        elif tag == "mtllib":
            mtllib = " ".join(parts[1:])
        elif tag == "f":
            corners = []
            for c in parts[1:]:
                idx = (c.split("/") + ["", ""])[:3]
                fix = lambda s, n: (int(s) - 1 if int(s) > 0 else n + int(s)) if s else None
                corners.append((fix(idx[0], len(v)), fix(idx[1], len(vt)), fix(idx[2], len(vn))))
            faces.setdefault(mat, []).append(corners)
    return v, vt, vn, faces, mtllib


def face_normal(a, b, c):
    u = [b[i] - a[i] for i in range(3)]
    w = [c[i] - a[i] for i in range(3)]
    n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]]
    length = sum(x * x for x in n) ** 0.5 or 1.0
    return [x / length for x in n]


def convert(obj_path, out_dir, textures):
    name = os.path.splitext(os.path.basename(obj_path))[0]
    v, vt, vn, faces, mtllib = read_obj(obj_path)
    mats = read_mtl(os.path.join(os.path.dirname(obj_path), mtllib or name + ".mtl"))
    blob = bytearray()
    gltf = {
        "asset": {"version": "2.0", "generator": "my-world tools/obj_to_gltf.py"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": name, "mesh": 0}],
        "meshes": [{"name": name, "primitives": []}],
        "materials": [],
        "accessors": [],
        "bufferViews": [],
        "buffers": [],
    }
    used_textures = {}

    def add_view(data, target):
        while len(blob) % 4:
            blob.append(0)
        gltf["bufferViews"].append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        return len(gltf["bufferViews"]) - 1

    def add_accessor(view, ctype, count, kind, lo=None, hi=None):
        acc = {"bufferView": view, "componentType": ctype, "count": count, "type": kind}
        if lo is not None:
            acc["min"], acc["max"] = lo, hi
        gltf["accessors"].append(acc)
        return len(gltf["accessors"]) - 1

    for mat_name, polys in faces.items():
        textured = mat_name in TEXTURES and textures is not None
        lookup, pos, nrm, uv, idx = {}, [], [], [], []
        for poly in polys:
            # Flat faces without normals get the face's normal.
            fallback = face_normal(v[poly[0][0]], v[poly[1][0]], v[poly[2][0]]) if len(poly) >= 3 else [0, 1, 0]
            corner_ids = []
            for vi, ti, ni in poly:
                key = (vi, ti if textured else None, ni)
                if key not in lookup:
                    lookup[key] = len(pos)
                    pos.append(v[vi])
                    nrm.append(vn[ni] if ni is not None else fallback)
                    if textured:
                        t = vt[ti] if ti is not None else [0.0, 0.0]
                        uv.append([t[0], 1.0 - t[1]])
                corner_ids.append(lookup[key])
            for k in range(1, len(corner_ids) - 1):
                idx.extend([corner_ids[0], corner_ids[k], corner_ids[k + 1]])
        if not idx:
            continue
        attrs = {}
        flat = [c for p in pos for c in p]
        view = add_view(struct.pack("<%df" % len(flat), *flat), ARRAY_BUFFER)
        lo = [min(p[i] for p in pos) for i in range(3)]
        hi = [max(p[i] for p in pos) for i in range(3)]
        attrs["POSITION"] = add_accessor(view, FLOAT, len(pos), "VEC3", lo, hi)
        flat = [c for n in nrm for c in n]
        attrs["NORMAL"] = add_accessor(add_view(struct.pack("<%df" % len(flat), *flat), ARRAY_BUFFER), FLOAT, len(nrm), "VEC3")
        if textured:
            flat = [c for t in uv for c in t]
            attrs["TEXCOORD_0"] = add_accessor(add_view(struct.pack("<%df" % len(flat), *flat), ARRAY_BUFFER), FLOAT, len(uv), "VEC2")
        indices = add_accessor(add_view(struct.pack("<%dI" % len(idx), *idx), ELEMENT_ARRAY_BUFFER), UINT32, len(idx), "SCALAR")

        m = mats.get(mat_name, {"kd": [0.8, 0.8, 0.8], "d": 1.0})
        material = {
            "name": mat_name,
            "pbrMetallicRoughness": {"baseColorFactor": m["kd"] + [m["d"]], "metallicFactor": 0.0, "roughnessFactor": 0.9},
        }
        if textured:
            tex_file, cutout = TEXTURES[mat_name]
            if tex_file not in used_textures:
                gltf.setdefault("images", []).append({"uri": tex_file})
                gltf.setdefault("textures", []).append({"source": len(gltf["images"]) - 1})
                used_textures[tex_file] = len(gltf["textures"]) - 1
            material["pbrMetallicRoughness"]["baseColorFactor"] = [1.0, 1.0, 1.0, 1.0]
            material["pbrMetallicRoughness"]["baseColorTexture"] = {"index": used_textures[tex_file]}
            if cutout:
                material["alphaMode"] = "MASK"
                material["alphaCutoff"] = 0.5
                material["doubleSided"] = True
        gltf["materials"].append(material)
        gltf["meshes"][0]["primitives"].append(
            {"attributes": attrs, "indices": indices, "material": len(gltf["materials"]) - 1}
        )

    for key in ("images", "textures"):
        if key in gltf and not gltf[key]:
            del gltf[key]
    gltf["buffers"].append({"uri": name + ".bin", "byteLength": len(blob)})
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, name + ".bin"), "wb") as f:
        f.write(blob)
    with open(os.path.join(out_dir, name + ".gltf"), "w") as f:
        json.dump(gltf, f, separators=(",", ":"))


def copy_textures(tex_dir, out_dir):
    for f in sorted(os.listdir(tex_dir)):
        src = os.path.join(tex_dir, f)
        if f.lower().endswith(".png"):
            shutil.copy2(src, os.path.join(out_dir, f))
        elif f.lower().endswith((".jpg", ".jpeg")):
            png = os.path.join(out_dir, os.path.splitext(f)[0] + ".png")
            if os.path.exists(png):
                continue
            done = subprocess.run(["sips", "-s", "format", "png", src, "--out", png], capture_output=True)
            if done.returncode != 0:
                print(f"Couldn't convert {f} to PNG; run this, then the textures are complete:", file=sys.stderr)
                print(f'  sips -s format png "{os.path.normpath(src)}" --out "{os.path.normpath(png)}"', file=sys.stderr)


def main():
    total = 0
    for src, out, tex in PACKS:
        src_dir = os.path.join(SRC, src)
        out_dir = os.path.join(DST, out)
        if not os.path.isdir(src_dir):
            print("missing", src_dir, file=sys.stderr)
            continue
        os.makedirs(out_dir, exist_ok=True)
        if tex:
            copy_textures(os.path.join(SRC, tex), out_dir)
        objs = sorted(f for f in os.listdir(src_dir) if f.lower().endswith(".obj"))
        for f in objs:
            convert(os.path.join(src_dir, f), out_dir, tex)
        total += len(objs)
        print(f"{out}: {len(objs)} models")
    print("converted", total, "models")


if __name__ == "__main__":
    main()
