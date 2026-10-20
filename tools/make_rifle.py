#!/usr/bin/env python3
"""Generate the original, texture-free frontier military rifle. Standard library only.

Run from anywhere: python3 tools/make_rifle.py
Metres, +Y up, +Z muzzle; origin is the right-hand grip. See the asset README.
"""

import math
from pathlib import Path
from asset_helpers import Mesh

OUT = Path(__file__).resolve().parents[1] / "assets/models/weapons"
# sRGB swatches; glTF factors are linear. (name, color, metallic, roughness)
PALETTE = [
    ("Olive ceramic", (0.36, 0.39, 0.27), 0.12, 0.82),
    ("Parkerized steel", (0.25, 0.28, 0.29), 0.72, 0.63),
    ("Charcoal polymer", (0.12, 0.14, 0.14), 0.0, 0.92),
    ("Exposed edges", (0.48, 0.50, 0.46), 0.65, 0.67),
    ("Amber optic", (0.90, 0.47, 0.12), 0.15, 0.4),
]


class Rifle(Mesh):
    def __init__(self, palette=None):
        super().__init__(PALETTE if palette is None else palette,
                         {4: [.65, .22, .025]} if palette is None else {})

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

    def write(self, name="Rifle", out=OUT, pieces=None):
        super().write(name, out, pieces)


if __name__ == "__main__":
    Rifle().build().write()
