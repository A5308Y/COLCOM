#!/usr/bin/env python3
"""Remaining colony machinery and stateful structure props; flat-colour frontier style.

    python3 -B tools/make_world_props.py
Geometry retains the definitions' footprints, contact surfaces and local facing.
"""
import math
from pathlib import Path
from asset_helpers import Mesh

ROOT = Path(__file__).resolve().parents[1]


class Station(Mesh):
    def __init__(self, primary=(.36,.39,.27), top=(.48,.35,.22), status=(.9,.49,.16)):
        super().__init__([("Panels",primary,.12,.84), ("Gunmetal",(.27,.30,.29),.65,.64),
                          ("Work surface",top,.1,.88), ("Status",status,0,.6),
                          ("Trim",(.56,.59,.52),.45,.65)], {3:[v*.5 for v in status]})

    def bench(self, width=.92, depth=.60, top=.535, pedestal=False):
        if pedestal:
            self.box((0,(top-.07)/2,0),(width-.12,top-.07,depth-.10),0,bevel=.015)
            for x in (-width*.34,width*.34):
                self.box((x,top*.5,depth/2-.044),(.018,top*.66,.014),1,bevel=.002)
        else:
            for x in (-width/2+.065,width/2-.065):
                for z in (-depth/2+.045,depth/2-.045):
                    self.box((x,(top-.05)/2,z),(.065,top-.05,.065),1,bevel=.007)
            self.box((0,.16,0),(width-.08,.045,depth-.06),0,bevel=.005)
        self.box((0,top-.025,0),(width,.05,depth),2,bevel=.007)
        for x in (-width/2+.035,width/2-.035):
            self.cylinder((x,top+.002,depth/2-.035),.012,.004,4,sides=6)

    def vents(self, at, width, count=5):
        for j in range(count):
            self.box((at[0]+width*(j/(count-1)-.5),at[1],at[2]),(.018,.12,.008),1,bevel=.002)

    def console(self, at, width=.20):
        self.box(at,(width,.13,.035),1,bevel=.006)
        self.box((at[0],at[1]+.015,at[2]+.02),(width*.72,.06,.006),3,bevel=.002)
        for x in (-.25,.25):
            self.cylinder((at[0]+width*x,at[1]-.04,at[2]+.024),.012,.012,4,axis="z",sides=6)


def stove():
    g=Station(primary=(.46,.46,.40),top=(.24,.27,.25))
    g.box((0,.35,0),(.9,.7,.75),0,bevel=.025)
    g.box((0,.72,0),(.92,.05,.77),1,bevel=.009)
    g.box((0,.29,.38),(.56,.36,.018),1,bevel=.012)
    g.box((0,.29,.392),(.42,.22,.012),2,bevel=.009)
    g.box((0,.49,.402),(.42,.026,.035),4,bevel=.005)
    for x in (-.2,.2):
        g.cylinder((x,.75,0),.11,.02,1,bore=.07)
        for z in (-.06,.06):g.box((x,.764,z),(.24,.012,.012),4,bevel=.002)
        g.cylinder((x,.61,.383),.035,.025,1,axis="z")
        g.box((x,.61,.399),(.008,.025,.004),3,bevel=.001)
    g.cylinder((.32,.82,-.26),.05,.2,1)
    return g


def sawbench():
    g=Station();g.bench(.95,.6,.535)
    g.cylinder((0,.6,-.08),.07,.7,2,axis="x")
    g.cylinder((.12,.62,.12),.16,.02,4,axis="z")
    for j in range(12):
        a=j*math.tau/12
        tooth=Station();tooth.box((0,0,0),(.024,.045,.022),4,bevel=.002)
        g.add(tooth,(.12+.15*math.sin(a),.62+.15*math.cos(a),.12),(0,0,-math.degrees(a)))
    g.box((.12,.58,.10),(.035,.10,.05),1,bevel=.004)
    return g


def stonecutter():
    g=Station(top=(.58,.57,.51));g.bench(.95,.65,.58,True)
    for x,z in [(-.2,0),(.05,.08)]:g.box((x,.66,z),(.18,.16,.18),2,bevel=.018)
    g.box((.28,.6,-.12),(.20,.04,.05),4,bevel=.003)
    g.vents((0,.26,.278),.38)
    return g


def smelter():
    g=Station(primary=(.47,.46,.40),top=(.32,.34,.31))
    g.box((0,.425,0),(.85,.85,.85),0,bevel=.035)
    g.box((0,.25,.425),(.46,.34,.035),1,bevel=.018)
    g.box((0,.25,.446),(.34,.22,.012),3,bevel=.009)
    for x in (-.12,0,.12):g.box((x,.25,.458),(.024,.24,.016),1,bevel=.003)
    for y in (.10,.43,.70):g.box((0,y,.427),(.72,.018,.014),4,bevel=.002)
    g.cylinder((0,.885,0),.2,.07,1,bore=.15)
    return g


def smithy():
    g=Station(primary=(.47,.46,.40))
    g.box((-.25,.25,0),(.45,.5,.7),0,bevel=.018)
    g.box((-.25,.51,0),(.3,.02,.45),3,bevel=.004)
    for z in (-.3,.3):g.box((-.25,.54,z),(.44,.08,.07),1,bevel=.007)
    g.box((.25,.2,0),(.18,.4,.18),1,bevel=.008)
    g.box((.25,.42,0),(.32,.06,.19),1,bevel=.007)
    anvil=Station()
    anvil.profile([(.42,-.10),(.42,.10),(.49,.19),(.51,.13),(.51,-.16),(.46,-.22)],.3,4)
    g.add(anvil,(.25,0,0))
    return g


def crafting():
    g=Station();g.bench(.95,.62,.585)
    g.box((-.2,.61,.05),(.3,.05,.25),0,bevel=.008)
    for x in (-.29,-.20,-.11):g.box((x,.638,.05),(.006,.003,.21),2,bevel=.001)
    g.box((.25,.60,-.1),(.18,.03,.04),4,bevel=.003)
    g.cylinder((.30,.66,.13),.035,.12,2,sides=6)
    return g


def machinist():
    g=Station(top=(.51,.54,.51));g.bench(top=.535,pedestal=True)
    g.box((-.3,.60,0),(.12,.14,.18),1,bevel=.007)
    g.box((.28,.65,-.15),(.10,.24,.12),0,bevel=.008)
    g.box((.13,.78,-.15),(.38,.06,.13),1,bevel=.007)
    g.cylinder((.04,.69,-.13),.025,.12,4,sides=6)
    g.cylinder((.2,.62,.05),.09,.04,4,axis="z")
    g.vents((0,.22,.255),.40)
    return g


def refinery():
    g=Station(top=(.59,.39,.24))
    g.cylinder((-.18,.45,0),.25,.90,1,sides=10)
    for y in (.035,.4,.86):g.cylinder((-.18,y,0),.255,.022,4,sides=10)
    g.cylinder((.25,.3,.1),.13,.60,2)
    pipe=Station();pipe.cylinder((0,0,0),.03,.4,1,sides=6)
    g.add(pipe,(.05,.75,.05),(0,0,70))
    g.box((-.18,.12,.26),(.24,.12,.02),3,bevel=.004)
    g.console((.25,.48,.235),.12)
    return g


def chemistry():
    g=Station(primary=(.77,.79,.71),top=(.80,.81,.74));g.bench(top=.53,pedestal=True)
    g.ball((-.25,.62,.05),.08,2,squash=1.125,sides=8)
    g.cylinder((-.25,.73,.05),.022,.07,4,sides=6)
    for x,y,z,r in [(0,.62,-.05,.04),(.25,.64,.05,.05)]:
        g.cylinder((x,y,z),r,.20 if x==0 else .24,2,sides=8)
        g.cylinder((x,y+.11,z),r*1.1,.025,0,sides=8)
    g.box((0,.56,.20),(.16,.018,.12),1,bevel=.004)
    g.console((-.26,.35,.255),.16)
    return g


def electronics():
    g=Station(status=(.26,.64,.66));g.bench(top=.53,pedestal=True)
    g.box((0,.72,-.2),(.5,.3,.04),1,bevel=.012)
    g.box((0,.72,-.175),(.44,.24,.01),3,bevel=.006)
    for x in (-.14,0,.14):g.box((x,.73,-.168),(.08,.007,.003),4,bevel=.001)
    g.box((.15,.545,.10),(.26,.02,.18),0,bevel=.003)
    for x in (.07,.14,.21):g.box((x,.56,.10),(.025,.012,.08),1,bevel=.001)
    g.cylinder((-.30,.565,.15),.03,.07,4,axis="x",sides=6)
    return g


def fabricator():
    g=Station(primary=(.49,.51,.45),status=(.26,.64,.66))
    # Open fabrication bay: framing and a recessed bed instead of transparent glass.
    g.box((0,.12,0),(.94,.24,.70),0,bevel=.02)
    g.box((0,.84,0),(.94,.12,.70),0,bevel=.015)
    for x in (-.42,.42):g.box((x,.51,0),(.10,.54,.70),1,bevel=.012)
    g.box((0,.50,-.32),(.74,.48,.06),0,bevel=.01)
    g.box((0,.28,0),(.65,.05,.52),4,bevel=.004)
    g.box((0,.70,0),(.66,.035,.04),4,bevel=.004)
    g.cylinder((0,.62,0),.03,.15,1,sides=6)
    g.box((.35,.82,.355),(.08,.04,.01),3,bevel=.002)
    return g


def salvage():
    g=Station(primary=(.42,.31,.22),top=(.45,.32,.22));g.bench(top=.535,pedestal=True)
    g.box((-.2,.60,.05),(.24,.12,.18),0,bevel=.018,tilt=8)
    g.box((.2,.555,-.05),(.2,.02,.16),2,bevel=.004)
    g.box((-.30,.65,-.20),(.16,.10,.08),1,bevel=.004)
    g.vents((0,.25,.255),.40)
    return g


def solar():
    g=Station(top=(.22,.31,.39))
    g.cylinder((0,.3,0),.04,.6,1)
    panel=Station(top=(.22,.31,.39));panel.box((0,0,0),(.96,.04,.8),4,bevel=.007)
    for x in range(4):
        for z in range(3):panel.box((-.3375+x*.225,.025,-.247+z*.247),(.213,.012,.235),2,bevel=.002)
    g.add(panel,(0,.66,0),(25,0,0))
    return g


def heater():
    g=Station(primary=(.77,.79,.71))
    g.box((0,.25,0),(.7,.42,.22),0,bevel=.012)
    g.box((0,.02,0),(.72,.04,.26),1,bevel=.005)
    g.box((0,.26,.115),(.56,.24,.01),3,bevel=.004)
    for x in (-.24,-.16,-.08,0,.08,.16,.24):g.box((x,.26,.126),(.014,.25,.012),1,bevel=.002)
    return g


def cooler():
    g=Station(primary=(.77,.79,.71),status=(.26,.64,.66))
    g.box((0,.35,0),(.7,.66,.4),0,bevel=.018)
    g.box((0,.01,0),(.64,.02,.36),1,bevel=.004)
    g.cylinder((0,.38,.205),.2,.018,1,axis="z",bore=.16,sides=10)
    for j in range(5):
        a=j*math.tau/5
        fan=Station();fan.box((0,.07,0),(.045,.16,.014),4,bevel=.005)
        g.add(fan,(0,.38,.205),(0,0,math.degrees(a)))
    g.box((.25,.62,.205),(.06,.03,.01),3,bevel=.002)
    return g


def battery():
    g=Station(primary=(.77,.79,.71),status=(.26,.64,.66))
    g.box((0,.4,.1),(.6,.8,.3),0,bevel=.018)
    g.box((0,.02,.1),(.62,.04,.32),1,bevel=.005)
    g.box((0,.62,-.055),(.12,.03,.01),3,bevel=.002)
    for x in (-.20,.20):g.box((x,.4,-.056),(.012,.62,.012),4,bevel=.002)
    g.vents((0,.18,-.058),.30)
    return g


def stairs():
    g=Station()
    for j in range(4):
        x=-.375+j*.25;h=(j+1)*.25
        start=len(g.groups[0][2])
        g.box((x,h/2,0),(.25,h,.9),0,bevel=0)
        _,normals,indices=g.groups[0]
        # Replace the top face with adjacent wood/grip regions at the exact
        # contact height. Layering markings over the tread would z-fight.
        triangles=[indices[k:k+3] for k in range(start,len(indices),3)]
        indices[start:]=[i for tri in triangles if normals[tri[0]][1]<.999 for i in tri]
        def top(left,right,near,far,material):
            g.face([(left,h,near),(left,h,far),(right,h,far),(right,h,near)],material)
        top(x-.125,x-.10,-.45,.45,0)
        top(x+.10,x+.125,-.45,.45,0)
        edges=[-.45,-.32,-.26,-.03,.03,.26,.32,.45]
        for k,(near,far) in enumerate(zip(edges,edges[1:])):
            top(x-.10,x+.10,near,far,1 if k in (1,3,5) else 0)
    return g


def ladder():
    g=Station()
    for z in (-.16,.16):g.box((.4,.5,z),(.04,1.,.04),1,bevel=.004)
    for y in (.1,.3,.5,.7,.9):g.box((.4,y,0),(.03,.03,.32),2,bevel=.003)
    return g


def grave():
    marker=Station(primary=(.34,.26,.16),top=(.35,.25,.16))
    marker.box((0,.3,-.42),(.06,.6,.06),2,bevel=.006)
    marker.box((0,.44,-.42),(.3,.06,.06),2,bevel=.006)
    empty=Station(primary=(.19,.16,.12),top=(.40,.30,.20))
    empty.box((0,.01,.05),(.46,.02,.78),0,bevel=0)
    empty.box((.36,.07,.05),(.20,.14,.76),2,bevel=.018)
    filled=Station(primary=(.40,.30,.20),top=(.40,.30,.20))
    filled.box((0,.06,.05),(.52,.12,.8),2,bevel=.018)
    filled.box((0,.13,.05),(.36,.06,.66),2,bevel=.012)
    # All pieces share the first palette; indices 0/2 encode dark pit and earth.
    marker.palette[0]=("Dark earth",(.19,.16,.12),0,.95)
    marker.palette[2]=("Earth and marker",(.40,.30,.20),0,.95)
    pieces=[("Marker",marker,(0,0,0)),("Pit",empty,(0,0,0)),("Mound",filled,(0,0,0))]
    return marker,pieces,[("Empty",[0,1]),("Filled",[0,2])]


def fence():
    post=Station();post.box((0,.35,0),(.12,.7,.12),2,bevel=.008)
    pieces=[("Post",post,(0,0,0))]
    for i,(x,z) in enumerate([(1,0),(-1,0),(0,1),(0,-1)]):
        rail=Station()
        for y in (.25,.5):rail.box((x*.25,y,z*.25),(.5 if x else .05,.07,.5 if z else .05),0,bevel=.004)
        pieces.append((f"Rail{i}",rail,(0,0,0)))
    return post,pieces,[(f"Links{mask}",[0]+[i+1 for i in range(4) if mask & (1<<i)]) for mask in range(16)]


STATIC = [("stove","Stove",stove),("sawbench","Sawbench",sawbench),("stonecutter","Stonecutter",stonecutter),
          ("smelter","Smelter",smelter),("smithy","Smithy",smithy),("crafting_bench","CraftingBench",crafting),
          ("machinist_bench","MachinistBench",machinist),("refinery","Refinery",refinery),
          ("chemistry_lab","ChemistryLab",chemistry),("electronics_bench","ElectronicsBench",electronics),
          ("fabricator","Fabricator",fabricator),("salvage_bench","SalvageBench",salvage),
          ("pv_system","PVSystem",solar),("electric_heater","ElectricHeater",heater),("cooler","Cooler",cooler),
          ("home_battery","HomeBattery",battery),("stairs","Stairs",stairs),("ladder","Ladder",ladder)]


def models():
    for ident,name,build in STATIC:
        yield ident,name,build(),None,None
    for ident,name,build in [("grave","Grave",grave),("fence","Fence",fence)]:
        mesh,pieces,scenes=build()
        yield ident,name,mesh,pieces,scenes


def write_all(out=ROOT / "assets/models/world"):
    for _,name,mesh,pieces,scenes in models():
        Mesh.write(mesh,name,out,pieces,scenes=scenes,generator="My World make_world_props.py")


if __name__ == "__main__":
    write_all()
