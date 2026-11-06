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

1. Build the extension: `client/build.ps1 -Release`
   (Linux/macOS: `cargo build --release -p hearth-client` then copy
   `target/release/libhearth_client.so` into `client/godot/bin/`)
2. Open `client/godot` in Godot 4.7 and press **F5**.
3. Type your name and the server address, click **Connect**, then put the
   headset on.

```
┌─ Hearth ─────────────────┐
│  Your name               │
│  [ Ada               ]   │
│  Server address          │
│  [ 192.168.1.20:7777 ]   │
│  [      Connect      ]   │
└──────────────────────────┘
```

The address can be `host` or `host:port`; the port defaults to 7777, and your
last name and address are remembered. To skip the screen entirely:

```sh
godot --path client/godot -- --server 192.168.1.20 --name Ada
```

**No headset?** The client falls back to a flat window: click to capture the
mouse, look with the mouse, move with WASD, Escape to release. Handy for testing
two players on one machine.

## What you need

- Rust 1.95+ ([rustup](https://rustup.rs))
- [Godot 4.7](https://godotengine.org) for the client
- A headset with any OpenXR runtime (SteamVR, Oculus, Virtual Desktop, Monado…)

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

Not there yet: voice (the protocol carries it, nothing captures audio), more than
one room, and avatars with more than a head and two hands.

## License

MIT, see [LICENSE](LICENSE).
