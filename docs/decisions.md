# Design decisions

Newest at the bottom. Each entry: what was decided, why, and what would make us revisit it.

## D1 — Client networking and voice in a Rust GDExtension (`gdext`), not C++

Godot 4 has no QUIC and no Opus encoder, so native code is needed either way. A
Rust extension can use `quinn`, the `opus` crate, and the **same**
`hearth-protocol` crate as the server, so client and server can't disagree about
the wire format. GDScript still handles all gameplay. It talks to a small
`HearthClient` node. We rejected C++ with msquic/ngtcp2: it would mean a second
QUIC stack and a second protocol implementation to keep in sync. We also rejected
ENet/raw UDP: we'd lose TLS, reliable streams and congestion control.

## D2 — Hand-written little-endian binary encoding

There's no serde/bincode/protobuf. The format is small, fixed and fully specified in
`/protocol/PROTOCOL.md`, so another client (C++, GDScript, anything) can implement it
from the doc alone. The protocol crate has zero dependencies. Revisit if the message
count grows past what's comfortable to hand-maintain.

## D3 — Milestone 1 accepts any server certificate; TOFU comes next

The server generates a self-signed certificate and the client skips verification.
This is **insecure against man-in-the-middle attacks**. It's acceptable only for LAN/testing.
**The first task after milestone 1, before any server is exposed to the internet:**
trust-on-first-use. The client pins the certificate fingerprint per `host:port` on the
first connection and refuses a changed fingerprint unless the user confirms.

## D4 — Quantization: positions at 1/512 m in i16, rotations as smallest-three in 32 bits

Positions give ±64 m at about 2 mm precision, which is enough for one room-sized region.
Bigger worlds will need several region origins. Rotations take 2 index bits plus
3 × 10 bits, with under 0.5° worst-case error (checked by a test). With these, a full
pose (head + both hands) is 33 bytes.

## D5 — Fixed-size pose: always send head + both hands

Hands that aren't tracked are flagged in `flags` instead of left out. A fixed 33 bytes
makes validation a simple length check, and leaving hands out would save at most 20
bytes. Optional hips/feet will be added with a new message type or protocol version.

## D6 — No delta compression or interest management in milestone 1

A pose is 33 bytes against roughly 30–40 bytes of QUIC/UDP/IP overhead per datagram,
so deltas would save very little, and with 2 players nobody is out of range. Both
principles still apply. We add them when rooms grow, starting with distance-based
send rates in the server's relay loop.

## D7 — Welcome carries no peer list

The server sends `Welcome`, then one `PeerJoined` per existing player. That keeps
every control message small with a fixed upper bound (≤ 256 bytes), whatever the
player count, and clients need only one code path for "a player appeared".

## D8 — Godot `*.import` files are committed

`.gitignore` excludes `.godot/` (the import cache) but not per-asset `*.import`
files. Those hold import settings, and Godot's docs say to version them.

## D9 — Server: one room, a shared player map, rate limits per client

Milestone 1 has a single room. The player map sits behind a `std::sync::Mutex`
that is never held across an `.await`. Join/leave broadcasts happen while the lock
is held, so every client sees membership changes in the same order. Each datagram
is decoded (that's the validation), then encoded once and sent to every other
player. Pose and voice have token-bucket limits, so a client can't make the server
amplify a flood to everyone. Revisit for multiple rooms, or if lock contention
shows up in profiles.

## D10 — TLS certificate is kept in `data_dir`

It's generated on first run and reused afterwards. This milestone doesn't need it, but
TOFU (D3) does: a pinned fingerprint only works if the certificate stays the same
across restarts. On Unix the key file is written with mode 0600. The Docker
deployment must mount `data_dir` as a volume.

## D11 — Crypto backend: `ring`, not `aws-lc-rs`

quinn/rustls default to aws-lc-rs, which needs CMake and NASM to build on Windows.
ring builds with just a C compiler on both platforms, which keeps the static binary
and the GDExtension simple to build.

## D12 — Client: one background thread owns the connection, Godot gets signals

`hearth-client` keeps all networking in `net.rs`, which knows nothing about Godot:
`Client::connect` spawns a thread with a current-thread tokio runtime, and the
Godot node polls an `std::sync::mpsc` channel in `_process` and re-emits each
event as a Godot signal. Poses go the other way through an unbounded channel.
That keeps the engine thread free of `.await`, lets the networking be tested
without Godot (`client/hearth-client/tests/relay.rs`), and means dropping the
node closes the connection. The node also throttles `send_pose` to the server's
`tick_hz`, so a 90 Hz headset doesn't trip the server's rate limit.

## D13 — The client tries every address a host resolves to

`localhost` resolves to `::1` first on most systems, while the server binds
`0.0.0.0`. Rather than bind dual-stack on the server (which needs per-OS socket
options), the client walks the resolved addresses in order with a 5 s timeout each
and keeps the first connection that succeeds.

## D14 — Voice is deferred past the first playable version

The protocol carries Opus frames and the server already relays them, but neither
side captures or plays audio: that needs an Opus encoder in the extension (a C
dependency on every platform) plus jitter buffering. Head and hand presence is
what makes the space feel inhabited, so it ships first. The client ignores
`VoiceRelay` datagrams for now.

## D15 — The server GUI is its own crate, the client's connect screen is Godot UI

`hearth-server-gui` (eframe/egui) is separate from `hearth-server` so the plain
server keeps building on a headless Linux box or in Docker, where winit and glow
have no window system to link against. It talks to the server through one
`tokio::sync::watch` channel carrying the roster, published while the room lock
is held, so the player list can never show a stale membership order. `start.bat`
launches the GUI, or the console server with `--noGUI`.

On the client, the connect screen is a plain Godot `CanvasLayer`: it runs before
XR is switched on, so you type the address on the monitor and put the headset on
afterwards. A 3D panel plus a VR keyboard would be the alternative, and that is
a lot of machinery for one field you fill in once (the last address and name are
remembered in `user://settings.cfg`).

## D16 — The client asks VR or desktop at launch instead of guessing

It used to enter VR whenever an OpenXR runtime answered, which is wrong often
enough to matter: people leave SteamVR running, test two clients on one machine,
or want to show the room on a monitor. So the connect screen has two join
buttons, and the VR one is disabled when no runtime is found. `--mode desktop`
does the same from the command line. OpenXR still initializes at startup
(Godot only registers the interface when the project setting is on), so a
headset runtime may wake up even for a desktop player.

The client ships as a single `dist/Hearth.exe` with the pack embedded, built by
`client/build.ps1 -Export`, so sharing it is one file plus an address.
