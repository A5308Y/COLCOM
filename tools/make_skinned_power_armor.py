#!/usr/bin/env python3
"""Articulated powered suits using the clothing skinning pipeline.

python3 tools/make_skinned_power_armor.py
The inventory suit and helmet retain their original models. The complete metal
shell and fitted details share the body skin weights, including the back and gloves.
"""
import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from gltf_pose import Gltf, deform, multiply, unit
from make_gear import (POWER_PALETTE, power_chest, power_shoulder, power_forearm,
                       power_hip, power_leg, power_boot)
from make_rifle import linear
from skinned_asset import SkinnedAsset

ROOT = Path(__file__).resolve().parents[1]


def closest_triangle(p, a, b, c):
    """Closest point barycentrics, including edge and vertex regions."""
    sub = lambda x, y: tuple(x[i]-y[i] for i in range(3))
    dot = lambda x, y: sum(u*v for u, v in zip(x, y))
    ab, ac, ap = sub(b, a), sub(c, a), sub(p, a)
    d1, d2 = dot(ab, ap), dot(ac, ap)
    if d1 <= 0 and d2 <= 0:
        return (1, 0, 0)
    bp = sub(p, b)
    d3, d4 = dot(ab, bp), dot(ac, bp)
    if d3 >= 0 and d4 <= d3:
        return (0, 1, 0)
    vc = d1*d4-d3*d2
    if vc <= 0 and d1 >= 0 and d3 <= 0:
        v = d1/(d1-d3)
        return (1-v, v, 0)
    cp = sub(p, c)
    d5, d6 = dot(ab, cp), dot(ac, cp)
    if d6 >= 0 and d5 <= d6:
        return (0, 0, 1)
    vb = d5*d2-d1*d6
    if vb <= 0 and d2 >= 0 and d6 <= 0:
        w = d2/(d2-d6)
        return (1-w, 0, w)
    va = d3*d6-d5*d4
    if va <= 0 and d4-d3 >= 0 and d5-d6 >= 0:
        w = (d4-d3)/((d4-d3)+(d5-d6))
        return (0, 1-w, w)
    total = va+vb+vc
    if abs(total) < 1e-20:
        return (1, 0, 0)
    return (va/total, vb/total, vc/total)


class BodySurface:
    """Spatially indexed body triangles for fitting panels and transferring weights."""
    def __init__(self, vertices, indices):
        self.vertices = vertices
        self.triangles = []
        for k in range(0, len(indices), 3):
            tri = indices[k:k+3]
            a, b, c = [vertices[j][0] for j in tri]
            ab, ac = [tuple(q[i]-a[i] for i in range(3)) for q in (b, c)]
            cross = (ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0])
            if sum(v*v for v in cross) > 1e-18:
                self.triangles.append(tri)
        self.cells = {}
        self.cache = {}
        for i, tri in enumerate(self.triangles):
            ps = [vertices[j][0] for j in tri]
            lo = [math.floor(min(p[k] for p in ps)/.08) for k in range(3)]
            hi = [math.floor(max(p[k] for p in ps)/.08) for k in range(3)]
            for x in range(lo[0], hi[0]+1):
                for y in range(lo[1], hi[1]+1):
                    for z in range(lo[2], hi[2]+1):
                        self.cells.setdefault((x, y, z), set()).add(i)

    def covers(self, p, tolerance=.006):
        """Whether a projected panel covers this point on the shell."""
        cells = [range(math.floor((v-tolerance)/.08), math.floor((v+tolerance)/.08)+1) for v in p]
        candidates = set()
        for x in cells[0]:
            for y in cells[1]:
                for z in cells[2]:
                    candidates.update(self.cells.get((x,y,z), ()))
        for i in sorted(candidates):
            ps = [self.vertices[j][0] for j in self.triangles[i]]
            bary = closest_triangle(p, *ps)
            q = tuple(sum(v[k]*w for v, w in zip(ps, bary)) for k in range(3))
            if math.dist(p, q) < tolerance:
                return True
        return False

    def sample(self, p):
        key = tuple(round(v, 7) for v in p)
        if key in self.cache:
            return self.cache[key]
        cell = tuple(math.floor(v/.08) for v in p)
        best, checked = None, set()
        for radius in range(8):
            candidates = set()
            for x in range(cell[0]-radius, cell[0]+radius+1):
                for y in range(cell[1]-radius, cell[1]+radius+1):
                    for z in range(cell[2]-radius, cell[2]+radius+1):
                        candidates.update(self.cells.get((x, y, z), ()))
            for i in sorted(candidates-checked):
                tri = self.triangles[i]
                vertices = [self.vertices[j] for j in tri]
                bary = closest_triangle(p, *(v[0] for v in vertices))
                q = tuple(sum(v[0][k]*w for v, w in zip(vertices, bary)) for k in range(3))
                distance = math.dist(p, q)
                if best is None or distance < best[0]:
                    best = (distance, q, vertices, bary)
            checked.update(candidates)
            boundary = min(min(p[k]-(cell[k]-radius)*.08, (cell[k]+radius+1)*.08-p[k]) for k in range(3))
            if best and best[0] <= boundary:
                break
        if best is None:
            return (p, (0, 1, 0), (0, 0, 0, 0), (1, 0, 0, 0), math.inf)
        distance, q, vertices, bary = best
        n = unit(tuple(sum(v[1][k]*w for v, w in zip(vertices, bary)) for k in range(3)))
        weights = {}
        for vertex, factor in zip(vertices, bary):
            for j, w in zip(vertex[2], vertex[3]):
                weights[j] = weights.get(j, 0)+w*factor
        values = sorted(weights.items(), key=lambda x: (-x[1], x[0]))[:4]
        total = sum(w for _, w in values)
        js = tuple(j for j, _ in values)+(0,)*(4-len(values))
        ws = tuple(w/total for _, w in values)+(0,)*(4-len(values))
        result = (q, n, js, ws, distance)
        self.cache[key] = result
        return result


def sealed_body(outfit):
    """Continuous armor foundation, retargeted like the clothing arms.

    The base character supplies anatomy and joint weights, not skin materials.
    Simplify to a modest mesh, keeping different fingers apart.
    """
    sex = outfit.path.stem.split('_')[0]
    body = Gltf(ROOT / f'assets/models/characters/base/Superhero_{sex}_FullBody.gltf')
    skin = body.data['skins'][0]
    names = [body.data['nodes'][j]['name'] for j in skin['joints']]
    target_names = [outfit.data['nodes'][j]['name'] for j in outfit.data['skins'][0]['joints']]
    remap = [target_names.index(name) for name in names]
    globals = {outfit.data['nodes'][i].get('name'): m for i, m in outfit.globals().items()}
    matrices = [multiply(globals[name], [[bind[c*4+r] for c in range(4)] for r in range(4)])
                for name, bind in zip(names, body.values(skin['inverseBindMatrices']))]
    mesh = max(body.data['meshes'], key=lambda m: sum(body.data['accessors'][p['indices']]['count'] for p in m['primitives']))
    clusters, old_triangles = {}, []
    for primitive in mesh['primitives']:
        attrs = {k: body.values(v) for k, v in primitive['attributes'].items()}
        positions, normals = deform(attrs['POSITION'], attrs['NORMAL'], attrs['JOINTS_0'], attrs['WEIGHTS_0'], matrices)
        indices = [i[0] for i in body.values(primitive['indices'])]
        mapping = {}
        for i in sorted(set(indices)):
            p, n = positions[i], normals[i]
            js, ws = attrs['JOINTS_0'][i], attrs['WEIGHTS_0'][i]
            dominant = names[max(zip(js, ws), key=lambda x: x[1])[0]]
            hand = dominant.startswith(('hand_', 'thumb_', 'index_', 'middle_', 'ring_', 'pinky_'))
            cell = .006 if hand else .017 if dominant == 'Head' else .026
            key = (dominant, *(round(v/cell) for v in p))
            if key not in clusters:
                clusters[key] = (len(clusters), [], [], {})
            index, points, directions, weights = clusters[key]
            points.append(p)
            directions.append(n)
            for j, w in zip(js, ws):
                target = remap[j]
                weights[target] = weights.get(target, 0) + w
            mapping[i] = index
        old_triangles.extend(tuple(mapping[i] for i in indices[k:k+3]) for k in range(0, len(indices), 3))
    vertices = []
    for _, ps, ns, weights in clusters.values():
        p = tuple(sum(q[k] for q in ps)/len(ps) for k in range(3))
        n = unit(tuple(sum(q[k] for q in ns) for k in range(3)))
        weights = sorted(weights.items(), key=lambda x: (-x[1], x[0]))[:4]
        total = sum(w for _, w in weights)
        vertices.append((p, n, tuple(j for j, _ in weights)+(0,)*(4-len(weights)),
                         tuple(w/total for _, w in weights)+(0,)*(4-len(weights))))
    indices = [i for tri in dict.fromkeys(old_triangles) if len(set(tri)) == 3 for i in tri]
    return vertices, indices


def write(sex):
    source = Gltf(ROOT / f"assets/models/characters/outfits/{sex}_Peasant.gltf")
    suit, suit_indices = sealed_body(source)
    names = [source.data['nodes'][j]['name'] for j in source.data['skins'][0]['joints']]
    origins = {source.data['nodes'][i].get('name'): tuple(m[k][3] for k in range(3))
               for i, m in source.globals().items()}
    female = sex == 'Female'
    asset = SkinnedAsset(source, 'My World powered armor; clothing skinning pipeline')
    g, accessor = asset.data, asset.accessor
    name = 'PoweredArmor' + sex
    for label, color, metal, rough in POWER_PALETTE:
        material = {'name': label, 'pbrMetallicRoughness': {
            'baseColorFactor': [linear(v) for v in color]+[1],
            'metallicFactor': metal, 'roughnessFactor': rough}}
        if label == 'Ion cyan':
            material['emissiveFactor'] = [.06, 3.8, 6.0]
        g['materials'].append(material)
    # Buffers contain (position, normal, joints, weights), grouped by material.
    plates = [[] for _ in POWER_PALETTE]
    plate_indices = [[] for _ in POWER_PALETTE]
    surface = BodySurface(suit, suit_indices)
    paint = [[] for _ in POWER_PALETTE]

    def add(piece, bone, fit):
        at = origins[bone]
        for mat, (positions, normals, indices) in enumerate(piece.groups):
            # The continuous metal shell replaces every old fabric insert.
            if mat == 2 or not indices:
                continue
            dst, ids = plates[mat], plate_indices[mat]
            lookup = {}
            points = [tuple(v+at[k] for k, v in enumerate(fit(p))) for p in positions]
            def triangle(a, b, c):
                samples = [surface.sample(p) for p in (a, b, c)]
                edge = max(math.dist(a, b), math.dist(b, c), math.dist(c, a))
                curvature = max(math.dist(samples[i][1], samples[j][1]) for i, j in [(0,1), (1,2), (2,0)])
                # Subdivide curved areas more finely so chords cannot cut through
                # the shell. Flat back/chest panels need fewer vertices.
                if edge > .075 or (edge > .025 and curvature > .35):
                    ab, bc, ca = [tuple((x[k]+y[k])/2 for k in range(3)) for x, y in [(a,b), (b,c), (c,a)]]
                    for tri in [(a,ab,ca), (ab,b,bc), (ca,bc,c), (ab,bc,ca)]:
                        triangle(*tri)
                    return
                ab = tuple(b[k]-a[k] for k in range(3))
                ac = tuple(c[k]-a[k] for k in range(3))
                face = unit((ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0]))
                if sum(face[k]*sum(v[1][k] for v in samples)/3 for k in range(3)) < .15:
                    return  # The continuous shell replaces panel backs and buried caps.
                for q, n, js, ws, distance in samples:
                    # Ceramic and frame colors will be baked onto one continuous
                    # surface. Only the trim and lights need separate relief.
                    relief = .028 if mat == 4 else .025
                    paint[mat].append((q, n, js, ws))
                    vertex = (tuple(q[k]+n[k]*relief for k in range(3)), n, js, ws)
                    if vertex not in lookup:
                        lookup[vertex] = len(dst)
                        dst.append(vertex)
                    ids.append(lookup[vertex])
            for k in range(0, len(indices), 3):
                triangle(*(points[i] for i in indices[k:k+3]))

    width = .90 if female else 1
    add(power_chest(articulated=True), 'spine_03', lambda p: (p[0]*width, p[1], p[2]*(.96 if female else 1)))

    for suffix, side in [('l', 1), ('r', -1)]:
        shoulder, elbow, wrist = [origins[f'{part}_{suffix}'] for part in ['upperarm', 'lowerarm', 'hand']]
        upper_length, lower_length = abs(elbow[0]-shoulder[0]), abs(wrist[0]-elbow[0])
        radius = .94 if female else 1
        add(power_shoulder(side), f'upperarm_{suffix}', lambda p: (p[0]*upper_length/.255, p[1]*radius, p[2]*radius))
        add(power_forearm(side), f'lowerarm_{suffix}', lambda p: (p[0]*lower_length/.223, p[1]*radius, p[2]*radius))

    add(power_hip(), 'pelvis', lambda p: (p[0]*width, p[1], p[2]))

    for suffix in ['l', 'r']:
        thigh, knee, ankle = [origins[f'{part}_{suffix}'] for part in ['thigh', 'calf', 'foot']]
        def leg_fit(a, b, authored):
            # Match the real knee/ankle, including the female leg's slanted rest axis.
            y = unit(tuple(a[i]-b[i] for i in range(3)))
            x = unit((y[1], -y[0], 0))
            z = (x[1]*y[2]-x[2]*y[1], x[2]*y[0]-x[0]*y[2], x[0]*y[1]-x[1]*y[0])
            length = math.dist(a, b)/authored
            return lambda p: tuple(x[i]*p[0]+y[i]*p[1]*length+z[i]*p[2] for i in range(3))
        add(power_leg(), f'thigh_{suffix}', leg_fit(thigh, knee, .429))
        add(power_leg(True), f'calf_{suffix}', leg_fit(knee, ankle, .455))
        add(power_boot(), f'foot_{suffix}', lambda p: (p[0], p[1], p[2]))

    def mesh(label, groups):
        primitives = []
        for mat, vertices, indices in groups:
            if not indices:
                continue
            attrs = {key: accessor([v[i] for v in vertices], kind, ctype)
                     for i, key, kind, ctype in [(0, 'POSITION', 'VEC3', 5126), (1, 'NORMAL', 'VEC3', 5126),
                                                (2, 'JOINTS_0', 'VEC4', 5123), (3, 'WEIGHTS_0', 'VEC4', 5126)]}
            primitives.append({'attributes': attrs, 'indices': accessor([(i,) for i in indices], 'SCALAR', 5125, 34963), 'material': mat})
        g['meshes'].append({'name': name+label, 'primitives': primitives})
    # Bake the broad panel silhouettes onto the body topology. Sharing vertices
    # makes armor coverage continuous through every bend, with no layered shells
    # to intersect at the knees, shoulders or spine.
    masks = {mat: BodySurface(paint[mat], list(range(len(paint[mat])))) for mat in (0, 1)}
    shell_indices = {0: [], 1: []}
    for k in range(0, len(suit_indices), 3):
        tri = suit_indices[k:k+3]
        center = tuple(sum(suit[j][0][axis] for j in tri)/3 for axis in range(3))
        frame = masks[1].covers(center)
        ceramic = masks[0].covers(center) if frame else True
        shell_indices[1 if frame and not ceramic else 0].extend(tri)
    shell = []
    for p, n, js, ws in suit:
        bone = names[js[max(range(4), key=lambda i: ws[i])]]
        hand = bone.startswith(('hand_', 'thumb_', 'index_', 'middle_', 'ring_', 'pinky_'))
        thickness = .004 if hand else .010
        shell.append((tuple(p[k]+n[k]*thickness for k in range(3)), n, js, ws))
    mesh('Shell', [(mat, shell, ids) for mat, ids in shell_indices.items()])
    mesh('Details', [(i, plates[i], plate_indices[i]) for i in (3, 4)])
    asset.write(ROOT / f'assets/models/armor/{name}.gltf')
    count = sum(len(plate_indices[i])//3 for i in (3, 4))
    print(f'{name}: {count} fitted detail triangles + {len(suit_indices)//3} continuous metal shell triangles; {len(asset.blob):,} bytes')


if __name__ == '__main__':
    for sex in ['Male', 'Female']:
        write(sex)
