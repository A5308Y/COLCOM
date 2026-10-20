"""Powered suit coverage and articulation; python3 -B tools/test_power_armor.py."""
import math
from collections import Counter
from pathlib import Path
import unittest

from gltf_pose import Gltf, deform
from make_skinned_power_armor import sealed_body

ROOT = Path(__file__).resolve().parents[1] / 'assets/models'


class PoweredArmor(unittest.TestCase):
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
                actual = [i[0] for primitive in suit['primitives'] for i in g.values(primitive['indices'])]
                triangles = lambda ids: Counter(tuple(ids[k:k+3]) for k in range(0, len(ids), 3))
                self.assertEqual(triangles(actual), triangles(expected), 'the shell must cover every body triangle')
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
                                self.assertEqual(len(ps), len(body))
                                self.assertLess(max(math.dist(a, b) for a, b in zip(ps, body)), .011,
                                                'the complete shell must follow the body, including joints')
                            else:
                                self.assertTrue(any(sum(w > .05 for w in ws) > 1 for ws in attrs['WEIGHTS_0']),
                                                'panels must bend with the body rather than remain rigid')
                                # Sample every panel material and all views; compare against
                                # the independently animated body, not another armor mesh.
                                for point in ps[::23]:
                                    key = tuple(math.floor(v/.05) for v in point)
                                    nearby = [q for x in range(key[0]-2, key[0]+3)
                                              for y in range(key[1]-2, key[1]+3)
                                              for z in range(key[2]-2, key[2]+3)
                                              for q in cells.get((x,y,z), ())]
                                    self.assertTrue(nearby, 'an armor panel floated away from the body')
                                    self.assertLess(min(math.dist(point, q) for q in nearby), .06,
                                                    'armor must remain snug during bending and gun poses')


if __name__ == '__main__':
    unittest.main()
