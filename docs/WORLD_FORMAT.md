# Hearth world format — version 1

A world is a folder while you build it and one `.hworld` file when it ships.
The reference implementation is `client/godot/world_format.gd`, which both the
game and Hearth Studio use. This document is the spec for anyone writing
another tool, and it must stay in sync with that file.

```
<slug>/
  world.json
  assets/<name>.glb        textures embedded in the glb
```

Zipped up, that folder is `<slug>.hworld`: a plain zip with `world.json` at the
root. The **slug** is the folder and file name, and it is what a server calls
the world on the wire, so it is limited to `a-z`, `0-9`, `-` and `_`, 1–32
characters (`docs/PROTOCOL.md`).

## world.json

```json
{
  "format": 1,
  "kind": "world",
  "name": "The Hearth",
  "spawn": { "pos": [0, 0, 3.5], "yaw": 180 },
  "kill_y": -45,
  "sky": {
    "top": "#171c38", "horizon": "#806166", "ground": "#0d0d14",
    "ambient": "#667099", "fog": 0.012,
    "sun": { "color": "#adc2ff", "energy": 0.35, "angles": [-35, 25] }
  },
  "objects": [
    { "name": "Floor", "shape": "box", "size": [16, 0.5, 16], "pos": [0, -0.25, 0],
      "color": "#424248", "roughness": 0.55, "body": "static" },
    { "name": "Crate", "shape": "box", "size": [0.45, 0.45, 0.45], "pos": [2.6, 0.25, -2.4],
      "color": "#a67342", "body": "grabbable", "mass": 4, "reach": 0.5 },
    { "name": "Lantern", "shape": "cylinder", "size": [0.3, 0.42, 0.3], "pos": [0, 0.9, -5],
      "color": "#c9a227", "body": "grabbable", "mass": 1.5, "reach": 0.6, "locked": true },
    { "name": "Tree", "model": "assets/tree.glb", "pos": [4, 0, -3], "body": "static" },
    { "name": "Torchlight", "light": { "color": "#ffb366", "energy": 1.6, "range": 7 },
      "pos": [4.6, 3.3, -4.6] }
  ]
}
```

| Key | Meaning |
|---|---|
| `format` | 1. A loader refuses a number it does not know. |
| `kind` | `"world"`. `"avatar"` is reserved and not implemented. |
| `name` | What people see. Free text. |
| `spawn` | `pos` in metres and `yaw` in degrees: where players appear. |
| `sky` | Optional. Hex colours, `fog` density 0–1, and a `sun` with `angles` as `[pitch, yaw]` degrees. Anything missing falls back to the dusk sky the game ships with. |
| `objects` | Up to 5000 entries, drawn in order. |

### An object

Every object may carry `name`, `pos` (metres), `rot` (degrees) and `scale`, and
must be **exactly one** of these three:

- **`shape`** — `box`, `sphere`, `cylinder`, `plane` or `torus`, with `size` as
  `[x, y, z]` metres. A cylinder reads `size` as `[top diameter, height,
  bottom diameter]`, a torus as `[inner diameter, _, outer diameter]`. Takes
  `color`, `roughness`, `metallic`, `emission` and `emission_energy`.
- **`model`** — a path that must match `assets/<name>.glb`. Nothing else: a
  world never carries `.obj` or `.fbx`, because the studio converts those at
  import (docs/decisions.md D26), so a world from a stranger's server never
  reaches those parsers.
- **`light`** — `{ color, energy, range, shadow }`, an omni light.

`body` says what the physics does with it:

| `body` | What you get |
|---|---|
| `none` | Visual only. |
| `static` (default) | Immovable, with collision from the mesh. You can stand on it. |
| `grabbable` | A rigid body you can pick up and throw, with `mass` in kg. |

A `grabbable` also takes:

| Key | Meaning |
|---|---|
| `reach` | How far a hand may be from it to take it, in metres. Default `0.35`, capped at 8. It is a floor, never a limit: an object asking for less than the hand already has changes nothing, so `reach` is for things you want to grab from across the room. |
| `locked` | `true` means it cannot be taken out of somebody else's hands — wait until they put it down. Default `false`. |

Collision is the cheapest shape that matches: a box, a sphere and an
equal-radius cylinder get the matching primitive, anything else convex gets a
hull, and only a torus, a plane and a glTF model get real triangles. Building a
triangle mesh for every box cost thirty seconds of stalled frames on the first
load of a world (`docs/decisions.md` D33), so prefer a primitive shape over a
model where one will do.

`locked` is enforced by the client doing the grabbing, because the server never
reads a world and so cannot know an item is locked (`docs/PROTOCOL.md`,
Items). A patched client can still take a locked item.

`kill_y` on the world is the height you cannot fall below: go past it and you
are put back on the spawn. It defaults to 24 m under the spawn, so a world
never has to think about it; name it explicitly when your world has a long
drop you want people to survive, or a short one you do not.

Colours are `#rrggbb`. Unknown keys are ignored, and a single unusable object
is skipped with a warning rather than failing the world, so a world written by
a newer studio still opens in an older client.

## Limits

A world usually arrives from a server nobody authenticated, so these are
enforced on load, not merely recommended:

| Limit | Value |
|---|---|
| `world.json` | 8 MB |
| objects | 5000 |
| files in a package | 2000 |
| unpacked size | 256 MB |
| one model | 200 MB |
| positions | ±4096 m, scale ≤ 256 |
| package contents | only `world.json` and `assets/*.glb` |

A package entry whose name climbs out of the folder (`..`), is absolute, or is
of any other type makes the whole package refused. `client/godot/tests/world_format_test.gd`
covers each of those cases.

## Making one

Hearth Studio (`dist/HearthStudio.exe`) writes this format: build, **Pack**,
and drop the `.hworld` on a server window. Nothing stops you writing the JSON
by hand or generating it — the hub world that ships with Hearth was generated
from a script, and the studio opens it like any other world.
