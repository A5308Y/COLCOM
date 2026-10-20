"""Geometry and assembly contracts for 10d props: python3 -B tools/test_world_models.py."""
import math
from pathlib import Path
import unittest

from asset_helpers import point
from gltf_pose import Gltf
import make_world_props
import make_plants

MODELS = Path(__file__).resolve().parents[1] / "assets/models"


def scene_meshes(g, scene=0):
    globals = g.globals()
    todo = list(g.data['scenes'][scene]['nodes'])
    while todo:
        index = todo.pop()
        node = g.data['nodes'][index]
        todo.extend(node.get('children', []))
        if 'mesh' in node:
            for primitive in g.data['meshes'][node['mesh']]['primitives']:
                yield node, primitive, globals[index]


def positions(g, scene=0):
    return [point(matrix, p) for _,primitive,matrix in scene_meshes(g,scene)
            for p in g.values(primitive['attributes']['POSITION'])]


class WorldModels(unittest.TestCase):
    def test_generated_geometry_is_finite_opaque_and_within_budget(self):
        names = [('world',name) for _,name,_,_,_ in make_world_props.models()]
        names += [(folder,name) for folder,name,_,_ in make_plants.models()]
        self.assertEqual(len(names),31)
        for folder,name in names:
            with self.subTest(model=name):
                g=Gltf(MODELS / folder / (name+'.gltf'))
                self.assertLessEqual(len(g.data['materials']),5)
                self.assertNotIn('textures',g.data)
                for material in g.data['materials']:
                    self.assertEqual(material.get('alphaMode','OPAQUE'),'OPAQUE')
                for scene in range(len(g.data['scenes'])):
                    total=0
                    for _,prim,_ in scene_meshes(g,scene):
                        attrs=prim['attributes']
                        self.assertEqual(set(attrs),{'POSITION','NORMAL'})
                        pts=g.values(attrs['POSITION']);normals=g.values(attrs['NORMAL'])
                        self.assertTrue(all(math.isfinite(v) for p in pts for v in p))
                        self.assertTrue(all(abs(math.sqrt(sum(v*v for v in n))-1)<1e-5 for n in normals))
                        ids=[v[0] for v in g.values(prim['indices'])]
                        self.assertTrue(all(i<len(pts) for i in ids))
                        total+=len(ids)//3
                    self.assertGreater(total,0)
                    self.assertLessEqual(total,2000 if folder=='world' else 1000)
                    pts=positions(g,scene)
                    lo=[min(p[i] for p in pts) for i in range(3)]
                    hi=[max(p[i] for p in pts) for i in range(3)]
                    self.assertGreaterEqual(lo[1],-1e-6)
                    self.assertLessEqual(lo[1],.04)
                    self.assertLessEqual(hi[1],1.001)
                    self.assertTrue(lo[0]>=-.501 and hi[0]<=.501 and lo[2]>=-.501 and hi[2]<=.501,(name,lo,hi))

    def test_work_surfaces_stair_treads_and_ladder_rungs_keep_contact_heights(self):
        contacts={'Stove':[.745],'Sawbench':[.535],'Stonecutter':[.58],'CraftingBench':[.585],
                  'MachinistBench':[.535],'ChemistryLab':[.53],'ElectronicsBench':[.53],
                  'SalvageBench':[.535],'Stairs':[.25,.5,.75,1.0],'Ladder':[.115,.315,.515,.715,.915]}
        for name,heights in contacts.items():
            with self.subTest(model=name):
                g=Gltf(MODELS / 'world' / (name+'.gltf'))
                tops=[]
                for _,primitive,_ in scene_meshes(g):
                    ps=g.values(primitive['attributes']['POSITION'])
                    ns=g.values(primitive['attributes']['NORMAL'])
                    tops.extend(p[1] for p,n in zip(ps,ns) if n[1]>.999)
                for height in heights:
                    self.assertTrue(any(abs(y-height)<1e-5 for y in tops),(name,height))

    def test_grave_scenes_and_every_fence_mask_share_the_right_meshes(self):
        grave=Gltf(MODELS / 'world/Grave.gltf')
        self.assertEqual([n['name'] for n,_,_ in scene_meshes(grave,0)][0],'Pit')
        names=lambda scene: {n['name'] for n,_,_ in scene_meshes(grave,scene)}
        self.assertEqual(names(0),{'Marker','Pit'})
        self.assertEqual(names(1),{'Marker','Mound'})
        fence=Gltf(MODELS / 'world/Fence.gltf')
        self.assertEqual(len(fence.data['meshes']),5)
        for mask in range(16):
            nodes={n['name'] for n,_,_ in scene_meshes(fence,mask)}
            self.assertEqual(nodes,{'Post'} | {f'Rail{i}' for i in range(4) if mask & (1<<i)})
            for node,_,_ in scene_meshes(fence,mask):
                self.assertEqual(node['mesh'],0 if node['name']=='Post' else int(node['name'][-1])+1)

    def test_stair_markings_cover_each_tread_once_without_coplanar_overlaps(self):
        g=Gltf(MODELS / 'world/Stairs.gltf')
        areas={h:0. for h in (.25,.5,.75,1.)}
        for _,primitive,_ in scene_meshes(g):
            pts=g.values(primitive['attributes']['POSITION'])
            normals=g.values(primitive['attributes']['NORMAL'])
            ids=[v[0] for v in g.values(primitive['indices'])]
            for k in range(0,len(ids),3):
                a,b,c=[pts[i] for i in ids[k:k+3]]
                if normals[ids[k]][1]<.999:continue
                for height in areas:
                    if all(abs(p[1]-height)<1e-6 for p in (a,b,c)):
                        areas[height]+=abs((b[0]-a[0])*(c[2]-a[2])-(c[0]-a[0])*(b[2]-a[2]))/2
        for height,area in areas.items():
            self.assertAlmostEqual(area,.25*.9,places=6,msg=f'tread at {height} has overlapping or missing faces')

    def test_original_crops_grow_in_height_and_keep_distinct_mature_features(self):
        for species,count in [('Healroot',4),('Cotton',4),('RubberPlant',3)]:
            heights=[]
            for stage in range(1,count+1):
                g=Gltf(MODELS / f'plants/{species}{stage}.gltf')
                heights.append(max(p[1] for p in positions(g)))
            self.assertTrue(all(a<b for a,b in zip(heights,heights[1:])),(species,heights))
            mature=Gltf(MODELS / f'plants/{species}{count}.gltf')
            materials={p['material'] for m in mature.data['meshes'] for p in m['primitives']}
            self.assertIn(3 if species=='Healroot' else 4,materials)


if __name__=='__main__':
    unittest.main()
