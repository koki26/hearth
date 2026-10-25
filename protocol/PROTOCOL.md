# Hearth wire protocol — version 1

The reference implementation is `server/hearth-protocol/src/lib.rs`. The server
and the Godot client extension both use it. This document is the spec for anyone
writing another implementation, and it must stay in sync with that code.

## Transport

- QUIC (`quinn`), TLS 1.3. ALPN: `hearth/1`. TLS server name: `hearth`. The server
  certificate is self-signed (see docs/decisions.md D3).
- The server closes a connection that stays idle for 10 s. Clients send poses at
  `tick_hz`, which keeps the connection alive.
- **Control stream:** the client opens one bidirectional stream right after
  connecting. Every reliable message goes on it.
- **Datagrams:** pose and voice go in QUIC unreliable datagrams, one message per
  datagram.
- Leaving = closing the connection. There is no goodbye message.

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
| `0x01` | Hello | C→S | `version u16`, `name` |
| `0x02` | Welcome | S→C | `your_id u16`, `tick_hz u8` |
| `0x03` | Reject | S→C | `reason u8`: 1 = server full, 2 = bad version, 3 = bad name |
| `0x04` | PeerJoined | S→C | `id u16`, `name` |
| `0x05` | PeerLeft | S→C | `id u16` |

**Handshake:**
1. The client sends `Hello`.
2. The server replies with either:
   - `Reject`, then closes the connection, or
   - `Welcome`, then one `PeerJoined` for every player already in the room.
3. After that, the server sends `PeerJoined` / `PeerLeft` as players come and go.

Example: `Hello { version: 1, name: "Ada" }` is
`07 00 | 01 | 01 00 | 03 41 64 61`.

## Datagrams

| Type | Name | Direction | Body after the type byte |
|---|---|---|---|
| `0x10` | Pose | C→S | `Pose` (33 bytes) |
| `0x11` | PoseRelay | S→C | `from u16`, `Pose` |
| `0x20` | Voice | C→S | `seq u16`, Opus frame (the rest of the datagram, 1–1000 bytes) |
| `0x21` | VoiceRelay | S→C | `from u16`, `seq u16`, Opus frame |

Clients only send `Pose` and `Voice`. The server drops relay types that come from clients.

Server limits per client:
- **Pose:** a token bucket at 1.5 × `tick_hz` per second with a burst of 10.
- **Voice:** 75 frames/s with a burst of 25.

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
