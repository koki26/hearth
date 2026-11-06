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

## D17 — Render to the headset before joining, and let a trigger join

`use_xr` used to be switched on only after the join succeeded, so the headset
stayed black while the desktop window already showed the head tracking: the room
was being rendered to the window, not to the HMD. It is now switched on as soon
as a runtime answers — retried once a second while the connect screen is open,
because a runtime often starts after the game does — and the headset shows the
room with a prompt in front of you. Pulling either trigger joins with whatever
the screen holds, so a mode can be picked with the headset on. Typing a new name
or address still needs the monitor; the saved ones cover every later launch.

## D18 — Any OpenXR runtime, with an escape hatch for SteamVR

The client never names a runtime: it asks for OpenXR and Windows hands over
whichever runtime is registered as active, so SteamVR, Oculus, Virtual Desktop
and Monado all work with no code per vendor (verified on SteamVR/OpenXR 2.18.4
and VirtualDesktopXR 1.1). The catch is that runtimes fight over that slot —
Virtual Desktop claims it while its streamer runs — so `play.bat --steamvr` sets
`XR_RUNTIME_JSON` from Steam's install path before launching. It has to be the
launcher: the OpenXR loader reads that variable when the instance is created,
which happens before any of our code runs.

The join button is not one fixed binding either. The default OpenXR action map
binds controllers differently per vendor, so trigger, grip and the face buttons
all count.

## D19 — The launcher keeps a running SteamVR, and locomotion is stick-based

Creating an OpenXR instance on a different runtime makes an already-running
SteamVR drop its session and shut down, which looks like Hearth closing SteamVR.
`play.bat` therefore checks for `vrmonitor.exe` and points `XR_RUNTIME_JSON` at
Steam's runtime when it finds it. The check has to live in the launcher, because
the loader reads that variable while the engine starts. Starting the exe
directly keeps the plain system behaviour.

Movement is the conventional pair: left stick walks relative to where you look,
right stick turns smoothly about the head rather than about the play-space
origin, so the view does not swing. Both read `primary` and fall back to
`secondary`, which is where the default action map puts a trackpad. Teleport and
snap turning are the comfort options to add next if smooth turning makes anyone
sick.

## D20 — The runtime choice lives in the extension, not in a launcher

D19 put the "keep a running SteamVR" check in `play.bat`, which only helps the
people who use the launcher. The GDExtension's `on_stage_init(Core)` runs before
the engine brings up OpenXR (verified: the engine then reports SteamVR/OpenXR
instead of VirtualDesktopXR), so the check moved into the exe and `play.bat` is
gone. `XR_RUNTIME_JSON` set by hand is still honoured and skips the check.

The SteamVR probe shells out to `tasklist` and `reg query` with
`CREATE_NO_WINDOW` instead of taking a process-listing or registry crate: two
short commands at startup, no dependency, and the whole file is behind
`#[cfg(windows)]`.

## D21 — A home world of local props, and a jump that rides a raycast

The scene is a real place now: a courtyard around a fire, crates and gems, in
`world.tscn` so `main.tscn` stays the networking rig. Crates are plain
`RigidBody3D` and the hands are frozen kinematic bodies, which is enough to
shove them without any grab code. None of it is networked: poses and voice are
all the protocol carries, and syncing objects needs an owner per object, a new
message type and a tie-break for two players grabbing the same crate. Keeping
them local is honest and costs nothing to replace later.

Jumping moves the `XROrigin3D` with a velocity and a downward `RayCast3D` under
the head, rather than wrapping the origin in a `CharacterBody3D`. The ray tells
us the height of whatever is beneath (floor or crate), which is all a jump and a
landing need; a character body would also want a capsule, slide resolution and a
decision about what happens when someone walks their real body into a wall.
Hands sit on collision layer 2 so they never block that ray.

The mic toggle is state and HUD only, because there is no mic yet (D14). It
ships anyway: the HUD is where it has to be either way, and the state is what
voice will read when it lands.
