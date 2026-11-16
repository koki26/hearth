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
`/docs/PROTOCOL.md`, so another client (C++, GDScript, anything) can implement it
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

`hearth-flame-gui` (eframe/egui) is separate from `hearth-flame` so the plain
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

## D22 — Voice is real Opus, and cmake is now a build requirement for the client

This supersedes D14. The `opus` crate builds libopus with cmake, which D11 went
out of its way to avoid for the server — but there is no pure-Rust Opus encoder,
and the alternative was hand-rolling a codec (16 kHz ADPCM: worse sound, more
bandwidth, and a protocol doc that no longer matches the name "Opus"). Only
people building the client pay the cmake cost; players run the exe. On Windows
`client/build.ps1` adds the default CMake install to `PATH` and picks the
Visual Studio 2022 generator, because cmake otherwise chooses a generator it
cannot find.

The audio path: a Godot `AudioEffectCapture` on a muted "Mic" bus is drained
every frame and handed to the extension as mono floats; Rust buffers them and
emits one 20 ms Opus frame per 960 samples, so GDScript never has to care about
frame boundaries. Incoming frames are decoded per speaker — Opus decoders are
stateful, so a shared one would garble two people talking at once — and pushed
into an `AudioStreamGenerator` on that player's head, which makes voices
positional for free. The capture buffer is drained even while muted, so
unmuting does not replay a backlog. There is no jitter buffer yet: frames play
as they arrive.

## D23 — Grabbing freezes the body and hands it back its speed

Picking something up freezes the `RigidBody3D` and copies the holder's transform
onto it each frame; letting go unfreezes it and sets `linear_velocity` from how
far it travelled in the last frame, which is what makes throwing work without a
physics joint. A joint would be more correct against walls, but needs tuning per
mass and can explode when two hands fight over one body. On the desktop the
"hand" is a point in front of the camera with a longer reach, so one code path
serves both.

The HUD is one icon, not text: an SVG mic that turns red and crossed out when
muted. Text in a headset is either too small to read or too big to ignore, and
a mute indicator is the one thing you want to check at a glance.

## D24 — A world is JSON plus a zip, and the server never looks inside

`world.json` lists objects — a primitive with a size and a colour, a `.glb`
model, or a light — and `assets/` holds the models. Zipped, that is one
`.hworld` file. The spec is `docs/WORLD_FORMAT.md`.

JSON because a world has to be writable by hand, diffable, and implementable
from the doc by another tool; a Godot `.tscn` or a binary format would tie the
format to one engine and one version of it. The server stores the file, its
size and its SHA-256 and streams the bytes: it has no parser to attack, no
opinion about what a world contains, and gains nothing from understanding one.
Revisit if worlds need server-side logic, which would mean scripts, which is a
much bigger decision than a file format.

Objects are deliberately coarse: five primitives, one light type, three body
kinds. It covers a courtyard, a house or an obstacle course, and it is the
whole of what `main.gd` already knows how to collide with and pick up (D21,
D23). The hub world that ships with Hearth is the old hardcoded `world.tscn`
expressed in it, which is the proof it is enough.

## D25 — One server, many instances, and a hub nobody can close

This supersedes the single room of D9. The server keeps a repository of worlds
(`data/worlds`), runs an instance per world the host starts, and scopes
membership, poses and voice to an instance. Player ids stay unique across the
whole server, so the window and the logs never show two players with one
number, and a client needs no extra bookkeeping when somebody moves.

Instance 0 is the hub and is always running, even when its world file is
missing — a client then keeps the room built into its own exe and can still see
other people. That means a fresh server with no worlds at all still works as a
place to meet, which is what a server was before this change.

Players arrive in the hub and walk to everything else. `client_instancing`
lets them start their own instance of any world in the repository; those close
when the last player leaves, so nobody can fill the server with empty rooms.
`max_instances` caps the rest.

The refusal path is the interesting part: `JoinInstance` and `CreateInstance`
are answered with `Moved`, and `Moved` naming where you already are means no.
A `Reject` would have been the obvious choice, but `Reject` in this protocol
means "the connection is over", and being told you cannot open a world is not
that. One answer per request, success or not, and the client needs one code
path.

## D26 — Two exes from one Godot project, and models are converted at import

Hearth Studio is the same project with the `studio` export feature tag, booted
through `boot.tscn` into `studio.tscn` instead of `main.tscn`. A second Godot
project would have been a separate app in the honest sense, and would also have
been a second copy of the world loader, the materials and the player rig to
keep in step with the first. Instead the studio's Test button loads the game's
own `main.tscn` with `Session.offline`, so what you test is what players get.

The studio imports `.glb`, `.gltf`, `.obj` and `.fbx`, and everything leaves as
`.glb`. That one rule is what keeps the game's loader to a single format: a
world downloaded from a stranger's server can never feed an obj or fbx parser,
because a world may not contain those files at all. `.obj` is read by a small
parser in `studio_import.gd` and written back out with `GLTFDocument`, which
does work in an exported build (checked). `.fbx` shells out to FBX2glTF, the
same converter Godot's own importer uses; without it, FBX import says so
instead of half-working.

## D27 — Downloaded worlds are cached by hash, and that is not authentication

`WorldAdded` carries size and SHA-256; the client reads at most that many
bytes, checks both, and only then unpacks. The cache is addressed by hash
(`user://worlds_cache/<sha256>.hworld`), so the same world from two servers is
fetched once and a world that changed can never be served from the cache.

What this does **not** do is tell you the server is who it claims to be. D3 is
still open: the client accepts any certificate, so whoever is on the path picks
which bytes your glTF parser eats. The format limits (256 MB, no paths that
climb out of the folder, `.glb` only, caps on entries and objects) bound the
damage, not the class of bug. Trust-on-first-use is now the most valuable thing
left undone in this project.

The cache never evicts. shortcut: no eviction, add least-recently-used cleanup
when somebody notices the disk space.

## D28 — One set of commands, and the log the window shows

The window and the console server are two faces on the same `Server` handle, so
the commands live in `command.rs` and both call `command::run`. Every one of
them is a plain synchronous method on `Server` — no await, no channel — which
is what makes a shared command set cheap: the console reads stdin on a thread
and calls it, the window calls it from a text box. A second implementation of
"start that world" would have been the obvious way to get a CLI, and the two
would have drifted within a week.

`quit` is the one command the window answers differently: it has a close
button, and a typed `quit` there would be a trap. Stdin ending is *not* a quit,
because a server run as a service or in a container has no stdin, and one fed
commands from a pipe should keep running afterwards.

The log is a ring buffer of 2000 formatted lines (`log.rs`) that implements
`MakeWriter`, so `tracing_subscriber::fmt` writes the same text it would write
to a console and we keep it. That is a dozen lines against a custom `Layer`
with a field visitor, and the window shows exactly what the console shows. The
window tees to stdout as well, which only exists under `cargo run`, since the
exe is built with `windows_subsystem = "windows"`.

Connection attempts, refusals, moves between instances and world transfers are
all logged at info, not debug. A host's first question is "did they even reach
me", and port-scanner noise on a game server is a price worth paying for an
answer. `RUST_LOG=debug` still adds the per-datagram detail.

## D29 — The server owns who holds an item, but not whether it may be taken

Items are networked by index: a grabbable is identified by its position in its
world's `objects` list, so no id scheme and no handshake is needed, and the
server never has to know what a world contains. Ownership lives in the
instance (`Instance.held`), so two instances of one world have separate items.

The server grants every `Grab`, taking the item off whoever had it. It cannot
do better: it has never read the world, so it cannot know the world marked that
item `locked`. So `locked` is enforced by the client doing the grabbing, which
simply does not ask. A patched client can take a locked item. The alternative
is teaching the server to parse worlds, which is exactly what D24 refused, and
the thing being protected is a lantern. Revisit if griefing ever actually
happens, and then by sending the server a small item manifest rather than the
whole world.

What the server does guarantee is worth having and is cheap: exactly one holder
at a time, everybody told about every change in the same order, an arrival told
what is already in hands, and an `Item` transform dropped unless it comes from
the holder. Two people grabbing the same crate in the same frame cannot both
end up carrying it, which is the bug that actually matters.

A throw stays owned by the thrower until the body comes to rest, and only then
is it released. Releasing at the moment of the throw would have each client
simulate its own arc from its own slightly different starting state, and the
crate would come to rest somewhere different for everybody. Streaming it for
the second it is in the air costs a handful of datagrams and makes the landing
agree.

## D30 — Every arrival is a lift ride, and the player is never attached to it

Joining a world drops a lift car out of the sky on three glowing rails, and it
opens at the spawn. The player is not parented to the car and has no special
state: the car floor is a static body, and the ground raycast from D21 already
puts the play space on whatever is underneath. Riding down is standing still,
and stepping off is walking, with nothing to undo — which is also why the car
has no walls. Once you are off, it falls away and the rails fade, so the world
is not left with a lift in the middle of it.

The lift leaves as soon as the connection is up, but the world it is taking you
to only finishes downloading a moment later. Rather than hold the ride until
the world lands, `ride()` called again moves the car to the new spawn and
carries on from the height it had reached, so a slow download does not reset
the descent.

## D31 — The menu is driven by a stick, not a pointer

The in-world menu is a panel of rows moved through with the thumbstick or the
arrow keys and picked with the trigger or Enter. A laser pointer would be the
obvious design, and this game has no pointer: building one first would mean the
menu is unusable in a headset until it exists. Rows are data from `main.gd`, so
the live instance list and the volume sliders are the same kind of thing, and
the menu knows nothing about either.

Movement is off while the menu is open, because the stick cannot both pick a
row and walk into a portal.

## D32 — The music and sound effects are synthesised, and replaceable by file

`client/godot/tools/make_audio.py` generates every sound from sine waves and
noise: no dependency, no licence, and regenerable. The two music tracks loop
seamlessly because every oscillator and tremolo is snapped to a whole number of
cycles over the loop length, which is cheaper than crossfading a tail.

Lookup is by name, `.ogg` before `.wav`, so dropping a real recording next to a
generated one replaces it with no code change, and a name with no file is
silently ignored. That is what makes `play()` safe to call from anywhere
without guarding every call site.

## D33 — Primitives get primitive collision shapes

`world_format.gd` built a concave triangle mesh for every static object. That
is the most expensive shape Godot has, and a world of a couple of hundred boxes
and spheres cost about thirty seconds of five-second frames on first load: the
whole of the reported "it gets really laggy when connecting". Measured on the
hub, the first eight frames went from 30.8 s to 0.10 s.

A box, a sphere and an equal-radius cylinder now get the matching primitive
shape, which is exact as well as cheap. Anything convex but irregular (a
squashed sphere, a cone) gets a convex hull. Only a torus, a plane and a glTF
model still get real triangles, because those can be concave and you can walk
through the middle of them. A grabbable always gets a hull: you carry it rather
than walk through it.

The lesson generalises: the cost was never on the wire. 20 Hz of quantised
poses is about 2 KB/s, and no amount of network tuning would have moved this.

## D34 — The launcher asks VR or desktop before the game starts

`boot.tscn` asks how you want to play, with two buttons, before `main.tscn`
loads. OpenXR has to be started (or deliberately left alone) before the first
frame is drawn, and a flat screen is unreadable once a headset is already on.
`--mode` or `--world` on the command line answers the question instead, which
is what the studio's Test button and every scripted run rely on.

## D35 — You join from a panel in the lift, not from a 2D screen

The server address and your name are rows on a panel bolted to the lift car,
typed with `keyboard.gd`: a grid of keys driven by the thumbstick, because this
game has no laser pointer and you cannot reach a real keyboard with a headset
on. On the desktop the real keyboard types into the same panel, which stays up
as the readout, so there is one code path and not two.

`connect.tscn` is gone. The old 2D connect screen could not be used from inside
a headset at all, which meant VR players had to set the address up on the
monitor first and then put the headset on.

Walking is off until you have joined: step out of the lift with no server and
the car falls away with the only way in standing on it.

## D36 — The car carries the player, gravity does not

`main.gd` pins the play space to `Elevator.car_y()` while the car is coming
down. Leaving it to gravity and the ground ray looked exactly like the bug it
was: the car leaves at a constant 6.5 m/s while the player starts from rest, so
you hung in the air, free-fell about seven metres, slammed onto the car most of
the way down and juddered the rest.

The rails also run outside the car rather than inside it, because inside they
came up through the floor you were standing on.

## D37 — Panels are pointed at, and the hit test is arithmetic

`panel.gd` gives the lift panel and the keyboard one job in common: tell me
where a ray crosses this rectangle, in its own coordinates. Rows and keys are
then looked up from that local point, because both already know where they drew
everything.

No Area3D and no collision layer. These panels are rebuilt whenever a row
changes, and keeping a collider per row in step with that is more code than
intersecting one plane -- which is also exact, and costs no physics.

The pointer is the mouse through the camera on the desktop and a controller in
the headset. The thumbstick and arrow keys still move the highlight, so neither
way of driving a panel was removed.

## D38 — The mouse points at panels instead of looking around

While a panel is up on the desktop the mouse is released. The click that opened
a field used to also capture the mouse, after which the next movement of the
wrist swung the panel off the screen -- which is what "I cannot even type into
the panel on desktop" was. Typing itself was never broken: `tests/input_test.gd`
drives the real scene with real key events and passes.

The lesson: every piece had its own test and the routing between them had none.
That test now loads `main.tscn` and pushes actual `InputEventKey`s through it.

The keyboard can be carried by the bar across its top, and clicking away from
everything puts it away, keeping what you typed. Once you have moved it, it
stops being pinned to the lift car.

## D39 — A quad faces +Z, so -Z points away from the player

`WorldMenu.open` turned the panel's back to the player: a `QuadMesh` faces +Z
and culls its back face, so `look_at(position - forward)` left the backing
invisible and the text mirrored. Label3D is double-sided by default, which is
exactly why it looked almost right instead of plainly wrong. Anything placed
facing the player goes through `main.gd`'s `_face`.

## D40 — The key grid is for the headset only

Typing an address needs two different things in two places, so it is two
layouts behind one object (`keyboard.gd`).

In the headset it is a grid of keys you point at, because there is no keyboard
you can reach with a headset on. On the desktop there is one, and it is better
than any grid of keys you could click, so all that comes up is the line you are
typing -- big, in the middle of the view -- and `main.gd` feeds real key presses
to the same `type` and `backspace`.

The desktop line is deliberately not pointable: `_width` stays zero, so
`FlatPanel.hit` misses it. It is a readout, not a control, and a readout parked
in the middle of the view would otherwise swallow the clicks meant for the field
behind it.

The stick and the trigger still arrive at it, because main.gd sends them to
whatever is up; with no grid they do nothing, which is tested rather than
assumed.

## D41 — One file is the whole interface style

`client/godot/ui.gd` holds every colour, size and material the interface uses,
and `docs/UI_STYLE.md` is the guide. Near-black surfaces, a hairline round each
panel, a bright mark at each corner: cheap enough to draw twice (once per eye),
and a reskin is nine colours rather than a sweep through every file.

Panels in the world read the tokens at runtime. Flat controls -- the launcher
and the world builder -- are styled by `ui_theme.tres`, which `project.godot`
applies to every Control in the project, so neither `boot.gd` nor `studio.gd`
contains a line of styling. That theme is generated from the same tokens by
`tools/make_theme.gd`, which `build.ps1` runs on every build, so it cannot
drift from them.

Content keeps its own colours: an icon, an avatar, a world, a flame. Those are
what you are looking at, not the frame round them. The microphone icon stays a
drawn icon, and a world stays whatever its `world.json` says.

## D42 — Contrast is checked, not eyeballed

Every glyph gets a dark outline from `UiStyle.label()`, because a panel is
translucent and the world behind it is any colour at all: text that works only
against the panel it was designed on stops working when somebody walks behind
it.

The tokens are checked as well. `UiStyle.contrast()` is the WCAG ratio and
`tests/panel_test.gd` fails if body text drops below 7.0 on any surface it can
land on, or a meaning colour below 4.5 on a panel. A reskin cannot quietly ship
grey on grey. `UiStyle.on(surface)` picks the readable text colour for a
background that is not a token -- a world colour, a swatch, a chosen tint.

Text is also placed rather than fitted: `PAD` inset from every edge, labels in
one column and values from `VALUE_AT` in another so values line up with each
other, and each row's text centred in the row so the highlight lands on it. A
field's name and its contents are two labels in two places, never one run-on
line. The highlight used to sit a fifth of a row below the text it was
highlighting.

## D43 — Draw order is banded per panel

Interface ignores depth, so `render_priority` is the only thing deciding what
covers what, and it is global. `LAYER_BODY`/`FILL`/`EDGE`/`TEXT` order a panel
against itself; a panel that has to sit in front of another adds
`UiStyle.BAND` to lift its whole range. Without that the panel behind drew its
text over the nearer panel's background, and the VR key grid came out with the
lift panel showing through it.

`tools/shot.gd` renders the real scene to PNGs. It exists because four
separate interface bugs -- grey mush text, a selection washing out its own row,
a settings menu left behind by a descending lift, and the key grid showing
through -- were all invisible in the code and obvious in a picture. Three of
them shipped. Render the shots when changing anything in `ui.gd`.

## D44 — The lift waits until you have somewhere to go

The car starts at the top of the shaft with its doors shut, the panel on its
-Z wall beside you. Picking a server is what calls `descend`; the doors open on
the world when it lands. It used to ride down the moment the game started,
which was four seconds of descending before anybody had typed an address, and
it arrived at a hub you had not asked for.

The car is a box with doors on +X, so the doorway never covers the panel and
the panel never blocks the way out. The three rails run outside the other
three sides for the same reason. `carrying()` (not `descending()`) is what
main.gd pins the play space to, so the player is held up while it is parked as
well as while it moves.

## D45 — Nothing walkable shares a plane with anything else

Two solid tops at exactly the same height, overlapping, flicker as you walk:
the GPU has no way to choose between them. The hub had three such pairs, and
the worst was the top step against the dais -- right where you stand to type.

`tools/make_hub.py` now asserts it: any two static boxes or cylinders whose top
faces land on the same height and whose footprints overlap fail the build. It
only checks shapes square to the world, because comparing turned footprints
with a circle either misses real overlaps or flags every beam that meets
another end to end. The causeways, which are turned, ride a centimetre proud
instead -- a lip you cannot see and do not trip over.

## D46 — The hub music is a piece of music

It was one held chord with a bell. It is now Am - F - C - G twice with a tune
over it: eight bars at 60 bpm is exactly 32 seconds, and every voice goes in
through `Track.add`, whose tails wrap past the end, so the last bar rings into
the first.

Under it is a tonic drone spanning the whole loop, snapped to whole cycles.
Without it the eighth bar had decayed to a third of the opening level by the
time it came round, which is a lull you hear every thirty seconds; with it the
seam is a 0.6% sample jump and the end sits at 46% of the opening level, which
reads as a cadence rather than a dropout.

## D47 — The launcher asks in both places at once

"How do you want to play?" is on the monitor and, if a runtime answers, on a
panel inside the headset as well. Both stay live, so it does not matter which
one you happen to be looking at, and a headset that only shows up a few
seconds after launch is picked up and gets its copy then.

The handover is provisional, and that matters. OpenXR will start a session
with nothing on anybody's head: on the machine this was built on,
`initialize()` succeeds with no headset attached, the viewport switches to XR,
and the monitor goes black -- leaving a desktop player staring at nothing with
no way to answer. So `boot.gd` watches `get_tracking_status()` and, if real
tracking has not turned up within three seconds, takes the viewport back and
uninitialises the session. Without that last step OpenXR complains once a
frame for ever that nothing is rendering it.

The headset panel is a `WorldMenu`, the same two rows as the buttons, driven by
the stick and any controller button. It is placed in the world rather than
stuck to your face, but it comes back if it ends up out of view -- which
includes the first frames, before the headset has reported a pose worth
placing anything against. Placed from a guessed 1.6 m eye height against a
camera still reporting the origin, it sat above the view and a check on
horizontal distance alone never noticed.

## D48 — Leaving VR releases the headset

Switching to "this screen" used to set `use_xr = false` and stop there. The
OpenXR session stayed up, so the player -- still wearing the thing -- got a
black layer while the game carried on happily on a monitor they could not see.
From inside the headset that is indistinguishable from the switch doing
nothing.

`_leave_vr` now uninitialises the interface, and resets the camera transform:
an `XRCamera3D` keeps whatever pose the headset last handed it, so without that
the desktop view stays stuck at the angle your head happened to be at. The
launcher does the same when you pick the screen from inside the headset.

Both ways of choosing now work and both are reachable from either place: the
launcher asks before the game starts, on the monitor and on a panel in the
headset (D47), and "Playing on:" on the lift panel switches live.

## D49 — The player has a body

Walking was a straight add to the play space position. The only collision in
the game was the ray that looks for the floor, so every wall, pillar and crate
in every world was walk-through. Each step now sweeps a capsule and stops at
what it hits, retrying each axis on its own so you slide along a wall rather
than stick to it.

Two things that had to be measured rather than reasoned about:

* `direct_space_state.cast_motion` from `_process` finds nothing at all. Space
  state queries are only valid inside physics; called from `_process` they
  fail quietly, and the player walked through 5.99 m of an attempted 6 with
  every wall present and solid. A `ShapeCast3D` node works from anywhere,
  which is how the ground ray has always done it.
* `get_closest_collision_safe_fraction()` returns **1.0 while `is_colliding()`
  is true** when the cast starts already touching. So you would stop against a
  door and then walk straight through it on the next step. Once in contact the
  surface normal decides instead: into it is blocked, away from it is free,
  which is also what lets you walk away again.

The lift's walls and its shut doors are solid, which is what makes walking and
turning inside the car safe while you type: there is nowhere to go until the
doors open.

## D50 — The sticks are for walking, never for menus

A panel read the thumbsticks as well as the pointer, with auto-repeat. A stick
sitting slightly off centre therefore walked the selection up the panel on its
own, for ever. Panels are now pointer-and-click, or the arrow keys, one step
per press; the sticks only ever move and turn you.

Also gone: the status line that hung in the world on a Label3D, which read as
a sign bolted to the sky. It is a log line, and anything the player has to act
on is on the lift panel. The microphone icon is grey when nothing is going out
and bright when it is, from the peak of every sixteenth captured sample.

A world arriving from a server is checksummed against the one already built,
and skipped if they match -- which they usually do, because both came from the
same hub file. That was a couple of hundred objects and their collision
rebuilt for no change at all.

## D51 — Protocol v4: shooting, and data the client cannot rewrite

`docs/TRUST.md` is the reasoning; this is what landed.

**Shooting.** The server never reads a world, so it has no walls and cannot
raycast. The shooting client traces the shot and claims a hit; the server owns
health, death and respawn, and checks everything it can without a world: same
instance, both alive, within fifty metres of the two last known head
positions, and no more than ten claims a second. Line of sight it cannot
check, and that is written down rather than pretended away.

Respawn is asked for by the client and granted only to the dead, which keeps
the server event-driven with no tick loop for one timer (D28). There is
nothing to gain by asking early.

**Stored data.** A key/value store per player, on the server, written to disk.
Three rules do the work: a client can only write its own data, because no
message names another player; the server picks the namespace from the world
the player is in, so one creator cannot reach into another; and the quotas are
checked before anything touches a disk. A player name never reaches the
filesystem -- the file is named after a hash of it -- which is cheaper than
reasoning about every separator and reserved name for ever.

Keyed on the name typed at the connect panel for now, which anybody can claim.
The store is built first so that accounts can be swapped underneath it without
a protocol break.

## D52 — Accounts: one identity server, many game servers

Designed, not yet built (docs/TRUST.md). A central server knows who people
are and nothing else, and hands out a short-lived signed token. Game servers
anybody can host verify the signature against a public key they already hold,
and never see a password, never call the account server mid-match, and cannot
impersonate a player to another server.

The hook is already in place: `Hello` carries a version and a name, and the
token is an optional third field. A server with no account server configured
ignores it and keeps using the name, so community servers work standalone from
the first day and turning accounts on is a server setting rather than a
protocol break.

## D53 — Three folders, and the things in them have names

The repository now says what it is becoming (D52, docs/TRUST.md):

| folder | crate | binary | called |
|---|---|---|---|
| `/client` | `hearth-client` | *(GDExtension)* | the client, and Hearth Studio |
| `/server` | `hearth-flame`, `hearth-flame-gui` | `flame`, `flame-gui` | **a Flame** |
| `/main-server` | `hearth-fireplace` | `fireplace` | **the Fireplace** |
| `/shared` | `hearth-protocol` | — | the wire protocol |
| `/protocol` | — | — | the spec, `PROTOCOL.md` |

The protocol crate moved out of `/server` because all three depend on it and
it belongs to none of them -- `client/hearth-client/Cargo.toml` was already
reaching into `../../server/` for something that is not the server.

One of these is run centrally and one of them is run by anybody, and the
folder names should not have to be explained.

## D54 — Turning accounts off is behind a warning, not behind a recompile

Some people want to join a friend's machine with no account at all, and some
want to point accounts at their own. Both are reasonable; both hand away a
protection the default gives you. So both are in the interface, behind a page
that says so in plain words, and the warning is shown **every time** the page
is opened rather than acknowledged once. It is one extra click, and it is the
only thing between somebody and handing their sign-in to a stranger.

The account host is one named constant, `ConnectDetails.DEFAULT_FIREPLACE`, so
pointing the shipped default at a real box is one line.

The page is three pages of one `WorldMenu` switched by `_panel_page`. No new
scene, no new UI machinery: headings, buttons and typed fields already exist
(D37, D40).

## D55 — A desktop player must not be driven by an idle headset

`project.godot` sets `openxr/enabled`, so Godot opens an OpenXR session at
startup before anything asks for one. An `XRCamera3D` is then driven by the
headset pose **even with `use_xr` off** -- so on a desktop machine with a
runtime installed and nothing on anybody head, the view sat at whatever stale
angle the runtime last reported. Found by screenshotting the interface and
wondering why the panel was in a corner; blamed on the screenshot harness for
two rounds before being looked at properly.

`main.gd` now releases the session on startup when the player chose the
screen, as `boot.gd` already did when they picked it from inside a headset
(D47).

## D56 — The GDScript extractor is ours, in-repo, not a graphify plugin

graphify ships 30-odd language extractors and none of them reads `.gd`, so
every build of the client graph saw the Rust, Python and PowerShell and treated
the 24 GDScript files -- the whole client, most of the work -- as text for an
LLM to describe. Its `LANGUAGE_EXTRACTORS` table looked like the place to
register one, but that table is a registry seed: its own docstring says wiring
dispatch through it is "a later, separate step", so an entry there does nothing.

`client/godot/tools/gdscript_graph.py` is a standalone pass instead. It emits
graphify's own node and edge shape, stamped `_origin: "ast"`, and imports
`graphify.ids.make_id` (with a local mirror if graphify is not installed) so
ids cannot drift from the other tiers and strand edges on ghost nodes.

The thing that makes it worth having rather than clever: GDScript's usual way
of crossing a file here is a method on a typed variable -- `var _lift:
Elevator` then `_lift.park()`. Resolving only `Class.method()` found 1705 edges
and almost none of the traffic between files; resolving declared variable types
as well found 1917, and the 321 cross-file calls include
`main._process -> elevator.car_y`, which is the shape the client is actually
built from. 655 nodes against 225 from graphify's own pass, and zero id overlap
with it, so it adds rather than duplicates.

It is also installed globally, at
`~/.claude/skills/graphify/extractors/gdscript_graph.py`, with graphify's
`SKILL.md` Part A patched to run it and merge the result into the AST tier --
same file, same `_origin: "ast"` stamp, so every later step is untouched. So
`/graphify` now reads GDScript in any project, not just this one. The pass
returns nothing when a corpus has no `.gd` files, which is what makes it safe
to leave in every run.

The repo copy stays the source of truth because it is the one under version
control; the global one is a copy of it. They can drift, and nothing checks
that they have not.

## D57 — A failed join says so in red, and the lift does not move

D33 already said the lift leaves "as soon as the connection is up". The code
did not: `_join()` sent the car down the moment you picked a server, before
anything had answered. So a refused or unreachable server rode you the whole
way down, opened the doors on the hub, and then snapped the car back to the
top when the failure finally arrived -- the worst possible reading, because
the one thing that looks like success is the doors opening.

The descent now happens only in `_on_connected`. Nothing moves while
`Connecting...` is on the Join row, and a failure leaves you where you already
were, in the lift, with the panel in front of you.

The reason is shown there in red: a menu row may carry `"warn": true` and is
drawn in `UiStyle.WARN` instead of the heading grey it used to share with the
"Point and click" hint, where it read as another piece of furniture. WARN was
already in the contrast test's meaning-colour loop, so the floor it has to
clear on panel ink (4.5:1) was being measured before anything used it here.

`input_test.gd` covers it by joining a port nothing listens on and checking
the car's `height()` has not changed -- not `descending()`, which is true of a
parked car too, since it means "the doors are not open yet".

## D58 — Embers and sticks, and a menu with tabs instead of one long list

The words first. A running instance is an **ember** and a client is a
**stick**, everywhere a player can read it: the menu, the portals, the status
line. `docs/PROTOCOL.md` keeps saying "instance", and so does every identifier
in the code, because renaming the wire would touch every file in the repo and
buy nothing a reader of the spec needs. The glossary line at the top of the
rooms section is where the two vocabularies meet.

Protocol v5 gives `InstanceAdded` and `CreateInstance` a `name` and a `kind`.
A name is what the ember's maker called it, encoded like any other `name`; the
hub and the embers a host starts are named after their world. A kind is public,
friends+, friends or invite, and a byte over 3 is a protocol error rather than
a silent default. Nothing enforces anything but public: there are no accounts
yet (D52), so a Flame cannot tell whose friend anybody is. The field is carried
and repeated so clients can show it, and the shortcut is marked in
`hearth_protocol::ember` with what would lift it.

The menu is three pages behind a strip of icon tabs -- People, Worlds,
Settings -- instead of one list with every heading in it. Worlds is a drill:
the worlds a Flame hands out, then one world's embers, then the rows that light
another (a typed name, a kind that cycles, and "Light it"). A row may now carry
an `"icon"` (an SVG from `client/godot/icons/`, tinted like the text beside it)
and a `"note"` (a read-only right-hand column -- `"text"` already meant "a
field, and picking it opens the keyboard", which made a world row that merely
counted its embers open the keyboard instead of opening the world).

Two smaller things. The panel grows out of its own middle when it opens: every
part it draws hangs off one child node, and that child is what the tween
scales, because scaling the panel itself moves the plane `FlatPanel.hit`
measures the pointer against and the rows would be somewhere else for the
length of the animation. And the menu can be carried: `is_bar` moved up into
`FlatPanel`, so the grab strip the keyboard already had works for any panel
that wants one. The lift panel sets `movable = false`, since it is bolted to
the car and put back every frame.

The "Playing on: the headset / this screen" row is gone from the lift panel.
The launcher already asks, before the first frame is drawn, which is the only
time the answer is free (D18); and `_offer_vr` still picks up a headset that
shows up late. A switch that tore the session down mid-flight was one more way
to end up looking at a black layer (D55) for a choice nobody needed twice.

## D59 — The official servers live on one Always Free box, and nothing depends on that

Both servers are plain Rust with no C in them -- no CMake, no libopus, that is
all client -- so a Flame and a Fireplace build and run on Windows, Linux and
macOS from the same checkout. `start.bat` and `start.sh` are the same script
twice, with the default flipped: the window on Windows, the console on Linux,
because a server has no screen to put a window on. `docs/RUNNING.md` is the
whole of how to run either one anywhere, and `deploy/` carries a systemd unit
for each.

The official ones run on a single Oracle Cloud Always Free Arm instance
(`docs/ORACLE.md`), which is 2 OCPUs and 12 GB after the 2026 halving, and ten
terabytes of egress a month -- far more Flame than we will need, free, and
replaceable in an hour if Oracle changes its mind again. Nothing in the code
knows about any of that: it is one box running two services, and the guide is
ordinary Linux with the cloud's two firewalls (a VCN security list *and* the
image's own iptables, which is where everybody loses an afternoon) called out.

The Fireplace stays plain HTTP and goes behind Caddy, which owns 80 and 443
and renews the certificate itself. A second TLS implementation inside the
Fireplace would be a second thing to get wrong, and half of TLS reads as
security without being any. Its port is never opened in the security list;
the terminator reaches it on localhost.

What would make us revisit it: a Flame busy enough to want its own machine,
or accounts arriving, which turns the Fireplace from a health check into
something with keys worth protecting -- at which point its data directory
wants a backup that is not a `scp` somebody remembers to run.

## D60 — Accounts, friends and presence, and the client is what says where it is

The Fireplace stopped being a health check. It holds accounts, friendships
and where people are, over an HTTP API (`docs/FIREPLACE.md`), and the game
signs in to it from the lift panel.

**The client reports its own presence; a Flame never does.** This is the one
decision everything else here hangs off. A Flame that had to tell the
Fireplace who was standing in it would put a single server in the path of
every match, which D52 and `docs/TRUST.md` refuse -- so instead the player's
own client says "I am on this Flame, in this ember", every 45 seconds, and a
report nobody repeats is forgotten after 120. The cost is that a client can
claim to be somewhere it is not. It can only lie about itself, and what it
says is checked anyway, because that address ends up in a friend's client and
from there in a connection attempt.

Following a friend therefore needed **no protocol change and no change to a
Flame**: it is the address they reported, put through the join that already
existed, and then `join_instance` on the ember they named.

The smaller choices, each a sentence:

* **axum**, because the hand-rolled loop could not read a request body at
  all, and a correct HTTP/1.1 subset with bodies, limits and timeouts is more
  than the sixty lines it would have replaced.
* **One JSON file, read into memory, written atomically** -- the Flame's
  store pattern (D51), beside-then-rename. A friendship touches two accounts,
  and one file is the only way it cannot be half true in one direction. When
  thousands of accounts stop fitting, the answer is a database, not a
  cleverer file.
* **argon2id**, on `spawn_blocking`. It costs 19 MiB and real time on
  purpose, which is exactly why it must not run on the async runtime: a
  handful of people signing in at once would otherwise stall every other
  request, the health check included.
* **A versioned token that is not a JWT**: `v1.<claims>.<signature>`,
  Ed25519 through `ring`, which D11 already chose and which is already in the
  tree. No new dependency, and no `alg` field for anybody to lie about.
  Anything whose prefix is not exactly `v1.` is refused rather than guessed
  at, which is what leaves room for a `v2.`.
* **An hour for asking, thirty days for staying signed in.** Typing a
  password on a keyboard floating in mid-air is miserable; the long token
  lives in `user://settings.cfg` on the player's own machine, which is the
  bargain every game launcher makes. A password is never saved, and is shown
  as dots wherever it is typed -- a panel in VR hangs in a room other people
  can be standing in.
* **Loopback by default.** The Fireplace speaks plain HTTP (D59) and belongs
  behind a terminator on the same box. A port that is never open is a better
  last line than a firewall rule that might be wrong.
* **No email, so no password reset.** *(Superseded by D63.)* A reset flow is most of the work and
  all of the abuse surface; a forgotten password is a new account until there
  is somewhere to send one. The create-account page says so, in those words,
  because the alternative is a support request with no answer.

What would make us revisit it: enough accounts that writing the whole file on
every change hurts, which means a database; needing to revoke a token before
it expires, which means a session store rather than the `token_epoch` number
that is there now; or a Flame wanting to enforce a `friends` ember, which
means the token in `Hello` -- and that waits on D3.

## D61 — A warm, rounded interface, friends and chat beside the menu, and a camera you hold

The interface was correct and looked like a tool: square near-black panels,
sliders drawn as `=====-----`, every page a list. It is now something people
would expect from a finished game, and still one file to reskin
(`docs/UI_STYLE.md`).

* **One rounded-rectangle shader for every surface.** The signed distance of a
  rounded box, anti-aliased by its own `fwidth`, with a border, a soft edge
  and a sheen as uniforms. Panels, chips, switches, slider tracks and knobs,
  fields, the selection and the shadow are all that one quad. No textures to
  ship, no nine-patch to keep in step with a size, and no dearer than the flat
  quads it replaced. Its border is laid *over* the fill rather than
  cross-faded with it, which is the difference between a hairline and a
  bright seam.
* **Interface ignores depth, shapes included.** Text already did. With the
  shapes depth-tested, a menu opened near a wall lost its panels and kept its
  words; now the two agree and `render_priority` alone orders them.
* **Warm, not blue.** The project is called Hearth and its servers are
  Flames and embers; a cold UI blue fought every one of those words. The
  accent is ember amber, surfaces are charcoal, text is ash-white. Every pair
  is measured (`tests/panel_test.gd`).
* **Friends and chat are side panels, not tabs.** They are what you open the
  menu for half the time, and a tab hides two of the three. They hinge off the
  menu's edges, turned in towards you, and travel with it when it is carried.
* **Tap and hold on one button.** Tap is the menu, as before; hold is a quick
  menu of four round buttons by your hand. The tap is now decided on release,
  which costs nothing anybody can feel and frees the hold for the few things
  you reach for mid-conversation.
* **Chat is a protocol message, not a stored value.** `Say` / `Said`
  (protocol v6), per instance, the sender included in the echo so everyone
  sees the same order. A line a second with a burst of five; over that it is
  dropped, not punished. No control characters, so nothing in a line can do
  anything to the Flame's log.
* **The camera renders its own picture.** A `SubViewport` behind the lens,
  shown live on the screen at the back at 480x270 and re-rendered once at
  1920x1080 when you shoot. The body is on a layer of its own that the lens
  leaves out. It is local only: other players do not see your camera yet,
  because that would be a pose for a held thing that is not an item, and
  there is no message for it.

## D62 — Accounts are the key to saves, and a Flame only ever sees a ticket

A Flame now checks who somebody is, and what it learns is as little as that
can be (docs/TRUST.md, docs/FIREPLACE.md).

* **A join ticket, bound to the Flame's certificate.** After the TLS
  handshake the client asks the Fireplace (`POST /join`) for a ticket naming
  the SHA-256 of the certificate it was just shown, and sends it in `Hello`
  (protocol v7). The Flame refuses any ticket naming another certificate. That
  is what makes handing a ticket to a Flame safe before pinning exists (D3): a
  copied ticket works nowhere else, and a man in the middle gets one for
  itself.
* **The Flame learns a display name and a per-Flame id, nothing more.** The id
  is an HMAC of the account and the certificate under a secret only the
  Fireplace holds, so it is stable on one Flame and unrelated on every other.
  Two hosts comparing notes cannot tell they share a player, and no host ever
  sees an account number or a handle.
* **Checked offline.** The Flame fetches the Fireplace's public key once and
  keeps it in `data/fireplace.key`; a Fireplace that is down does not stop a
  Flame that has seen it before from letting people in.
* **Saves belong to accounts.** The store is keyed on the per-Flame id. A
  guest plays if `allow_guests` is on, but every `Store` is answered with an
  empty `Value`, `Welcome` says so (`welcome::SAVES`), and worlds can ask
  `saves_kept()`. Signing in again elsewhere closes the older session, so one
  account never writes its saves from two places.
* **Every limit can be off.** Players on the server, players per instance,
  instances on the server, instances per player. `-1` in `flame.toml` is "no
  limit", which is the same thing the window's "No limit" box writes, and the
  window saves every change itself, half a second after the last one, and
  never a config the server would refuse to start with.
* **Not yet:** `friends` embers are still unenforced, because a ticket says
  nothing about friends; and a world scripting language (FireSharp) is its
  own piece of work.

## D63 — Email is optional, and only ever for getting back in

D60 said "no email, so no password reset", and the account page said so in
those words. That was honest, and it turned a forgotten password into a lost
friends list and lost saves -- since D62 an account is the key to saves, so
the cost of forgetting went up and the old answer stopped being good enough.
This supersedes that bullet.

* **Email is optional, and so is sending it.** Making an account still takes
  a handle and a password and nothing else. A signed-in player can add, change
  or remove an address; it must be confirmed by a link before it can reset
  anything, and one nobody confirms is deleted after 7 days. Each Fireplace
  chooses in `[mail]` whether it sends email at all, and the default is
  `backend = "none"`: a fresh self-host needs no SMTP and offers no email
  fields (`GET /features` tells the client which).
* **It is used for two things and nothing else.** A reset link the player
  asked for, and a short notice when the password or the address changes --
  to the *old* address when it is the address that changed, because that is
  the one somebody taking over an account wants kept quiet. Never marketing,
  never a newsletter, never shared. The lawful basis is performance of the
  service (GDPR Art. 6(1)(b)), so there is no consent box, because there is
  nothing to consent to.
* **Everybody gets a way back in, with or without email.** Eight recovery
  codes are shown once when an account is made, kept only as SHA-256 hashes
  (80 random bits each: a fast hash is enough, argon2 is for things people
  choose), and can be remade while signed in. An operator can also run
  `fireplace admin reset-password <handle>` over SSH and hand the link over
  some channel they trust.
* **A reset link is signed, not stored.** It is the same `v1.` shape as a
  token, signed by the same key, with its own `typ`, 128 random bits of
  nonce, the account's `token_epoch` and a 30-minute expiry. Using it moves the
  epoch, which ends every sign-in and refresh token *and* makes the link
  itself worthless, so it works once without a table of used links. Nothing
  about it is in `people.json`, so a leaked data file holds no live link --
  stronger than storing a hash. It is also what lets the admin command work
  from a second process without touching the running server.
* **Asking for a reset never says whether the address is known.** The same
  page, the same status, the same time: the lookup is in memory and sending
  happens on a task of its own, so a request never waits on SMTP and never
  learns whether a letter went out. Limited per address and per account; over
  the account's limit the answer is still the same and nothing is sent.
* **Every operator brings their own SMTP.** `lettre` with rustls and ring
  only, so the build stays pure Rust. The whole of `[mail]` and `[operator]`
  is checked at startup and a bad setting stops the server with a sentence
  saying which one -- including email switched on with nobody named as the
  data controller, because `/privacy` has to say who that is. The password
  comes from a file or `FIREPLACE_SMTP_PASSWORD`, and is never printed.
* **The privacy page describes the server it is on.** `/privacy` and
  `/terms` are filled from that Fireplace's own config, so somebody hosting
  their own describes their own server, mail provider and authority. No
  analytics, no third-party anything, no cookies, so no banner.
* **Right of access and erasure are routes, not promises.** `GET /me/export`
  is everything stored about the account; `DELETE /me` removes it and its
  address at once. Removing an address deletes it rather than hiding it.

What stays the same for a player who never adds an address: every route
answers as it did (`POST /accounts` gains `recovery_codes`, `GET /me` gains
three fields), the people file holds exactly what it held -- the new fields
are skipped when empty, so old backups load and a rolled-back binary still
reads a new file -- and nothing is ever sent to anybody.
