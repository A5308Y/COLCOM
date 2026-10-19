#!/usr/bin/env python3
"""Generate the original, texture-free frontier military rifle. Standard library only.

Run from anywhere: python3 tools/make_rifle.py
Metres, +Y up, +Z muzzle; origin is the right-hand grip. See the asset README.
"""

import json
import math
from pathlib import Path
import struct

OUT = Path(__file__).resolve().parents[1] / "assets/models/weapons"
# sRGB swatches; glTF factors are linear. (name, color, metallic, roughness)
PALETTE = [
    ("Olive ceramic", (0.36, 0.39, 0.27), 0.12, 0.82),
    ("Parkerized steel", (0.25, 0.28, 0.29), 0.72, 0.63),
    ("Charcoal polymer", (0.12, 0.14, 0.14), 0.0, 0.92),
    ("Exposed edges", (0.48, 0.50, 0.46), 0.65, 0.67),
    ("Amber optic", (0.90, 0.47, 0.12), 0.15, 0.4),
]


def linear(v):
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


class Rifle:
    def __init__(self):
        self.groups = [[[], [], []] for _ in PALETTE]

    def face(self, points, material):
        positions, normals, indices = self.groups[material]
        a, b, c = points[:3]
        u, v = [b[i] - a[i] for i in range(3)], [c[i] - a[i] for i in range(3)]
        n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
        length = math.sqrt(sum(x * x for x in n))
        assert length > 1e-10
        start = len(positions)
        positions.extend(points)
        normals.extend([[x / length for x in n]] * len(points))
        for i in range(1, len(points) - 1):
            indices.extend((start, start + i, start + i + 1))

    def box(self, center, size, material, bevel=0.006, tilt=0, worn=False):
        """Chamfered rectangular extrusion; broad planes, narrow edge highlights."""
        x, y, z = [v / 2 for v in size]
        b = min(bevel, x * 0.4, y * 0.4, z * 0.4)
        rings = []
        for depth, inset in [(-z, 0), (z, 0)]:
            xx, yy = x - inset, y - inset
            ring = [(xx - b, -yy), (xx, -yy + b), (xx, yy - b), (xx - b, yy),
                    (-xx + b, yy), (-xx, yy - b), (-xx, -yy + b), (-xx + b, -yy)]
            if b == 0:
                ring = [(xx, -yy), (xx, yy), (-xx, yy), (-xx, -yy)]
            angle = math.radians(tilt)
            rings.append([(center[0] + px,
                           center[1] + py * math.cos(angle) - depth * math.sin(angle),
                           center[2] + py * math.sin(angle) + depth * math.cos(angle)) for px, py in ring])
        self.face(list(reversed(rings[0])), material)
        self.face(rings[-1], material)
        for j in range(len(rings) - 1):
            for i in range(len(rings[0])):
                k = (i + 1) % len(rings[0])
                edge = 3 if worn and i in (2, 4) else material
                self.face([rings[j][i], rings[j][k], rings[j + 1][k], rings[j + 1][i]], edge)

    def profile(self, points, width, material):
        """Extrude a convex counterclockwise silhouette in the Y/Z plane."""
        area = sum(points[i][0] * points[(i + 1) % len(points)][1]
                   - points[(i + 1) % len(points)][0] * points[i][1] for i in range(len(points)))
        if area < 0:
            points = list(reversed(points))
        rings = [[(x, y, z) for y, z in points] for x in (-width / 2, width / 2)]
        self.face(list(reversed(rings[0])), material)
        self.face(rings[1], material)
        for i in range(len(points)):
            j = (i + 1) % len(points)
            self.face([rings[0][i], rings[0][j], rings[1][j], rings[1][i]], material)

    def tube(self, y, z0, z1, radius, material, bore=0, sides=8):
        rings = [[(math.cos(i * math.tau / sides) * r, y + math.sin(i * math.tau / sides) * r, z)
                  for i in range(sides)] for z, r in [(z0, radius), (z1, radius), (z1, bore), (z0, bore)]]
        for a, b in [(0, 1), (1, 2), (2, 3), (3, 0)] if bore else [(0, 1)]:
            for i in range(sides):
                j = (i + 1) % sides
                self.face([rings[a][i], rings[a][j], rings[b][j], rings[b][i]], material)
        if not bore:
            self.face(list(reversed(rings[0])), material)
            self.face(rings[1], material)

    def build(self):
        # Receiver and overhanging olive shell. Grip stays at the origin.
        self.box((0, .103, .08), (.084, .112, .34), 1, worn=True)
        self.box((0, .161, .08), (.096, .055, .36), 0)
        self.box((0, .049, .067), (.076, .042, .23), 2)
        # Shoulder stock: sloped neck, cheek rest, and padded butt.
        self.profile([(.06, -.31), (.035, -.19), (.092, -.06), (.158, -.06), (.178, -.31)], .065, 0)
        self.box((0, .165, -.224), (.076, .037, .19), 2)
        self.box((0, .103, -.324), (.082, .175, .025), 2)
        self.box((0, .075, -.16), (.042, .026, .13), 1)
        # Slightly swept pistol grip and heel, separate from the magazine.
        self.box((0, -.031, -.025), (.059, .168, .068), 2, tilt=17)
        self.box((0, -.104, -.047), (.066, .022, .074), 1, tilt=17)
        for y in (-.025, -.050, -.075):
            z = -.025 + (y + .031) * math.tan(math.radians(17)) - .032
            self.box((0, y, z), (.061, .009, .009), 1, bevel=.002, tilt=17)
        # Trigger guard leaves a real opening below the receiver.
        self.box((0, -.035, .067), (.035, .014, .105), 1, bevel=.003)
        self.box((0, .005, .111), (.035, .08, .014), 1, bevel=.003)
        self.box((0, .008, .042), (.013, .039, .013), 3, bevel=.002, tilt=-15)
        # Detachable magazine with a broad base and two pressed ribs.
        self.box((0, -.036, .165), (.062, .182, .092), 1, tilt=10, worn=True)
        self.box((0, -.12, .15), (.074, .023, .106), 2, tilt=10)
        for side in (-1, 1):
            for z in (.144, .181):
                self.box((side * .032, -.028, z), (.006, .118, .009), 2, bevel=.001, tilt=10)
        # Fore-end shroud, dark vent recesses, and its supporting lower rail.
        self.box((0, .125, .35), (.091, .102, .27), 0)
        self.box((0, .065, .34), (.069, .022, .25), 2)
        for side in (-1, 1):
            for z in (.27, .325, .38, .435):
                self.box((side * .046, .137, z), (.005, .034, .024), 2, bevel=.002, tilt=-18)
            self.box((side * .043, .11, .066), (.006, .034, .096), 2, bevel=.002)
            self.box((side * .047, .111, .071), (.005, .012, .055), 3, bevel=.002)
        # Low rail and compact, open reflex sight (no transparent/glassy material).
        self.box((0, .195, .075), (.05, .016, .22), 1, bevel=.003)
        for z in (-.012, .02, .052, .084, .116, .148):
            self.box((0, .205, z), (.057, .01, .015), 2, bevel=.002)
        self.box((0, .221, .036), (.068, .021, .066), 1, bevel=.003)
        for side in (-1, 1):
            self.box((side * .029, .252, .036), (.013, .064, .043), 2, bevel=.003)
        self.box((0, .283, .036), (.069, .013, .043), 1, bevel=.003)
        for side in (-1, 1):
            self.box((side * .037, .233, .034), (.009, .012, .018), 4, bevel=.002)
        # Exposed barrel, collar, and hollow octagonal muzzle brake.
        self.tube(.127, .45, .625, .018, 1)
        self.tube(.127, .477, .501, .026, 3)
        self.tube(.127, .605, .678, .029, 1, bore=.013)
        for side in (-1, 1):
            self.box((side * .028, .127, .639), (.004, .017, .032), 2, bevel=.001)
        # Small plate seams and two visible fasteners, kept readable at game scale.
        for side in (-1, 1):
            for z in (-.044, .201):
                self.box((side * .044, .092, z), (.007, .014, .014), 3, bevel=.002)
        return self

    def write(self):
        blob = bytearray()
        gltf = {"asset": {"version": "2.0", "generator": "My World make_rifle.py"},
                "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [{"name": "Rifle", "mesh": 0}],
                "meshes": [{"name": "Rifle", "primitives": []}], "materials": [], "accessors": [], "bufferViews": []}

        def accessor(values, kind, ctype, target):
            while len(blob) % 4:
                blob.append(0)
            flat = [v for row in values for v in row] if kind == "VEC3" else values
            data = struct.pack(f"<{len(flat)}{'f' if ctype == 5126 else 'I'}", *flat)
            view = len(gltf["bufferViews"])
            gltf["bufferViews"].append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
            blob.extend(data)
            a = {"bufferView": view, "componentType": ctype, "count": len(values), "type": kind}
            if kind == "VEC3":
                a.update(min=[min(p[i] for p in values) for i in range(3)], max=[max(p[i] for p in values) for i in range(3)])
            gltf["accessors"].append(a)
            return len(gltf["accessors"]) - 1

        triangles = 0
        for i, (name, color, metal, rough) in enumerate(PALETTE):
            material = {"name": name, "pbrMetallicRoughness": {
                "baseColorFactor": [linear(c) for c in color] + [1], "metallicFactor": metal, "roughnessFactor": rough}}
            if i == 4:
                material["emissiveFactor"] = [.65, .22, .025]
            gltf["materials"].append(material)
            positions, normals, indices = self.groups[i]
            gltf["meshes"][0]["primitives"].append({"material": i, "attributes": {
                "POSITION": accessor(positions, "VEC3", 5126, 34962),
                "NORMAL": accessor(normals, "VEC3", 5126, 34962)},
                "indices": accessor(indices, "SCALAR", 5125, 34963)})
            triangles += len(indices) // 3
        gltf["buffers"] = [{"uri": "Rifle.bin", "byteLength": len(blob)}]
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "Rifle.bin").write_bytes(blob)
        (OUT / "Rifle.gltf").write_text(json.dumps(gltf, indent=2) + "\n")
        print(f"Rifle: {triangles} triangles, {len(PALETTE)} materials, {len(blob):,} bytes")


if __name__ == "__main__":
    Rifle().build().write()
