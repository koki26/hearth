# Hearth

A small social VR space. Run a server, everyone joins it, and you see each
other's head and hands move in real time.

Rust and Godot 4, QUIC over the wire, under 1500 lines all in.

## Host a server

On Windows, double-click **`start.bat`**. It builds the server the first time,
then opens this:

```
┌─ Hearth Server ──────────────────┐
│  Listening on 0.0.0.0:7777       │
│  Friends connect to              │
│      192.168.1.20:7777  [copy]   │
│                                  │
│  Port          [ 7777 ]          │
│  Max players   [   16 ]          │
│  Data folder   [ data  ]         │
│                                  │
│  [ Start ]  [ Save config ]      │
│  ────────────────────────────    │
│  Players (2/16)                  │
│  1  Ada                          │
│  2  Bob                          │
└──────────────────────────────────┘
```

Press **Start**, then give your friends the address next to *Friends connect to*.
For them to reach you from outside your network, forward **UDP port 7777** to
your machine.

No window wanted? `start.bat --noGUI` runs the same server in the console,
reading `deploy/hearth.toml`. On Linux and macOS:

```sh
cargo run --release -p hearth-server -- deploy/hearth.toml
```

## Join from the client

Build the client once:

```powershell
client/build.ps1 -Export          # -> dist/Hearth.exe
```

The first run needs Godot's export templates; if they are missing the script
says so, and `-InstallTemplates` downloads them for you (about 1.2 GB, once).
You get `dist/Hearth.exe` with everything packed in, next to the extension it
loads, `hearth_client.dll` — zip the two up and anyone on Windows can play.

Run it, and it asks how you want to play:

```
┌─ Hearth ───────────────────────────┐
│  Your name                         │
│  [ Ada                         ]   │
│  Server address                    │
│  [ 192.168.1.20:7777           ]   │
│  [ Play in VR ] [ Play on desktop ]│
└────────────────────────────────────┘
```

The headset starts showing the room as soon as a runtime answers, even before
you join, so you can keep it on: **pull either trigger** (grip and the face
buttons work too) to join in VR with the name and address on the screen. **Play
in VR** does the same from the monitor, and says so when no runtime answers. A
headset that only shows up later is picked up within a second.

|             | VR                        | Desktop                          |
|-------------|---------------------------|----------------------------------|
| Move        | left stick                | WASD                             |
| Turn, look  | right stick               | mouse (click to grab, Esc frees) |
| Jump        | **A**                     | **Space**                        |
| Mute mic    | **X**                     | **V**                            |
| Pick up     | grip or trigger, near it  | **E**, standing close            |
| Join        | either trigger            | the buttons on screen            |

A mic icon in front of you turns red and crossed out while you are muted — the
only thing on the HUD.

Any OpenXR runtime works — SteamVR, Oculus, Virtual Desktop, Monado. If SteamVR
is already running, Hearth joins it instead of waking a different runtime and
leaving SteamVR to shut down; otherwise it takes whatever Windows has set as
*the* OpenXR runtime. To pin one yourself, set `XR_RUNTIME_JSON` to its JSON
manifest before starting the game.

**Play on desktop** keeps you in the window: click to capture the mouse, look
with the mouse, move with WASD, Escape to release. Two desktop clients on one
machine is the easy way to test a server.

The address takes `host` or `host:port` (port 7777 by default), and your last
name and address come back next time. To skip the screen:

```sh
Hearth.exe -- --server 192.168.1.20 --name Ada --mode desktop
```

To work on the client instead of shipping it, run `client/build.ps1` (debug
extension, no export), open `client/godot` in Godot 4.7 and press F5. On Linux
and macOS, `cargo build --release -p hearth-client` and copy
`target/release/libhearth_client.so` (or `.dylib`) into `client/godot/bin/`.

## What you need

- Rust 1.95+ ([rustup](https://rustup.rs))
- To build the client: [CMake](https://cmake.org) and a Visual Studio C++
  toolchain, which libopus needs. Players running the exe need neither.
- [Godot 4.7](https://godotengine.org) to build or edit the client (players only
  need the exported exe)
- For VR: a headset with any OpenXR runtime
- Windows for `start.bat` and the exe; everything else is cross-platform

## How it works

| Piece | Where |
|---|---|
| Relay server, one room | `server/hearth-server` |
| Server window | `server/hearth-server-gui` |
| Wire protocol, used by both sides | `server/hearth-protocol` |
| Godot extension (`HearthClient` node) | `client/hearth-client` |
| The VR scene and its scripts | `client/godot` |

A pose is head + both hands quantized into 33 bytes, sent as an unreliable QUIC
datagram 20 times a second; joins and leaves go over a reliable stream. The
server validates and rate-limits everything it forwards. The byte-level spec is
[protocol/PROTOCOL.md](protocol/PROTOCOL.md), and the reasoning behind the design
is in [docs/decisions.md](docs/decisions.md).

```sh
cargo test --workspace   # protocol, quantization, and real clients against a real server
```

## Heads up

The client **does not verify the server's certificate**, so somebody on the path
could read or change the traffic. Keep it to a LAN, a VPN or a tunnel you trust
until certificate pinning lands (decisions D3).

Not there yet:

- **Shared world objects.** Crates and gems are local to each player.
- **A jitter buffer.** Voice frames play as they arrive, so a bumpy connection
  crackles.
- More than one room, and legs.

## License

MIT, see [LICENSE](LICENSE).
