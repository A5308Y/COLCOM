"""Shared glTF writer for clothes and articulated armor on outfit skeletons."""
import copy
import json
from pathlib import Path
import struct


class SkinnedAsset:
    def __init__(self, source, generator):
        self.source = source
        self.blob = bytearray()
        # Outfit files put their 65 skeleton nodes before the render nodes.
        skeleton_end = max(source.data['skins'][0]['joints']) + 1
        self.data = {"asset": {"version": "2.0", "generator": generator},
                     "scene": 0, "scenes": [{"nodes": []}],
                     "nodes": copy.deepcopy(source.data["nodes"][:skeleton_end]),
                     "meshes": [], "materials": [], "bufferViews": [], "accessors": []}

    def accessor(self, rows, kind, ctype=5126, target=34962):
        while len(self.blob) % 4:
            self.blob.append(0)
        flat = [v for row in rows for v in row]
        raw = struct.pack("<" + len(flat) * {5126: "f", 5123: "H", 5125: "I"}[ctype], *flat)
        g = self.data
        g["bufferViews"].append({"buffer": 0, "byteOffset": len(self.blob), "byteLength": len(raw), **({"target": target} if target else {})})
        a = {"bufferView": len(g["bufferViews"]) - 1, "componentType": ctype, "count": len(rows), "type": kind}
        if kind == "VEC3":
            a.update(min=[min(r[i] for r in rows) for i in range(3)], max=[max(r[i] for r in rows) for i in range(3)])
        g["accessors"].append(a)
        self.blob.extend(raw)
        return len(g["accessors"]) - 1

    def write(self, path):
        path = Path(path)
        g = self.data
        skin = copy.deepcopy(self.source.data["skins"][0])
        skin["inverseBindMatrices"] = self.accessor(self.source.values(skin["inverseBindMatrices"]), "MAT4", target=0)
        g["skins"] = [skin]
        skeleton_nodes = len(g["nodes"])
        roots = sorted(set(range(skeleton_nodes)) - {c for n in g["nodes"] for c in n.get("children", [])})
        g["nodes"].append({"name": "Armature", "children": roots})
        for i, mesh in enumerate(g["meshes"]):
            g["nodes"].append({"name": mesh["name"], "mesh": i, "skin": 0})
        root = len(g["nodes"])
        g["nodes"].append({"name": path.stem, "children": list(range(skeleton_nodes, root))})
        g["scenes"][0]["nodes"] = [root]
        g["buffers"] = [{"uri": path.with_suffix('.bin').name, "byteLength": len(self.blob)}]
        path.with_suffix('.bin').write_bytes(self.blob)
        path.write_text(json.dumps(g, indent=2) + "\n")
