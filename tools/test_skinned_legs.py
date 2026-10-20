"""Leg apparel coverage and actual animation: python3 -B tools/test_skinned_legs.py."""
from collections import Counter
import math
from pathlib import Path
import unittest

from gltf_pose import Gltf, deform
from make_skinned_legs import lower_body
from make_skinned_power_armor import BodySurface

ROOT = Path(__file__).resolve().parents[1]/'assets/models'


class LegApparel(unittest.TestCase):
    def test_worn_legs_include_both_complete_trousers_and_boots(self):
        for sex in ('Male','Female'):
            outfit = Gltf(ROOT/f'characters/outfits/{sex}_Peasant.gltf')
            reference, triangles, _ = lower_body(outfit)
            for item in ('CargoTrousers','FlakLegGuards'):
                with self.subTest(sex=sex, item=item):
                    g = Gltf(ROOT/f'items/{item}{sex}.gltf')
                    self.assertEqual(g.values(g.data['skins'][0]['inverseBindMatrices']), outfit.values(outfit.data['skins'][0]['inverseBindMatrices']))
                    self.assertEqual(g.data['skins'][0]['joints'], outfit.data['skins'][0]['joints'])
                    self.assertNotIn('images', g.data)
                    actual = []
                    covered = set()
                    padded = set()
                    positions = normals = None
                    names = [g.data['nodes'][i]['name'] for i in g.data['skins'][0]['joints']]
                    for mesh in g.data['meshes']:
                        if mesh['name'].endswith('Pockets'):
                            continue
                        for primitive in mesh['primitives']:
                            ids = [i[0] for i in g.values(primitive['indices'])]
                            actual.extend(tuple(ids[k:k+3]) for k in range(0,len(ids),3))
                            attrs = {k:g.values(v) for k,v in primitive['attributes'].items()}
                            positions, normals = attrs['POSITION'], attrs['NORMAL']
                            if mesh['name'].endswith('Pads'):
                                padded.update(ids)
                            for i in set(ids):
                                covered.update(names[j] for j,w in zip(attrs['JOINTS_0'][i],attrs['WEIGHTS_0'][i]) if w>.1)
                    if item == 'CargoTrousers':
                        self.assertEqual(Counter(actual), Counter(tuple(t) for t in triangles), 'leg replacement must include every trouser and boot face')
                    else:
                        # Panel boundaries split faces, but must preserve the complete
                        # source surface without missing areas or duplicate overlays.
                        base = [tuple(p[k]-normals[i][k]*(.018 if i in padded else 0) for k in range(3)) for i,p in enumerate(positions)]
                        def area(ps, faces):
                            result = 0
                            for a,b,c in faces:
                                u,v = [tuple(ps[i][k]-ps[a][k] for k in range(3)) for i in (b,c)]
                                cross = (u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0])
                                result += math.sqrt(sum(x*x for x in cross))/2
                            return result
                        self.assertAlmostEqual(area(base,actual), area([v[0] for v in reference],triangles), delta=1e-4)
                        used = {i for tri in actual for i in tri}
                        required = {i for tri in triangles for i in tri}
                        self.assertTrue(required <= used, 'clipping must retain the original coverage vertices')
                    for side in ('l','r'):
                        for bone in ('thigh','calf','foot'):
                            self.assertIn(f'{bone}_{side}', covered)
                    self.assertLess(min(v[0][1] for v in reference), .05, 'feet must reach the floor')

    def test_leg_clothing_and_guards_follow_combat_and_seated_poses(self):
        animation = Gltf(ROOT/'characters/animations/UAL1_Standard.glb')
        for sex in ('Male','Female'):
            reference, native_triangles, _ = lower_body(Gltf(ROOT/f'characters/outfits/{sex}_Peasant.gltf'))
            for item in ('CargoTrousers','FlakLegGuards'):
                g = Gltf(ROOT/f'items/{item}{sex}.gltf')
                for clip in ('Idle_Loop','Walk_Loop','Pistol_Idle_Loop','Pistol_Shoot','Fixing_Kneeling','Sitting_Idle_Loop'):
                    with self.subTest(sex=sex,item=item,clip=clip):
                        matrices = g.skin_matrices(pose=animation.sample(clip,.4))
                        native, native_normals = deform(*[list(v) for v in zip(*reference)], matrices)
                        surface = BodySurface([(p,n,v[2],v[3]) for p,n,v in zip(native,native_normals,reference)], [i for tri in native_triangles for i in tri])
                        for mesh in g.data['meshes']:
                            for primitive in mesh['primitives']:
                                attrs = {k:g.values(v) for k,v in primitive['attributes'].items()}
                                self.assertTrue(all(abs(sum(ws)-1)<1e-5 for ws in attrs['WEIGHTS_0']))
                                ps,_ = deform(*(attrs[k] for k in ('POSITION','NORMAL','JOINTS_0','WEIGHTS_0')), matrices)
                                ids = [i[0] for i in g.values(primitive['indices'])]
                                self.assertLess(max(ids),len(ps))
                                self.assertTrue(all(math.isfinite(v) and abs(v)<3 for p in ps for v in p))
                                if not mesh['name'].endswith('Pockets'):
                                    self.assertGreaterEqual(len(ps),len(native))
                                    self.assertLess(max(math.dist(a,b) for a,b in zip(ps,native)), .019,
                                                    'trousers and guards must stay against the animated legs')
                                    for p in ps[len(native)::31]:
                                        self.assertLess(surface.sample(p)[4], .04, 'new panel edges must follow the animated leg surface')
                                else:
                                    for p in ps[::11]:
                                        self.assertLess(min(math.dist(p,q) for q in native), .07,
                                                        'cargo pockets must follow their trouser surface')


if __name__ == '__main__':
    unittest.main()
