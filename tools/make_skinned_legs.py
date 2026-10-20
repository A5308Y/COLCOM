#!/usr/bin/env python3
"""Fitted cargo trousers and flak guards, sharing the live outfit's leg skin.

python3 -B tools/make_skinned_legs.py
Native boots are included so lower-body replacement never removes the feet.
"""
from pathlib import Path
import sys
sys.dont_write_bytecode = True

from gltf_pose import Gltf, unit
from make_rifle import linear
from make_skinned_power_armor import BodySurface
from skinned_asset import SkinnedAsset

ROOT = Path(__file__).resolve().parents[1]


def lower_body(source):
    vertices, triangles, boots = [], [], []
    for mesh in source.data['meshes']:
        if not mesh['name'].endswith(('_Legs', '_Feet')):
            continue
        for primitive in mesh['primitives']:
            attrs = {k: source.values(v) for k, v in primitive['attributes'].items()}
            offset = len(vertices)
            vertices.extend(zip(*(attrs[k] for k in ('POSITION', 'NORMAL', 'JOINTS_0', 'WEIGHTS_0'))))
            ids = [i[0]+offset for i in source.values(primitive['indices'])]
            for k in range(0, len(ids), 3):
                triangles.append(ids[k:k+3])
                boots.append(mesh['name'].endswith('_Feet'))
    return vertices, triangles, boots


def guard_panels(vertices, triangles, boots, origins):
    """Cut crisp pad boundaries into the existing skin, keeping one surface."""
    vertices = list(vertices)
    lookup = {tuple(round(x, 7) for x in p): i for i, (p, _, _, _) in enumerate(vertices)}

    def crossing(a, b, t):
        va, vb = vertices[a], vertices[b]
        p = tuple(va[0][k]*(1-t)+vb[0][k]*t for k in range(3))
        key = tuple(round(x, 7) for x in p)
        if key in lookup:
            return lookup[key]
        n = unit(tuple(va[1][k]*(1-t)+vb[1][k]*t for k in range(3)))
        weights = {}
        for vertex, factor in ((va,1-t),(vb,t)):
            for j,w in zip(vertex[2],vertex[3]):
                weights[j] = weights.get(j,0)+w*factor
        values = sorted(weights.items(), key=lambda v: (-v[1],v[0]))[:4]
        total = sum(w for _,w in values)
        js = tuple(j for j,_ in values)+(0,)*(4-len(values))
        ws = tuple(w/total for _,w in values)+(0,)*(4-len(values))
        lookup[key] = len(vertices)
        vertices.append((p,n,js,ws))
        return lookup[key]

    def split(poly, axis, threshold, sign):
        inside, outside = [], []
        for a,b in zip(poly, poly[1:]+poly[:1]):
            da, db = [(vertices[i][0][axis]-threshold)*sign for i in (a,b)]
            (inside if da >= 0 else outside).append(a)
            if (da >= 0) != (db >= 0):
                cut = crossing(a,b,da/(da-db))
                inside.append(cut)
                outside.append(cut)
        return inside, outside

    regions = []
    for side in ('l','r'):
        hip,knee,ankle = [origins[f'{part}_{side}'] for part in ('thigh','calf','foot')]
        for center,width,height,front in [((hip[0],hip[1]-.16),.080,.115,hip[2]+.025),
                                         ((knee[0],knee[1]-.012),.075,.070,knee[2]+.025),
                                         ((knee[0],(knee[1]+ankle[1])/2),.060,.125,ankle[2]+.025)]:
            x,y = center
            # Planar boundaries make clean panel edges on the curved leg surface.
            regions.append([(0,x-width,1),(0,x+width,-1),(1,y-height,1),(1,y+height,-1),(2,front,1)])
    faces = []
    for tri,boot in zip(triangles,boots):
        remaining = [list(tri)]
        for region in regions:
            next_remaining = []
            for poly in remaining:
                if any(all((vertices[i][0][axis]-threshold)*sign < 0 for i in poly) for axis,threshold,sign in region):
                    next_remaining.append(poly)
                    continue
                inside = poly
                for axis,threshold,sign in region:
                    if len(inside)<3:
                        break
                    inside,outside = split(inside,axis,threshold,sign)
                    if len(outside)>=3:
                        next_remaining.append(outside)
                if len(inside)>=3:
                    faces.append((inside,1))
            remaining = next_remaining
        faces.extend((poly,3 if boot else 0) for poly in remaining)
    groups = [[] for _ in range(5)]
    padded = set()
    for poly,mat in faces:
        for k in range(1,len(poly)-1):
            tri = [poly[0],poly[k],poly[k+1]]
            if len(set(tri))==3:
                groups[mat].extend(tri)
                if mat==1:
                    padded.update(tri)
    # Shared boundary vertices also move the neighboring fabric, making a bevel
    # instead of opening cracks or layering another shell over the knee.
    vertices = [(tuple(p[k]+n[k]*.018 for k in range(3)) if i in padded else p,n,js,ws)
                for i,(p,n,js,ws) in enumerate(vertices)]
    return vertices,groups


def write(sex, guarded):
    source = Gltf(ROOT / f'assets/models/characters/outfits/{sex}_Peasant.gltf')
    vertices, triangles, boots = lower_body(source)
    origins = {source.data['nodes'][i].get('name'): tuple(m[k][3] for k in range(3)) for i, m in source.globals().items()}
    name = ('FlakLegGuards' if guarded else 'CargoTrousers')+sex
    asset = SkinnedAsset(source, 'My World fitted leg apparel; Quaternius CC0 outfit foundation')
    palette = [
        ('Ballistic cloth' if guarded else 'Canvas trousers', (.24,.28,.23) if guarded else (.39,.34,.24), 0, .94),
        ('Padded guards' if guarded else 'Cargo pockets', (.40,.44,.34) if guarded else (.47,.41,.29), 0, .91),
        ('Webbing and seams', (.15,.18,.16), 0, .96),
        ('Boots', (.13,.16,.17), 0, .84),
        ('Fasteners', (.48,.51,.47), .5, .65),
    ]
    for label, color, metal, rough in palette:
        asset.data['materials'].append({'name': label, 'pbrMetallicRoughness': {
            'baseColorFactor': [linear(v) for v in color]+[1], 'metallicFactor': metal, 'roughnessFactor': rough}})
    groups = [[] for _ in palette]
    for tri, boot in zip(triangles, boots):
        p = tuple(sum(vertices[i][0][k] for i in tri)/3 for k in range(3))
        side = 'l' if p[0] > 0 else 'r'
        knee, ankle = origins[f'calf_{side}'], origins[f'foot_{side}']
        # A narrow reinforced knee band and trouser cuffs give the cloth structure.
        mat = 3 if boot else 2 if p[1] < ankle[1]+.12 else 1 if abs(p[1]-knee[1]) < .045 else 0
        groups[mat].extend(tri)
    surface = BodySurface(vertices, [i for tri in triangles for i in tri])
    detail_vertices, detail_groups = [], [[] for _ in palette]

    def patch(center, width, height, facing, material, depth):
        # A small raised, rounded patch; every point inherits the trouser skin.
        # Subdivision follows knee bends without using rigid bone attachments.
        start, steps = len(detail_vertices), 6
        for row in range(steps+1):
            v = row/steps*2-1
            for col in range(steps+1):
                u = col/steps*2-1
                taper = 1-.22*abs(v)
                p = (center[0]+u*width*taper if facing == 0 else center[0],
                     center[1]+v*height,
                     center[2] if facing == 0 else center[2]+u*width*taper)
                q, n, js, ws, _ = surface.sample(p)
                relief = .006+depth*(1-.55*abs(u))*(1-.55*abs(v))
                detail_vertices.append((tuple(q[k]+n[k]*relief for k in range(3)), n, js, ws))
        for row in range(steps):
            for col in range(steps):
                a = start+row*(steps+1)+col
                for tri in [(a,a+1,a+steps+2), (a,a+steps+2,a+steps+1)]:
                    ps = [detail_vertices[i][0] for i in tri]
                    ab, ac = [tuple(p[k]-ps[0][k] for k in range(3)) for p in ps[1:]]
                    normal = (ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0])
                    if sum(normal[k]*detail_vertices[tri[0]][1][k] for k in range(3)) < 0:
                        tri = tuple(reversed(tri))
                    detail_groups[material].extend(tri)

    for suffix, sign in [('l',1), ('r',-1)]:
        hip, knee, ankle = [origins[f'{bone}_{suffix}'] for bone in ('thigh','calf','foot')]
        if not guarded:
            patch((hip[0]+sign*.20, hip[1]-.15, -.025), .058, .073, sign, 1, .021)
            patch((hip[0]+sign*.20, hip[1]-.095, -.025), .059, .017, sign, 2, .025)

    if guarded:
        vertices, groups = guard_panels(vertices, triangles, boots, origins)
        pad_indices = groups[1]
        groups[1] = []

    def mesh(label, points, ids_by_material):
        attrs = {key: asset.accessor([v[i] for v in points], kind, ctype) for i, key, kind, ctype in [
            (0,'POSITION','VEC3',5126), (1,'NORMAL','VEC3',5126), (2,'JOINTS_0','VEC4',5123), (3,'WEIGHTS_0','VEC4',5126)]}
        asset.data['meshes'].append({'name': name+label, 'primitives': [
            {'attributes': attrs, 'indices': asset.accessor([(i,) for i in ids], 'SCALAR',5125,34963), 'material': mat}
            for mat, ids in enumerate(ids_by_material) if ids]})
    mesh('Legs', vertices, groups)
    if guarded:
        mesh('Pads', vertices, [[], pad_indices, [], [], []])
    else:
        mesh('Pockets', detail_vertices, detail_groups)
    asset.write(ROOT / f'assets/models/items/{name}.gltf')
    print(f'{name}: {sum(len(ids)//3 for ids in groups)+(len(pad_indices)//3 if guarded else sum(len(ids)//3 for ids in detail_groups))} triangles')


if __name__ == '__main__':
    for sex in ('Male','Female'):
        for guarded in (False, True):
            write(sex, guarded)
