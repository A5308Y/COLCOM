"""Small glTF reader and CPU skinning helpers for garment generation/review."""
import bisect
import json
from pathlib import Path
import struct

from asset_helpers import IDENTITY, matrix, multiply, point, unit


class Gltf:
    def __init__(self, path):
        self.path = Path(path)
        raw = self.path.read_bytes()
        binary = None
        if raw[:4] == b"glTF":
            length, _ = struct.unpack_from("<II", raw, 12)
            self.data = json.loads(raw[20:20 + length])
            offset = 20 + length
            size, _ = struct.unpack_from("<II", raw, offset)
            binary = raw[offset + 8:offset + 8 + size]
        else:
            self.data = json.loads(raw)
        self.buffers = [self.path.with_name(b["uri"]).read_bytes() if "uri" in b and "/" not in b["uri"]
                        else (self.path.parent / b["uri"]).read_bytes() if "uri" in b else binary
                        for b in self.data["buffers"]]
        self._cache = {}

    def values(self, index):
        if index in self._cache:
            return self._cache[index]
        a = self.data["accessors"][index]
        v = self.data["bufferViews"][a["bufferView"]]
        count = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}[a["type"]]
        fmt = "<" + count * {5121: "B", 5123: "H", 5125: "I", 5126: "f"}[a["componentType"]]
        offset = v.get("byteOffset", 0) + a.get("byteOffset", 0)
        stride = v.get("byteStride", struct.calcsize(fmt))
        result = [struct.unpack_from(fmt, self.buffers[v["buffer"]], offset + i * stride) for i in range(a["count"])]
        if a.get("normalized"):
            divisor = {5121: 255, 5123: 65535}[a["componentType"]]
            result = [tuple(v / divisor for v in row) for row in result]
        self._cache[index] = result
        return result

    def globals(self, pose=None):
        result = {}
        def visit(i, parent):
            node = self.data["nodes"][i]
            local = dict(node)
            local.update((pose or {}).get(node.get("name"), {}))
            result[i] = multiply(parent, matrix(local))
            for child in node.get("children", []):
                visit(child, result[i])
        children = {c for n in self.data["nodes"] for c in n.get("children", [])}
        for i in range(len(self.data["nodes"])):
            if i not in children:
                visit(i, IDENTITY)
        return result

    def sample(self, name, time):
        anim = next(a for a in self.data["animations"] if a["name"] == name)
        result = {}
        for c in anim["channels"]:
            s = anim["samplers"][c["sampler"]]
            times = [v[0] for v in self.values(s["input"])]
            values = self.values(s["output"])
            i = max(0, min(len(times) - 2, bisect.bisect_right(times, time) - 1))
            t = max(0, min(1, (time - times[i]) / max(1e-12, times[min(i + 1, len(times) - 1)] - times[i])))
            a, b = values[i], values[min(i + 1, len(values) - 1)]
            path = c["target"]["path"]
            if path == "rotation" and sum(x * y for x, y in zip(a, b)) < 0:
                b = tuple(-x for x in b)
            value = tuple(x * (1 - t) + y * t for x, y in zip(a, b))
            if path == "rotation":
                value = unit(value)
            result.setdefault(self.data["nodes"][c["target"]["node"]]["name"], {})[path] = value
        return result

    def skin_matrices(self, skin=0, pose=None):
        globals = self.globals(pose)
        skin = self.data["skins"][skin]
        binds = self.values(skin["inverseBindMatrices"])
        return [multiply(globals[j], [[bind[c * 4 + r] for c in range(4)] for r in range(4)])
                for j, bind in zip(skin["joints"], binds)]


def deform(positions, normals, joints, weights, matrices):
    points, directions = [], []
    for p, n, js, ws in zip(positions, normals, joints, weights):
        ps, ns = [(point(matrices[j], p), w) for j, w in zip(js, ws) if w > 0], [(point(matrices[j], n, 0), w) for j, w in zip(js, ws) if w > 0]
        points.append(tuple(sum(v[i] * w for v, w in ps) for i in range(3)))
        directions.append(unit(tuple(sum(v[i] * w for v, w in ns) for i in range(3))))
    return points, directions
