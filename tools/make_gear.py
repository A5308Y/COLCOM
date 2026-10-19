#!/usr/bin/env python3
"""Generate all weapons and armour in the Rifle's five-material art style.

    python3 tools/make_gear.py

Standard library only. Weapons are grip-authored in metres; armour scenes
are authored in the skeleton's upright frame, relative to each named bone.
"""

import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from make_rifle import Rifle

ROOT = Path(__file__).resolve().parents[1]


class Gear(Rifle):
    def add(self, other, at=(0, 0, 0), turn=(0, 0, 0)):
        def rotate(v):
            x, y, z = v
            a, b, c = [math.radians(t) for t in turn]
            y, z = y * math.cos(a) - z * math.sin(a), y * math.sin(a) + z * math.cos(a)
            x, z = x * math.cos(b) + z * math.sin(b), -x * math.sin(b) + z * math.cos(b)
            x, y = x * math.cos(c) - y * math.sin(c), x * math.sin(c) + y * math.cos(c)
            return (x, y, z)

        for target, (positions, normals, indices) in zip(self.groups, other.groups):
            offset = len(target[0])
            target[0].extend(tuple(p[i] + at[i] for i in range(3)) for p in map(rotate, positions))
            target[1].extend(map(rotate, normals))
            target[2].extend(i + offset for i in indices)
        return self

    def shell(self, rings, material=0):
        """Open-bottom octagonal helmet shell, with a closed faceted crown."""
        rings = [[(rx * math.cos(i * math.tau / 8), y, -.01 - rz * math.sin(i * math.tau / 8))
                  for i in range(8)] for y, rx, rz in rings]
        for lower, upper in zip(rings, rings[1:]):
            for i in range(8):
                j = (i + 1) % 8
                self.face([lower[i], lower[j], upper[j], upper[i]], material)
        self.face(rings[-1], material)


def grip(g):
    g.box((0, -.035, -.012), (.06, .16, .067), 2, tilt=16)
    g.box((0, -.11, -.035), (.069, .018, .079), 1, tilt=16)
    for side in (-1, 1):
        g.box((side * .031, -.035, -.012), (.005, .09, .045), 0, bevel=.002, tilt=16)
    g.box((0, -.035, .071), (.032, .012, .09), 1, bevel=.002)
    g.box((0, .01, .11), (.032, .086, .013), 1, bevel=.002)
    g.box((0, .026, .042), (.012, .046, .012), 3, bevel=.002, tilt=-15)


def optic(g, z):
    g.box((0, .205, z), (.065, .025, .07), 1, bevel=.004)
    for side in (-1, 1):
        g.box((side * .027, .234, z), (.012, .05, .042), 2, bevel=.003)
        g.box((side * .036, .215, z), (.008, .012, .016), 4, bevel=.002)
    g.box((0, .26, z), (.066, .012, .042), 1, bevel=.003)


def pistol():
    g = Gear()
    grip(g)
    g.box((0, .073, .058), (.073, .052, .224), 2)
    g.box((0, .119, .073), (.081, .059, .272), 1, worn=True)
    g.box((0, .153, .095), (.064, .016, .18), 0, bevel=.003)
    g.tube(.118, .201, .218, .019, 1, bore=.009)
    g.box((0, .166, .182), (.013, .014, .019), 4, bevel=.002)
    g.box((0, .158, -.036), (.045, .018, .018), 2, bevel=.003)
    for side in (-1, 1):
        for z in (-.035, -.012, .011):
            g.box((side * .041, .12, z), (.005, .036, .007), 2, bevel=.001, tilt=-12)
        g.box((side * .039, .07, .069), (.005, .009, .039), 3, bevel=.002)
    return g


def assault_rifle():
    g = Gear()
    grip(g)
    # Compact bullpup: magazine behind the hand, short shroud, broad stock.
    g.box((0, .10, -.079), (.10, .13, .39), 1, worn=True)
    g.box((0, .16, -.078), (.112, .05, .38), 0)
    g.box((0, .084, -.267), (.116, .18, .027), 2)
    g.box((0, .176, -.19), (.083, .029, .13), 2)
    g.box((0, -.026, -.155), (.072, .155, .105), 1, tilt=-8, worn=True)
    g.box((0, -.101, -.144), (.08, .024, .115), 2, tilt=-8)
    g.box((0, .126, .236), (.105, .113, .25), 0)
    g.box((0, .058, .234), (.073, .023, .23), 2)
    g.box((0, .194, .075), (.055, .02, .51), 1, bevel=.003)
    for z in (-.12, -.075, -.03, .015, .06, .105, .15, .195, .24):
        g.box((0, .207, z), (.064, .011, .016), 2, bevel=.002)
    optic(g, -.018)
    for side in (-1, 1):
        for z in (.16, .207, .254, .301):
            g.box((side * .053, .13, z), (.005, .036, .022), 2, bevel=.002, tilt=-15)
        for z in (-.185, -.145):
            g.box((side * .037, -.02, z), (.006, .105, .009), 2, bevel=.001, tilt=-8)
        g.box((side * .051, .103, .014), (.006, .025, .085), 2, bevel=.002)
        g.box((side * .055, .103, .014), (.004, .009, .047), 3, bevel=.001)
    g.tube(.129, .345, .435, .019, 1)
    g.tube(.129, .408, .464, .031, 1, bore=.014)
    return g


def laser_rifle():
    g = Gear()
    grip(g)
    # Longer square emitter, broad heat sinks, under-slung power cell.
    g.box((0, .11, .12), (.105, .137, .41), 1, worn=True)
    g.box((0, .171, .12), (.119, .039, .43), 0)
    g.profile([(.025, -.29), (.035, -.17), (.095, -.07), (.166, -.07), (.172, -.29)], .078, 0)
    g.box((0, .095, -.30), (.094, .185, .028), 2)
    g.box((0, .18, -.19), (.081, .027, .18), 2)
    g.box((0, -.025, .195), (.09, .155, .125), 2, tilt=5)
    g.box((0, -.093, .19), (.098, .026, .139), 0, tilt=5)
    for side in (-1, 1):
        g.box((side * .047, -.022, .195), (.006, .064, .075), 1, bevel=.003)
        g.box((side * .052, -.015, .195), (.006, .014, .034), 4, bevel=.002)
    g.box((0, .13, .43), (.126, .123, .25), 0)
    g.box((0, .13, .552), (.13, .13, .025), 1, worn=True)
    # Opaque recessed aperture and tiny amber emitter, not a glowing tube.
    g.box((0, .13, .567), (.079, .078, .008), 2, bevel=.004)
    g.box((0, .13, .573), (.037, .036, .007), 4, bevel=.003)
    for side in (-1, 1):
        for z in (.335, .38, .425, .47, .515):
            g.box((side * .065, .13, z), (.025, .084, .016), 1, bevel=.003, worn=True)
        g.box((side * .055, .104, .086), (.008, .042, .118), 2, bevel=.003)
    optic(g, .025)
    return g


def knife():
    g = Gear()
    g.box((0, 0, -.015), (.029, .042, .118), 2, bevel=.004)
    for z in (-.05, -.025, 0, .025):
        g.box((0, 0, z), (.033, .047, .009), 0, bevel=.002)
    g.box((0, 0, -.08), (.035, .05, .018), 1, bevel=.003)
    g.box((0, 0, .053), (.041, .077, .014), 1, bevel=.003, worn=True)
    # Diamond cross-section, clipped point: visibly a blade rather than a bar.
    upper = [(0, .032, .058), (0, .03, .23), (0, 0, .317)]
    lower = [(0, -.026, .058), (0, -.02, .245), (0, 0, .317)]
    for side in (-1, 1):
        spine = (side * .009, .008, .06)
        mid = (side * .007, .008, .23)
        faces = [[upper[0], spine, mid, upper[1]], [upper[1], mid, upper[2]],
                 [spine, lower[0], lower[1], mid], [mid, lower[1], lower[2]]]
        for i, face in enumerate(faces):
            g.face(face if side > 0 else list(reversed(face)), 1 if i < 2 else 3)
    g.face([upper[0], lower[0], (.009, .008, .06)], 1)
    g.face([lower[0], upper[0], (-.009, .008, .06)], 1)
    return g


def club():
    g = Gear()
    # A weighted industrial baton, still a simple blunt weapon.
    g.box((0, .12, 0), (.049, .46, .049), 1, worn=True)
    g.box((0, -.015, 0), (.062, .18, .062), 2)
    for y in (-.08, -.045, -.01, .025, .06):
        g.box((0, y, 0), (.067, .012, .067), 0, bevel=.003)
    g.box((0, -.115, 0), (.074, .025, .074), 1, worn=True)
    g.box((0, .109, 0), (.09, .02, .09), 1)
    g.box((0, .397, 0), (.105, .265, .095), 0, bevel=.012)
    g.box((0, .527, 0), (.113, .025, .103), 1, worn=True)
    for side in (-1, 1):
        g.box((side * .052, .4, 0), (.017, .21, .059), 1, worn=True)
        for y in (.32, .37, .42, .47):
            g.box((side * .063, y, 0), (.009, .014, .067), 2, bevel=.002)
    return g


def chest(powered=False):
    g = Gear()
    width = .47 if powered else .43
    g.box((0, -.02, -.018), (width, .35, .31), 2, bevel=.02)
    g.box((0, .053, .143), (width - .035, .278, .095), 0, bevel=.02, worn=True)
    g.box((0, -.115, .15), (width - .055, .073, .09), 1, bevel=.012)
    g.box((0, .028, -.18), (width - .045, .325, .065), 1, bevel=.015)
    g.box((0, -.192, -.018), (width + .014, .05, .35), 2)
    for side in (-1, 1):
        g.box((side * .123, .196, -.025), (.061, .074, .235), 1, bevel=.009)
        g.box((side * (width / 2), -.02, -.015), (.035, .247, .26), 1, bevel=.012)
        for y in (-.10, -.047, .006):
            g.box((side * (width / 2 + .018), y, -.015), (.009, .012, .21), 2, bevel=.002)
        g.box((side * .13, -.082, .209), (.071, .068, .039), 2)
        g.box((side * .13, -.06, .232), (.049, .012, .009), 3, bevel=.002)
    g.box((-.113, .116, .195), (.052, .025, .015), 1, bevel=.003)
    g.box((-.113, .116, .205), (.025, .009, .007), 4, bevel=.001)
    if powered:
        g.box((0, .008, -.262), (.30, .31, .122), 0, bevel=.016)
        g.box((0, .016, -.331), (.21, .21, .021), 2)
        for y in (-.055, -.01, .035, .08):
            g.box((0, y, -.345), (.18, .012, .014), 1, bevel=.002)
        g.box((0, .054, .199), (.035, .124, .019), 2, bevel=.003)
        g.box((0, .077, .211), (.014, .033, .009), 4, bevel=.002)
    return g


def shoulder(side, powered=False):
    g = Gear()
    g.box((side * .025, .045, 0), (.23 if powered else .19, .10, .22 if powered else .19), 0, bevel=.018, worn=True)
    g.box((side * .025, -.015, 0), (.205 if powered else .17, .027, .17), 2, bevel=.008)
    if powered:
        g.box((side * .145, -.004, 0), (.18, .135, .14), 1, bevel=.015)
        g.box((side * .15, -.012, .076), (.12, .07, .032), 0)
        g.box((side * .17, .025, -.078), (.09, .035, .025), 2)
    return g


def helmet(powered=False):
    g = Gear()
    g.shell([(.105, .145, .153), (.195, .151, .157), (.269, .106, .122), (.296, .05, .063)])
    # Brows and cheek protection leave the basic helmet's face open.
    g.box((0, .132, .14), (.24, .024, .047), 1, bevel=.007)
    for side in (-1, 1):
        g.box((side * .141, .12, -.014), (.035, .112, .103), 1, bevel=.007)
        g.box((side * .158, .153, -.017), (.018, .045, .066), 2, bevel=.004)
        g.box((side * .147, .094, .047), (.035, .055, .055), 0, bevel=.006)
    g.box((0, .21, -.162), (.069, .046, .025), 2)
    if powered:
        g.box((0, .082, .132), (.224, .082, .071), 2, bevel=.012)
        g.box((0, .108, .174), (.189, .032, .015), 4, bevel=.004)
        g.box((0, .037, .135), (.148, .042, .071), 1, bevel=.01)
        g.box((0, -.012, -.01), (.20, .10, .21), 2, bevel=.01)
        for side in (-1, 1):
            g.box((side * .067, .038, .174), (.025, .024, .015), 2, bevel=.004)
    else:
        g.box((-.085, .158, .168), (.035, .009, .009), 4, bevel=.002)
    return g


def armor(powered=False):
    pieces = [("Chest", chest(powered), (0, 0, 0)),
              ("ShoulderLeft", shoulder(1, powered), (.212, .145, -.072)),
              ("ShoulderRight", shoulder(-1, powered), (-.212, .145, -.072))]
    if powered:
        pieces.append(("Helmet", helmet(True), (0, .289, -.024)))
    assembled = Gear()
    for _, geometry, at in pieces:
        assembled.add(geometry, at)
    return assembled, pieces


def models():
    yield "weapons", "Rifle", Rifle().build(), None
    for name, build in [("Pistol", pistol), ("AssaultRifle", assault_rifle), ("LaserRifle", laser_rifle),
                        ("Knife", knife), ("Club", club)]:
        yield "weapons", name, build(), None
    h = helmet()
    yield "armor", "SteelHelmet", h, [("Helmet", h, (0, 0, 0))]
    for name, powered in [("SteelArmor", False), ("PoweredArmor", True)]:
        assembled, pieces = armor(powered)
        yield "armor", name, assembled, pieces


if __name__ == "__main__":
    for folder, name, geometry, pieces in models():
        geometry.write(name, ROOT / "assets/models" / folder, pieces)
