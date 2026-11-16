# Hearth wire protocol — version 8

Throughout this document **the server** means a **Flame**: one game server,
hosted by anybody. The Fireplace -- the one server that knows who people are --
does not speak this protocol at all, and has its own (`docs/TRUST.md`).

The reference implementation is `shared/hearth-protocol/src/lib.rs`. The server
and the Godot client extension both use it. This document is the spec for anyone
writing another implementation, and it must stay in sync with that code.

## Transport

- QUIC (`quinn`), TLS 1.3. ALPN: `hearth/1` — deliberately unchanged across
  protocol versions, so a mismatch arrives as a `Reject` instead of an opaque
  TLS failure. TLS server name: `hearth`. The server certificate is self-signed
  (see docs/decisions.md D3).
- The server closes a connection that stays idle for 10 s. Clients send poses at
  `tick_hz`, which keeps the connection alive.
- **Control stream:** the client opens one bidirectional stream right after
  connecting. Every reliable message goes on it.
- **Datagrams:** pose and voice go in QUIC unreliable datagrams, one message per
  datagram.
- **World downloads:** a stream of their own, see below. A client may have one
  open at a time; the server allows two bidirectional streams per connection.
- Leaving = closing the connection. There is no goodbye message.

## Rooms

A server runs one or more **instances**, each playing one **world** from the
server's repository. An instance is an **ember** and a client is a **stick**
everywhere a player can read it; this document keeps the wire's own words. Instance `0` is the **hub**: it always exists, it cannot be
stopped, and every player lands there. Poses, voice and peer membership are
scoped to an instance — two players in different instances cannot see or hear
each other, and neither knows the other is there.

A **slug** names a world: 1–32 bytes of `a-z`, `0-9`, `-` and `_`. It is the
stem of the `<slug>.hworld` file in the server's world directory. A server
never parses a world; the format is the client's business
(`docs/WORLD_FORMAT.md`).

An **item** is one grabbable object, named by its index in its world's
`objects` list: `0 .. 4999`, matching the format's object limit. Because the
server never reads a world, an item is only a number to it, and its entire job
is that exactly one player holds one at a time. Ownership is per instance, so
two instances of the same world have separate items.

## Encoding rules

- All integers are **little-endian**.
- `name`: `u8 byte length` followed by UTF-8 bytes. It must be 1–32 bytes,
  valid UTF-8 and contain no control characters. A bad name rejects the whole message.
- Every message starts with a `u8` type. Unknown types, truncated messages and
  trailing bytes are errors. Drop the message (datagram) or close the connection
  (control stream).

## Control stream

Each message is framed as `u16 body length` followed by the body. Body length ≤ 256; a
larger prefix is a protocol error.

| Type | Name | Direction | Body after the type byte |
|---|---|---|---|
| `0x01` | Hello | C→S | `version u16`, `name`, `ticket` (`u16` length + ASCII, ≤ 1024; empty = guest) |
| `0x02` | Welcome | S→C | `your_id u16`, `tick_hz u8`, `instance u16`, `flags u8` |
| `0x03` | Reject | S→C | `reason u8`: 1 = server full, 2 = bad version, 3 = bad name, 4 = account required, 5 = bad ticket |
| `0x04` | PeerJoined | S→C | `id u16`, `name`, `account` (u8 length + up to 64 lowercase hex digits: the per-Flame id from their ticket, empty for a guest) |
| `0x05` | PeerLeft | S→C | `id u16` |
| `0x06` | WorldAdded | S→C | `slug`, `size u32`, `sha256 32 bytes` |
| `0x07` | InstanceAdded | S→C | `instance u16`, `slug`, `players u16`, `name`, `kind u8` |
| `0x08` | InstanceClosed | S→C | `instance u16` |
| `0x09` | InstanceCount | S→C | `instance u16`, `players u16` |
| `0x0A` | JoinInstance | C→S | `instance u16` |
| `0x0B` | CreateInstance | C→S | `slug`, `name`, `kind u8` |
| `0x0C` | Moved | S→C | `instance u16` |
| `0x0D` | WorldRequest | C→S | `slug` — on a download stream, never on the control stream |
| `0x0E` | Grab | C→S | `item u16` |
| `0x0F` | Release | C→S | `item u16` |
| `0x1A` | Held | S→C | `item u16`, `by u16` — `0` means nobody holds it |

`slug` is length-prefixed like `name`, with the narrower character set above.

An instance's `name` is what its maker called it, encoded like any other
`name`; the hub and the instances a host starts are named after their world.
`kind` says who it is open to: `0` public, `1` friends+, `2` friends,
`3` invite. A higher value is a protocol error. Nothing enforces anything but
`public`: there are no accounts yet, so a server cannot tell whose friend
anybody is (docs/TRUST.md). It is carried and repeated so clients can show it.
`Welcome.flags` bit 0 means the server lets players start their own instances;
bit 1 means you joined with an account and the server keeps what worlds `Store`
for you. A guest has bit 1 clear, and every `Store` is answered with an empty
`Value`.

The `ticket` comes from the Fireplace (`POST /join`, `docs/FIREPLACE.md`) and
names the SHA-256 of the server's certificate, so it is good on that server
only. A `Hello` that ends after `name` (version 6 and older) still decodes, so
an old client is told `bad version` rather than dropped.

A rejected join is told twice: the `Reject` message, and the QUIC close frame,
which carries `rejected:` followed by the same reason byte. The server closes
right after the message, and a client can learn about the close before it has
read the stream, so a client that sees `ApplicationClosed` should look at the
reason before reporting a plain disconnection.

**Handshake:**
1. The client sends `Hello`.
2. The server replies with either:
   - `Reject`, then closes the connection, or
   - `Welcome` (`instance` is always the hub), then one `WorldAdded` per world
     in its repository, one `InstanceAdded` per running instance, and one
     `PeerJoined` per player already in the hub.
3. After that, the server sends `PeerJoined` / `PeerLeft` for its instance,
   `InstanceAdded` / `InstanceClosed` / `InstanceCount` for the server, and
   `Moved` when this player changes instance.

**Changing instance.** `JoinInstance` and `CreateInstance` are each answered by
exactly one `Moved`. `Moved` naming the instance the player is already in means
the request failed — unknown instance, instancing not allowed, no world by that
slug, or no room for another instance — which keeps a refusal off the fatal
`Reject` path. On a successful `Moved`, the client forgets every peer it knew;
the server then sends a `PeerJoined` for each player in the new instance.

A player-made instance closes when its last player leaves. The ones a host
starts stay until the host stops them, and stopping one sends everybody in it
back to the hub (`Moved`).

## World downloads

`WorldAdded` tells a client what a world is called, how big it is and what it
hashes to. To fetch it:

1. The client opens a **new bidirectional stream** and sends one `WorldRequest`,
   framed exactly like a control message.
2. The server writes the raw `.hworld` bytes and finishes the stream. A world it
   does not have gets the stream reset instead.
3. The client reads at most `size` bytes, checks the length and the SHA-256
   against the `WorldAdded`, and only then unpacks it.

The hash check is the only thing standing between a client and a world it did
not ask for, and with D3 still open it does not authenticate the server — it
only ties the bytes to what that connection advertised. Clients cache by hash,
so the same world from two servers is downloaded once. Limits: a world is
1 byte to 256 MB, and a client may have one download stream open at a time.

Example: `Hello { version: 2, name: "Ada", ticket: "" }` is
`09 00 | 01 | 02 00 | 03 41 64 61 | 00 00`.

## Datagrams

| Type | Name | Direction | Body after the type byte |
|---|---|---|---|
| `0x10` | Pose | C→S | `Pose` (33 bytes) |
| `0x11` | PoseRelay | S→C | `from u16`, `Pose` |
| `0x20` | Voice | C→S | `seq u16`, Opus frame (the rest of the datagram, 1–1000 bytes) |
| `0x21` | VoiceRelay | S→C | `from u16`, `seq u16`, Opus frame |
| `0x30` | Item | C→S | `item u16`, `Transform` (10 bytes) |
| `0x31` | ItemRelay | S→C | `from u16`, `item u16`, `Transform` |

Clients only send `Pose`, `Voice` and `Item`. The server drops relay types that come from
clients, and relays only to players in the sender's instance.

An `Item` is dropped unless the sender is the item's holder, so nobody can move
something out of somebody else's hands.

Server limits per client:
- **Pose:** a token bucket at 1.5 × `tick_hz` per second with a burst of 10.
- **Voice:** 75 frames/s with a burst of 25.
- **Item:** 3 × `tick_hz` per second with a burst of 20 — two hands at the pose
  rate, with the same headroom for jitter.

- **Control stream:** 5 messages/s with a burst of 20. Over that, the
  connection is closed: walking through a portal is a human-speed action.

The server drops datagrams over those limits and clears unknown `flags` bits before relaying.

### Pose (33 bytes)

```
seq   u16        increments per pose sent, wraps
flags u8         bit0 = VR, bit1 = left hand tracked, bit2 = right hand tracked
head  Transform
left  Transform  sent even when the hand isn't tracked (check the flags)
right Transform
```

`Transform` (10 bytes) is `pos: 3 × i16` (x, y, z), then `rot: u32`.

- **Position:** metres × 512, rounded and clamped to i16. That gives ±64 m at about 2 mm precision,
  relative to the region origin (one region at the world origin for now). Godot axes:
  +Y up, metres.
- **Rotation, smallest-three:** normalize the quaternion `(x, y, z, w)`, find the
  component with the largest absolute value, and negate the whole quaternion if that
  component is negative. Then pack:
  - bits 31–30: index of the largest component (0 = x … 3 = w)
  - bits 29–20, 19–10, 9–0: the other three components in x, y, z, w order, each
    stored as `round(c / (1/√2) × 511) + 511`, so 511 means exactly 0
  - to decode: `c = (bits − 511) / 511 × (1/√2)`, the largest component is
    `sqrt(max(0, 1 − a² − b² − c²))`, then normalize.

  Identity `(0, 0, 0, 1)` encodes as `0xDFF7FDFF`. Worst-case error is under 0.5°.

Example (`pose_golden_bytes` test): seq `0x0102`, flags `VR|LEFT`, head pos
`(1, −2, 3)` with identity rotation, left pos `(−32768, 0, 32767)` with rot `0`, right pos
`(0, 0, 0)` with rot `0xFFFFFFFF`:

```
10 02 01 03
01 00 FE FF 03 00 FF FD F7 DF
00 80 00 00 FF 7F 00 00 00 00
00 00 00 00 00 00 FF FF FF FF
```

### Voice

Mono 48 kHz Opus, 20 ms frames. `seq` increments per frame and is used for
jitter buffering and spotting lost frames.

### Items

`Grab` is **always granted**, and it takes the item off whoever held it. The
server has never read the world, so it cannot know whether that world allows an
item to be taken from a hand: the rule lives in the world file (`locked`, see
`docs/WORLD_FORMAT.md`) and is enforced by the asking client, which simply
does not send the `Grab`. A patched client can therefore still take a locked
item. That is a deliberate limit, not an oversight — fixing it properly means
teaching the server to read world metadata.

`Release` is ignored unless the sender is the holder, so a `Release` that
crosses a `Grab` cannot drop the new holder's item.

Everyone in the instance gets a `Held` for every change, the asker included: a
grab is not real until the server says so. A client arriving in an instance is
sent one `Held` per item somebody there is carrying. Everything a player was
carrying becomes `by = 0` when they change instance or disconnect.

A client that has thrown something should keep sending `Item` for it and only
`Release` once it has come to rest, so that it lands in the same place for
everybody instead of each client simulating its own throw.

## Shooting (v4)

The server **never reads a world** (D3, D27), so it has no walls and cannot
raycast. Hit detection is therefore split: the shooting client traces the shot
and *claims* a hit, and the server owns every consequence. The reasoning is in
`docs/TRUST.md`.

| byte | message | direction | body |
|---|---|---|---|
| `0x32` | `Shot` | client → server | transform (6 + 4 bytes) |
| `0x33` | `ShotRelay` | server → others | `u16 from`, transform |
| `0x40` | `Hit` | client → server | `u16 target` |
| `0x41` | `Health` | server → all | `u16 player`, `u8 hp` |
| `0x42` | `Died` | server → all | `u16 player`, `u16 by` |
| `0x43` | `Spawned` | client ↔ server | `u16 player` |

`Shot` is a datagram: a lost tracer is only a lost tracer. `Hit` is on the
control stream, because a lost hit is a lost fight.

What the server checks before a `Hit` does anything:

* the target exists and is in the same instance
* neither of them is already dead
* the two last-known head positions are within 50 m, which is the weapon
  range plus slack for the half second between the shooter seeing somebody
  and the server hearing about it
* the shooter has not sent more than ten hits a second

What it cannot check is line of sight. A patched client can claim a hit
through a wall; closing that needs the server to hold a collision-only digest
of each world, which is a format change rather than a protocol one.

`Health` is the only thing that sets health, and nothing a client sends can
set its own. `Spawned` from a client means "let me back in" and is refused
unless the server already thinks they are dead; from the server it announces
that somebody is back. Respawn is asked for rather than timed so the server
stays event-driven, with no tick loop (D28), and there is nothing to gain by
asking early.

## Chat (v6)

| byte | message | direction | body |
|---|---|---|---|
| `0x60` | `Say` | client → server | `u8 len` + UTF-8 text |
| `0x61` | `Said` | server → client | `from u16`, `u8 len` + UTF-8 text |

A line is 1 to 240 bytes of UTF-8 with no control characters — so no
newlines, and nothing that could put an escape sequence in the Flame's log —
and not only whitespace. Anything else is a protocol error.

`Said` goes to everyone in the speaker's instance, **the speaker included**:
the echo is the receipt, and it puts every line in the same order for
everybody. Chat is per instance; there is no whisper and no server-wide
channel.

The Flame allows a line a second with a burst of five. Lines over that are
dropped, not punished — typing quickly is not abuse — and the Flame logs every
line it relays, with the speaker's name.

## Stored data (v4)

Somewhere for a creator to keep a score, a key, a door somebody opened, that a
patched client cannot rewrite.

| byte | message | direction | body |
|---|---|---|---|
| `0x50` | `Store` | client → server | key, `u16 len` + value |
| `0x51` | `Fetch` | client → server | key |
| `0x52` | `Value` | server → client | key, `u16 len` + value |

A key is a length-prefixed string of `a-z0-9-_`, at most 64 bytes, validated
the same way a slug is: it names a file, so nothing that could climb out of a
directory is accepted. A value is at most 1024 bytes, and an empty one
deletes the key.

The rules that make this safe to expose to anybody's world:

* **A client only ever writes its own data.** There is no field naming
  another player, so there is nothing to abuse.
* **The server picks the namespace**, from the world the player is standing
  in. The client never names it, so one creator cannot reach into another.
* **Quotas** on key length, value size and 256 keys per player, checked
  before anything touches the disk. Over a quota is answered with `Value`
  carrying what is really stored, so a refused write cannot leave a world
  believing itself.

`MAX_CONTROL_LEN` went from 256 to 1400 bytes in v4 to carry a full value.

Until accounts land, data is keyed on the **name typed at the connect panel**,
which anybody can claim. That is the hole accounts close; the store is built
now so the identity underneath it can change without a protocol break.
