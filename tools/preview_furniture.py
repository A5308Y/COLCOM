#!/usr/bin/env python3
"""CPU geometry gallery for room furniture and fixtures (not game screenshots).

    python3 tools/preview_furniture.py --out target/furniture-review
"""

import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from make_furniture import Furniture, models
from preview_gear import write_gallery


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/furniture-review"))
    args = parser.parse_args()
    entries = []
    for ident, name, geometry, pieces in models():
        # These cabinets face -Z in their native game placement.
        if ident in ("dresser", "medical_cabinet"):
            rotated = Furniture().add(geometry, turn=(0, 180, 0))
            rotated.palette = geometry.palette
            geometry = rotated
        entries.append(("items", name, geometry, pieces))
    write_gallery(entries, args.out, "furniture &amp; room fixtures", "Warm timber, worn gunmetal, muted olive panels and soft fabrics.")
