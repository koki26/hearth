# Hearth

A minimal social VR space: a QUIC relay server and a Godot 4 VR client, both in
Rust over one shared wire protocol.

Two or more people put on a headset, join the same server and see each other's
head and hands move in real time.

- **Server** — `server/hearth-server`, a QUIC relay for one room (16 players by
  default). It validates every message and rate-limits each client.
- **Client** — `client/hearth-client`, a Godot 4 GDExtension exposing a
  `HearthClient` node, plus the Godot project in `client/godot`.
- **Protocol** — `server/hearth-protocol`, used by both sides. Byte-level spec in
  [protocol/PROTOCOL.md](protocol/PROTOCOL.md).

Poses are 33 bytes (head + both hands, quantized) sent as unreliable datagrams at
20 Hz; joins and leaves go over a reliable QUIC stream.

## Requirements

- Rust 1.94+ (`rustup`)
- Godot 4.7 (for the client)
- A headset with an OpenXR runtime (SteamVR, Oculus, Virtual Desktop, Monado…).
  Without one, the client runs in a flat-screen fallback mode.

## Run the server

```sh
cargo run -p hearth-server -- deploy/hearth.toml
```

It listens on UDP 7777 (IPv4). Settings are in `deploy/hearth.toml`; all keys are
optional. `data_dir` holds the TLS certificate — keep it across restarts.

## Run the client

Build the extension and copy it into the Godot project:

```powershell
# Windows
client/build.ps1            # add -Release for an optimized build
```

```sh
# Linux / macOS
cargo build --release -p hearth-client
cp target/release/libhearth_client.so client/godot/bin/      # .dylib on macOS
```

Then open `client/godot` in Godot and press F5, or run it directly:

```sh
godot --path client/godot -- --server 192.168.1.20 --port 7777 --name Ada
```

Arguments after `--` are optional (defaults: `127.0.0.1`, `7777`, a random name).
The same flags work in the editor under *Project → Project Settings → Run → Main
Run Args*.

Controls: VR headsets track head and hands with no setup. Without a headset, click
to capture the mouse, look with the mouse, move with WASD, release with Escape.

## Security

The client **accepts any server certificate**, so a man-in-the-middle can read and
alter traffic. Use it on a LAN or over a VPN/tunnel you trust, not on the open
internet, until trust-on-first-use pinning lands (see
[docs/decisions.md](docs/decisions.md) D3).

## Not implemented yet

Voice (the protocol carries it, neither side captures or plays audio), more than
one room, avatars beyond head and hands, and certificate pinning.

## Tests

```sh
cargo test --workspace
```

Covers the wire format (including a fuzz-ish garbage pass), quantization error
bounds, and both the hand-rolled and the real client against an in-process server.

## License

MIT, see [LICENSE](LICENSE).
