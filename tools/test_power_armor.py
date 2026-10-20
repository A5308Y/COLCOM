"""Powered suit coverage and articulation; python3 -B tools/test_power_armor.py."""
import math
from collections import Counter
from pathlib import Path
import unittest

from gltf_pose import Gltf, deform
from make_skinned_power_armor import sealed_body

ROOT = Path(__file__).resolve().parents[1] / 'assets/models'


def nearest(cells, point):
    key = tuple(math.floor(v/.05) for v in point)
    nearby = [q for x in range(key[0]-2, key[0]+3) for y in range(key[1]-2, key[1]+3)
              for z in range(key[2]-2, key[2]+3) for q in cells.get((x, y, z), ())]
    return min((math.dist(point, q) for q in nearby), default=math.inf)


class PoweredArmor(unittest.TestCase):
    def test_enclosure_has_no_open_edges_or_dark_undersuit_patches(self):
        # Inspect the exported geometry independently of sealed_body's triangle
        # list: copying that list alone cannot prove a surface has no gaps.
        for sex in ('Male', 'Female'):
            g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
            shell = next(m for m in g.data['meshes'] if m['name'].endswith('Shell'))
            edges = Counter()
            for primitive in shell['primitives']:
                attrs = {k: g.values(v) for k, v in primitive['attributes'].items()}
                ids = [v[0] for v in g.values(primitive['indices'])]
                # Include skin weights: coincident positions with different
                # influences can separate when the character moves.
                keys = [(p, js, ws) for p, js, ws in zip(attrs['POSITION'], attrs['JOINTS_0'], attrs['WEIGHTS_0'])]
                for k in range(0, len(ids), 3):
                    tri = ids[k:k+3]
                    for a, b in ((0, 1), (1, 2), (2, 0)):
                        edges[tuple(sorted((keys[tri[a]], keys[tri[b]])))] += 1
                    if primitive['material'] == 1:
                        self.assertLess(sum(attrs['POSITION'][i][1] for i in tri)/3, .04,
                                        'graphite on the body can read as exposed undersuit; reserve it for soles')
            self.assertFalse(any(count == 1 for count in edges.values()), f'{sex} has an open armor seam')

    def test_suit_covers_the_whole_body_with_five_opaque_materials(self):
        for sex in ['Male', 'Female']:
            with self.subTest(sex=sex):
                g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
                outfit = Gltf(ROOT / f'characters/outfits/{sex}_Peasant.gltf')
                skin = g.data['skins'][0]
                names = [g.data['nodes'][j]['name'] for j in skin['joints']]
                self.assertEqual(names, [outfit.data['nodes'][j]['name'] for j in outfit.data['skins'][0]['joints']])
                self.assertEqual(g.values(skin['inverseBindMatrices']), outfit.values(outfit.data['skins'][0]['inverseBindMatrices']))
                self.assertEqual(len(g.data['materials']), 5)
                self.assertNotIn('images', g.data, 'the suit must not reuse skin textures')
                for material in g.data['materials']:
                    self.assertEqual(material.get('alphaMode', 'OPAQUE'), 'OPAQUE')
                self.assertTrue(all(p['material'] != 2 for m in g.data['meshes'] for p in m['primitives']),
                                'no fabric inserts may be exposed between plates')
                suit = next(m for m in g.data['meshes'] if m['name'].endswith('Shell'))
                _, expected = sealed_body(outfit)
                # Flat-shaded facets duplicate vertices but keep every body triangle;
                # the open-edge test proves the copies still form one closed surface.
                actual = sum(g.data['accessors'][primitive['indices']]['count'] for primitive in suit['primitives'])
                self.assertEqual(actual, len(expected), 'the shell must cover every body triangle')
                covered = set()
                for p in suit['primitives']:
                    self.assertIn(p['material'], (0, 1), 'the entire body must be covered by opaque metal')
                    attrs = {k: g.values(v) for k, v in p['attributes'].items()}
                    for js, ws in zip(attrs['JOINTS_0'], attrs['WEIGHTS_0']):
                        covered.update(names[j] for j, w in zip(js, ws) if w > .1)
                for bone in ['Head', 'neck_01', 'pelvis', 'spine_01', 'spine_02', 'spine_03']:
                    self.assertIn(bone, covered)
                for side in ['l', 'r']:
                    for part in ['upperarm', 'lowerarm', 'hand', 'thigh', 'calf', 'foot']:
                        self.assertIn(f'{part}_{side}', covered, 'a body region was left exposed')
                    for finger in ['thumb', 'index', 'middle', 'ring', 'pinky']:
                        self.assertTrue(any(b.startswith(finger+'_') and b.endswith('_'+side) for b in covered), 'gloves must include all fingers')

    def test_plates_have_volume_instead_of_a_painted_skinsuit(self):
        # An offset copy of the body reads as a colored bodysuit. Plates must
        # stand clear of the chest, back, arms and legs; joints may stay thin.
        for sex in ('Male', 'Female'):
            with self.subTest(sex=sex):
                g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
                body, _ = sealed_body(Gltf(ROOT / f'characters/outfits/{sex}_Peasant.gltf'))
                cells = {}
                for p, *_ in body:
                    cells.setdefault(tuple(math.floor(v/.05) for v in p), []).append(p)
                shell = next(m for m in g.data['meshes'] if m['name'].endswith('Shell'))
                offsets = {'torso': [], 'arms': [], 'legs': []}
                for primitive in shell['primitives']:
                    for p in g.values(primitive['attributes']['POSITION'])[::3]:
                        part = 'torso' if .95 < p[1] < 1.4 and abs(p[0]) < .13 else \
                               'arms' if p[1] > 1.3 and (.22 < abs(p[0]) < .33 or .5 < abs(p[0]) < .58) else \
                               'legs' if .15 < p[1] < .8 else None
                        if part:
                            offsets[part].append(nearest(cells, p))
                for part, values in offsets.items():
                    self.assertGreater(sum(values)/len(values), .02,
                                       f'{part} armor must not be a thin skinsuit')
                    self.assertLess(sum(values)/len(values), .06, f'{part} armor must stay close-fitting')

    def test_boots_have_flat_soles_and_enclose_the_toes(self):
        for sex in ('Male', 'Female'):
            with self.subTest(sex=sex):
                g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
                shell = next(m for m in g.data['meshes'] if m['name'].endswith('Shell'))
                points = [p for primitive in shell['primitives'] for p in g.values(primitive['attributes']['POSITION'])]
                sole = min(p[1] for p in points)
                for side in (-1, 1):
                    foot = [p for p in points if p[0]*side > 0 and p[1] < .06]
                    flat = [p for p in foot if p[1] < sole+.003]
                    self.assertGreater(len(flat), 25, 'boots need a flat sole, not bare toes')
                    toe = max(p[2] for p in foot)
                    self.assertLess(max(p[1] for p in foot if p[2] > toe-.03), .07, 'toe cap must be low and closed')

    def test_both_legs_read_as_armored_instead_of_plain_trousers(self):
        for sex in ('Male', 'Female'):
            g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
            shell = next(m for m in g.data['meshes'] if m['name'].endswith('Shell'))
            for side in (-1, 1):
                counts = {0: 0, 1: 0}
                for primitive in shell['primitives']:
                    positions = g.values(primitive['attributes']['POSITION'])
                    ids = [i[0] for i in g.values(primitive['indices'])]
                    for k in range(0, len(ids), 3):
                        ps = [positions[i] for i in ids[k:k+3]]
                        if all(p[1] < .85 and p[0]*side > 0 for p in ps):
                            counts[primitive['material']] += 1
                self.assertGreater(counts[0], 500, 'both legs must have visible titanium coverage')
                self.assertGreater(counts[0]/sum(counts.values()), .7, 'rear and side armor must not disappear into the dark frame')

    def test_armor_hugs_the_body_in_combat_and_movement(self):
        animation = Gltf(ROOT / 'characters/animations/UAL1_Standard.glb')
        for sex in ['Male', 'Female']:
            g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
            reference, _ = sealed_body(Gltf(ROOT / f'characters/outfits/{sex}_Peasant.gltf'))
            for clip, time in [('Idle_Loop', .5), ('Walk_Loop', .4), ('Pistol_Idle_Loop', .35),
                               ('Pistol_Shoot', .12), ('Fixing_Kneeling', .7), ('Sitting_Idle_Loop', .5)]:
                with self.subTest(sex=sex, clip=clip):
                    matrices = g.skin_matrices(pose=animation.sample(clip, time))
                    body, _ = deform(*[list(values) for values in zip(*reference)], matrices)
                    cells = {}
                    for point in body:
                        key = tuple(math.floor(v/.05) for v in point)
                        cells.setdefault(key, []).append(point)
                    for mesh in g.data['meshes']:
                        for p in mesh['primitives']:
                            attrs = {k: g.values(v) for k, v in p['attributes'].items()}
                            indices = [i[0] for i in g.values(p['indices'])]
                            self.assertLess(max(indices), len(attrs['POSITION']))
                            self.assertTrue(all(abs(sum(w)-1) < 1e-5 and min(w) >= 0 for w in attrs['WEIGHTS_0']))
                            ps, _ = deform(attrs['POSITION'], attrs['NORMAL'], attrs['JOINTS_0'], attrs['WEIGHTS_0'], matrices)
                            self.assertTrue(all(math.isfinite(v) and abs(v) < 3 for row in ps for v in row))
                            if mesh['name'].endswith('Shell'):
                                # Plates have volume but must stay on the moving body.
                                for point in ps[::5]:
                                    self.assertLess(nearest(cells, point), .095,
                                                    'the armor shell must follow the body, including joints')
                            else:
                                self.assertTrue(any(sum(w > .05 for w in ws) > 1 for ws in attrs['WEIGHTS_0']),
                                                'panels must bend with the body rather than remain rigid')
                                # Sample every panel material and all views; compare against
                                # the independently animated body, not another armor mesh.
                                for point in ps[::23]:
                                    self.assertLess(nearest(cells, point), .095,
                                                    'armor details must remain on the shell during bending and gun poses')


if __name__ == '__main__':
    unittest.main()
