"""Generates the arena world: client/godot/worlds/arena/world.json.

    python client/godot/tools/make_arena.py

The example level. A lobby you arrive into, a gate you walk through, and an
arena on the other side of it: cover to hide behind, ramps to run up, a
gallery to drop off, and a blaster on a rack at each end.

What this is NOT, and the reason it is a level rather than a game: nothing
here shoots. Firing at another player needs the server to arbitrate it --
shots, hits, health, respawns, a score -- and none of those exist on the wire
yet (see docs/PROTOCOL.md). The blasters are ordinary grabbables, so you
can pick them up, carry them and throw them at each other, which the item
protocol already does. The gunplay is the next piece of work, not this one.

Same rules as the hub (see make_hub.py): everything solid is `static`, only
light is `none`, and no two walkable surfaces share a plane.
"""

import json
import math
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "worlds" / "arena"

FLOOR = 0.0
LOBBY_HALF = 9.0
ARENA_HALF = 26.0
WALL = 5.5

STONE = "#5d6068"
STONE_DARK = "#3f424a"
METAL = "#596273"
TEAM_A = "#d8554a"
TEAM_B = "#4a9ad8"
LIGHT = "#9fd8ff"

objects = []


def add(name, shape, size, pos, color=STONE, body="static", **extra):
    entry = {
        "name": name, "shape": shape,
        "size": [round(v, 4) for v in size], "pos": [round(v, 4) for v in pos],
        "color": color, "body": body,
    }
    entry.update(extra)
    objects.append(entry)
    return entry


def light(name, pos, color, energy, reach):
    objects.append({
        "name": name,
        "light": {"color": color, "energy": energy, "range": reach},
        "pos": [round(v, 4) for v in pos],
    })


def glow(name, shape, size, pos, color, energy=2.6):
    return add(name, shape, size, pos, color=color, body="none",
               emission=color, emission_energy=energy, roughness=0.3)


def room(name, half_x, half_z, centre_z, top):
    """A floor with four walls, open where the gate is."""
    add("%sFloor" % name, "box", [half_x * 2, 1.0, half_z * 2],
        [0.0, top - 0.5, centre_z], color=STONE, roughness=0.8)
    for side, size, at in [
        ("N", [half_x * 2, WALL, 0.6], [0.0, top + WALL / 2, centre_z - half_z]),
        ("S", [half_x * 2, WALL, 0.6], [0.0, top + WALL / 2, centre_z + half_z]),
        ("W", [0.6, WALL, half_z * 2], [-half_x, top + WALL / 2, centre_z]),
        ("E", [0.6, WALL, half_z * 2], [half_x, top + WALL / 2, centre_z]),
    ]:
        add("%sWall%s" % (name, side), "box", size, at, color=STONE_DARK, roughness=0.75)


# --- the lobby -------------------------------------------------------------

# You land here. The gate out is the gap in its north wall.
room("Lobby", LOBBY_HALF, LOBBY_HALF, LOBBY_HALF + 2.0, FLOOR)
# Knock the gate out of the north wall by covering it with two stubs instead.
objects[:] = [o for o in objects if o["name"] != "LobbyWallN"]
for i, x in enumerate([-LOBBY_HALF / 2 - 1.0, LOBBY_HALF / 2 + 1.0]):
    add("LobbyGateWall%d" % i, "box", [LOBBY_HALF - 2.0, WALL, 0.6],
        [x, FLOOR + WALL / 2, 2.0], color=STONE_DARK, roughness=0.75)

# The gate itself: a lit frame you walk through to reach the arena.
for i, x in enumerate([-2.0, 2.0]):
    add("GatePost%d" % i, "box", [0.5, 4.2, 0.9], [x, FLOOR + 2.1, 2.0],
        color=METAL, metallic=0.7, roughness=0.35)
add("GateArch", "box", [4.9, 0.5, 0.9], [0.0, FLOOR + 4.45, 2.0],
    color=METAL, metallic=0.7, roughness=0.35)
glow("GateGlow", "box", [3.5, 0.12, 0.3], [0.0, FLOOR + 4.1, 2.0], LIGHT, energy=3.2)
light("GateLight", [0.0, FLOOR + 3.4, 2.6], LIGHT, 2.6, 12.0)

# A step up to the gate, so the way out reads as a way out.
add("GateStep", "cylinder", [7.0, 0.3, 7.0], [0.0, FLOOR + 0.15, 4.4],
    color=STONE_DARK, roughness=0.7)

# Benches to wait on, and a light over each.
for i, x in enumerate([-5.5, 5.5]):
    add("LobbyBench%d" % i, "box", [1.0, 0.25, 4.0], [x, FLOOR + 0.52, 12.0],
        color=STONE, roughness=0.65)
    add("LobbyLeg%dA" % i, "box", [0.8, 0.5, 0.5], [x, FLOOR + 0.25, 10.4],
        color=STONE_DARK, roughness=0.7)
    add("LobbyLeg%dB" % i, "box", [0.8, 0.5, 0.5], [x, FLOOR + 0.25, 13.6],
        color=STONE_DARK, roughness=0.7)
    glow("LobbyLamp%d" % i, "sphere", [0.5, 0.5, 0.5], [x, FLOOR + 3.6, 12.0], LIGHT)
    light("LobbyLight%d" % i, [x, FLOOR + 3.6, 12.0], LIGHT, 2.0, 11.0)

# --- the arena -------------------------------------------------------------

ARENA_Z = -(ARENA_HALF + 3.0)
room("Arena", ARENA_HALF, ARENA_HALF, ARENA_Z, FLOOR)

# A ramp up to a gallery at each end, which is what the walkable-slope work is
# for: a run of shallow steps you can go straight up without jumping.
for end, sign in (("A", -1.0), ("B", 1.0)):
    base_z = ARENA_Z + sign * (ARENA_HALF - 3.0)
    for i in range(7):
        add("Ramp%s%d" % (end, i), "box", [9.0, 0.4, 1.6],
            [sign * 12.0, FLOOR + 0.2 + i * 0.4, base_z - sign * i * 1.6],
            color=STONE_DARK, roughness=0.75)
    # Butted up against the last step, not laid over it: the ramp tops out at
    # the gallery's height, so any overlap would be two walkable faces on one
    # plane and the whole platform would flicker as you crossed it.
    add("Gallery%s" % end, "box", [16.0, 0.6, 7.0],
        [sign * 12.0, FLOOR + 2.5, base_z - sign * 13.95], color=STONE, roughness=0.75)
    add("GalleryRail%s" % end, "box", [16.0, 0.9, 0.3],
        [sign * 12.0, FLOOR + 3.25, base_z - sign * 17.25], color=METAL,
        metallic=0.6, roughness=0.45)
    colour = TEAM_A if end == "A" else TEAM_B
    glow("Banner%s" % end, "box", [6.0, 2.6, 0.2],
        [sign * 12.0, FLOOR + 4.4, base_z + sign * 2.4], colour, energy=1.6)
    light("EndLight%s" % end, [sign * 12.0, FLOOR + 4.0, base_z - sign * 8.0],
          colour, 2.2, 18.0)

# Cover. Two rings of blocks at different heights: the low ones you can see
# over, the tall ones you cannot.
for i in range(10):
    angle = i / 10.0 * math.tau
    radius = 9.0 if i % 2 == 0 else 16.0
    tall = i % 3 == 0
    add("Cover%d" % i, "box",
        [2.6, 2.4 if tall else 1.2, 2.6],
        [math.cos(angle) * radius, FLOOR + (1.2 if tall else 0.6), ARENA_Z + math.sin(angle) * radius],
        color=STONE, roughness=0.8, rot=[0.0, round(math.degrees(angle), 3), 0.0])

# A pillar in the middle with a light on it, so the centre is worth holding.
add("Centre", "cylinder", [2.2, 4.0, 2.2], [0.0, FLOOR + 2.0, ARENA_Z],
    color=STONE_DARK, roughness=0.6)
glow("CentreGlow", "torus", [2.4, 0.3, 3.0], [0.0, FLOOR + 4.1, ARENA_Z], LIGHT, energy=2.4)
light("CentreLight", [0.0, FLOOR + 5.0, ARENA_Z], LIGHT, 2.8, 22.0)

# --- things to pick up -----------------------------------------------------

# Blasters. Ordinary grabbables: you can take them, carry them and throw them.
# They do not fire, because firing is a thing the server would have to judge.
for end, sign in (("A", -1.0), ("B", 1.0)):
    rack_z = ARENA_Z + sign * (ARENA_HALF - 5.0)
    add("Rack%s" % end, "box", [3.2, 1.0, 0.8], [sign * -9.0, FLOOR + 0.5, rack_z],
        color=METAL, metallic=0.6, roughness=0.5)
    colour = TEAM_A if end == "A" else TEAM_B
    for i in range(2):
        add("Blaster %s%d" % (end, i + 1), "box", [0.7, 0.18, 0.22],
            [sign * -9.0 + (i - 0.5) * 1.1, FLOOR + 1.15, rack_z],
            color=colour, roughness=0.4, metallic=0.3,
            body="grabbable", mass=1.2, reach=0.6)

# And something to argue over in the middle.
add("Prize", "sphere", [0.4, 0.4, 0.4], [0.0, FLOOR + 4.6, ARENA_Z],
    color="#ffd479", roughness=0.15, emission="#ffd479", emission_energy=2.4,
    body="grabbable", mass=0.8, reach=1.2, locked=True)

# --- checks and write ------------------------------------------------------

ghosts = [o for o in objects if o.get("body") == "none" and not o.get("emission")]
assert not ghosts, "collision-less objects that are not lights: %s" % [o["name"] for o in ghosts]

tops = {}
for e in objects:
    if e.get("body") != "static" or e.get("shape") not in ("box", "cylinder"):
        continue
    if any(abs(v) > 0.001 for v in e.get("rot", [0, 0, 0])):
        continue
    tops.setdefault(round(e["pos"][1] + e["size"][1] / 2, 4), []).append(e)
# How much two footprints have to share before it is worth complaining. Walls
# meeting at a corner overlap by the square of their thickness and nobody has
# ever seen it; a path laid over a plaza overlaps by square metres and
# flickers the whole way along.
MIN_OVERLAP = 1.0


def _overlap(a, b):
    """Shared floor area of two footprints, in square metres.

    Round things are compared as circles. Boxing them instead made a disc
    thirty metres clear of another disc look like a thirty square metre
    overlap, because their corners met."""
    if a.get("shape") == "cylinder" and b.get("shape") == "cylinder":
        gap = math.dist((a["pos"][0], a["pos"][2]), (b["pos"][0], b["pos"][2]))
        radii = (a["size"][0] + b["size"][0]) / 2
        return 0.0 if gap >= radii else math.pi * ((radii - gap) / 2) ** 2
    across = min(a["pos"][0] + a["size"][0] / 2, b["pos"][0] + b["size"][0] / 2)         - max(a["pos"][0] - a["size"][0] / 2, b["pos"][0] - b["size"][0] / 2)
    along = min(a["pos"][2] + a["size"][2] / 2, b["pos"][2] + b["size"][2] / 2)         - max(a["pos"][2] - a["size"][2] / 2, b["pos"][2] - b["size"][2] / 2)
    return max(0.0, across) * max(0.0, along)


for top, sharing in tops.items():
    for i, a in enumerate(sharing):
        for b in sharing[i + 1:]:
            assert _overlap(a, b) < MIN_OVERLAP, (
                "%s and %s %s at y=%s and overlap by %.1f m2: that flickers"
                % (a["name"], b["name"], "share a top face", top, _overlap(a, b)))

assert sum(1 for o in objects if o.get("body") == "grabbable") >= 4

world = {
    "format": 1,
    "kind": "world",
    "name": "arena",
    # In the lobby, facing the gate.
    "spawn": {"pos": [0.0, FLOOR, 13.0], "yaw": 180.0},
    "sky": {
        "top": "#0a0d1c", "horizon": "#2a2436", "ground": "#06070c",
        "ambient": "#4a5d8c", "fog": 0.004,
        "sun": {"color": "#8fa8ff", "energy": 0.22, "angles": [-50.0, 15.0]},
    },
    "kill_y": -25.0,
    "objects": objects,
}

OUT.mkdir(parents=True, exist_ok=True)
path = OUT / "world.json"
path.write_text(json.dumps(world, indent="\t"), encoding="utf-8")
print("%s: %d objects (%d solid, %d lights, %d pickups), %d KiB" % (
    path.name, len(objects),
    sum(1 for o in objects if o.get("body") == "static"),
    sum(1 for o in objects if "light" in o),
    sum(1 for o in objects if o.get("body") == "grabbable"),
    len(path.read_bytes()) // 1024,
))
