# Hearth

A small social VR space. Run a server -- a **Flame** -- everyone joins it, and
you see each other's head and hands move in real time. Build the places you
meet in with Hearth Studio, drop them on a Flame, and walk between them
through portals.

Anybody can host a Flame. One day there will also be a **Fireplace**: a single
server that knows who people are, so your name means the same thing on
everybody's Flame (`docs/TRUST.md`). Accounts, friends and following them are
the Fireplace's: `docs/FIREPLACE.md`.

Rust and Godot 4, QUIC over the wire.

## Host a Flame

On Windows, double-click **`start.bat`**. It builds the Flame the first time,
then opens this:

```
┌─ Hearth - Flame ─── listening on 0.0.0.0:7777 ─┐
│  Friends connect to  192.168.1.20:7777  [copy] │
│  2/16 players in 2 instances, 2 worlds, players │
│  may start their own: yes                       │
│  Port [ 7777 ]   Max players [ 16 ]             │
│  Data folder   [ data        ]                  │
│  Worlds folder [ data/worlds ]                  │
│  Hub world     [ hub         ]                  │
│  [x] Let players start their own instances      │
│  ──── Worlds ─────────────────────────────────  │
│  hub        1.4 kB   hub                        │
│  courtyard  8.4 MB         [ Start ]            │
│  Drop a .hworld file here to add it   [Rescan]  │
│  ──── Instances (2/16 players, 2 running) ────  │
│  0  hub        2 players                        │
│      1  Ada                            [ Kick ] │
│      2  Bob                            [ Kick ] │
│  3  courtyard  0 players               [ Stop ] │
│  ─────────────────────────────────────────────  │
│  [ type a command, or help                    ] │
│  Log  412 lines  [x] Follow   [Copy] [Clear]    │
│  19:21:04 INFO connection attempt addr=…        │
│  19:21:04 INFO player joined id=2 name=Bob      │
│  19:21:05 INFO sending a world id=2 slug=hub    │
│  19:21:05 INFO world sent id=2 sent=1397        │
│  > start courtyard                              │
│  started courtyard as instance 3                │
└─────────────────────────────────────────────────┘
```

Everything the Flame does is in that log, connection attempts and refusals
included, so "it will not let me in" has an answer on screen. The box above it
takes the same commands as the console Flame, below.

Press **Start**, then give your friends the address next to *Friends connect to*.
For them to reach you from outside your network, forward **UDP port 7777** to
your machine.

Everyone lands in the **hub**, the world that keeps running and cannot be
stopped. Drop a packed world (`.hworld`, see below) on the window to add it to
the Flame's repository, press **Start** next to it, and a portal to it appears
in the hub for everyone. Clients download the world they walk into, once, and
check it against the hash the Flame advertised. Tick *Let players start their
own instances* and they can open their own copy of any world in the list; those
close again when the last player leaves.

No window wanted? `start.bat --noGUI` runs the same Flame in the console,
reading `deploy/flame.toml`. On Linux and macOS, `bash start.sh` does the same
(and `bash start.sh --gui` opens the window):

```sh
bash start.sh
```

Putting one on the internet, as a service, on either OS:
[docs/RUNNING.md](docs/RUNNING.md). The box the official servers run on:
[docs/ORACLE.md](docs/ORACLE.md). Running your own Fireplace with email:
[docs/SELFHOST-MAIL.md](docs/SELFHOST-MAIL.md), and what GDPR asks of whoever
runs one: [docs/PRIVACY-OPS.md](docs/PRIVACY-OPS.md).

It is operated by typing at it, with the commands the window also takes
(`flame --help` prints them):

```
> worlds
hub                            1397 bytes
courtyard                   8812044 bytes
> start courtyard
started courtyard as instance 1
> players
instance 0 — hub (2 player(s))
    1  Ada
    2  Bob
instance 1 — courtyard (0 player(s))
```

| Command | What it does |
|---|---|
| `help` | the list |
| `status` | players, worlds and instances in one line |
| `worlds` | the worlds this Flame can hand out |
| `players` | who is connected, and where |
| `start <world>` | start an instance of a world |
| `stop <instance>` | stop it and send its players back to the hub |
| `restart <instance>` | stop it and start its world again |
| `kick <player>` | disconnect one player |
| `instancing on\|off` | whether players may open their own instances |
| `rescan` | look at the worlds folder again |
| `add <file.hworld>` | copy a packed world into the worlds folder |
| `quit` | stop the Flame |

Commands work from a pipe too (`echo "start courtyard" | flame`), and a
Flame with no console at all — a service, a container — simply has nobody
typing. `RUST_LOG=debug` turns up the detail, in the window as well.

## Join from the client

Build the client once:

```powershell
client/build.ps1 -Export          # -> dist/Hearth.exe and dist/HearthStudio.exe
```

Run it again after **any** change to `client/godot`: without `-Export` the
script builds the extension and runs the tests but leaves `dist/Hearth.exe`
exactly as it was, and a stale exe looks like a change that did not work.

The first run needs Godot's export templates; if they are missing the script
says so, and `-InstallTemplates` downloads them for you (about 1.2 GB, once).
You get `dist/Hearth.exe` with everything packed in, next to the world builder
`dist/HearthStudio.exe` and the extension they load, `hearth_client.dll` — zip
them up and anyone on Windows can play and build.

Run it, and the launcher asks one question before anything starts:

```
┌─ Hearth ───────────────────────────┐
│             Hearth                 │
│      How do you want to play?      │
│   [ In the headset                ]│
│   [ On this screen                ]│
└────────────────────────────────────┘
```

It is asked here because OpenXR has to be started, or deliberately left alone,
before the first frame is drawn (`docs/decisions.md` D34).

If a headset is on, the same question is on a panel inside it: stick to move,
any button to pick. Both stay live, so answer on whichever you are looking at,
and a headset that only shows up a few seconds after launch gets its copy
then. If a runtime starts but nothing is actually tracking, the screen takes
itself back after three seconds rather than leaving you staring at black.

Then you are in the lift, doors shut, with the Flame's address on the wall
beside you. It does not move until you have picked somewhere to go; the doors
open on the hub when it lands:

```
   ┌──────────────────────────────┐
   │  -- Hearth --                │
   │  Server  192.168.1.20:7777   │
   │  Name    Ada                 │
   │                              │
   │ ▸Join                        │
   │  Playing on: the headset     │
   │  Quit Hearth                 │
   └──────────────────────────────┘
```

**Point at a row and click it** -- with the mouse on the desktop, with a
controller in the headset -- or move the highlight with the stick or the arrow
keys and press Enter. While a panel is up the mouse is yours to point with, not
to look around with. Everything you see here is styled from one file --
`client/godot/ui.gd` -- so reskinning it is a change of colours
(`docs/UI_STYLE.md`).

Clicking a field asks you to type, and that looks different in each place.

On the desktop you get one line, big, in the middle of the view. You already
have a keyboard, so you just type on it:

```
        ┌────────────────────────────────┐
        │   Server  192.168.1.20:7777_   │
        │  type on your keyboard - Enter │
        └────────────────────────────────┘
```

In the headset you get keys, because there is no keyboard you can reach with it
on. Point at one and pull the trigger, or move the highlight with the stick.
Drag the bar across the top to put the whole thing somewhere else:

```
   ════════════════════════════════  <- grab here to move it
       Server  192.168.1.20:7777_
      1 2 3 4 5 6 7 8 9 0
      q w e r t y u i o p
      a s d f g h j k l :
      z x c v b n m . - _
      Shift Space Back Clear Cancel Done
```

Either way, Enter -- or a click away from everything -- puts it away and keeps
what you typed. The panel is in the world rather than
on a 2D layer because a 2D screen cannot be read with a headset on
(`docs/decisions.md` D35). A headset that only shows up later is picked up
within a second, and **Playing on:** switches either way without restarting.

Walking is off until you have joined, and the doors stay shut until then, so
there is no way to strand yourself outside the car.

|             | VR                        | Desktop                          |
|-------------|---------------------------|----------------------------------|
| Move        | left stick                | WASD                             |
| Turn, look  | right stick               | mouse (click to capture, Esc frees) |
| Jump        | **A**                     | **Space**                        |
| Mute mic    | **X**                     | **V**                            |
| Pick up     | grip or trigger, near it  | hold **left mouse** on the crosshair, or **E** |
| Point       | controller, with a beam   | the mouse                        |
| Join        | trigger, on the panel     | click the panel, or Enter        |
| Change world| walk into a portal        | walk into a portal               |
| Menu        | tap **Y** / **B**         | tap **Y**                        |
| Quick menu  | hold **Y** / **B**        | hold **Y**                       |
| In the menu | point and pull the trigger| click, or arrows, Enter, Esc     |
| Chat        | quick menu, or the Chat panel | **Enter** or **T**           |
| Camera      | quick menu; trigger in that hand shoots | **C**; click shoots |

You arrive by lift: a car comes down out of the sky on three glowing rails and
opens where the world spawns you. Step off and it drops away behind you.

Rings stand in front of where you spawn, one per world running on the Flame,
with its name and how many people are in it. Walk into one and you are there;
inside a world, one ring takes you back to the hub. The menu lists the same
worlds for when you would rather not walk. Either side of it hang your
friends (who is online, where, and a Join button) and the chat for the ember
you are in; its Camera tab shows the last picture you took, and Settings has
the mic, music and sound.

Hold the menu button for the quick menu: camera, chat, mic, menu. The camera is
a real one you hold, with what its lens sees on the screen at its back.
Pictures are saved at 1920x1080 to **Pictures/Hearth**. Chat goes to everyone
in the same ember, pops up over the speaker's head, and shows at the bottom of
your view while the menu is shut.

A mic icon in front of you turns red and crossed out while you are muted — the
only thing on the HUD. Fall off the edge of a world and you are put back where
you spawned.

Things you can pick up are shared: the Flame decides who is holding what, so
two people reaching for the same crate cannot both end up with it, and you can
watch somebody else carry one around. A world can say how far away one of its
objects can be grabbed from, and mark one as not takeable out of your hands.

Music and sound effects are generated by `client/godot/tools/make_audio.py`
into `client/godot/audio`. Drop an `.ogg` or `.wav` of the same name over any
of them to use your own; a missing file is simply silent.

Any OpenXR runtime works — SteamVR, Oculus, Virtual Desktop, Monado. If SteamVR
is already running, Hearth joins it instead of waking a different runtime and
leaving SteamVR to shut down; otherwise it takes whatever Windows has set as
*the* OpenXR runtime. To pin one yourself, set `XR_RUNTIME_JSON` to its JSON
manifest before starting the game.

**On this screen** keeps you in the window: click to capture the mouse, look
with the mouse, move with WASD, Escape to release. Two desktop clients on one
machine is the easy way to test a Flame.

On the desktop there is a crosshair, and it tells you what you are about to
pick up: white over nothing, blue over something you can take, red over
something somebody else is holding that is `locked`, green while you are
carrying it. Hold the left mouse button to pick up whatever it is on, let go to
drop. An object can ask to be grabbable from further away with `reach`
(`docs/WORLD_FORMAT.md`).

The address takes `host` or `host:port` (port 7777 by default), and your last
name and address come back next time. To skip the launcher:

```sh
Hearth.exe -- --server 192.168.1.20 --name Ada --mode desktop
```

To work on the client instead of shipping it, run `client/build.ps1` (debug
extension, no export), open `client/godot` in Godot 4.7 and press F5. On Linux
and macOS, `cargo build --release -p hearth-client` and copy
`target/release/libhearth_client.so` (or `.dylib`) into `client/godot/bin/`.

## Build a world

Double-click **`HearthStudio.exe`**, next to `Hearth.exe`:

```
┌─ Hearth Studio ─────────────────────────────────────────────┐
│ [ Worlds ] [ Avatars ]                                      │
├──────────────┬──────────────────────────────┬───────────────┤
│ Your worlds  │                              │ Crate         │
│  hub         │     right drag looks         │  Pos  x  y  z │
│  my-place  ◀ │     WASD/QE flies            │  Rot  x  y  z │
│ [New][Copy]  │     drag moves, Alt lifts    │  Size x  y  z │
│              │                              │  Colour  ▣    │
│ Box          │                              │  Body  grab ▼ │
│ Sphere       │                              │  Mass  4.0    │
│ Cylinder     │                              │ [Copy][Delete]│
│ Plane        │                              │               │
│ Torus        │                              │ In this world │
│ Light        │                              │  Floor  (box) │
│ [Import...]  │                              │  Crate  (box) │
├──────────────┴──────────────────────────────┴───────────────┤
│ [Save] [Pack .hworld] [Test] [Spawn here]   Saved my-place  │
└─────────────────────────────────────────────────────────────┘
```

Click a piece on the left, then click the ground to place it. Click a thing to
select it, drag it along the floor, hold **Alt** to lift it, and type exact
numbers on the right. **Body** decides whether people walk through it, stand on
it, or pick it up and throw it.

**Import a model...** takes `.glb`, `.gltf`, `.obj` and `.fbx`, and stores it in
your world as `.glb`. FBX needs
[FBX2glTF](https://github.com/godot-rust/FBX2glTF/releases) next to the exe or
in the folder the studio names when you try — the same converter Godot's own
importer uses.

**Test** drops you into the world with the real player rig, on the desktop;
**F10** brings you back. **Pack .hworld** writes one file and opens the folder
it is in: that file is what you hand to a Flame. Your worlds live in
Godot's user folder (`%APPDATA%/Godot/app_userdata/Hearth/worlds`), and the hub
world ships with the studio, so there is something to take apart on the first
run.

The **Avatars** tab is greyed out. The format already tells worlds and avatars
apart, so that is where they will go.

The format is a folder with a `world.json` and a few `.glb` files, zipped:
[docs/WORLD_FORMAT.md](docs/WORLD_FORMAT.md). Nothing stops you writing one
by hand or generating it.

## What you need

- Rust 1.95+ ([rustup](https://rustup.rs))
- To build the client: [CMake](https://cmake.org) and a Visual Studio C++
  toolchain, which libopus needs. Players running the exe need neither.
- [Godot 4.7](https://godotengine.org) to build or edit the client (players only
  need the exported exe)
- For VR: a headset with any OpenXR runtime
- Windows for the exported client exe; both servers and `start.sh` run on
  Linux and macOS too ([docs/RUNNING.md](docs/RUNNING.md))

## How it works

```
client/       the client, and Hearth Studio
server/       a Flame: one game server, hosted by anybody
main-server/  the Fireplace: the one server that knows who people are
shared/       what all three need: the wire protocol
docs/         every spec and every decision
deploy/       example configuration, and systemd units
```

| Piece | Where |
|---|---|
| A Flame: game server, worlds and instances | `server/hearth-flame` |
| The window a Flame runs in | `server/hearth-flame-gui` |
| Wire protocol, used by all three | `shared/hearth-protocol` |
| The Fireplace: accounts, friends, presence | `main-server/hearth-fireplace` |
| Godot extension (`HearthClient` node) | `client/hearth-client` |
| The VR scene and its scripts | `client/godot` |
| World format, used by the game and the studio | `client/godot/world_format.gd` |
| Hearth Studio | `client/godot/studio.gd` |

A pose is head + both hands quantized into 33 bytes, sent as an unreliable QUIC
datagram 20 times a second; joins and leaves go over a reliable stream. The
Flame validates and rate-limits everything it forwards. The byte-level spec is
[docs/PROTOCOL.md](docs/PROTOCOL.md), and the reasoning behind the design
is in [docs/decisions.md](docs/decisions.md).

```sh
cargo test --workspace   # protocol, quantization, instances, downloads, real clients
client/build.ps1 -Test   # the world format, including packages built to break it
```

## Heads up

The client **does not verify the Flame's certificate**, so somebody on the path
could read or change the traffic — including the world you download, which your
client then feeds to a glTF parser. The format's limits bound what that can do;
they do not make it safe. Keep it to a LAN, a VPN or a tunnel you trust until
certificate pinning lands (decisions D3, D27).

Not there yet:

- **Shared world objects.** Crates and gems move for you alone, in any world.
- **A jitter buffer.** Voice frames play as they arrive, so a bumpy connection
  crackles.
- **Avatars.** Everyone is a head and two hands, and there are no legs.

## License

MIT, see [LICENSE](LICENSE).
