#!/usr/bin/env python3
"""Generate the remaining inventory items. Standard library only.

    python3 tools/make_items.py

Faceted silhouettes and rough materials match the gear. Raw resources keep
natural colors; manufactured supplies use restrained industrial packaging.
"""

import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from make_gear import Gear

ROOT = Path(__file__).resolve().parents[1]
CHARCOAL = (.14, .16, .16)
STEEL = (.48, .51, .49)
OLIVE = (.36, .39, .27)
CREAM = (.80, .77, .65)
AMBER = (.79, .46, .18)


def palette(primary, secondary=STEEL, edge=CREAM, accent=AMBER, metal=0):
    return [("Body", primary, metal, .84 if metal == 0 else .64),
            ("Secondary", secondary, .35 if secondary == STEEL else 0, .83),
            ("Bindings", CHARCOAL, 0, .93),
            ("Edges", edge, metal * .7, .78),
            ("Accent", accent, 0, .88)]


class Item(Gear):
    def cylinder(self, at, radius, height, material, axis="y", bore=0, sides=8):
        piece = Item(self.palette)
        piece.tube(0, -height / 2, height / 2, radius, material, bore=bore, sides=sides)
        self.add(piece, at, {"x": (0, 90, 0), "y": (90, 0, 0), "z": (0, 0, 0)}[axis])

    def rock(self, at, size, material=0, seed=0, flecks=None):
        """A closed irregular six-sided lump, with a stable flat bottom."""
        rings = []
        for level, factor in [(-.5, .68), (-.03, 1), (.30, .64)]:
            rings.append([(at[0] + size[0] / 2 * factor * (1 + .15 * math.sin(i * 7 + seed)) * math.cos(-i * math.tau / 6),
                           at[1] + level * size[1],
                           at[2] + size[2] / 2 * factor * (1 + .12 * math.cos(i * 9 + seed)) * math.sin(-i * math.tau / 6)) for i in range(6)])
        self.face(list(reversed(rings[0])), material)
        for k in range(2):
            for i in range(6):
                j = (i + 1) % 6
                mat = flecks if flecks is not None and (i + seed + k) % 3 == 0 else material
                self.face([rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]], mat)
        tip = (at[0] + .02 * size[0], at[1] + .5 * size[1], at[2])
        for i in range(6):
            self.face([rings[-1][i], rings[-1][(i + 1) % 6], tip], material)

    def straps(self, width, height, depth, positions=(-.12, .12)):
        for x in positions:
            self.box((x, height, 0), (.024, .012, depth + .008), 2, bevel=.002)
            for z in (-depth / 2, depth / 2):
                self.box((x, height / 2, z), (.024, height, .009), 2, bevel=.002)

    def ball(self, at, radius, material, squash=1.0, sides=10):
        """A smooth closed ovoid; `squash` scales its height."""
        piece = Item(self.palette)
        rings = [(radius * squash * math.sin(a), radius * math.cos(a) + 1e-4, radius * math.cos(a) + 1e-4, 0)
                 for a in [math.radians(d) for d in (-80, -45, 0, 45, 80)]]
        piece.section(rings, material, sides=sides, smooth=True)
        self.add(piece, at)

    def cross_mark(self, y, size=.12, z=0):
        self.box((0, y, z), (size * .28, .009, size), 4, bevel=.001)
        for side in (-1, 1):
            self.box((side * size * .32, y, z), (size * .36, .009, size * .28), 4, bevel=.001)


def wood():
    g = Item(palette((.35, .25, .16), (.22, .17, .12), (.65, .49, .29), (.46, .35, .22)))
    for y, z in [(.073, -.077), (.073, .077), (.195, 0)]:
        g.cylinder((0, y, z), .073, .56, 0, "x")
        for side in (-1, 1):
            g.cylinder((side * .281, y, z), .062, .004, 3, "x")
            g.cylinder((side * .284, y, z), .024, .003, 4, "x", sides=6)
    # Tensioned bands follow the log bundle's hull rather than a box around it.
    hull = [(0, -.077), (.0214, -.1286), (.073, -.15), (.1246, -.1286),
            (.2466, -.0516), (.268, 0), (.2466, .0516), (.1246, .1286),
            (.073, .15), (.0214, .1286), (0, .077)]
    for x in (-.16, .16):
        for a, b in zip(hull, hull[1:] + hull[:1]):
            dy, dz = b[0] - a[0], b[1] - a[1]
            length = math.hypot(dy, dz)
            ny, nz = -dz / length * .0045, dy / length * .0045
            band = Item(g.palette)
            band.profile([(a[0] + ny, a[1] + nz), (b[0] + ny, b[1] + nz),
                          (b[0] - ny, b[1] - nz), (a[0] - ny, a[1] - nz)], .024, 2)
            g.add(band, (x, 0, 0))
    return g


def planks():
    g = Item(palette((.61, .46, .28), (.39, .29, .18), (.73, .58, .38), OLIVE))
    for y in (.025, .08, .135):
        for z in (-.085, .085):
            g.box((0, y, z), (.58, .048, .15), 0, bevel=.006)
            g.box((-.02, y + .0245, z - .026), (.46, .002, .004), 1, bevel=0)
    g.straps(.58, .162, .324, (-.17, .17))
    return g


def stone(ore=None):
    colors = {None: ((.46, .45, .40), (.32, .33, .31), (.64, .62, .55)),
              "iron": ((.43, .31, .22), (.23, .25, .25), (.60, .40, .25)),
              "copper": ((.38, .42, .32), (.27, .50, .39), (.65, .42, .25))}
    primary, secondary, edge = colors[ore]
    g = Item(palette(primary, secondary, edge, metal=.18 if ore else 0))
    for x, y, z, size, seed in [(-.115, .09, -.035, (.26, .18, .25), 2),
                                (.12, .075, .04, (.235, .15, .22), 7),
                                (-.005, .205, .005, (.23, .21, .21), 13)]:
        g.rock((x, y, z), size, seed=seed, flecks=1 if ore else 3)
    if ore:
        g.rock((.10, .158, .11), (.09, .08, .08), material=3, seed=4)
    return g


def blocks():
    g = Item(palette((.58, .57, .51), (.36, .37, .34), (.70, .68, .61), OLIVE))
    for x, y, z in [(-.104, .075, -.10), (.104, .075, -.10), (-.104, .075, .10), (.104, .075, .10), (0, .23, 0)]:
        g.box((x, y, z), (.20, .15, .19), 0, bevel=.012)
    return g


def ingots(copper=False):
    g = Item(palette((.62, .38, .22) if copper else STEEL,
                     (.38, .25, .17) if copper else (.30, .33, .32),
                     (.74, .51, .31) if copper else (.66, .68, .63), OLIVE, metal=.72))
    for y, z in [(.04, -.08), (.04, .08), (.125, 0)]:
        g.box((0, y, z), (.44, .08, .135), 0, bevel=.015, worn=True)
        g.box((0, y + .041, z), (.09, .003, .043), 1, bevel=.001)
    return g


def latex():
    g = Item(palette((.78, .77, .64), STEEL, (.90, .88, .74), (.58, .62, .43)))
    g.cylinder((0, .125, 0), .12, .24, 0)
    g.cylinder((0, .248, 0), .125, .018, 1)
    g.cylinder((0, .264, 0), .047, .018, 2)
    g.box((0, .135, .12), (.10, .08, .008), 4, bevel=.004)
    for side in (-1, 1):
        g.box((side * .105, .286, 0), (.018, .06, .025), 2, bevel=.003)
    g.box((0, .316, 0), (.22, .016, .025), 2, bevel=.003)
    return g


def rubber():
    g = Item(palette((.17, .19, .18), (.25, .27, .25), (.32, .33, .29), OLIVE))
    for y, z in [(.026, -.01), (.083, .015), (.14, -.005)]:
        g.box((0, y, z), (.37, .05, .255), 0, bevel=.008)
        g.box((0, y + .026, z), (.32, .003, .16), 1, bevel=.003)
    g.straps(.37, .17, .28, (-.11, .11))
    return g


def silicon():
    g = Item(palette((.22, .29, .34), (.32, .43, .48), STEEL, OLIVE, metal=.28))
    g.box((0, .035, 0), (.36, .06, .29), 2)
    for y in (.072, .094, .116):
        g.box((0, y, 0), (.31, .012, .255), 0, bevel=.012)
    for x in (-.1, -.05, 0, .05, .1):
        g.box((x, .123, 0), (.003, .002, .21), 1, bevel=0)
    for z in (-.075, -.025, .025, .075):
        g.box((0, .123, z), (.26, .002, .003), 1, bevel=0)
    return g


def scrap():
    g = Item(palette((.44, .29, .19), (.26, .29, .28), STEEL, (.64, .42, .24), metal=.3))
    for at, size, turn in [((-.055, .06, 0), (.32, .035, .24), (0, 24, 7)),
                            ((.06, .10, -.01), (.25, .027, .19), (0, -22, -14)),
                            ((-.04, .15, -.03), (.25, .025, .13), (15, 5, 25))]:
        piece = Item(g.palette)
        piece.box((0, 0, 0), size, 0, worn=True)
        g.add(piece, at, turn)
    g.cylinder((.14, .075, .09), .048, .19, 1, "x", bore=.029)
    return g


def circuit(waste=False):
    g = Item(palette((.22, .37, .26), (.31, .32, .28), (.61, .41, .23), (.75, .61, .31)))
    g.box((0, .025, 0), (.34, .025, .25), 0, bevel=.004)
    for x, z in [(-.07, -.035), (.075, .04), (.08, -.07)]:
        g.box((x, .052, z), (.076, .03, .054), 2, bevel=.003)
    for x in (-.12, -.025, .12):
        g.box((x, .039, 0), (.005, .003, .19), 3, bevel=0)
    for z in (-.08, 0, .08):
        g.box((-.035, .039, z), (.23, .003, .004), 3, bevel=0)
    for x in (-.10, -.06, -.02, .02, .06, .10):
        g.box((x, .038, -.126), (.018, .008, .02), 4, bevel=.001)
    g.cylinder((-.10, .083, .072), .026, .085, 1)
    if waste:
        piece = Item(g.palette)
        piece.box((0, 0, 0), (.12, .035, .10), 0, bevel=.006)
        g.add(piece, (.13, .017, .115), (0, 28, 0))
        g.box((.06, .105, -.025), (.14, .055, .10), 1, worn=True)
    return g


def mechanical():
    g = Item(palette(OLIVE, STEEL, (.68, .65, .51), (.63, .40, .23), metal=.3))
    g.cylinder((-.08, .039, .015), .092, .05, 1, bore=.032)
    for i in range(8):
        angle = math.tau * i / 8
        tooth = Item(g.palette)
        tooth.box((0, 0, 0), (.038, .048, .038), 1, bevel=.003)
        g.add(tooth, (-.08 + .094 * math.cos(angle), .039, .015 + .094 * math.sin(angle)), (0, -math.degrees(angle), 0))
    g.cylinder((.095, .075, -.025), .06, .21, 0, "z")
    g.cylinder((.095, .075, .106), .022, .065, 1, "z")
    g.cylinder((.095, .075, -.116), .055, .021, 2, "z")
    for z in (-.075, -.025, .025):
        g.cylinder((.095, .075, z), .062, .012, 3, "z", bore=.055)
    return g


def optical():
    g = Item(palette(OLIVE, STEEL, (.39, .55, .60), AMBER, metal=.25))
    g.box((0, .095, -.025), (.19, .18, .15), 0, worn=True)
    g.cylinder((0, .105, .102), .067, .11, 2, "z", bore=.046)
    g.cylinder((0, .105, .147), .044, .006, 3, "z")
    g.box((-.075, .161, .055), (.023, .014, .012), 4, bevel=.002)
    g.box((0, .01, 0), (.25, .02, .24), 1)
    return g


def structural():
    g = Item(palette(OLIVE, STEEL, (.59, .61, .55), AMBER, metal=.45))
    for y in (.025, .072, .119):
        g.box((0, y, 0), (.40, .035, .30), 0, bevel=.008, worn=True)
    for z in (-.115, .115):
        g.box((0, .155, z), (.36, .036, .032), 1)
    for x in (-.14, .14):
        g.box((x, .155, 0), (.032, .036, .22), 1)
    return g


def chemical():
    g = Item(palette((.62, .52, .28), STEEL, (.74, .65, .42), (.72, .38, .19), metal=.18))
    for x, h, r in [(-.075, .22, .06), (.075, .18, .052)]:
        g.cylinder((x, h / 2, 0), r, h, 0)
        g.cylinder((x, h + .008, 0), r * .85, .016, 1)
        g.cylinder((x, h + .023, 0), r * .37, .018, 2)
        g.box((x, h * .55, r), (r * 1.1, .07, .009), 2, bevel=.003)
        g.box((x, h * .55, r + .006), (.014, .04, .005), 4, bevel=.001)
    return g


def fabric(cotton=False, padding=False):
    g = Item(palette((.80, .79, .68) if not padding else (.48, .45, .33),
                     (.61, .60, .50), (.91, .88, .75), OLIVE))
    if cotton:
        for x, y, z in [(-.105, .08, -.04), (.10, .08, .04), (0, .19, -.015)]:
            g.rock((x, y, z), (.25, .16, .24), seed=int(y * 100), flecks=3)
    else:
        for y, width in [(.035, .39), (.11, .365), (.185, .34)]:
            g.box((0, y, 0), (width, .065, .275), 0, bevel=.014)
            for z in (-.125, .125):
                g.box((0, y + .016, z), (width - .025, .006, .008), 1, bevel=.002)
    g.straps(.39, .27 if cotton else .225, .29, (-.10, .10))
    return g


def food():
    g = Item(palette((.42, .34, .23), (.46, .17, .12), (.73, .61, .39), (.42, .51, .25)))
    g.box((0, .02, 0), (.40, .035, .33), 0)
    for x in (-.19, .19):
        g.box((x, .069, 0), (.025, .10, .33), 0)
    for z in (-.155, .155):
        g.box((0, .069, z), (.36, .10, .025), 0)
    for x, y, z, mat in [(-.09, .125, -.07, 1), (.085, .13, -.07, 4), (-.085, .14, .075, 4), (.09, .12, .075, 1)]:
        g.rock((x, y, z), (.14, .13, .14), mat, seed=4)
        g.box((x, y + .065, z), (.012, .022, .012), 0, bevel=.002)
    return g


def meal():
    g = Item(palette(STEEL, (.47, .35, .22), (.83, .78, .60), (.37, .48, .22), metal=.3))
    g.box((0, .024, 0), (.37, .045, .30), 0, bevel=.015)
    g.box((0, .048, 0), (.33, .012, .26), 2, bevel=.012)
    g.rock((-.065, .075, 0), (.17, .055, .19), 3, seed=3)
    for x, z in [(.08, -.05), (.07, .065), (.015, .02)]:
        g.rock((x, .078, z), (.077, .065, .08), 1 if z < 0 else 4, seed=7)
    for x in (-.18, .18):
        g.box((x, .066, 0), (.015, .06, .29), 0, bevel=.004)
    for z in (-.145, .145):
        g.box((0, .066, z), (.35, .06, .015), 0, bevel=.004)
    return g


def healroot():
    g = Item(palette((.53, .46, .29), (.36, .46, .27), (.66, .61, .42), (.43, .58, .34)))
    for x, z in [(-.03, -.045), (.035, .035)]:
        piece = Item(g.palette)
        piece.cylinder((0, 0, 0), .024, .35, 0, "x", sides=6)
        g.add(piece, (x, .033, z), (0, 12 if x > 0 else -10, 0))
    for x, z, turn in [(-.025, -.06, -25), (.03, .05, 15), (.08, -.02, 40)]:
        leaf = Item(g.palette)
        leaf.profile([(-.013, -.07), (-.025, .01), (.006, .10), (.025, .015)], .007, 4)
        g.add(leaf, (x, .075, z), (10, turn, 80))
    g.box((-.07, .062, 0), (.025, .013, .15), 2, bevel=.002)
    return g


def medicine(herbal=False):
    g = Item(palette((.38, .46, .29) if herbal else (.78, .79, .70), STEEL, CREAM,
                     CREAM if herbal else AMBER))
    w, h, d = (.30, .14, .235) if herbal else (.39, .20, .29)
    g.box((0, h / 2, 0), (w, h, d), 0, bevel=.016)
    g.box((0, h + .009, 0), (w + .008, .022, d + .008), 0, bevel=.009)
    g.cross_mark(h + .025, .115)
    for x in (-w * .32, w * .32):
        g.box((x, h * .63, d / 2 + .006), (.027, .055, .013), 2, bevel=.003)
    for side in (-1, 1):
        g.box((side * .055, h + .05, -d * .28), (.018, .07, .022), 2)
    g.box((0, h + .083, -d * .28), (.12, .016, .022), 2, bevel=.002)
    return g


TITANIUM = (.56, .59, .60)
GRAPHITE = (.19, .21, .22)
# Muted identification teal for cyberware: a painted mark, never emissive.
TEAL = (.25, .50, .52)


def cyber_palette(primary=TITANIUM, secondary=GRAPHITE):
    return palette(primary, secondary, (.72, .74, .72), TEAL, metal=.6)


def prosthetic(leg=False, bionic=False):
    g = Item(cyber_palette() if bionic else palette(OLIVE, STEEL, (.64, .65, .57), AMBER, metal=.35))
    g.box((0, .065, -.025), (.07, .085, .35), 1, worn=True)
    for z in (-.19, .15):
        g.cylinder((0, .065, z), .066, .09, 2, "x", bore=.022)
        g.cylinder((0, .065, z), .035, .095, 3, "x")
    g.box((0, .09, -.09), (.11, .055, .18), 0)
    g.box((0, .065, -.245), (.14, .115, .075), 2, bevel=.012)
    for side in (-1, 1):
        g.box((side * .044, .065, .005), (.018, .024, .21), 3, bevel=.003)
    if leg:
        g.box((0, .039, .24), (.15, .075, .22), 0, bevel=.018)
        g.box((0, .01, .24), (.16, .018, .23), 2)
    else:
        g.box((0, .057, .235), (.115, .055, .11), 0)
        for x in (-.043, -.014, .014, .043):
            g.box((x, .052, .315), (.023, .035, .066), 1, bevel=.004)
        g.box((-.075, .05, .24), (.045, .035, .026), 1, bevel=.004)
    if bionic:
        # A graphite muscle shroud over the strut and a painted teal service stripe.
        g.box((0, .118, -.025), (.085, .022, .26), 1, bevel=.008)
        g.box((0, .131, -.025), (.014, .006, .22), 4, bevel=.002)
        for z in (-.19, .15):
            g.cylinder((0, .065, z), .046, .1, 4, "x", sides=8)
    return g


def bionic_eyes():
    g = Item(cyber_palette(GRAPHITE, (.78, .80, .78)))
    # A padded transport case holding the pair, lenses up.
    g.box((0, .022, 0), (.25, .044, .15), 0, bevel=.012)
    g.box((0, .046, 0), (.23, .006, .13), 2, bevel=.002)
    for x in (-.058, .058):
        g.ball((x, .085, 0), .042, 1)
        g.cylinder((x, .125, 0), .018, .006, 4, "y", sides=10)
        g.cylinder((x, .129, 0), .008, .004, 2, "y", sides=8)
    g.box((0, .06, -.062), (.05, .02, .012), 3, bevel=.003)
    return g


def neural_coprocessor():
    g = Item(cyber_palette(GRAPHITE, STEEL))
    g.box((0, .012, 0), (.17, .024, .12), 0, bevel=.004)
    g.box((0, .032, 0), (.07, .016, .06), 1, bevel=.004)
    g.box((0, .042, 0), (.03, .004, .02), 4, bevel=0)
    # Gold-tone contact fingers and a short braided lead.
    for i in range(6):
        g.box((-.07 + i * .028, .025, .056), (.012, .003, .018), 3, bevel=0)
    g.cylinder((.06, .02, -.075), .009, .05, 2, "z", sides=6)
    return g


def reinforced_spine():
    g = Item(cyber_palette())
    for i in range(7):
        z = -.24 + i * .08
        g.cylinder((0, .045, z), .04, .05, 0, "z", sides=8)
        g.box((0, .088, z), (.018, .04, .03), 1, bevel=.004)
        for side in (-1, 1):
            g.box((side * .055, .05, z), (.05, .016, .022), 1, bevel=.004)
        if i < 6:
            g.cylinder((0, .045, z + .04), .03, .03, 2, "z", sides=8)
    # A painted teal mark along the top of the spinous processes.
    g.box((0, .111, 0), (.008, .004, .5), 4, bevel=0)
    return g


def subdermal_plating():
    g = Item(cyber_palette((.62, .64, .62), GRAPHITE))
    # Overlapping ceramic scales on a flexible graphite mesh backing.
    g.box((0, .006, 0), (.32, .012, .24), 1, bevel=.003)
    for row, z in enumerate((-.08, 0, .08)):
        for col in range(4):
            x = -.11 + col * .074 + (.037 if row == 1 else 0)
            if abs(x) > .14:
                continue
            g.box((x, .02 + row * .004, z), (.07, .012, .09), 0, bevel=.006, tilt=-6)
    g.box((-.135, .03, 0), (.02, .008, .2), 4, bevel=.002)
    return g


def synthetic_heart():
    g = Item(cyber_palette(STEEL, GRAPHITE))
    g.box((0, .012, 0), (.2, .024, .2), 1, bevel=.008)
    g.ball((0, .1, 0), .075, 0, squash=1.05)
    g.cylinder((0, .1, 0), .078, .03, 4, "y", sides=10)
    for x, z, height in [(-.03, -.02, .07), (.025, .02, .06), (.045, -.035, .05)]:
        g.cylinder((x, .19, z), .016, height, 2, "y", bore=.008, sides=8)
    return g


def filter_lungs():
    g = Item(cyber_palette(STEEL, GRAPHITE))
    g.box((0, .012, 0), (.26, .024, .2), 1, bevel=.008)
    for x in (-.065, .065):
        g.ball((x, .095, 0), .062, 0, squash=1.25)
        g.cylinder((x, .095, 0), .065, .018, 4, "y", sides=10)
    g.cylinder((0, .2, 0), .016, .07, 2, "y", sides=8)
    for side in (-1, 1):
        g.box((side * .03, .165, 0), (.07, .014, .014), 2, bevel=.003, tilt=0)
    return g


def frag_grenades():
    g = Item(palette(OLIVE, STEEL, CREAM, AMBER, metal=.2))
    for x, z in [(-.06, -.02), (.065, .025)]:
        g.ball((x, .055, z), .05, 0, squash=1.2)
        # Fragmentation bands, a fuse body and a spoon lever.
        for dy in (-.02, .015):
            g.cylinder((x, .055 + dy, z), .051, .008, 2, "y", sides=10)
        g.cylinder((x, .125, z), .016, .03, 1, "y", sides=8)
        g.box((x + .018, .1, z), (.008, .06, .018), 1, bevel=.002, tilt=0)
        g.cylinder((x - .02, .135, z), .009, .004, 3, "x", bore=.005, sides=8)
    return g


def field_medkit():
    g = Item(palette(OLIVE, STEEL, CREAM, (.62, .16, .13)))
    g.box((0, .055, 0), (.26, .11, .16), 0, bevel=.025)
    g.box((0, .062, .081), (.16, .07, .012), 0, bevel=.01)
    g.cross_mark(.112, .085)
    g.box((0, .115, -.04), (.2, .01, .022), 2, bevel=.002)
    g.straps(.26, .11, .16, (-.09, .09))
    return g


def backpack(kind="field"):
    """A framed pack lying on its back: body, lid, side pouches and straps."""
    colors = {"field": ((.45, .41, .30), (.34, .31, .23), CREAM, OLIVE),
              "medic": ((.74, .74, .66), (.55, .56, .50), CREAM, (.62, .16, .13)),
              "radio": ((.30, .33, .27), GRAPHITE, CREAM, (.79, .46, .18))}[kind]
    g = Item(palette(*colors, metal=.2 if kind == "radio" else 0))
    w, h, d = (.28, .15, .36) if kind != "radio" else (.26, .14, .3)
    g.box((0, h / 2, 0), (w, h, d), 0, bevel=.03 if kind != "radio" else .012)
    if kind == "radio":
        # The set's face: dials, a handset and a whip antenna folded alongside.
        g.box((0, h + .006, .03), (w - .05, .012, d - .12), 1, bevel=.004)
        for x in (-.07, 0, .07):
            g.cylinder((x, h + .02, .09), .018, .02, 4, "y", sides=8)
        g.box((.05, h + .025, -.05), (.06, .03, .1), 2, bevel=.008)
        g.cylinder((-w / 2 - .012, .06, -.02), .006, .4, 3, "z", sides=6)
    else:
        g.box((0, h + .012, -d * .22), (w + .01, .03, d * .5), 1, bevel=.012)
        for side in (-1, 1):
            g.box((side * (w / 2 + .03), h * .45, .02), (.06, h * .7, .16), 1, bevel=.012)
        if kind == "medic":
            g.cross_mark(h + .03, .1, z=.08)
    # Shoulder straps lie on top, loose.
    for x in (-.07, .07):
        g.box((x, h + .004, .09), (.04, .008, .2), 2, bevel=.002)
    return g


def cargo_trousers():
    g = Item(palette((.47, .43, .33), (.36, .33, .25), CREAM, OLIVE))
    # Folded in three: soft layers with pocket flaps and a belt band on top.
    for y, width in [(.025, .36), (.075, .345), (.125, .33)]:
        g.box((0, y, 0), (width, .05, .26), 0, bevel=.018)
    for side in (-1, 1):
        g.box((side * .09, .152, .02), (.09, .008, .07), 1, bevel=.004)
        g.box((side * .09, .157, -.012), (.09, .006, .012), 2, bevel=.002)
    g.box((0, .152, -.115), (.31, .01, .03), 2, bevel=.003)
    return g


def flak_leg_guards():
    g = Item(palette((.34, .37, .27), (.24, .26, .22), CREAM, OLIVE))
    for x in (-.085, .085):
        # Quilted thigh pad, a rounded knee cup and a shin plate, lying flat.
        g.box((x, .028, -.08), (.15, .05, .2), 0, bevel=.02)
        for z in (-.13, -.08, -.03):
            g.box((x, .054, z), (.14, .004, .006), 1, bevel=0)
        g.ball((x, .04, .06), .045, 1, squash=.8)
        g.box((x, .024, .17), (.12, .042, .16), 0, bevel=.016)
        for z in (-.12, .17):
            g.box((x, .05, z), (.155, .008, .022), 2, bevel=.002)
    return g


def tactical_goggles():
    g = Item(palette(GRAPHITE, (.22, .30, .31), CREAM, OLIVE, metal=.2))
    for x in (-.055, .055):
        g.box((x, .035, 0), (.09, .06, .045), 0, bevel=.012)
        g.box((x, .036, .024), (.072, .044, .006), 1, bevel=.006)
    g.box((0, .042, 0), (.03, .02, .03), 0, bevel=.005)
    # The elastic strap lies in a loose loop behind the frame.
    for i in range(8):
        a, b = math.pi * i / 8, math.pi * (i + 1) / 8
        p, q = (.11 * math.cos(a), -.09 * math.sin(a) - .02), (.11 * math.cos(b), -.09 * math.sin(b) - .02)
        mid = ((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
        length = math.hypot(q[0] - p[0], q[1] - p[1])
        strap = Item(g.palette)
        strap.box((0, 0, 0), (length + .004, .012, .006), 2, bevel=0)
        g.add(strap, (mid[0], .006, mid[1]), (0, -math.degrees(math.atan2(q[1] - p[1], q[0] - p[0])), 0))
    return g


def jacket(parka=False):
    pal = palette((.51, .37, .23) if parka else (.31, .38, .40),
                  (.34, .30, .23) if parka else (.24, .29, .29), CREAM, OLIVE)
    body = Item(pal)
    # A curved shoulder yoke, fitted waist and natural hem replace the box body.
    rings = [(-.34, .200, .166, -.016), (-.22, .183, .157, -.018),
             (-.105, .180, .159, -.019), (.025, .218, .184, -.021),
             (.12, .210, .181, -.025), (.205, .215, .140, -.055),
             (.268, .118, .108, -.045)] if parka else [
             (-.235, .174, .150, -.018), (-.12, .165, .150, -.018),
             (.020, .210, .178, -.022), (.115, .202, .174, -.025),
             (.200, .205, .130, -.055), (.260, .114, .100, -.045)]
    body.section(rings, 0, sides=16, smooth=True, roundness=3.5)
    # The placket follows the fabric's front surface instead of forming a bar.
    for (y0, _, rz0, z0), (y1, _, rz1, z1) in zip(rings, rings[1:]):
        body.face([(-.004, y0, z0 + rz0 + .002), (.004, y0, z0 + rz0 + .002),
                   (.004, y1, z1 + rz1 + .002), (-.004, y1, z1 + rz1 + .002)], 2)
    y, rx, rz, z = rings[-1]
    body.section([(y - .009, rx + .001, rz + .001, z),
                  (y + .004, rx, rz, z)], 1, sides=16, smooth=True, roundness=3.5)
    if parka:
        # A small folded hood with a narrow lining, rather than a shoulder block.
        body.section([(.118, .124, .065, -.112), (.166, .144, .073, -.112),
                      (.202, .116, .052, -.112)], 0, sides=16, smooth=True)
        for i in range(16):
            a, b = i * math.tau / 16, (i + 1) * math.tau / 16
            body.face([(.116 * math.cos(a), .204, -.112 - .052 * math.sin(a)),
                       (.116 * math.cos(b), .204, -.112 - .052 * math.sin(b)),
                       (.104 * math.cos(b), .204, -.112 - .040 * math.sin(b)),
                       (.104 * math.cos(a), .204, -.112 - .040 * math.sin(a))], 3)
    # Small angled welt openings read as cloth pockets, with no protruding pouches.
    for side in (-1, 1):
        body.face([(side * .064, -.157, .124 if parka else .110),
                   (side * .125, -.127, .100 if parka else .086),
                   (side * .125, -.121, .100 if parka else .086),
                   (side * .064, -.151, .124 if parka else .110)]
                  [::1 if side > 0 else -1], 1)
    pieces = [("Body", body, (0, 0, 0))]
    for side, label in [(1, "SleeveLeft"), (-1, "SleeveRight")]:
        sleeve = Item(pal)
        upper = Item(pal)
        upper.section([(-.255 if parka else -.212, .063 if parka else .058, .077 if parka else .068, 0),
                       (-.12, .080 if parka else .072, .094 if parka else .084, 0),
                       (.055, .095 if parka else .090, .115 if parka else .112, 0)], 0, sides=16, smooth=True)
        sleeve.add(upper, turn=(0, 0, side * 90))
        if not parka:
            cuff = Item(pal)
            cuff.section([(-.221, .059, .069, 0), (-.207, .060, .070, 0)], 1, sides=16, smooth=True)
            sleeve.add(cuff, turn=(0, 0, side * 90))
        pieces.append((label, sleeve, (side * .212, .145, -.072)))
    if parka:
        for side, label in [(1, "ForearmLeft"), (-1, "ForearmRight")]:
            sleeve, lower, cuff = Item(pal), Item(pal), Item(pal)
            lower.section([(-.231, .048, .058, 0), (-.13, .063, .076, 0), (.008, .065, .079, 0)], 0, sides=16, smooth=True)
            cuff.section([(-.240, .048, .058, 0), (-.220, .050, .060, 0)], 1, sides=16, smooth=True)
            sleeve.add(lower, turn=(0, 0, side * 90))
            sleeve.add(cuff, turn=(0, 0, side * 90))
            pieces.append((label, sleeve, (side * .463, .145, -.080)))
    assembled = Item(pal)
    for _, geometry, at in pieces:
        assembled.add(geometry, at)
    return assembled, pieces


def wool_hat():
    g = Item(palette((.40, .31, .28), (.29, .25, .22), CREAM, OLIVE))
    g.section([(.112, .119, .130, -.010), (.166, .124, .137, -.010),
               (.213, .114, .129, -.014), (.247, .083, .101, -.020),
               (.263, .035, .050, -.023)], 0, sides=16, smooth=True)
    # A softly rolled knit cuff, with a small fabric tag instead of a badge.
    g.section([(.106, .120, .132, -.010), (.124, .129, .141, -.010),
               (.146, .124, .136, -.010)], 1, sides=16, smooth=True)
    g.face([(-.014, .118, .132), (.014, .118, .132), (.014, .131, .132), (-.014, .131, .132)], 4)
    return g


# Cyberware, belt items, leg and face gear (stored props only: none are worn models).
NEW_ITEMS = [
    ("bionic_arm", "BionicArm", lambda: prosthetic(bionic=True)),
    ("bionic_leg", "BionicLeg", lambda: prosthetic(True, bionic=True)),
    ("bionic_eyes", "BionicEyes", bionic_eyes), ("neural_coprocessor", "NeuralCoprocessor", neural_coprocessor),
    ("reinforced_spine", "ReinforcedSpine", reinforced_spine), ("subdermal_plating", "SubdermalPlating", subdermal_plating),
    ("synthetic_heart", "SyntheticHeart", synthetic_heart), ("filter_lungs", "FilterLungs", filter_lungs),
    ("frag_grenade", "FragGrenade", frag_grenades), ("field_medkit", "FieldMedkit", field_medkit),
    ("cargo_trousers", "CargoTrousers", cargo_trousers), ("flak_leg_guards", "FlakLegGuards", flak_leg_guards),
    ("tactical_goggles", "TacticalGoggles", tactical_goggles),
    ("field_pack", "FieldPack", backpack), ("medic_pack", "MedicPack", lambda: backpack("medic")),
    ("radio_pack", "RadioPack", lambda: backpack("radio")),
]


def models():
    builders = [
        ("wood", "Wood", wood), ("planks", "Planks", planks), ("stone", "Stone", stone),
        ("stone_blocks", "StoneBlocks", blocks), ("iron_ore", "IronOre", lambda: stone("iron")),
        ("steel", "Steel", ingots), ("copper_ore", "CopperOre", lambda: stone("copper")),
        ("copper", "Copper", lambda: ingots(True)), ("latex", "Latex", latex), ("rubber", "Rubber", rubber),
        ("silicon", "Silicon", silicon), ("scrap_metal", "ScrapMetal", scrap), ("e_waste", "EWaste", lambda: circuit(True)),
        ("mechanical_components", "MechanicalComponents", mechanical), ("electrical_components", "ElectricalComponents", circuit),
        ("optical_components", "OpticalComponents", optical), ("structural_components", "StructuralComponents", structural),
        ("chemical_components", "ChemicalComponents", chemical), ("textile_components", "TextileComponents", lambda: fabric(padding=True)),
        ("cotton", "Cotton", lambda: fabric(cotton=True)), ("cloth", "Cloth", fabric),
        ("raw_food", "RawFood", food), ("meal", "Meal", meal), ("healroot", "Healroot", healroot),
        ("herbal_medicine", "HerbalMedicine", lambda: medicine(True)), ("medicine", "Medicine", medicine),
        ("prosthetic_arm", "ProstheticArm", prosthetic), ("prosthetic_leg", "ProstheticLeg", lambda: prosthetic(True)),
    ] + NEW_ITEMS
    for ident, name, build in builders:
        yield ident, name, build(), None
    for ident, name, parka in [("clothes", "SimpleClothes", False), ("parka", "Parka", True)]:
        assembled, pieces = jacket(parka)
        yield ident, name, assembled, pieces
    hat = wool_hat()
    yield "wool_hat", "WoolHat", hat, [("Hat", hat, (0, 0, 0))]


if __name__ == "__main__":
    for _, name, geometry, pieces in models():
        geometry.write(name, ROOT / "assets/models/items", pieces)
