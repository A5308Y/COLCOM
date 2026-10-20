#!/usr/bin/env python3
"""CPU review at gameplay scale, using the Rust definitions' exact model transforms.

    python3 -B tools/preview_world_props.py --out target/world-props-review
    cargo run --release --example look -- --world-props  # actual GPU review
"""
import argparse
import json
from pathlib import Path
import subprocess

from asset_helpers import Mesh, matrix, multiply, point, unit
from gltf_pose import Gltf
from preview_gear import write_gallery

ROOT = Path(__file__).resolve().parents[1]


def srgb(v):
    return v*12.92 if v<=.0031308 else 1.055*v**(1/2.4)-.055


def geometry(entry):
    source=Gltf(ROOT / 'assets' / entry['path'])
    palette=[]
    for m in source.data['materials']:
        pbr=m['pbrMetallicRoughness']
        palette.append((m.get('name','Material'),tuple(srgb(v) for v in pbr['baseColorFactor'][:3]),
                        pbr.get('metallicFactor',1),pbr.get('roughnessFactor',1)))
    palette += [('Cell outline',(.42,.46,.42),0,.95)]
    result=Mesh(palette)
    globals=source.globals()
    for placement in entry['placements']:
        base=matrix({'matrix':placement})
        todo=list(source.data['scenes'][entry['scene']]['nodes'])
        while todo:
            index=todo.pop()
            node=source.data['nodes'][index]
            todo.extend(node.get('children',[]))
            if 'mesh' not in node:continue
            transform=multiply(base,globals[index])
            for prim in source.data['meshes'][node['mesh']]['primitives']:
                attrs=prim['attributes'];dst=result.groups[prim['material']]
                start=len(dst[0])
                dst[0].extend(point(transform,p) for p in source.values(attrs['POSITION']))
                dst[1].extend(unit(point(transform,n,w=0)) for n in source.values(attrs['NORMAL']))
                dst[2].extend(start+i[0] for i in source.values(prim['indices']))
    # A one-cell ground outline, deliberately outside the asset geometry.
    for x in (-.5,.5):result.box((x,-.008,0),(.008,.008,1.),len(palette)-1,bevel=0)
    for z in (-.5,.5):result.box((0,-.008,z),(1.,.008,.008),len(palette)-1,bevel=0)
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out',type=Path,default=Path('target/world-props-review'))
    args=parser.parse_args()
    raw=subprocess.run(['cargo','run','--quiet','--example','look','--','--world-manifest'],cwd=ROOT,
                       check=True,stdout=subprocess.PIPE,text=True)
    entries=json.loads(raw.stdout)
    shown=[]
    for entry in entries:
        mesh=geometry(entry)
        for label,angle in [('',0),(' - rear',180)]:
            name=entry['label'].replace('/','-')+label
            shown.append(('items',name,mesh if angle==0 else Mesh(mesh.palette).add(mesh,turn=(0,angle,0)),None))
    write_gallery(shown,args.out,'world prop review',
                  'Front/rear views of actual definition stages, machinery and state scenes. Every tile is one cell and every image uses the same scale; outlines are review guides. Lighting and emissive effects are approximate.',cell_scale=210)


if __name__=='__main__':
    main()
