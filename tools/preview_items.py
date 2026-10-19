#!/usr/bin/env python3
"""Render the item geometry gallery without a GPU. Standard library only.

    python3 tools/preview_items.py --out target/item-review
"""

import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from make_items import models
from preview_gear import write_gallery


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/item-review"))
    args = parser.parse_args()
    entries = (("items", name, geometry, pieces) for _, name, geometry, pieces in models())
    write_gallery(entries, args.out, "resources, supplies &amp; clothing", "Faceted materials, natural resource colors and muted industrial finishes.")
