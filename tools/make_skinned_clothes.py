#!/usr/bin/env python3
"""Tailored, deforming clothes fitted to the CC0 Peasant outfit skeleton.

python3 tools/make_skinned_clothes.py
Blended torso/sleeve weights and matching bind poses keep the fabric aligned
with the outfit used in-game. Stored props and loading fallbacks stay separate.
"""
import copy
import json
import math
from pathlib import Path
import struct
import sys

sys.dont_write_bytecode = True
from gltf_pose import Gltf, unit
from make_items import palette
from make_items import Item
from make_rifle import linear

ROOT = Path(__file__).resolve().parents[1]


def build(sex, parka):
    source = Gltf(ROOT / f"assets/models/characters/outfits/{sex}_Peasant.gltf")
    g = source.data
    names = [g["nodes"][j]["name"] for j in g["skins"][0]["joints"]]
    name_index = {name: i for i, name in enumerate(names)}
    world = source.globals()
    origins = {g["nodes"][i]["name"]: tuple(m[j][3] for j in range(3)) for i, m in world.items()}
    spine_y = origins["spine_03"][1]
    female = sex == "Female"
    arms, samples = [], []
    for mesh in g["meshes"]:
        if not mesh["name"].endswith(("_Body", "_Arms")):
            continue
        for primitive in mesh["primitives"]:
            attrs = {k: source.values(v) for k, v in primitive["attributes"].items()}
            positions, js, ws = attrs["POSITION"], attrs["JOINTS_0"], attrs["WEIGHTS_0"]
            indices = [i[0] for i in source.values(primitive["indices"])]
            if mesh["name"].endswith("_Body"):
                samples.extend(zip(positions, js, ws))
                continue
            fitted = []
            for p, joints, weights in zip(positions, js, ws):
                p = list(p)
                # Native hands and bare forearms stay intact. Pull only the puffed
                # upper sleeves inside the tailored garment, with no added shell.
                for suffix in ("l", "r"):
                    amount = sum(w for j, w in zip(joints, weights) if names[j] == f"upperarm_{suffix}")
                    if amount < .03:
                        continue
                    _, y, z = origins[f"upperarm_{suffix}"]
                    dy, dz = p[1] - y, p[2] - z
                    r = max(1, math.hypot(dy / .053, dz / .064))
                    p[1], p[2] = y + dy / r, z + dz / r
                fitted.append(tuple(p))
            arms.append((attrs, fitted, indices, primitive["material"]))

    def weights_for(p):
        # Transfer torso weights onto the small sewn-on details.
        nearest = sorted(samples, key=lambda s: sum((a-b)**2 for a, b in zip(s[0], p)))[:4]
        total = {}
        for q, js, ws in nearest:
            influence = 1 / max(.00001, sum((a-b)**2 for a, b in zip(q, p)))
            for j, w in zip(js, ws):
                total[j] = total.get(j, 0) + w * influence
        return total

    def packed(weights):
        weights = sorted(((name_index[j] if isinstance(j, str) else j, w) for j, w in weights.items() if w > 0), key=lambda x: (-x[1], x[0]))[:4]
        total = sum(w for _, w in weights)
        return (tuple(j for j, _ in weights) + (0,) * (4-len(weights)),
                tuple(w/total for _, w in weights) + (0,) * (4-len(weights)))

    vertices, triangles, groups = [], [], []

    def surface(rings, ring_weights, materials, reverse=False):
        start, sides = len(vertices), len(rings[0])
        for k, ring in enumerate(rings):
            for i, p in enumerate(ring):
                # Smooth along the cut, with enough facets to match the other items.
                u = [ring[(i+1) % sides][j] - ring[(i-1) % sides][j] for j in range(3)]
                lo, hi = rings[max(0, k-1)][i], rings[min(len(rings)-1, k+1)][i]
                v = [hi[j] - lo[j] for j in range(3)]
                n = unit((u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]))
                if reverse:
                    n = tuple(-x for x in n)
                js, ws = packed(ring_weights(k, p))
                vertices.append((p, n, js, ws))
        for k in range(len(rings)-1):
            for i in range(sides):
                a, b = start+k*sides+i, start+k*sides+(i+1) % sides
                for tri in [(a, b, b+sides), (a, b+sides, a+sides)]:
                    triangles.append(tuple(reversed(tri)) if reverse else tri)
                    groups.append(materials[k])

    waist = origins["spine_01"][1]
    shoulder = origins["upperarm_l"]
    neck = origins["neck_01"]
    extra = .008 if parka else 0
    width = .91 if female else 1
    # Hem, waist, chest and shoulder are independent of the old tunic's skirt.
    cuts = [(waist-(.12 if parka else .035), (.207 if female else .197) / width if parka else .177, .130 if parka else .120, -.013),
            (waist-(.102 if parka else .019), (.207 if female else .197) / width if parka else .177, .130 if parka else .120, -.013),
            (waist+.045, .170, .116, -.005),
            (spine_y-.04, .183, .124, -.009),
            (spine_y+.065, .205, .133, -.020),
            (shoulder[1]+.025, .211, .116, -.036),
            (neck[1]+.012, .077 / width, .073-extra, neck[2]),
            (neck[1]+(.045 if parka else .025), .079 / width, .075-extra, neck[2])]
    rings = [[((rx*width+extra)*math.cos(i*math.tau/20), y,
               z-(rz+extra)*math.sin(i*math.tau/20)) for i in range(20)] for y, rx, rz, z in cuts]
    torso_bindings = [
        {"pelvis": .85 if parka else .2, "spine_01": .15 if parka else .8},
        {"pelvis": .85 if parka else .2, "spine_01": .15 if parka else .8},
        {"spine_01": .6, "spine_02": .4},
        {"spine_02": .3, "spine_03": .7},
        {"spine_03": 1}, {"spine_03": 1},
        {"spine_03": .85, "neck_01": .15}, {"spine_03": .85, "neck_01": .15},
    ]
    def torso_weights(k, p):
        return torso_bindings[k]
    surface(rings, torso_weights, [1, 0, 0, 0, 0, 0, 1])

    for suffix, side in [("l", 1), ("r", -1)]:
        start = origins[f"upperarm_{suffix}"]
        elbow = origins[f"lowerarm_{suffix}"]
        wrist = origins[f"hand_{suffix}"]
        sx, ex, wx = abs(start[0]), abs(elbow[0]), abs(wrist[0])
        # The seam starts inside the torso; a large fixed shoulder ring would
        # stick up into a point as soon as the upper arm rotates downward.
        sleeve_cuts = [(sx-.055, .046+extra, .069+extra),
                       (sx+.025, .077+extra, .088+extra),
                       ((sx+ex)/2, .068+extra, .079+extra),
                       (ex-.030, .061+extra, .073+extra),
                       (ex+.025, .060+extra, .072+extra)]
        if parka:
            sleeve_cuts.extend([((ex+wx)/2, .054+extra, .063+extra),
                                (wx-.034, .047+extra, .055+extra),
                                (wx-.018, .047+extra, .055+extra)])
        else:
            sleeve_cuts.append((ex+.048, .060, .072))
        sleeve_rings = []
        for x, ry, rz in sleeve_cuts:
            t = max(0, min(1, (x-sx)/(ex-sx)))
            z = start[2]*(1-t) + elbow[2]*t
            sleeve_rings.append([(side*x, start[1]+ry*math.cos(i*math.tau/16), z+rz*math.sin(i*math.tau/16)) for i in range(16)])
        def sleeve_weights(k, p):
            if k == 0:
                return {"spine_03": 1}
            if k == 1:
                return {"spine_03": .25, f"clavicle_{suffix}": .1, f"upperarm_{suffix}": .65}
            x = abs(p[0])
            t = max(0, min(1, (x-ex+.055)/.11))
            return {f"upperarm_{suffix}": 1-t, f"lowerarm_{suffix}": t}
        surface(sleeve_rings, sleeve_weights, [0]*(len(sleeve_cuts)-2)+[1], reverse=side < 0)

    # A narrow placket and two flat chest patches read as work clothes at game zoom.
    detail = Item(palette((.51, .37, .23)))
    front = .122 + extra
    detail.box((0, spine_y-.045, front+.008), (.014, .24, .007), 2, bevel=.002)
    for side in (-1, 1):
        detail.box((side*.095, spine_y+.005, front+.003), (.063, .068, .009), 1, bevel=.003)
    if parka:
        # Folded hood rests behind the neck and does not surround the raised arms.
        hood = Item(detail.palette)
        hood.section([(0, .099, .044, -.139), (.045, .108, .052, -.139),
                      (.082, .089, .037, -.139)], 0, sides=12, smooth=True)
        detail.add(hood, (0, neck[1]-.12, 0))
    for material, (positions, normals, indices) in enumerate(detail.groups[:3]):
        offset = len(vertices)
        for p, n in zip(positions, normals):
            js, ws = packed(weights_for(p))
            vertices.append((p, n, js, ws))
        for k in range(0, len(indices), 3):
            triangles.append(tuple(offset+i for i in indices[k:k+3]))
            groups.append(material)
    return source, vertices, triangles, arms, groups


def write(sex, parka):
    source, vertices, triangles, arms, materials = build(sex, parka)
    name = ("Parka" if parka else "SimpleClothes") + sex
    out = ROOT / "assets/models/items"
    blob = bytearray()
    g = {"asset": {"version": "2.0", "generator": "My World make_skinned_clothes.py; Quaternius CC0 tailoring"},
         "scene": 0, "scenes": [{"nodes": [68]}], "nodes": copy.deepcopy(source.data["nodes"][:65]),
         "meshes": [], "materials": [], "bufferViews": [], "accessors": []}

    def accessor(rows, kind, ctype=5126, target=34962):
        while len(blob) % 4:
            blob.append(0)
        flat = [v for row in rows for v in row]
        raw = struct.pack("<" + len(flat) * {5126: "f", 5123: "H", 5125: "I"}[ctype], *flat)
        g["bufferViews"].append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(raw), **({"target": target} if target else {})})
        a = {"bufferView": len(g["bufferViews"]) - 1, "componentType": ctype, "count": len(rows), "type": kind}
        if kind == "VEC3":
            a.update(min=[min(r[i] for r in rows) for i in range(3)], max=[max(r[i] for r in rows) for i in range(3)])
        g["accessors"].append(a)
        blob.extend(raw)
        return len(g["accessors"]) - 1

    pal = palette((.51, .37, .23) if parka else (.31, .38, .40), (.34, .30, .23) if parka else (.24, .29, .29))
    for label, color, metal, rough in pal[:3]:
        g["materials"].append({"name": label, "pbrMetallicRoughness": {"baseColorFactor": [linear(v) for v in color] + [1], "metallicFactor": metal, "roughnessFactor": rough}})
    g["materials"].extend(copy.deepcopy(source.data["materials"]))
    for key in ("images", "textures", "samplers"):
        if key in source.data:
            g[key] = copy.deepcopy(source.data[key])
    for img in g.get("images", []):
        img["uri"] = "../characters/outfits/" + img["uri"]
    attrs = {key: accessor([v[i] for v in vertices], kind, ctype) for i, key, kind, ctype in [
        (0, "POSITION", "VEC3", 5126), (1, "NORMAL", "VEC3", 5126), (2, "JOINTS_0", "VEC4", 5123), (3, "WEIGHTS_0", "VEC4", 5126)]}
    groups = [[], [], []]
    for tri, material in zip(triangles, materials):
        groups[material].extend((i,) for i in tri)
    g["meshes"].append({"name": name + "Fabric", "primitives": [
        {"attributes": attrs, "indices": accessor(ids, "SCALAR", 5125, 34963), "material": mat}
        for mat, ids in enumerate(groups) if ids]})
    primitives = []
    for native, positions, indices, mat in arms:
        attributes = {key: accessor(positions if key == "POSITION" else values, {"POSITION": "VEC3", "NORMAL": "VEC3", "TANGENT": "VEC4", "TEXCOORD_0": "VEC2", "JOINTS_0": "VEC4", "WEIGHTS_0": "VEC4"}[key], 5123 if key == "JOINTS_0" else 5126)
                      for key, values in native.items() if key in ("POSITION", "NORMAL", "TANGENT", "TEXCOORD_0", "JOINTS_0", "WEIGHTS_0")}
        primitives.append({"attributes": attributes, "indices": accessor([(i,) for i in indices], "SCALAR", 5125, 34963), "material": mat + 3})
    g["meshes"].append({"name": name + "NativeArms", "primitives": primitives})
    skin = copy.deepcopy(source.data["skins"][0])
    skin["inverseBindMatrices"] = accessor(source.values(skin["inverseBindMatrices"]), "MAT4", target=0)
    g["skins"] = [skin]
    g["nodes"].extend([{"name": "Armature", "children": [64]},
                       {"name": name + "Fabric", "mesh": 0, "skin": 0},
                       {"name": name + "NativeArms", "mesh": 1, "skin": 0},
                       {"name": name, "children": [65, 66, 67]}])
    g["buffers"] = [{"uri": name + ".bin", "byteLength": len(blob)}]
    (out / (name + ".bin")).write_bytes(blob)
    (out / (name + ".gltf")).write_text(json.dumps(g, indent=2) + "\n")
    print(f"{name}: {len(triangles)} new fabric triangles, {len(blob):,} bytes; native arms retain their original topology")


if __name__ == "__main__":
    for parka in (False, True):
        for sex in ("Male", "Female"):
            write(sex, parka)
