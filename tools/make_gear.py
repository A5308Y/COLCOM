#!/usr/bin/env python3
"""Generate faceted weapons, a fabric flak jacket and high-tech powered armor.

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
    def section(self, rings, material, sides=8, smooth=False, roundness=2):
        """Closed tapered body; rings are (height, half-width, half-depth, Z)."""
        def curve(v):
            return math.copysign(abs(v) ** (2 / roundness), v)
        rings = [[(rx * curve(math.cos(i * math.tau / sides)), y, z - rz * curve(math.sin(i * math.tau / sides)))
                  for i in range(sides)] for y, rx, rz, z in rings]
        self.face(list(reversed(rings[0])), material)
        start = len(self.groups[material][0])
        for a, b in zip(rings, rings[1:]):
            for i in range(sides):
                j = (i + 1) % sides
                self.face([a[i], a[j], b[j], b[i]], material)
        if smooth:
            positions, normals, _ = self.groups[material]
            sums = {}
            for p, n in zip(positions[start:], normals[start:]):
                total = sums.setdefault(tuple(p), [0.0, 0.0, 0.0])
                for axis in range(3):
                    total[axis] += n[axis]
            for i in range(start, len(positions)):
                n = sums[tuple(positions[i])]
                length = math.sqrt(sum(v * v for v in n))
                normals[i] = tuple(v / length for v in n)
        self.face(rings[-1], material)

    def panel(self, points, material, lift=.012, thickness=.008):
        """Convex +Z-facing plate with a raised, faceted center and closed back."""
        if sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(points, points[1:] + points[:1])) < 0:
            points = list(reversed(points))
        center = tuple(sum(p[i] for p in points) / len(points) + (lift if i == 2 else 0) for i in range(3))
        back = [(x, y, z - thickness) for x, y, z in points]
        back_center = tuple(sum(p[i] for p in back) / len(back) for i in range(3))
        for i, a in enumerate(points):
            j = (i + 1) % len(points)
            self.face([back[j], back[i], back_center], material)
            self.face([a, points[j], center], material)
            self.face([back[i], back[j], points[j], a], material)

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


def helmet():
    g = Gear()
    g.shell([(.105, .145, .153), (.195, .151, .157), (.269, .106, .122), (.296, .05, .063)])
    # Brows and cheek protection leave the basic helmet's face open.
    g.box((0, .132, .14), (.24, .024, .047), 1, bevel=.007)
    for side in (-1, 1):
        g.box((side * .141, .12, -.014), (.035, .112, .103), 1, bevel=.007)
        g.box((side * .158, .153, -.017), (.018, .045, .066), 2, bevel=.004)
        g.box((side * .147, .094, .047), (.035, .055, .055), 0, bevel=.006)
    g.box((0, .21, -.162), (.069, .046, .025), 2)
    g.box((-.085, .158, .168), (.035, .009, .009), 4, bevel=.002)
    return g


FLAK_PALETTE = [
    ("Ballistic fabric", (.33, .36, .28), 0, .97),
    ("Padded panels", (.40, .43, .33), 0, .94),
    ("Webbing", (.17, .20, .18), 0, .96),
    ("Fasteners", (.46, .49, .44), .5, .72),
    ("Fabric label", (.65, .63, .48), 0, .94),
]
POWER_PALETTE = [
    ("Titanium ceramic", (.30, .39, .45), .65, .40),
    ("Graphite frame", (.12, .17, .21), .72, .51),
    ("Flex joints", (.065, .085, .10), 0, .92),
    ("Machined edges", (.56, .66, .70), .8, .34),
    ("Ion cyan", (.10, .82, 1.0), .05, .3),
]


def powered_piece():
    g = Gear(POWER_PALETTE)
    g.emissive = {4: [.06, 3.8, 6.0]}
    return g


def flak_chest():
    g = Gear(FLAK_PALETTE)
    # Substantial ballistic padding, clearly thicker than everyday clothing.
    g.section([(-.245, .185, .14, -.018), (-.04, .218, .162, -.018),
               (.13, .196, .145, -.018)], 0, sides=12, smooth=True)
    g.panel([(-.115, -.214, .162), (.115, -.214, .162), (.17, -.12, .158),
             (.17, .052, .155), (.104, .125, .154), (-.104, .125, .154),
             (-.17, .052, .155), (-.17, -.12, .158)], 1, lift=.028, thickness=.02)
    rear = Gear(FLAK_PALETTE)
    rear.panel([(-.13, -.20, .17), (.13, -.20, .17), (.167, -.10, .17),
                (.14, .12, .16), (-.14, .12, .16), (-.167, -.10, .17)], 1, lift=.020, thickness=.018)
    g.add(rear, turn=(0, 180, 0))
    for side in (-1, 1):
        g.box((side * .124, .135, -.012), (.059, .049, .25), 2, bevel=.011)
        g.box((side * .122, .103, .131), (.047, .037, .022), 3, bevel=.005)
        g.box((side * .197, -.092, -.018), (.022, .067, .21), 2, bevel=.004)
        g.box((side * .092, -.13, .190), (.104, .09, .038), 0, bevel=.014)
        for y in (-.014, .024):
            g.box((side * .082, y, .189), (.13, .010, .007), 2, bevel=.002)
    g.box((-.081, .066, .19), (.045, .015, .005), 4, bevel=.001)
    return g


def flak_shoulder(side):
    g = Gear(FLAK_PALETTE)
    pad = Gear(FLAK_PALETTE)
    pad.section([(-.035, .087, .092, 0), (.012, .101, .103, 0),
                 (.073, .067, .073, 0)], 1, sides=12, smooth=True)
    g.add(pad, (side * .025, .014, 0))
    return g


def power_chest(articulated=False):
    g = powered_piece()
    g.section([(-.29, .20, .17, -.035), (-.06, .22, .17, -.025),
               (.17, .205, .16, -.025)], 2, sides=12)
    for side in (-1, 1):
        g.panel([(side * .024, .115, .135), (side * .145, .155, .09),
                 (side * .208, .085, .083), (side * .172, -.018, .134),
                 (side * .062, -.08, .158), (side * .043, .015, .182)], 0, lift=.022)
        for y in (-.11, -.19):
            g.panel([(side * .119, y - .031, .117), (side * .18, y - .01, .087),
                     (side * .19, y + .035, .078), (side * .15, y + .017, .123)], 1)
        g.box((side * .107, .145, -.015), (.052, .055, .17), 1, bevel=.009)
        g.box((side * .164, .079, .130), (.055, .009, .01), 3, bevel=.002)
    # Suspended diamond reactor, with dark edging between it and the chest plates.
    g.panel([(0, -.075, .172), (.060, .009, .184), (0, .108, .176), (-.060, .009, .184)], 1, lift=.018)
    g.panel([(0, -.052, .195), (.041, .012, .200), (0, .080, .195), (-.041, .012, .200)], 4, lift=.022)
    for y, width in [(-.12, .118), (-.185, .106), (-.25, .090)]:
        g.panel([(-width, y + .029, .123), (0, y + .008, .162), (width, y + .029, .123),
                 (width * .8, y - .018, .125), (0, y - .035, .155), (-width * .8, y - .018, .125)], 0)
    g.section([(.13, .18, .17, -.06), (.24, .11, .125, -.04)], 1)
    # Compact spine reactor and exhaust slots: no bulky backpack.
    rear = powered_piece()
    if articulated:
        rear.panel([(-.15, -.075, .18), (.15, -.075, .18), (.16, .10, .18),
                    (0, .175, .17), (-.16, .10, .18)], 0, lift=.045)
        rear.panel([(-.13, -.22, .18), (.13, -.22, .18), (.145, -.09, .18),
                    (-.145, -.09, .18)], 0, lift=.035)
    else:
        rear.panel([(-.13, -.22, .18), (.13, -.22, .18), (.16, .10, .18),
                    (0, .175, .17), (-.16, .10, .18)], 0, lift=.045)
    rear.box((0, -.015, .228), (.049, .22, .014), 1, bevel=.007)
    for y in (-.06, -.015, .03):
        rear.box((0, y, .238), (.022, .022, .008), 4, bevel=.003)
    for side in (-1, 1):
        rear.panel([(side * .055, .11, .198), (side * .175, .12, .175),
                    (side * .163, .235, .16), (side * .065, .235, .18)], 1, lift=.018)
    g.add(rear, turn=(0, 180, 0))
    return g


def power_shoulder(side):
    g = powered_piece()
    cap = powered_piece()
    cap.section([(-.04, .108, .115, 0), (.044, .126, .132, 0),
                 (.116, .063, .076, 0)], 0)
    g.add(cap, (side * .035, .01, 0))
    sleeve = powered_piece()
    sleeve.section([(-.255, .060, .078, 0), (-.12, .079, .09, 0), (-.015, .078, .09, 0)], 1, sides=12)
    g.add(sleeve, turn=(0, 0, side * 90))
    g.panel([(side * .015, -.035, .114), (side * .105, -.025, .088),
             (side * .119, .045, .102), (side * .025, .066, .127)], 1)
    g.box((side * .064, .038, .135), (.059, .009, .009), 4, bevel=.002)
    return g


def power_helmet():
    g = powered_piece()
    g.shell([(-.012, .10, .11), (.080, .143, .147), (.192, .143, .15),
             (.275, .099, .115), (.31, .035, .053)], 0)
    # Sculpted cheek plates and split, swept eyes rather than an amber box visor.
    for side in (-1, 1):
        g.panel([(side * .012, .005, .126), (side * .104, .052, .114),
                 (side * .123, .145, .106), (side * .035, .121, .158)], 1, lift=.015)
        g.panel([(side * .026, .139, .151), (side * .113, .162, .109),
                 (side * .117, .181, .108), (side * .023, .159, .158)], 4, lift=.002, thickness=.004)
        g.panel([(side * .012, .184, .151), (side * .117, .198, .102),
                 (side * .096, .25, .10), (side * .025, .275, .113)], 0)
        g.box((side * .14, .109, -.016), (.026, .078, .09), 1, bevel=.008)
    g.panel([(0, .002, .150), (.020, .118, .167), (0, .152, .177), (-.020, .118, .167)], 3, lift=.009)
    g.section([(-.13, .105, .125, -.01), (.025, .104, .108, -.01)], 2)
    return g


def power_forearm(side):
    g = powered_piece()
    sleeve = powered_piece()
    sleeve.section([(-.223, .047, .057, 0), (-.14, .078, .077, 0), (0, .067, .067, 0)], 1)
    g.add(sleeve, turn=(0, 0, side * 90))
    g.panel([(side * .032, -.043, .061), (side * .173, -.050, .066),
             (side * .222, -.015, .063), (side * .194, .042, .068), (side * .075, .071, .070)], 0, lift=.025)
    g.panel([(side * .098, -.018, .099), (side * .163, -.012, .101),
             (side * .168, .009, .101), (side * .092, .019, .099)], 4, lift=.003)
    return g


def power_hip():
    g = powered_piece()
    g.section([(-.145, .15, .12, .005), (-.035, .205, .145, 0), (.085, .17, .13, .01)], 2)
    g.panel([(-.16, .048, .13), (0, .016, .165), (.16, .048, .13),
             (.12, -.095, .14), (0, -.146, .14), (-.12, -.095, .14)], 0)
    return g


def power_leg(shin=False):
    g = powered_piece()
    if shin:
        g.section([(-.405, .070, .11, -.035), (-.20, .086, .12, -.04), (.075, .082, .12, -.02)], 1, sides=12)
        g.panel([(-.069, -.10, .078), (0, -.135, .119), (.069, -.10, .078),
                 (.046, -.35, .071), (0, -.39, .084), (-.046, -.35, .071)], 0)
        g.panel([(-.073, .013, .081), (0, .050, .077), (.073, .013, .081),
                 (.066, -.055, .094), (0, -.087, .113), (-.066, -.055, .094)], 0)
        g.box((0, -.034, .12), (.050, .009, .009), 4, bevel=.002)
    else:
        g.section([(-.36, .079, .105, .012), (-.17, .11, .14, .02), (-.025, .103, .14, .015)], 1, sides=12)
        g.panel([(-.084, -.045, .156), (0, -.018, .169), (.084, -.045, .156),
                 (.067, -.27, .132), (0, -.332, .111), (-.067, -.27, .132)], 0, lift=.019)
    return g


def power_boot():
    g = powered_piece()
    g.section([(-.105, .083, .175, .065), (-.035, .085, .175, .065), (.065, .063, .095, -.005)], 1)
    g.panel([(-.067, -.068, .242), (.067, -.068, .242), (.054, .014, .152), (-.054, .014, .152)], 0)
    return g


def armor(powered=False):
    pieces = [("Chest", power_chest() if powered else flak_chest(), (0, 0, 0)),
              ("ShoulderLeft", power_shoulder(1) if powered else flak_shoulder(1), (.212, .145, -.072)),
              ("ShoulderRight", power_shoulder(-1) if powered else flak_shoulder(-1), (-.212, .145, -.072))]
    if powered:
        pieces.append(("Helmet", power_helmet(), (0, .289, -.024)))
        pieces.extend([
            ("ForearmLeft", power_forearm(1), (.463, .145, -.080)),
            ("ForearmRight", power_forearm(-1), (-.463, .145, -.080)),
            ("Pelvis", power_hip(), (0, -.362, -.050)),
            ("ThighLeft", power_leg(), (.114, -.340, -.043)),
            ("ThighRight", power_leg(), (-.114, -.340, -.043)),
            ("ShinLeft", power_leg(True), (.114, -.769, -.043)),
            ("ShinRight", power_leg(True), (-.114, -.769, -.043)),
            ("BootLeft", power_boot(), (.114, -1.224, -.094)),
            ("BootRight", power_boot(), (-.114, -1.224, -.094)),
        ])
    assembled = powered_piece() if powered else Gear(FLAK_PALETTE)
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
    for name, powered in [("FlakJacket", False), ("PoweredArmor", True)]:
        assembled, pieces = armor(powered)
        yield "armor", name, assembled, pieces


if __name__ == "__main__":
    for folder, name, geometry, pieces in models():
        geometry.write(name, ROOT / "assets/models" / folder, pieces)
