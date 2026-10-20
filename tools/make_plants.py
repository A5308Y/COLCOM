#!/usr/bin/env python3
"""Original flat-colour healroot, cotton and rubber plants, using shared mesh helpers.

    python3 -B tools/make_plants.py
Each asset is one stalk; definition placements assemble the four-stalk crop plots.
"""
import math
from pathlib import Path

from asset_helpers import Mesh, cross, unit

ROOT = Path(__file__).resolve().parents[1]
PALETTE = [("Stem", (.32, .25, .15), 0, .95), ("Leaf", (.25, .43, .22), 0, .95),
           ("New growth", (.43, .58, .29), 0, .95), ("Flower", (.65, .30, .57), 0, .9),
           ("Cotton / latex", (.88, .84, .69), 0, .95)]


def leaf(g, start, tip, width, material=1):
    axis = tuple(b-a for a,b in zip(start, tip))
    side = unit(cross(axis, (0, 1, .1)))
    mid = tuple((a+b)/2 for a,b in zip(start, tip))
    edges = [start, tuple(mid[i]+side[i]*width for i in range(3)), tip,
             tuple(mid[i]-side[i]*width for i in range(3))]
    ridge = tuple(mid[i]+(.018 if i==1 else 0) for i in range(3))
    for i in range(4):
        tri = [edges[i], edges[(i+1)%4], ridge]
        g.face(tri, material)
        g.face(list(reversed(tri)), material)


def plant(species, stage):
    g = Mesh(PALETTE)
    if species == "RubberPlant":
        height = [.16, .58, .925][stage-1]
        g.cylinder((0,height/2,0), .012 if stage==1 else .025, height, 0, sides=6)
        for j in range(2 if stage==1 else 5 if stage==2 else 7):
            angle = j*2.4
            y = height*(.38+.065*j)
            reach = .08 if stage==1 else .19 if stage==2 else .26
            leaf(g, (0,y,0), (math.cos(angle)*reach,y+height*.18,math.sin(angle)*reach), reach*.32, 1+j%2)
        if stage==3:
            # A tiny collection cup and tapping band match the mature fallback.
            g.cylinder((.065,.18,.065),.042,.065,4,bore=.032,sides=6)
            g.box((.022,.23,.022),(.04,.018,.04),0,bevel=.003)
    else:
        height = ([.14,.22,.34,.43] if species=="Healroot" else [.14,.24,.40,.59])[stage-1]
        g.cylinder((0,height*.45,0), .008, height*.9, 0, sides=6)
        for j in range(2 if stage==1 else 4 if stage==2 else 6):
            angle = j*2.4
            y = height*(.15+.085*j)
            reach = .07 if stage==1 else .12 if species=="Healroot" else .16
            leaf(g,(0,y,0),(reach*math.cos(angle),y+.05,reach*math.sin(angle)),reach*.32,1+j%2)
        if stage>=3:
            for j in range(1 if stage==3 else 3):
                a=j*math.tau/3
                x,z = (.07*math.cos(a),.07*math.sin(a)) if j else (0,0)
                y=height-.035-j*.025
                if species=="Cotton":
                    g.ball((x,y-.02,z),.035,0,sides=6)
                    for k in range(3):
                        a=k*math.tau/3
                        g.ball((x+.018*math.cos(a),y,z+.018*math.sin(a)),.032 if stage==4 else .018,4,sides=6)
                else:
                    g.ball((x,y,z),.027,3,sides=6)
                    for k in range(5):
                        a=k*math.tau/5
                        leaf(g,(x,y,z),(x+.055*math.cos(a),y-.01,z+.055*math.sin(a)),.016,3)
    return g


def models():
    for species, stages in [("Healroot",4),("Cotton",4),("RubberPlant",3)]:
        for stage in range(1,stages+1):
            yield "plants", species+str(stage), plant(species,stage), None


def write_all(out=ROOT / "assets/models/plants"):
    for _,name,geometry,pieces in models():
        geometry.write(name,out,pieces,generator="My World make_plants.py")


if __name__ == "__main__":
    write_all()
