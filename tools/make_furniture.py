#!/usr/bin/env python3
"""Generate room furniture and fixtures in the faceted frontier sci-fi style.

    python3 tools/make_furniture.py

Standard library only. Geometry is in cell units, +Y up. Existing interaction,
storage and wall-slot coordinates are kept as the reference dimensions.
"""

import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from make_items import Item

ROOT = Path(__file__).resolve().parents[1]


class Furniture(Item):
    def __init__(self, medical=False, owner=False, glow=False):
        super().__init__([
            ("Timber" if not medical else "Enamel", (.48, .35, .22) if not medical else (.77, .79, .71), 0, .87),
            ("Gunmetal", (.27, .30, .29), .65, .64),
            ("Olive panels", (.36, .39, .27), .12, .84),
            ("Owner fabric" if owner else "Fabric", (.64, .68, .60), 0, .95),
            ("Trim", (.56, .59, .52), .45, .65),
        ])
        if owner:
            self.palette[4] = ("Linen", (.83, .82, .74), 0, .95)
        if medical:
            self.palette[3] = ("Upholstery", (.45, .53, .47), 0, .93)
            self.palette[4] = ("Medical marking", (.80, .48, .20), 0, .82)
        if glow:
            self.palette[3] = ("Warm diffuser", (.94, .75, .43), 0, .88)
            self.emissive[3] = [3.5, 2.6, 1.3]

    def legs(self, width, depth, height, material=1):
        for x in (-width / 2 + .065, width / 2 - .065):
            for z in (-depth / 2 + .065, depth / 2 - .065):
                self.box((x, height / 2, z), (.055, height, .055), material, bevel=.007)
                self.box((x, .015, z), (.071, .03, .071), 2, bevel=.005)

    def bolts(self, positions):
        for at in positions:
            self.cylinder(at, .014, .006, 4, "z", sides=6)


def bed(mat=False):
    g = Furniture(owner=True)
    if mat:
        g.box((0, .022, 0), (.80, .044, .90), 0, bevel=.014)
        g.box((0, .05, .12), (.72, .03, .50), 3, bevel=.008)
        g.box((0, .05, -.30), (.50, .05, .20), 4, bevel=.017)
        for x in (-.35, .35):
            g.box((x, .042, .12), (.013, .014, .48), 2, bevel=.002)
    else:
        g.legs(.78, .86, .24)
        for x in (-.385, .385):
            g.box((x, .16, 0), (.05, .13, .86), 0, bevel=.009)
        for z in (-.435, .435):
            g.box((0, .15, z), (.82, .12, .05), 0, bevel=.008)
        g.box((0, .35, -.435), (.82, .38, .052), 0, bevel=.014)
        g.box((0, .48, -.437), (.70, .055, .056), 2, bevel=.008)
        g.box((0, .29, 0), (.74, .08, .84), 4, bevel=.019)
        g.box((0, .35, -.30), (.52, .08, .20), 4, bevel=.022)
        g.box((0, .34, .13), (.76, .08, .56), 3, bevel=.015)
        for x in (-.358, .358):
            g.box((x, .381, .13), (.009, .003, .50), 2, bevel=.001)
        g.bolts([(-.34, .17, .464), (.34, .17, .464)])
    return g


def table(chess=False):
    g = Furniture()
    width, height = (.60, .40) if chess else (.90, .50)
    g.legs(width, width, height - .03)
    for i in range(5):
        x = -width / 2 + width / 10 + i * width / 5
        g.box((x, height, 0), (width / 5 - .004, .06 if not chess else .05, width), 0, bevel=.006)
    for z in (-width * .34, width * .34):
        g.box((0, height - .085, z), (width * .86, .065, .037), 1, bevel=.006)
    if chess:
        g.box((0, .43, 0), (.43, .01, .43), 1, bevel=.004)
        for x in range(8):
            for z in range(8):
                x0, z0 = -.20 + x * .05, -.20 + z * .05
                g.face([(x0, .436, z0), (x0, .436, z0 + .05),
                        (x0 + .05, .436, z0 + .05), (x0 + .05, .436, z0)], 3 if (x + z) % 2 else 1)
        for x, z, material in [(-.175, -.175, 1), (-.125, -.125, 1), (.025, -.175, 1), (.125, -.125, 1),
                                (-.125, .125, 3), (-.025, .175, 3), (.075, .125, 3), (.175, .175, 3)]:
            g.cylinder((x, .446, z), .016, .02, material, sides=6)
            g.cylinder((x, .470, z), .010, .03, material, sides=6)
    return g


def chair():
    g = Furniture()
    g.legs(.42, .42, .285)
    g.box((0, .286, 0), (.42, .028, .42), 0, bevel=.008)
    g.box((0, .311, .006), (.37, .027, .36), 2, bevel=.012)
    for x in (-.185, .185):
        g.box((x, .49, -.19), (.035, .48, .05), 1, bevel=.006)
    for x in (-.126, -.042, .042, .126):
        g.box((x, .55, -.19), (.078, .33, .045), 0, bevel=.007)
    g.box((0, .70, -.19), (.42, .055, .06), 2, bevel=.008)
    return g


def shelf(books=False):
    g = Furniture()
    if books:
        g.box((0, .45, -.29), (.884, .884, .02), 1, bevel=.007)
        for x in (-.425, .425):
            g.box((x, .45, -.155), (.05, .884, .29), 0, bevel=.008)
        for y in (.025, .28, .55, .875):
            g.box((0, y, -.145), (.884, .05, .29), 0, bevel=.006)
        for row, base in enumerate((.305, .575)):
            for i in range(9):
                h = (.155, .19, .205)[(i + row) % 3]
                x = -.35 + i * .086
                g.box((x, base + h / 2, -.12), (.066, h, .235), (0, 2, 3)[(i + row) % 3], bevel=.003)
                g.box((x, base + h * .75, -.001), (.045, .008, .005), 4, bevel=.001)
    else:
        for x in (-.44, .44):
            for z in (-.30, .30):
                g.box((x, .35, z), (.06, .70, .058), 1, bevel=.006)
                g.box((x, .025, z), (.068, .05, .06), 2, bevel=.005)
        for y in (.30, .60):
            # Inset from the post faces to avoid coplanar intersections.
            g.box((0, y, 0), (.93, .04, .65), 0, bevel=.007)
            # Trim stays below the surface where the stored items stand.
            for z in (-.317, .317):
                g.box((0, y - .018, z), (.90, .025, .02), 2, bevel=.003)
    return g


def dresser(medical=False):
    g = Furniture(medical=medical)
    if medical:
        g.box((0, .45, .15), (.70, .90, .40), 1, bevel=.014)
        for x in (-.165, .165):
            g.box((x, .45, -.061), (.318, .84, .022), 0, bevel=.009)
            g.box((x * .22, .43, -.081), (.023, .12, .019), 1, bevel=.004)
        g.box((-.16, .63, -.078), (.035, .16, .009), 4, bevel=.001)
        for side in (-1, 1):
            g.box((-.16 + side * .057, .63, -.078), (.078, .035, .009), 4, bevel=.001)
        g.box((.16, .78, -.078), (.11, .035, .009), 2, bevel=.003)
    else:
        g.box((0, .33, .12), (.80, .62, .50), 1, bevel=.012)
        g.box((0, .66, .12), (.84, .04, .54), 0, bevel=.008)
        for x in (-.38, .38):
            g.box((x, .33, .12), (.035, .61, .49), 0, bevel=.006)
        for y in (.225, .475):
            g.box((0, y, -.141), (.72, .205, .027), 0, bevel=.010)
            g.box((0, y + .025, -.162), (.15, .027, .02), 4, bevel=.004)
        for x in (-.33, .33):
            for z in (-.07, .31):
                g.box((x, .025, z), (.08, .05, .08), 1, bevel=.006)
    return g


def operating_table():
    g = Furniture(medical=True)
    g.box((0, .035, 0), (.60, .07, .70), 1, bevel=.018)
    g.box((0, .23, 0), (.18, .32, .49), 1, bevel=.012)
    for x in (-.10, .10):
        g.box((x, .22, 0), (.025, .22, .41), 4, bevel=.004)
    g.box((0, .42, 0), (.78, .06, .92), 1, bevel=.012)
    for z in (-.22, .22):
        g.box((0, .46, z), (.72, .03, .42), 3, bevel=.012)
    g.box((0, .49, -.32), (.40, .04, .16), 0, bevel=.012)
    return g


def wall_shelf():
    g = Furniture()
    g.box((0, .55, .06), (.19, .02, .12), 0, bevel=.003)
    for x in (-.08, .08):
        part = Furniture()
        part.profile([(.47, .006), (.54, .006), (.54, .09)], .012, 1)
        g.add(part, (x, 0, 0))
    return g


def wall_art(kind):
    g = Furniture()
    g.palette[4] = ("Print accent", (.65, .35, .22), 0, .94)
    if kind == "painting":
        for x in (-.084, .084):
            g.box((x, .62, .01), (.012, .15, .02), 0, bevel=.002)
        for y in (.551, .689):
            g.box((0, y, .01), (.156, .012, .02), 0, bevel=.002)
        g.box((0, .62, .016), (.156, .126, .006), 3, bevel=.001)
        g.face([(-.076, .565, .020), (.076, .565, .020), (-.03, .647, .020)], 2)
        g.face([(-.003, .565, .021), (.076, .565, .021), (.035, .624, .021)], 1)
        g.cylinder((.045, .656, .021), .013, .002, 4, "z")
    elif kind == "poster":
        g.box((0, .60, .003), (.15, .21, .005), 2, bevel=.001)
        g.box((0, .67, .006), (.12, .025, .002), 3, bevel=0)
        for y, width in [(.544, .10), (.526, .085), (.508, .10)]:
            g.box((0, y, .006), (width, .004, .002), 3, bevel=0)
        g.face([(-.04, .575, .007), (.04, .575, .007), (0, .63, .007)], 4)
    else:
        g.box((0, .55, .003), (.12, .15, .005), 3, bevel=.001)
        g.box((0, .615, .006), (.12, .025, .002), 2, bevel=.001)
        for x in range(7):
            for row in range(4):
                xx, yy = -.047 + x * .015, .585 - row * .026
                g.box((xx, yy, .006), (.008, .008, .002), 4 if (x + row) % 9 == 0 else 1, bevel=0)
    return g


def dartboard():
    g = Furniture()
    g.cylinder((0, .66, .009), .08, .016, 0, "z", sides=12)
    for i in range(12):
        a, b = i * math.tau / 12, (i + 1) * math.tau / 12
        g.face([(0, .66, .018), (.068 * math.cos(a), .66 + .068 * math.sin(a), .018),
                (.068 * math.cos(b), .66 + .068 * math.sin(b), .018)], 1 if i % 2 else 3)
    g.cylinder((0, .66, .02), .028, .003, 2, "z")
    g.cylinder((0, .66, .023), .01, .003, 4, "z")
    return g


def wall_plant():
    g = Furniture()
    g.palette[4] = ("Leaf tips", (.47, .56, .30), 0, .94)
    g.box((0, .66, .03), (.018, .018, .06), 1, bevel=.003)
    g.cylinder((0, .62, .07), .045, .07, 0, bore=.034)
    g.rock((0, .678, .07), (.12, .08, .115), 2, seed=4, flecks=4)
    for x in (-.034, .034):
        g.rock((x, .614, .09), (.043, .09, .042), 2, seed=3, flecks=4)
    return g


def lamp(wall=False, torch=False):
    g = Furniture(glow=True)
    if wall:
        g.box((0, .74, .006), (.055, .09, .012), 1, bevel=.003)
        g.box((0, .74, .038), (.018, .018, .064), 1, bevel=.002)
        g.box((0, .76, .077), (.074, .064, .065), 3, bevel=.007)
        for x in (-.039, .039):
            g.box((x, .76, .077), (.009, .078, .072), 1, bevel=.002)
        g.box((0, .80, .077), (.09, .014, .078), 2, bevel=.003)
    else:
        g.cylinder((0, .03, 0), .15, .06, 1)
        g.cylinder((0, .39, 0), .033, .72, 0)
        if torch:
            g.emissive[3] = [12, 5, 1]
            g.cylinder((0, .77, 0), .08, .08, 1)
            g.rock((0, .82, 0), (.12, .18, .12), 3, seed=2)
        else:
            g.cylinder((0, .80, 0), .088, .16, 3)
            for y in (.71, .90):
                g.cylinder((0, y, 0), .105, .02, 2)
            for x in (-.067, .067):
                for z in (-.067, .067):
                    g.box((x, .80, z), (.012, .18, .012), 1, bevel=.002)
    return g


def fireplace():
    g = Furniture(glow=True)
    g.palette[2] = ("Stone", (.49, .48, .43), 0, .94)
    g.emissive[3] = [8, 3, 1]
    g.box((0, .04, -.02), (.80, .08, .56), 2, bevel=.013)
    for x in (-.33, .33):
        g.box((x, .30, -.05), (.14, .44, .50), 2, bevel=.012)
    g.box((0, .30, -.275), (.53, .44, .05), 1, bevel=.007)
    g.box((0, .56, -.05), (.80, .09, .50), 2, bevel=.012)
    g.box((0, .62, .19), (.90, .05, .14), 0, bevel=.008)
    g.box((0, .74, -.18), (.42, .28, .24), 2, bevel=.016)
    for z in (.11, .19):
        g.cylinder((0, .11, z), .028, .35, 0, "x")
    for x in (-.10, .08):
        g.rock((x, .23, .15), (.12, .22, .10), 3, seed=2)
    for x in (-.18, -.09, 0, .09, .18):
        g.box((x, .14, .218), (.015, .13, .017), 1, bevel=.002)
    return g


def models():
    for ident, name, build in [
        ("bed", "Bed", bed), ("sleeping_spot", "SleepingSpot", lambda: bed(True)),
        ("table", "Table", table), ("chair", "Chair", chair), ("chess_table", "ChessTable", lambda: table(True)),
        ("bookshelf", "Bookshelf", lambda: shelf(True)), ("shelf", "Shelf", shelf),
        ("dresser", "Dresser", dresser), ("medical_cabinet", "MedicalCabinet", lambda: dresser(True)),
        ("operating_table", "OperatingTable", operating_table),
        ("painting", "Painting", lambda: wall_art("painting")), ("poster", "Poster", lambda: wall_art("poster")),
        ("calendar", "Calendar", lambda: wall_art("calendar")), ("dartboard", "Dartboard", dartboard),
        ("wall_shelf", "WallShelf", wall_shelf), ("wall_plant", "WallPlant", wall_plant),
        ("lamp", "Lamp", lamp), ("wall_lamp", "WallLamp", lambda: lamp(wall=True)),
        ("torch", "Torch", lambda: lamp(torch=True)), ("fireplace", "Fireplace", fireplace),
    ]:
        yield ident, name, build(), None


if __name__ == "__main__":
    for _, name, geometry, pieces in models():
        geometry.write(name, ROOT / "assets/models/furniture", pieces)
