#!/usr/bin/env python3
"""Review clothing on the actual outfit in locomotion, work and gun poses.

python3 tools/preview_clothes_poses.py --out target/clothes-poses
CPU skinning with the game's animation library; textures/lighting are omitted.
"""
import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from gltf_pose import Gltf, deform, matrix, multiply, point, unit
from make_gear import Gear, power_helmet, laser_rifle
from make_rifle import PALETTE, Rifle
from preview_gear import write_gallery

ROOT = Path(__file__).resolve().parents[1]
POSES = [
    ("Idle", "Idle_Loop", .5, None),
    ("Walk", "Walk_Loop", .4, None),
    ("Carry", "Walk_Loop", .4, "Driving_Loop"),
    ("Aim", "Pistol_Idle_Loop", .35, None),
    ("Fire", "Pistol_Shoot", .12, None),
    ("Kneel", "Fixing_Kneeling", .7, None),
    ("Sit", "Sitting_Idle_Loop", .5, None),
    ("Deployed", "Fixing_Kneeling", .7, "Pistol_Idle_Loop"),
]


def posed(sex, coat, clip, time, upper=None, leg_top="SimpleClothes"):
    powered = coat == "PoweredArmor"
    legged = coat in ("CargoTrousers", "FlakLegGuards")
    weapon_offset = 5
    outfit = Gltf(ROOT / f"assets/models/characters/outfits/{sex}_Peasant.gltf")
    folder = "armor" if powered else "items"
    garment = Gltf(ROOT / f"assets/models/{folder}/{coat}{sex}.gltf")
    head = Gltf(ROOT / f"assets/models/characters/base/Superhero_{sex}_Head.gltf")
    animation = Gltf(ROOT / "assets/models/characters/animations/UAL1_Standard.glb")
    pose = animation.sample(clip, time)
    if upper:
        # Match the game's upper-body mask, rooted at spine_01.
        nodes = animation.data["nodes"]
        stack = [next(i for i, n in enumerate(nodes) if n.get("name") == "spine_01")]
        upper_names = set()
        while stack:
            node = nodes[stack.pop()]
            upper_names.add(node.get("name"))
            stack.extend(node.get("children", []))
        pose.update((name, value) for name, value in animation.sample(upper, .35).items() if name in upper_names)
    colors = [tuple(pow(v, 1/2.2) for v in m["pbrMetallicRoughness"].get("baseColorFactor", [1, 1, 1])[:3])
              for m in garment.data["materials"][:3]]
    palette = [("Fabric", colors[0], 0, .9), ("Trim", colors[1], 0, .9), ("Seams", colors[2], 0, .9),
               ("Skin without textures", (.52, .40, .32), 0, .9), ("Trousers and boots", (.23, .26, .27), 0, .9)] + list(PALETTE)
    if powered or legged:
        palette = [(m["name"], tuple(pow(v, 1/2.2) for v in m["pbrMetallicRoughness"]["baseColorFactor"][:3]),
                    m["pbrMetallicRoughness"]["metallicFactor"], m["pbrMetallicRoughness"]["roughnessFactor"])
                   for m in garment.data["materials"]] + list(PALETTE)
    if legged:
        top = Gltf(ROOT / f"assets/models/items/{leg_top}{sex}.gltf")
        top_palette = [(m['name'], tuple(pow(v, 1/2.2) for v in m['pbrMetallicRoughness']['baseColorFactor'][:3]), 0, .9)
                       for m in top.data['materials'][:3]]
        palette = palette[:5]+top_palette+[("Skin without textures", (.52,.40,.32), 0, .9)]+list(PALETTE)
        weapon_offset = 9
    result = Gear(palette)

    def add(source, mesh, material):
        transforms = source.skin_matrices(pose=pose)
        for primitive in source.data["meshes"][mesh]["primitives"]:
            attrs = {key: source.values(value) for key, value in primitive["attributes"].items()}
            positions, normals = deform(attrs["POSITION"], attrs["NORMAL"], attrs["JOINTS_0"], attrs["WEIGHTS_0"], transforms)
            indices = [i[0] for i in source.values(primitive["indices"])]
            used = sorted(set(indices))
            remap = {j: i for i, j in enumerate(used)}
            mat = primitive["material"] if material is None else material[primitive["material"]] if isinstance(material, dict) else material
            dst = result.groups[mat]
            offset = len(dst[0])
            dst[0].extend(positions[i] for i in used)
            dst[1].extend(normals[i] for i in used)
            dst[2].extend(offset + remap[i] for i in indices)

    if powered:
        for i in range(len(garment.data["meshes"])):
            add(garment, i, None)
        # The helmet remains a rigid attachment above the blanket in-game.
        joint = next(i for i, j in enumerate(outfit.data["skins"][0]["joints"]) if outfit.data["nodes"][j]["name"] == "Head")
        rest = outfit.globals()[outfit.data["skins"][0]["joints"][joint]]
        transform = outfit.skin_matrices(pose=pose)[joint]
        for dst, (positions, normals, indices) in zip(result.groups, power_helmet().groups):
            offset = len(dst[0])
            dst[0].extend(point(transform, tuple(p[k]+rest[k][3] for k in range(3))) for p in positions)
            dst[1].extend(unit(point(transform, n, 0)) for n in normals)
            dst[2].extend(i+offset for i in indices)
    elif legged:
        for i in range(len(garment.data['meshes'])):
            add(garment, i, None)
        add(top, 0, {0: 5, 1: 6, 2: 7})
        add(top, 1, 8)
        for i, mesh in enumerate(head.data['meshes']):
            add(head, i, 7 if 'Face' in mesh.get('name', '') else 8)
    else:
        add(garment, 0, None)
        add(garment, 1, 3)
        for i, mesh in enumerate(outfit.data["meshes"]):
            if mesh["name"].endswith(("_Legs", "_Feet")):
                add(outfit, i, 4)
        for i, mesh in enumerate(head.data["meshes"]):
            add(head, i, 2 if "Face" in mesh.get("name", "") else 3)

    if clip.startswith("Pistol_") or (upper and upper.startswith("Pistol_")):
        globals = outfit.globals(pose)
        hand = next(i for i, n in enumerate(outfit.data["nodes"]) if n.get("name") == "hand_r")
        aim = unit((-.007, .77, .638, -.025))
        grip = matrix({"rotation": [-aim[0], -aim[1], -aim[2], aim[3]]})
        transform = multiply(globals[hand], grip)
        weapon = laser_rifle() if powered else Rifle().build()
        for dst, (positions, normals, indices) in zip(result.groups[weapon_offset:], weapon.groups):
            offset = len(dst[0])
            dst[0].extend(point(transform, p) for p in positions)
            dst[1].extend(unit(point(transform, n, 0)) for n in normals)
            dst[2].extend(i + offset for i in indices)
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("target/clothes-poses"))
    parser.add_argument("--pose", nargs="+", choices=[p[0] for p in POSES], help="Render only these poses")
    parser.add_argument("--garment", nargs="+", choices=["SimpleClothes", "Parka", "PoweredArmor", "CargoTrousers", "FlakLegGuards"], default=["SimpleClothes", "Parka"])
    parser.add_argument("--leg-top", choices=["SimpleClothes", "Parka"], default="SimpleClothes", help="Upper garment for leg-apparel previews")
    args = parser.parse_args()
    entries = []
    for sex in ("Male", "Female"):
        for coat in args.garment:
            for label, clip, time, upper in POSES:
                if args.pose and label not in args.pose:
                    continue
                geometry = posed(sex, coat, clip, time, upper, args.leg_top)
                for view, turn in [("", 0), ("-Side", -123), ("-Back", 180)]:
                    shown = geometry if turn == 0 else Gear(geometry.palette).add(geometry, turn=(0, turn, 0))
                    entries.append(("armor", f"{coat}-{sex}-{label}{view}", shown, None))
    if args.garment == ["PoweredArmor"]:
        write_gallery(entries, args.out, "powered armor pose review", "Male and female fits, a continuous metal shell with integrated panels and fitted cyan details that bend with the body. Sampled game animation clips and laser-rifle grip; CPU skinning without game lighting or emissive effects.")
    else:
        write_gallery(entries, args.out, "apparel pose review", "Actual outfit skeletons and shared animation clips. Includes movement, carrying, work, sitting and layered kneeling aim. CPU skinning; textures and game lighting omitted. Counts include native body parts and weapons where equipped.")
