#!/usr/bin/env python3
"""Articulated powered suits using the clothing skinning pipeline.

python3 tools/make_skinned_power_armor.py
The inventory suit and helmet retain their original models. The complete metal
shell and fitted details share the body skin weights, including the back and gloves.
`armor_form` turns the anatomical foundation into low-poly faceted plates, boots,
pauldrons and knee cops; `facet_normals` flat-shades it without opening seams.
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


CUFF = .13  # Boot shafts reach this far above the ankle joint.
CLEARANCE = .014
LAMES = 4  # Belly plates between the hips and the chest plate.
LAME_STEP = .016  # How far each lame tucks under the one above.
LAME_TUCK_HEIGHT = .02


def smooth(x):
    x = max(0., min(1., x))
    return x*x*(3-2*x)


class Faceted:
    """Low-poly plate: polygon rings along an axis, joined by planar facets.

    Corner radii start at the body's extents and grow until every member
    vertex sits inside, so the plate encloses the body instead of tracing it.
    """
    def __init__(self, origin, along, u, rings, corners, points):
        self.origin, self.along, self.u = origin, along, u
        self.v = tuple(along[(k+1)%3]*u[(k+2)%3]-along[(k+2)%3]*u[(k+1)%3] for k in range(3))
        self.rings, self.corners = rings, [math.radians(a) for a in corners]
        local = [self.local(p) for p in points]
        self.radii = []
        for j, s in enumerate(rings):
            gap = max(rings[min(j+1, len(rings)-1)]-s, s-rings[max(j-1, 0)])
            near = [q for q in local if abs(q[0]-s) <= gap*.5] or local
            self.radii.append([max(.02, max(q[1]*math.cos(t)+q[2]*math.sin(t) for q in near)+CLEARANCE)
                               for t in self.corners])
        for _ in range(8):
            need = {}
            for s, x, y in local:
                r = math.hypot(x, y)
                if r < 1e-6:
                    continue
                ratio = (r+CLEARANCE)/self.radius(s, math.atan2(y, x))
                if ratio > 1:
                    for key in self.entries(s, math.atan2(y, x)):
                        need[key] = max(need.get(key, 1), ratio)
            for (j, k), ratio in need.items():
                self.radii[j][k] *= ratio
            # Taper gently between rings; abrupt flares read as shelves.
            for j in list(range(1, len(rings)))+list(range(len(rings)-2, -1, -1)):
                for nb in (j-1, j+1):
                    if 0 <= nb < len(rings):
                        limit = 1-.9*min(1, abs(rings[j]-rings[nb])/.12)*.35
                        self.radii[j] = [max(r, o*limit) for r, o in zip(self.radii[j], self.radii[nb])]
            if not need:
                break

    def local(self, p):
        d = tuple(p[k]-self.origin[k] for k in range(3))
        return tuple(sum(d[k]*axis[k] for k in range(3)) for axis in (self.along, self.u, self.v))

    def bracket(self, s, angle):
        n = len(self.corners)
        angle %= math.tau
        k = max(i for i in range(n) if self.corners[i] <= angle) if angle >= self.corners[0] else n-1
        j = max([i for i in range(len(self.rings)-1) if self.rings[i] <= s] or [0])
        return j, k, (k+1) % n

    def entries(self, s, angle):
        j, k, k2 = self.bracket(s, angle)
        return [(j, k), (j, k2), (j+1, k), (j+1, k2)]

    def radius(self, s, angle):
        j, k, k2 = self.bracket(s, angle)
        def chord(ring):
            a, b = self.corners[k], self.corners[k2]
            pa = (self.radii[ring][k]*math.cos(a), self.radii[ring][k]*math.sin(a))
            pb = (self.radii[ring][k2]*math.cos(b), self.radii[ring][k2]*math.sin(b))
            d = (math.cos(angle), math.sin(angle))
            ex, ey = pb[0]-pa[0], pb[1]-pa[1]
            denom = d[0]*ey-d[1]*ex
            return (pa[0]*ey-pa[1]*ex)/denom if abs(denom) > 1e-12 else self.radii[ring][k]
        t = max(0, min(1, (s-self.rings[j])/(self.rings[j+1]-self.rings[j])))
        return chord(j)*(1-t)+chord(j+1)*t

    def project(self, p):
        """Snap to the nearest facet edge when close, then onto the plate."""
        s, x, y = self.local(p)
        angle = math.atan2(y, x) % math.tau
        # Generous snapping puts most triangles wholly on one facet plane.
        spacing = math.tau/len(self.corners)
        for corner in self.corners:
            if abs((angle-corner+math.pi) % math.tau-math.pi) < spacing*.3:
                angle = corner
        for j, ring in enumerate(self.rings[1:-1], 1):
            gap = min(ring-self.rings[j-1], self.rings[j+1]-ring)
            if abs(s-ring) < min(.03, max(.006, gap*.25)):
                s = ring
        radius = self.radius(s, angle)
        return tuple(self.origin[k]+self.along[k]*s+(self.u[k]*math.cos(angle)+self.v[k]*math.sin(angle))*radius
                     for k in range(3))


def facet_normals(vertices, groups, crease=28):
    """Flat-shade a skinned surface: split vertices only between facets.

    Coplanar triangles keep sharing a vertex, so facets read as planes. Split
    copies retain position, joints and weights, so the surface stays closed.
    """
    limit = math.cos(math.radians(crease))
    faces = []
    for mat, ids in groups.items():
        for k in range(0, len(ids), 3):
            a, b, c = (vertices[i][0] for i in ids[k:k+3])
            ab, ac = [tuple(q[j]-a[j] for j in range(3)) for q in (b, c)]
            n = (ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0])
            faces.append((mat, ids[k:k+3], n))
    around = {}
    for f, (_, tri, n) in enumerate(faces):
        for i in tri:
            around.setdefault(i, []).append(f)
    corner, output = {}, []
    for i, incident in around.items():
        clusters = []
        for f in incident:
            n = faces[f][2]
            u = unit(n) if any(n) else (0, 1, 0)
            for cluster in clusters:
                if sum(u[k]*cluster[0][k] for k in range(3)) > limit:
                    cluster[1].append(f)
                    break
            else:
                clusters.append((u, [f]))
        for _, members in clusters:
            total = tuple(sum(faces[f][2][k] for f in members) for k in range(3))
            output.append((vertices[i][0], unit(total) if any(total) else vertices[i][1], vertices[i][2], vertices[i][3]))
            for f in members:
                corner[f, i] = len(output)-1
    result = {mat: [] for mat in groups}
    for f, (mat, tri, _) in enumerate(faces):
        result[mat].extend(corner[f, i] for i in tri)
    return output, result


def dome_planes(axis):
    """Facet directions of a low-poly dome around an axis: crown plus two rings."""
    helper = (0, 0, 1) if abs(axis[2]) < .9 else (1, 0, 0)
    u = unit(tuple(helper[k]-axis[k]*sum(helper[j]*axis[j] for j in range(3)) for k in range(3)))
    v = (axis[1]*u[2]-axis[2]*u[1], axis[2]*u[0]-axis[0]*u[2], axis[0]*u[1]-axis[1]*u[0])
    planes = [axis]
    for tilt, sides, phase in ((38, 6, 0), (72, 8, 22.5)):
        for k in range(sides):
            a = math.radians(phase+k*360/sides)
            t = math.radians(tilt)
            planes.append(tuple(axis[j]*math.cos(t)+(u[j]*math.cos(a)+v[j]*math.sin(a))*math.sin(t) for j in range(3)))
    return planes


def armor_form(suit, indices, names, origins):
    """Turn the anatomical foundation into a low-poly plated armor shell.

    Each body segment is projected onto faceted plates whose shape encloses
    the body rather than following muscles, breasts or the seat. Feet, ankles
    and lower shins become boots with flat soles. Plates blend to narrow
    flexible bands at the joints, where skin weights blend. Topology and
    weights are unchanged, so the enclosure stays closed and bends with the
    pawn. Returns the shell vertices and each vertex's plate factor.
    """
    count = len(suit)
    dominant = [names[js[max(range(4), key=lambda i: ws[i])]] for _, _, js, ws in suit]
    region = []
    for (p, *_), bone in zip(suit, dominant):
        side = bone[-1]
        if bone.startswith(('foot_', 'ball_')):
            region.append('boot_'+side)
        elif bone.startswith('calf_') and p[1] < origins['foot_'+side][1]+CUFF:
            region.append('shaft_'+side)
        elif bone.startswith(('upperarm_', 'lowerarm_', 'thigh_', 'calf_')):
            region.append(bone)
        elif bone.startswith(('spine_', 'clavicle_')) or bone == 'pelvis':
            region.append('torso')
        else:
            region.append('flex')
    sub = lambda a, b: tuple(a[k]-b[k] for k in range(3))
    dot = lambda a, b: sum(a[k]*b[k] for k in range(3))
    target = [None]*count
    plate = [0.]*count
    for name in sorted(set(region)-{'flex'}):
        members = [i for i in range(count) if region[i] == name]
        points = [suit[i][0] for i in members]
        side = name[-1]
        if name.startswith('boot'):
            # A chunky foot hull: flat sole, toe cap, instep and heel.
            directions = [unit(d) for d in [
                (0, -1, 0), (0, 0, 1), (0, 0, -1), (1, 0, 0), (-1, 0, 0),
                (0, .35, 1), (1, 0, 1), (-1, 0, 1), (1, 0, -1), (-1, 0, -1),
                (1, -1, 0), (-1, -1, 0), (0, -1, 1), (0, -1, -1), (1, .4, 1), (-1, .4, 1)]]
            # No top planes: the instep follows the foot instead of filling to the ankle.
            support = [max(dot(q, d) for q in points)+CLEARANCE+.004 for d in directions]
            ankle = origins['foot_'+side]
            for i in members:
                p = suit[i][0]
                c = (ankle[0], max(.045, min(p[1], ankle[1])), min(max(p[2], ankle[2]-.02), ankle[2]+.06))
                r = math.dist(p, c)
                if r < 1e-6:
                    target[i] = p
                    continue
                ray = tuple(v/r for v in sub(p, c))
                exit = min(((h-dot(c, d))/dd for d, h in zip(directions, support)
                            if (dd := dot(ray, d)) > 1e-6), default=r)
                target[i] = tuple(c[k]+ray[k]*max(r+CLEARANCE, min(exit, r+.045)) for k in range(3))
                plate[i] = 1
            continue
        if name.startswith('shaft'):
            # Boot shaft: a faceted tube with a front ridge up to the cuff.
            ankle = origins['foot_'+side]
            top = ankle[1]+CUFF
            a = (ankle[0], top, ankle[2])
            s = [top-q[1] for q in points]
            lo, hi = min(s), max(s)
            form = Faceted(a, (0, -1, 0), (1, 0, 0), [lo, (lo+hi)/2, hi], [i*45 for i in range(8)], points)
            for i in members:
                target[i] = form.project(suit[i][0])
                plate[i] = smooth((top-.012-suit[i][0][1])/.025)
            continue
        if name == 'torso':
            hips = origins['thigh_l'][1]
            # Overlapping belly lames up to the chest plate: each flares out at its
            # lower edge and tucks under the plate above at its upper edge.
            chest = origins['spine_02'][1]+.04
            edges = [hips+.02+k*(chest-hips-.02)/LAMES for k in range(LAMES+1)]
            tucks = [e-LAME_TUCK_HEIGHT for e in edges[1:]]
            ys = sorted([min(q[1] for q in points), hips-.05, *edges, *tucks,
                         (origins['spine_02'][1]+origins['spine_03'][1])/2+.05,
                         origins['spine_03'][1]+.06, origins['neck_01'][1]-.06, max(q[1] for q in points)])
            shoulders = [origins['upperarm_l'], origins['upperarm_r']]
            for i in members:
                p = suit[i][0]
                plate[i] = 0 if abs(suit[i][1][1]) > .8 else min(
                    smooth((origins['neck_01'][1]-.035-p[1])/.03),
                    *(smooth((math.dist(p, q)-.075)/.035) for q in shoulders),
                    smooth((p[1]-(hips-.075))/.035))
            # Fit only what the plate covers; shoulder caps would widen the back.
            fitted = [suit[i][0] for i in members if plate[i] > .3]
            form = Faceted((0, 0, 0), (0, 1, 0), (1, 0, 0), ys, [i*30 for i in range(12)], fitted)
            # Set the tucks back after fitting so the taper can't smooth the steps away.
            for j, y in enumerate(ys):
                if y in tucks:
                    form.radii[j] = [r-LAME_STEP for r in form.radii[j]]
            for i in members:
                target[i] = form.project(suit[i][0])
            continue
        part = name[:-2]
        child = {'upperarm': 'lowerarm', 'lowerarm': 'hand', 'thigh': 'calf', 'calf': 'foot'}[part]
        a, b = origins[name], origins[child+'_'+side]
        along = unit(sub(b, a))
        length = math.dist(a, b)
        helper = (0, 1, 0) if part.endswith('arm') else (1, 0, 0)
        u = unit(sub(helper, tuple(v*dot(helper, along) for v in along)))
        s = [dot(sub(q, a), along) for q in points]
        lo, hi = min(s), max(s)
        start, end = {'upperarm': (.03, .04), 'lowerarm': (.04, .015), 'thigh': (.07, .045),
                      'calf': (.05, 0)}[part]
        if part == 'calf':
            end = length-(hi-.02)
        rings = [lo, start, start+(length-end-start)*.45, length-end, hi]
        corners = [i*45 for i in range(8)] if part in ('thigh', 'calf') else [22.5+i*45 for i in range(8)]
        for i, si in zip(members, s):
            plate[i] = min(smooth((si-start+.005)/.03), smooth((length-end-si+.005)/.03))
        fitted = [q for i, q in zip(members, points) if plate[i] > .3] or points
        form = Faceted(a, along, u, sorted(set(rings)), corners, fitted)
        for i in members:
            target[i] = form.project(suit[i][0])
    shell = []
    for i, (p, n, js, ws) in enumerate(suit):
        bone = dominant[i]
        hand = bone.startswith(('hand_', 'thumb_', 'index_', 'middle_', 'ring_', 'pinky_'))
        thin = tuple(p[k]+n[k]*(.004 if hand else .008) for k in range(3))
        q = thin if target[i] is None else tuple(thin[k]+(target[i][k]-thin[k])*plate[i] for k in range(3))
        shell.append(q)
    sole = min(p[1] for p, *_ in suit)
    shell = [(q[0], max(q[1], sole), q[2]) if region[i].startswith(('boot', 'shaft')) else q for i, q in enumerate(shell)]
    # Low-poly domes over the shoulders and kneecaps. Their rims step out from
    # the plates beneath like overlapping armor, but share the same vertices.
    for side, sign in (('l', 1), ('r', -1)):
        shoulder, knee = origins['upperarm_'+side], origins['calf_'+side]
        for centre, axis, spread, parts, extra in [
                ((shoulder[0]-sign*.01, shoulder[1]-.01, shoulder[2]), unit((sign*.55, 1, 0)), 64,
                 ('torso', 'upperarm_'+side), .024),
                ((knee[0], knee[1]+.005, knee[2]-.01), (0, 0, 1), 50, ('thigh_'+side, 'calf_'+side), .016)]:
            cap = [i for i in range(count) if region[i] in parts
                   and (r := math.dist(suit[i][0], centre)) > 1e-6
                   and sum((suit[i][0][k]-centre[k])/r*axis[k] for k in range(3)) > math.cos(math.radians(spread))
                   and r < .16]
            if not cap:
                continue
            radius = max(math.dist(suit[i][0], centre) for i in cap)+extra
            dome = dome_planes(axis)
            for i in cap:
                q = shell[i]
                r = math.dist(q, centre)
                ray = tuple((q[k]-centre[k])/r for k in range(3))
                exit = radius/max(sum(ray[k]*d[k] for k in range(3)) for d in dome)
                shell[i] = tuple(centre[k]+ray[k]*max(r, min(exit, r+.04)) for k in range(3))
                plate[i] = 1
    # Shade the new form rather than the anatomy underneath it.
    normals = [[0., 0., 0.] for _ in range(count)]
    for k in range(0, len(indices), 3):
        a, b, c = (shell[i] for i in indices[k:k+3])
        ab, ac = [tuple(q[j]-a[j] for j in range(3)) for q in (b, c)]
        face = (ab[1]*ac[2]-ab[2]*ac[1], ab[2]*ac[0]-ab[0]*ac[2], ab[0]*ac[1]-ab[1]*ac[0])
        for i in indices[k:k+3]:
            for j in range(3):
                normals[i][j] += face[j]
    shell = [(q, unit(tuple(normals[i])) if any(normals[i]) else suit[i][1], suit[i][2], suit[i][3])
             for i, q in enumerate(shell)]
    return shell, plate


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
    shell, _ = armor_form(suit, suit_indices, names, origins)
    sole = min(v[0][1] for v in shell)
    surface = BodySurface(shell, suit_indices)

    def add(piece, bone, fit):
        at = origins[bone]
        for mat, (positions, normals, indices) in enumerate(piece.groups):
            # Only shallow machined trim and lights sit above the enclosing shell.
            # Projecting old panel colors onto coarse triangles made dark, ragged
            # patches that looked like holes in the armor at shoulders and hips.
            if mat not in (3, 4) or not indices:
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
                    relief = .007 if mat == 4 else .005
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
    # One titanium enclosure, including shoulders, hips, elbows and knees.
    # Graphite is restricted to the boot soles; the helmet retains its dark
    # faceplate. Shared vertices keep all bends closed without loose plates.
    shell_indices = {0: [], 1: []}
    for k in range(0, len(suit_indices), 3):
        tri = suit_indices[k:k+3]
        material = 1 if max(shell[j][0][1] for j in tri) < sole+.032 else 0
        shell_indices[material].extend(tri)
    flat, flat_indices = facet_normals(shell, shell_indices)
    mesh('Shell', [(mat, flat, ids) for mat, ids in flat_indices.items()])
    mesh('Details', [(i, plates[i], plate_indices[i]) for i in (3, 4)])
    asset.write(ROOT / f'assets/models/armor/{name}.gltf')
    count = sum(len(plate_indices[i])//3 for i in (3, 4))
    print(f'{name}: {count} fitted detail triangles + {len(suit_indices)//3} continuous metal shell triangles; {len(asset.blob):,} bytes')


if __name__ == '__main__':
    for sex in ['Male', 'Female']:
        write(sex)
