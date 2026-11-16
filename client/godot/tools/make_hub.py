"""Generates the hub world: client/godot/worlds/hub/world.json.

    python client/godot/tools/make_hub.py

The hub is the room everybody lands in, so it is the one world worth building
by hand -- except that "by hand" for 150 ring-arranged objects means trigonometry
in a text editor. This writes it instead.

Rules it keeps, because breaking either of them is a bug people feel:

* Everything you could walk into, stand on or trip over is `body: "static"`.
  Only light itself -- flames, lamp globes, glowing cores -- is `body: "none"`,
  because colliding with a flame is worse than walking through one. The check at
  the bottom enforces this, so a new decoration cannot quietly become a ghost.
* The lift lands on a solid dais whose top is exactly y=0, which is where the
  spawn is and where elevator.gd puts the bottom of its rails.
"""

import json
import math
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "worlds" / "hub"

# Heights. The dais the lift lands on is the zero of the world.
DAIS_TOP = 0.0
DAIS_RADIUS = 4.6
PLAZA_TOP = -1.2
PLAZA_RADIUS = 27.0
COLONNADE_RADIUS = 22.0
PILLARS = 16
PILLAR_HEIGHT = 6.0
DECK_RADIUS = 41.0

STONE = "#6b6a72"
STONE_DARK = "#4a4a54"
STONE_WARM = "#7a6f63"
METAL = "#595f6e"
EMBER = "#ff9a4a"
COOL = "#7fd4ff"

objects = []


def add(name, shape, size, pos, color=STONE, body="static", **extra):
    entry = {
        "name": name,
        "shape": shape,
        "size": [round(v, 4) for v in size],
        "pos": [round(v, 4) for v in pos],
        "color": color,
        "body": body,
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


def glow(name, shape, size, pos, color, energy=2.4):
    """A lamp globe or a flame: the one thing allowed to have no collision."""
    return add(
        name, shape, size, pos, color=color, body="none",
        emission=color, emission_energy=energy, roughness=0.3,
    )


def ring(count, radius, start=0.0):
    """Angles and positions evenly around a circle, as (index, angle, x, z)."""
    for i in range(count):
        angle = start + i / count * math.tau
        yield i, angle, math.cos(angle) * radius, math.sin(angle) * radius


def tangent_yaw(angle):
    """Yaw in degrees that points a box's long (x) axis along the tangent."""
    return -(90.0 + math.degrees(angle))


# --- the ground ------------------------------------------------------------

# One solid disc for the plaza and one for the dais. Thick, so the kill plane
# is the only way under the world.
add("Plaza", "cylinder", [PLAZA_RADIUS * 2, 1.6, PLAZA_RADIUS * 2],
    [0.0, PLAZA_TOP - 0.8, 0.0], color=STONE, roughness=0.75)
# Tucked below the surface rather than flush with it: level, its top face and
# the plaza's were the same plane and the edge of the world flickered.
add("PlazaRim", "torus", [PLAZA_RADIUS * 2 - 1.2, 0.5, PLAZA_RADIUS * 2 + 0.6],
    [0.0, PLAZA_TOP - 0.32, 0.0], color=STONE_DARK, roughness=0.6)

# Two steps from the plaza up to the dais. Not three: the third one's top
# landed on exactly DAIS_TOP, and two coplanar faces at the spot you stand on
# is a z-fight you watch flicker the whole time you are typing.
for step in range(2):
    top = PLAZA_TOP + (step + 1) * 0.4
    radius = DAIS_RADIUS + (2 - step) * 0.75
    add("Step%d" % step, "cylinder", [radius * 2, 0.4, radius * 2],
        [0.0, top - 0.2, 0.0], color=STONE_DARK, roughness=0.7)

add("Dais", "cylinder", [DAIS_RADIUS * 2, 1.2, DAIS_RADIUS * 2],
    [0.0, DAIS_TOP - 0.6, 0.0], color=STONE_WARM, roughness=0.5)
# The lit ring the lift comes down into: the first thing you look at.
glow("DaisRing", "torus", [DAIS_RADIUS * 2 - 0.9, 0.12, DAIS_RADIUS * 2 - 0.35],
     [0.0, DAIS_TOP - 0.03, 0.0], COOL, energy=2.2)
light("DaisLight", [0.0, 2.2, 0.0], COOL, 2.2, 13.0)

# --- the colonnade ---------------------------------------------------------

for i, angle, x, z in ring(PILLARS, COLONNADE_RADIUS):
    base = PLAZA_TOP
    add("PillarBase%d" % i, "box", [1.7, 0.4, 1.7], [x, base + 0.2, z],
        color=STONE_DARK, roughness=0.7)
    add("Pillar%d" % i, "cylinder", [1.15, PILLAR_HEIGHT, 1.15],
        [x, base + 0.4 + PILLAR_HEIGHT / 2, z], color=STONE, roughness=0.55)
    add("Capital%d" % i, "box", [1.6, 0.45, 1.6],
        [x, base + 0.4 + PILLAR_HEIGHT + 0.22, z], color=STONE_DARK, roughness=0.6)

# The architrave: one beam per gap, laid along the tangent so the ring closes.
chord = 2 * COLONNADE_RADIUS * math.sin(math.pi / PILLARS)
inset = COLONNADE_RADIUS * math.cos(math.pi / PILLARS)
for i in range(PILLARS):
    mid = (i + 0.5) / PILLARS * math.tau
    add("Beam%d" % i, "box", [chord + 0.2, 0.55, 1.1],
        [math.cos(mid) * inset, PLAZA_TOP + 0.4 + PILLAR_HEIGHT + 0.72, math.sin(mid) * inset],
        color=STONE, roughness=0.6, rot=[0.0, round(tangent_yaw(mid), 3), 0.0])

# A lit band above the architrave, which is what you see of the hub from the
# lift on the way down.
glow("Crown", "torus",
     [COLONNADE_RADIUS * 2 - 0.8, 0.3, COLONNADE_RADIUS * 2 + 0.5],
     [0.0, PLAZA_TOP + 0.4 + PILLAR_HEIGHT + 1.25, 0.0], COOL, energy=1.6)

# Every other pillar carries a lamp. Only four of them are real lights: the
# rest are emissive globes, which cost nothing.
for i, angle, x, z in ring(8, COLONNADE_RADIUS - 1.6):
    height = PLAZA_TOP + 3.1
    add("Bracket%d" % i, "box", [0.5, 0.14, 0.14], [x, height, z],
        color=METAL, metallic=0.7, roughness=0.4, rot=[0.0, round(-math.degrees(angle), 3), 0.0])
    glow("Lamp%d" % i, "sphere", [0.42, 0.42, 0.42], [x, height - 0.26, z], COOL, energy=3.0)
    if i % 2 == 0:
        light("LampLight%d" % i, [x, height - 0.26, z], COOL, 1.5, 9.0)

# --- braziers --------------------------------------------------------------

for i, angle, x, z in ring(6, 14.5, start=math.pi / 6):
    add("BrazierStem%d" % i, "cylinder", [0.5, 1.5, 0.5], [x, PLAZA_TOP + 0.75, z],
        color=METAL, metallic=0.75, roughness=0.35)
    add("BrazierBowl%d" % i, "cylinder", [1.5, 0.5, 1.0], [x, PLAZA_TOP + 1.7, z],
        color=METAL, metallic=0.8, roughness=0.3)
    glow("Fire%d" % i, "sphere", [1.15, 0.95, 1.15], [x, PLAZA_TOP + 2.1, z], EMBER, energy=3.4)
    light("FireLight%d" % i, [x, PLAZA_TOP + 2.5, z], EMBER, 2.6, 15.0)

# --- benches ---------------------------------------------------------------

for i, angle, x, z in ring(6, 9.5, start=math.pi / 6):
    yaw = round(tangent_yaw(angle), 3)
    add("Bench%d" % i, "box", [2.8, 0.22, 0.75], [x, PLAZA_TOP + 0.52, z],
        color=STONE_WARM, roughness=0.65, rot=[0.0, yaw, 0.0])
    add("BenchLeg%dA" % i, "box", [0.3, 0.5, 0.65],
        [x + math.cos(angle + math.pi / 2) * 1.1, PLAZA_TOP + 0.25, z + math.sin(angle + math.pi / 2) * 1.1],
        color=STONE_DARK, roughness=0.7, rot=[0.0, yaw, 0.0])
    add("BenchLeg%dB" % i, "box", [0.3, 0.5, 0.65],
        [x - math.cos(angle + math.pi / 2) * 1.1, PLAZA_TOP + 0.25, z - math.sin(angle + math.pi / 2) * 1.1],
        color=STONE_DARK, roughness=0.7, rot=[0.0, yaw, 0.0])

# --- causeways out to four decks -------------------------------------------

# The causeways overlap the plaza and the decks at both ends. Their top used
# to be at exactly PLAZA_TOP, and two coplanar overlapping faces flicker, so
# they ride a centimetre proud: a lip you cannot see and do not trip over.
LIP = 0.01
for i, angle, x, z in ring(4, (PLAZA_RADIUS + DECK_RADIUS) / 2, start=math.pi / 4):
    span = DECK_RADIUS - PLAZA_RADIUS + 3.0
    add("Causeway%d" % i, "box", [span, 0.5, 4.0], [x, PLAZA_TOP - 0.25 + LIP, z],
        color=STONE_DARK, roughness=0.7, rot=[0.0, round(-math.degrees(angle), 3), 0.0])
    add("Rail%dA" % i, "box", [span, 0.5, 0.2],
        [x + math.cos(angle + math.pi / 2) * 1.9, PLAZA_TOP + 0.3 + LIP, z + math.sin(angle + math.pi / 2) * 1.9],
        color=METAL, metallic=0.6, roughness=0.45, rot=[0.0, round(-math.degrees(angle), 3), 0.0])
    add("Rail%dB" % i, "box", [span, 0.5, 0.2],
        [x - math.cos(angle + math.pi / 2) * 1.9, PLAZA_TOP + 0.3 + LIP, z - math.sin(angle + math.pi / 2) * 1.9],
        color=METAL, metallic=0.6, roughness=0.45, rot=[0.0, round(-math.degrees(angle), 3), 0.0])

for i, angle, x, z in ring(4, DECK_RADIUS, start=math.pi / 4):
    add("Deck%d" % i, "cylinder", [15.0, 0.8, 15.0], [x, PLAZA_TOP - 0.4, z],
        color=STONE, roughness=0.7)
    add("DeckPost%d" % i, "cylinder", [0.35, 3.4, 0.35], [x, PLAZA_TOP + 1.7, z],
        color=METAL, metallic=0.7, roughness=0.4)
    glow("DeckLamp%d" % i, "sphere", [0.7, 0.7, 0.7], [x, PLAZA_TOP + 3.5, z], COOL, energy=3.2)
    light("DeckLight%d" % i, [x, PLAZA_TOP + 3.5, z], COOL, 2.0, 14.0)

# --- islands you can actually land on --------------------------------------

# These were decoration with no collision, which is exactly the kind of thing
# that reads as a bug: a rock you fall through. They are solid now.
ISLANDS = [
    (-54.0, 13.0, -31.0, 19.0),
    (49.0, 19.0, -45.0, 15.0),
    (13.0, 25.0, 63.0, 23.0),
    (-67.0, 8.0, 39.0, 13.0),
    (71.0, 30.0, 19.0, 17.0),
]
for i, (x, y, z, width) in enumerate(ISLANDS):
    add("Island%d" % i, "sphere", [width, width * 0.45, width], [x, y, z],
        color=STONE_DARK, roughness=0.8)
    add("IslandCap%d" % i, "cylinder", [width * 0.62, 0.4, width * 0.62],
        [x, y + width * 0.2, z], color=STONE, roughness=0.75)
    glow("IslandLamp%d" % i, "sphere", [0.6, 0.6, 0.6], [x, y + width * 0.3, z], EMBER, energy=2.6)

# --- things to pick up -----------------------------------------------------

# `reach` is how far away it can be taken from, `locked` refuses to be taken
# out of somebody else's hands. Both are read by main.gd.
add("Crate", "box", [0.6, 0.6, 0.6], [2.2, PLAZA_TOP + 0.9, 3.4],
    color="#8a6a42", roughness=0.8, body="grabbable", mass=3.0, reach=0.5)
add("Crate Two", "box", [0.5, 0.5, 0.5], [2.9, PLAZA_TOP + 0.85, 2.6],
    color="#7d5f3b", roughness=0.8, body="grabbable", mass=2.5, reach=0.5)
add("Ball", "sphere", [0.34, 0.34, 0.34], [-3.1, PLAZA_TOP + 0.8, 3.0],
    color="#d8554a", roughness=0.45, body="grabbable", mass=0.8, reach=2.5)
add("Cup", "cylinder", [0.2, 0.26, 0.2], [-2.4, PLAZA_TOP + 0.75, -3.3],
    color="#cfd3dc", roughness=0.35, metallic=0.5, body="grabbable", mass=0.4, reach=0.4)
# The two that stay where they are put: a shrine lamp and the gem on its plinth.
add("Plinth", "cylinder", [1.0, 1.1, 1.0], [0.0, PLAZA_TOP + 0.55, -8.5],
    color=STONE_WARM, roughness=0.6)
add("Gem", "sphere", [0.34, 0.42, 0.34], [0.0, PLAZA_TOP + 1.4, -8.5],
    color="#7ef0c8", roughness=0.1, emission="#7ef0c8", emission_energy=2.2,
    body="grabbable", mass=0.5, reach=0.9, locked=True)
add("Lantern", "cylinder", [0.3, 0.44, 0.3], [5.4, PLAZA_TOP + 0.78, -5.4],
    color="#ffcf8a", roughness=0.3, emission=EMBER, emission_energy=2.0,
    body="grabbable", mass=0.6, reach=0.6, locked=True)

# --- write it out ----------------------------------------------------------

world = {
    "format": 1,
    "kind": "world",
    "name": "hub",
    # On the dais, looking across the plaza at the gem on its plinth.
    "spawn": {"pos": [0.0, DAIS_TOP, 0.0], "yaw": 0.0},
    "sky": {
        "top": "#0b1026", "horizon": "#3d3350", "ground": "#07080f",
        "ambient": "#56608c", "fog": 0.006,
        "sun": {"color": "#9fb4ff", "energy": 0.28, "angles": [-38.0, 24.0]},
    },
    "kill_y": -40.0,
    "objects": objects,
}

# Two solid things whose top faces land on exactly the same height, and which
# overlap, will z-fight: the surface flickers as you walk. Catch it here rather
# than in somebody's eyes.
tops = {}
for e in objects:
    # Only the shapes whose size.y really is their height (a torus is as tall
    # as its tube is thick, which is not what size.y says), and only ones
    # square to the world: comparing turned footprints with a circle either
    # misses real overlaps or flags every beam that meets another end to end.
    # That leaves the big flat ground surfaces, which is where this bites.
    if e.get("body") != "static" or e.get("shape") not in ("box", "cylinder"):
        continue
    if any(abs(v) > 0.001 for v in e.get("rot", [0, 0, 0])):
        continue
    top = round(e["pos"][1] + e["size"][1] / 2, 4)
    tops.setdefault(top, []).append(e)
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

# The rule this file exists to keep: nothing solid-looking is a ghost.
ghosts = [o for o in objects if o.get("body") == "none" and not o.get("emission")]
assert not ghosts, "collision-less objects that are not lights: %s" % [o["name"] for o in ghosts]
assert sum(1 for o in objects if o.get("body") == "grabbable") >= 4
assert any(o.get("locked") for o in objects)
assert any(float(o.get("reach", 0)) > 0.35 for o in objects)
assert world["spawn"]["pos"][1] == DAIS_TOP, "the lift lands on the dais, not in it"

OUT.mkdir(parents=True, exist_ok=True)
path = OUT / "world.json"
path.write_text(json.dumps(world, indent="\t"), encoding="utf-8")

solid = sum(1 for o in objects if o.get("body") == "static")
lights = sum(1 for o in objects if "light" in o)
print("%s: %d objects (%d solid, %d lights, %d pickups), %d KiB" % (
    path.name, len(objects), solid, lights,
    sum(1 for o in objects if o.get("body") == "grabbable"),
    len(path.read_bytes()) // 1024,
))
