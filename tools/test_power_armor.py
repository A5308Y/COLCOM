"""Powered suit coverage and articulation; python3 -B tools/test_power_armor.py."""
import math
from pathlib import Path
import unittest

from gltf_pose import Gltf, deform

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
                suit = next(m for m in g.data['meshes'] if m['name'].endswith('Undersuit'))
                covered = set()
                for p in suit['primitives']:
                    self.assertEqual(p['material'], 2, 'hood and gloves share the dark protective fabric')
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

    def test_plates_stay_rigid_and_skins_deform_in_combat_and_movement(self):
        animation = Gltf(ROOT / 'characters/animations/UAL1_Standard.glb')
        for sex in ['Male', 'Female']:
            g = Gltf(ROOT / f'armor/PoweredArmor{sex}.gltf')
            for clip, time in [('Idle_Loop', .5), ('Walk_Loop', .4), ('Pistol_Idle_Loop', .35),
                               ('Pistol_Shoot', .12), ('Fixing_Kneeling', .7), ('Sitting_Idle_Loop', .5)]:
                with self.subTest(sex=sex, clip=clip):
                    matrices = g.skin_matrices(pose=animation.sample(clip, time))
                    for mesh in g.data['meshes']:
                        for p in mesh['primitives']:
                            attrs = {k: g.values(v) for k, v in p['attributes'].items()}
                            indices = [i[0] for i in g.values(p['indices'])]
                            self.assertLess(max(indices), len(attrs['POSITION']))
                            self.assertTrue(all(abs(sum(w)-1) < 1e-5 and min(w) >= 0 for w in attrs['WEIGHTS_0']))
                            ps, _ = deform(attrs['POSITION'], attrs['NORMAL'], attrs['JOINTS_0'], attrs['WEIGHTS_0'], matrices)
                            self.assertTrue(all(math.isfinite(v) and abs(v) < 3 for row in ps for v in row))
                            if mesh['name'].endswith('Plates'):
                                for k in range(0, len(indices), 3):
                                    tri = indices[k:k+3]
                                    bones = {j for i in tri for j, w in zip(attrs['JOINTS_0'][i], attrs['WEIGHTS_0'][i]) if w > 0}
                                    self.assertEqual(len(bones), 1, 'ceramic plates must articulate as rigid pieces')
                                    a, b = tri[:2]
                                    self.assertAlmostEqual(math.dist(ps[a], ps[b]), math.dist(attrs['POSITION'][a], attrs['POSITION'][b]), delta=1e-5)


if __name__ == '__main__':
    unittest.main()
