#!/usr/bin/env python3
"""CPU geometry previews when Metal is unavailable (not screenshots of the game).

    python3 tools/preview_rifle.py --out target/rifle-review

Uses the same source geometry as make_rifle.py. No third-party dependencies.
The game remains the authority for PBR, shadows, grip and animation review.
"""

import argparse
import math
from pathlib import Path
import struct
import sys
import zlib

sys.dont_write_bytecode = True
from make_rifle import PALETTE, Rifle


from asset_helpers import cross, dot, unit


def render(rifle, path, eye, focus=(0, .08, .175), scale=1040):
    width, height = 1200, 520
    pixels = bytearray(bytes((47, 53, 56)) * width * height)
    depths = [-math.inf] * (width * height)
    eye = unit(eye)
    right = unit(cross((0, 1, 0), eye))
    up = cross(eye, right)
    light = unit((-0.5, 0.8, 0.5))

    def project(v):
        p = tuple(v[i] - focus[i] for i in range(3))
        return (width / 2 + dot(p, right) * scale, height / 2 - dot(p, up) * scale, dot(p, eye))

    def edge(a, b, x, y):
        return (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])

    for material, (positions, normals, indices) in enumerate(rifle.groups):
        for k in range(0, len(indices), 3):
            ids = indices[k:k + 3]
            normal = normals[ids[0]]
            # Cull by geometry; cloth can have interpolated vertex normals.
            points = [positions[i] for i in ids]
            geometric = cross(tuple(points[1][i] - points[0][i] for i in range(3)),
                              tuple(points[2][i] - points[0][i] for i in range(3)))
            if dot(geometric, eye) <= 0:
                continue
            vertex_normals = [normals[i] for i in ids]
            smooth = not all(n == normal for n in vertex_normals)
            a, b, c = [project(positions[i]) for i in ids]
            area = edge(a, b, *c[:2])
            if abs(area) < 1e-9:
                continue
            shade = math.sqrt(.22 + .78 * max(0, dot(normal, light)))
            color = bytes(min(255, round(255 * x * shade)) for x in rifle.palette[material][1])
            x0, x1 = max(0, math.floor(min(a[0], b[0], c[0]))), min(width - 1, math.ceil(max(a[0], b[0], c[0])))
            y0, y1 = max(0, math.floor(min(a[1], b[1], c[1]))), min(height - 1, math.ceil(max(a[1], b[1], c[1])))
            for y in range(y0, y1 + 1):
                for x in range(x0, x1 + 1):
                    w0, w1 = edge(b, c, x + .5, y + .5) / area, edge(c, a, x + .5, y + .5) / area
                    w2 = 1 - w0 - w1
                    if min(w0, w1, w2) < -1e-9:
                        continue
                    depth = a[2] * w0 + b[2] * w1 + c[2] * w2
                    index = y * width + x
                    if depth > depths[index]:
                        if smooth:
                            n = tuple(sum(vertex_normals[j][i] * w for j, w in enumerate((w0, w1, w2))) for i in range(3))
                            shade = math.sqrt(.22 + .78 * max(0, dot(unit(n), light)))
                            color = bytes(min(255, round(255 * v * shade)) for v in rifle.palette[material][1])
                        depths[index] = depth
                        pixels[index * 3:index * 3 + 3] = color

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    raw = b"".join(b"\0" + pixels[y * width * 3:(y + 1) * width * 3] for y in range(height))
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    path.write_bytes(png + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/rifle-review"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    rifle = Rifle().build()
    for name, eye in [("side", (-1, .08, .03)), ("quarter", (-1, .4, .65))]:
        render(rifle, args.out / f"rifle-{name}.png", eye)
    # Same silhouette and proportions as the two original stored-item parts,
    # rotated to +Z so both previews use the same camera and scale.
    before = Rifle()
    before.box((0, .04, .05), (.06, .06, .6), 1, bevel=0)
    before.box((0, .05, -.3), (.07, .08, .22), 0, bevel=0)
    # Original wood and iron palette, only for the comparison.
    original_palette = list(PALETTE)
    PALETTE[0] = ("Wood", (.55, .37, .2), 0, .85)
    PALETTE[1] = ("Iron", (.22, .23, .25), 0, .85)
    render(before, args.out / "rifle-before.png", (-1, .4, .65))
    PALETTE[:] = original_palette
    print(f"Geometry previews: {args.out}")


if __name__ == "__main__":
    main()
