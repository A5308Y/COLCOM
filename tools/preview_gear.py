#!/usr/bin/env python3
"""CPU geometry gallery for all gear; not game screenshots. Standard library only.

    python3 tools/preview_gear.py --out target/gear-review
"""

import argparse
import base64
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from make_gear import models
from preview_rifle import cross, dot, render, unit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/gear-review"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    cards = []
    for folder, name, geometry, _ in models():
        eye = unit((-1, .35, .65) if folder == "weapons" else (-.65, .25, 1))
        right = unit(cross((0, 1, 0), eye))
        up = cross(eye, right)
        positions = [p for group in geometry.groups for p in group[0]]
        focus = [(min(p[i] for p in positions) + max(p[i] for p in positions)) / 2 for i in range(3)]
        spans = [max(dot(p, axis) for p in positions) - min(dot(p, axis) for p in positions) for axis in (right, up)]
        scale = min(1040 / spans[0], 420 / spans[1])
        path = args.out / (name + ".png")
        render(geometry, path, eye, focus, scale)
        image = base64.b64encode(path.read_bytes()).decode()
        count = sum(len(group[2]) // 3 for group in geometry.groups)
        cards.append(f'<figure><img src="data:image/png;base64,{image}" alt="{name}"><figcaption>{name} · {count:,} triangles</figcaption></figure>')
    page = '''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>My World — gear art review</title><style>
body{margin:0;padding:32px;background:#191e20;color:#e3e7dc;font:16px system-ui}
h1{font-size:28px}p{color:#b8c0b3}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:18px}
figure{margin:0;background:#2f3538;border:1px solid #444c48;border-radius:8px;overflow:hidden}
img{width:100%;display:block}figcaption{padding:14px 18px;background:#242b2c}</style>
<h1>My World · weapons &amp; armor</h1><p>Shared olive, steel, charcoal and amber palette. CPU geometry previews; in-game lighting and animations require GPU review.</p><main>'''
    (args.out / "index.html").write_text(page + "".join(cards) + "</main></html>\n")
    print(f"Gear preview gallery: {args.out / 'index.html'}")


if __name__ == "__main__":
    main()
