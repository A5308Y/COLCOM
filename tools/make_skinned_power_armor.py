#!/usr/bin/env python3
"""Articulated powered suits using the clothing skinning pipeline.

python3 tools/make_skinned_power_armor.py
The inventory suit and helmet retain their original models. Worn plates stay
rigid on individual bones; the continuous hood, gloves and joint seals blend across the skeleton.
"""
import math
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from gltf_pose import Gltf, deform, multiply, unit
from make_gear import (POWER_PALETTE, power_chest, power_shoulder, power_forearm,
                       power_hip, power_leg, power_boot, powered_piece)
from make_rifle import linear
from skinned_asset import SkinnedAsset

ROOT = Path(__file__).resolve().parents[1]


def components(positions, indices):
    """Keep a closed plate's front, back and bevel on exactly the same bone."""
    keys = [tuple(round(v, 6) for v in p) for p in positions]
    adjacent = {}
    for k in range(0, len(indices), 3):
        tri = [keys[i] for i in indices[k:k+3]]
        for p in tri:
            adjacent.setdefault(p, set()).update(tri)
    labels = {}
    for p in adjacent:
        if p in labels:
            continue
        stack, points = [p], []
        labels[p] = len(labels)
        label = labels[p]
        while stack:
            q = stack.pop()
            points.append(q)
            for other in adjacent[q]:
                if other not in labels:
                    labels[other] = label
                    stack.append(other)
        center = tuple(sum(q[i] for q in points)/len(points) for i in range(3))
        for q in points:
            labels[q] = center
    return [labels.get(p, p) for p in keys]


def sealed_body(outfit):
    """Continuous gloves, hood and undersuit, retargeted like the clothing arms.

    The base character supplies anatomy and joint weights, not skin materials.
    Cluster hidden anatomy to a modest mesh, keeping different fingers apart.
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
    joints = {name: i for i, name in enumerate(names)}
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
    flex, flex_indices = [], []

    def packed(weights):
        values = sorted(((joints[j], w) for j, w in weights.items() if w > 0), key=lambda x: (-x[1], x[0]))[:4]
        total = sum(w for _, w in values)
        return (tuple(j for j, _ in values)+(0,)*(4-len(values)),
                tuple(w/total for _, w in values)+(0,)*(4-len(values)))

    def spine(y):
        if y > -.085:
            return 'spine_03'
        if y > -.205:
            return 'spine_02'
        return 'spine_01'

    def flexible_chest(p):
        chain = ['pelvis', 'spine_01', 'spine_02', 'spine_03']
        y = p[1]
        for a, b in zip(chain, chain[1:]):
            if y <= origins[b][1]:
                t = max(0, min(1, (y-origins[a][1])/(origins[b][1]-origins[a][1])))
                return {a: 1-t, b: t}
        return {'spine_03': 1}

    def add(piece, bone, fit, rigid_bone=None, flex_weights=None, omit_flex=False):
        at = origins[bone]
        for mat, (positions, normals, indices) in enumerate(piece.groups):
            if not indices or (omit_flex and mat == 2):
                continue
            centers = components(positions, indices) if rigid_bone else positions
            is_flex = mat == 2
            dst, ids = (flex, flex_indices) if is_flex else (plates[mat], plate_indices[mat])
            offset = len(dst)
            for p, n, center in zip(positions, normals, centers):
                q = fit(p)
                q = tuple(q[i]+at[i] for i in range(3))
                # Transform normals by the inverse transpose of the fitted basis.
                basis = [fit(tuple(int(i == j) for i in range(3))) for j in range(3)]
                v = [sum(basis[j][i]*n[j]/max(1e-12, sum(x*x for x in basis[j])) for j in range(3)) for i in range(3)]
                weights = flex_weights(q) if is_flex and flex_weights else {(rigid_bone(center) if rigid_bone else bone): 1}
                js, ws = packed(weights)
                dst.append((q, unit(v), js, ws))
            ids.extend(offset+i for i in indices)

    width = .90 if female else 1
    add(power_chest(articulated=True), 'spine_03', lambda p: (p[0]*width, p[1], p[2]*(.96 if female else 1)),
        rigid_bone=lambda p: spine(p[1]), flex_weights=flexible_chest, omit_flex=True)

    for suffix, side in [('l', 1), ('r', -1)]:
        shoulder, elbow, wrist = [origins[f'{part}_{suffix}'] for part in ['upperarm', 'lowerarm', 'hand']]
        upper_length, lower_length = abs(elbow[0]-shoulder[0]), abs(wrist[0]-elbow[0])
        radius = .94 if female else 1
        add(power_shoulder(side), f'upperarm_{suffix}', lambda p: (p[0]*upper_length/.255, p[1]*radius, p[2]*radius))
        add(power_forearm(side), f'lowerarm_{suffix}', lambda p: (p[0]*lower_length/.223, p[1]*radius, p[2]*radius))

    def hip_weights(p):
        # Hip seals follow the corresponding thigh without pulling the central plate.
        suffix = 'l' if p[0] > 0 else 'r'
        t = max(0, min(.75, (origins['pelvis'][1]-.015-p[1])/.16)) * min(1, abs(p[0])/.09)
        return {'pelvis': 1-t, f'thigh_{suffix}': t}
    add(power_hip(), 'pelvis', lambda p: (p[0]*width, p[1], p[2]), flex_weights=hip_weights)

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
        # A deforming knee seal bridges the rigid thigh and shin shells.
        seal = powered_piece()
        seal.section([(-.08, .071, .088, 0), (-.035, .076, .092, 0),
                      (.035, .076, .092, 0), (.08, .071, .088, 0)], 2, sides=12, smooth=True)
        def knee_weights(p):
            t = max(0, min(1, (p[1]-knee[1]+.065)/.13))
            return {f'thigh_{suffix}': t, f'calf_{suffix}': 1-t}
        add(seal, f'calf_{suffix}', lambda p: p, flex_weights=knee_weights)
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
    mesh('Plates', [(i, ps, ids) for i, (ps, ids) in enumerate(zip(plates, plate_indices))])
    mesh('Flex', [(2, flex, flex_indices)])
    mesh('Undersuit', [(2, suit, suit_indices)])
    asset.write(ROOT / f'assets/models/armor/{name}.gltf')
    count = sum(len(ids)//3 for ids in plate_indices) + len(flex_indices)//3
    print(f'{name}: {count} armor/joint triangles + {len(suit_indices)//3} continuous undersuit triangles; {len(asset.blob):,} bytes')


if __name__ == '__main__':
    for sex in ['Male', 'Female']:
        write(sex)
