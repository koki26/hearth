//! Hearth wire protocol. The byte-level spec is in /docs/PROTOCOL.md;
//! keep the two in sync.

use std::fmt;

pub const PROTOCOL_VERSION: u16 = 7;
/// Unchanged across protocol versions on purpose: a version mismatch should
/// reach the client as a `Reject`, not as an opaque TLS failure.
pub const ALPN: &[u8] = b"hearth/1";
pub const MAX_NAME_LEN: usize = 32;
/// A join ticket from the Fireplace (docs/TRUST.md). Printable ASCII; an
/// empty one means a guest.
pub const MAX_TICKET_LEN: usize = 1024;
/// World slugs are file stems in the server's repository, so they stay short
/// and filesystem-safe.
pub const MAX_SLUG_LEN: usize = 32;
/// A packed world a server may hand out. Also the client's download cap.
pub const MAX_WORLD_SIZE: u32 = 256 * 1024 * 1024;
/// Upper bound for a control message body; current messages are < 40 bytes.
pub const MAX_CONTROL_LEN: usize = 1400;
/// A key in a player's store. Short on purpose: it is a name, not a path.
pub const MAX_KEY_LEN: usize = 64;
/// One stored value. Small on purpose too -- somewhere to keep a score or a
/// door's state, not a place to upload a file.
pub const MAX_VALUE_LEN: usize = 1024;
/// Health is a byte, and this is full.
pub const MAX_HEALTH: u8 = 100;
/// Keeps a voice datagram under the 1200-byte QUIC minimum path MTU.
pub const MAX_VOICE_FRAME: usize = 1000;
/// One line of chat, in bytes of UTF-8. Long enough for a sentence, short
/// enough that a wall of text cannot cover somebody's view.
pub const MAX_CHAT_LEN: usize = 240;
/// Position units per metre: i16 covers ±64 m at ~2 mm resolution.
pub const POS_SCALE: f32 = 512.0;

pub mod flags {
    pub const VR: u8 = 1 << 0;
    pub const LEFT_HAND: u8 = 1 << 1;
    pub const RIGHT_HAND: u8 = 1 << 2;
}

/// Who an ember (one running instance) is open to. The client picks one when
/// it starts an ember, and the server carries it and repeats it to everybody.
///
/// shortcut: nothing enforces anything but `PUBLIC` -- there are no accounts
/// yet, so the server cannot tell whose friend anybody is (docs/TRUST.md).
/// Upgrade when the Fireplace issues accounts and a friend list.
pub mod ember {
    pub const PUBLIC: u8 = 0;
    pub const FRIENDS_PLUS: u8 = 1;
    pub const FRIENDS: u8 = 2;
    pub const INVITE: u8 = 3;
    /// How many kinds there are; anything this high or higher is a bad kind.
    pub const KINDS: u8 = 4;

    pub fn label(kind: u8) -> &'static str {
        match kind {
            FRIENDS_PLUS => "friends+",
            FRIENDS => "friends",
            INVITE => "invite",
            _ => "public",
        }
    }
}

/// `Welcome.flags`: what this server lets you do.
pub mod welcome {
    /// Players may start their own instance of a world in the repository.
    pub const CLIENT_INSTANCING: u8 = 1 << 0;
    /// You joined with an account, so the server keeps what worlds store for
    /// you. A guest has this clear, and every `Store` is ignored.
    pub const SAVES: u8 = 1 << 1;
}

/// The hub instance. Always running, cannot be stopped, everyone starts here.
pub const HUB_INSTANCE: u16 = 0;

/// `Held.by` for an item nobody is holding. Player ids start at 1, so zero is
/// free to mean "on the ground".
pub const NOBODY: u16 = 0;

/// An item is a grabbable object, named by its index in the world's `objects`
/// list. A world is capped at 5000 objects, so the index fits a u16 with room
/// to spare, and the server never has to know what a world contains.
pub const MAX_ITEM: u16 = 5000;

mod ty {
    pub const HELLO: u8 = 0x01;
    pub const WELCOME: u8 = 0x02;
    pub const REJECT: u8 = 0x03;
    pub const PEER_JOINED: u8 = 0x04;
    pub const PEER_LEFT: u8 = 0x05;
    pub const WORLD_ADDED: u8 = 0x06;
    pub const INSTANCE_ADDED: u8 = 0x07;
    pub const INSTANCE_CLOSED: u8 = 0x08;
    pub const INSTANCE_COUNT: u8 = 0x09;
    pub const JOIN_INSTANCE: u8 = 0x0A;
    pub const CREATE_INSTANCE: u8 = 0x0B;
    pub const MOVED: u8 = 0x0C;
    pub const WORLD_REQUEST: u8 = 0x0D;
    pub const GRAB: u8 = 0x0E;
    pub const RELEASE: u8 = 0x0F;
    pub const HELD: u8 = 0x1A;
    pub const POSE: u8 = 0x10;
    pub const POSE_RELAY: u8 = 0x11;
    pub const VOICE: u8 = 0x20;
    pub const VOICE_RELAY: u8 = 0x21;
    pub const ITEM: u8 = 0x30;
    pub const ITEM_RELAY: u8 = 0x31;
    pub const SHOT: u8 = 0x32;
    pub const SHOT_RELAY: u8 = 0x33;
    pub const HIT: u8 = 0x40;
    pub const HEALTH: u8 = 0x41;
    pub const DIED: u8 = 0x42;
    pub const SPAWNED: u8 = 0x43;
    pub const STORE: u8 = 0x50;
    pub const FETCH: u8 = 0x51;
    pub const VALUE: u8 = 0x52;
    pub const SAY: u8 = 0x60;
    pub const SAID: u8 = 0x61;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    TrailingBytes,
    UnknownType(u8),
    TooLong,
    BadName,
    BadSlug,
    BadWorldSize,
    BadVoiceFrame,
    BadRejectReason(u8),
    BadItem,
    BadKey,
    BadValue,
    BadEmberKind(u8),
    BadChat,
    BadTicket,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("message truncated"),
            Self::TrailingBytes => f.write_str("trailing bytes after message"),
            Self::UnknownType(t) => write!(f, "unknown message type 0x{t:02x}"),
            Self::TooLong => f.write_str("message too long"),
            Self::BadName => f.write_str("invalid display name"),
            Self::BadSlug => f.write_str("invalid world name"),
            Self::BadWorldSize => f.write_str("world is empty or too large"),
            Self::BadVoiceFrame => f.write_str("voice frame empty or too large"),
            Self::BadRejectReason(r) => write!(f, "unknown reject reason {r}"),
            Self::BadItem => f.write_str("item index out of range"),
            Self::BadKey => f.write_str("not a usable store key"),
            Self::BadValue => f.write_str("stored value too large"),
            Self::BadEmberKind(k) => write!(f, "unknown ember kind {k}"),
            Self::BadChat => f.write_str("chat line empty, too long or with control characters"),
            Self::BadTicket => f.write_str("join ticket too long or not printable"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn quantize_pos(metres: f32) -> i16 {
    // `as` saturates out-of-range values and maps NaN to 0.
    (metres * POS_SCALE).round() as i16
}

pub fn dequantize_pos(q: i16) -> f32 {
    q as f32 / POS_SCALE
}

const QMAX: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Smallest-three: 2-bit index of the dropped (largest) component, then the
/// other three in x,y,z,w order as 10 bits each, mapped so 511 is exactly 0.
pub fn quantize_rot(q: [f32; 4]) -> u32 {
    let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    let q = if len.is_finite() && len > 1e-6 { q.map(|c| c / len) } else { [0.0, 0.0, 0.0, 1.0] };
    let mut big = 0;
    for i in 1..4 {
        if q[i].abs() > q[big].abs() {
            big = i;
        }
    }
    // q and -q are the same rotation; make the dropped component positive.
    let sign = if q[big] < 0.0 { -1.0 } else { 1.0 };
    let mut packed = (big as u32) << 30;
    for (n, i) in (0..4).filter(|&i| i != big).enumerate() {
        let v = ((sign * q[i] / QMAX).clamp(-1.0, 1.0) * 511.0).round() as i32 + 511;
        packed |= (v as u32) << (20 - 10 * n);
    }
    packed
}

/// Always returns a unit quaternion, whatever the input bits.
pub fn dequantize_rot(packed: u32) -> [f32; 4] {
    let big = (packed >> 30) as usize;
    let mut q = [0.0f32; 4];
    let mut sum = 0.0;
    for (n, i) in (0..4).filter(|&i| i != big).enumerate() {
        let bits = (packed >> (20 - 10 * n)) & 0x3FF;
        q[i] = (bits as f32 - 511.0) / 511.0 * QMAX;
        sum += q[i] * q[i];
    }
    q[big] = (1.0 - sum).max(0.0).sqrt();
    let len = q.iter().map(|c| c * c).sum::<f32>().sqrt();
    q.map(|c| c / len)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transform {
    pub pos: [i16; 3],
    pub rot: u32,
}

impl Transform {
    pub fn from_f32(pos: [f32; 3], rot: [f32; 4]) -> Self {
        Self { pos: pos.map(quantize_pos), rot: quantize_rot(rot) }
    }
    pub fn pos_f32(&self) -> [f32; 3] {
        self.pos.map(dequantize_pos)
    }
    pub fn rot_f32(&self) -> [f32; 4] {
        dequantize_rot(self.rot)
    }
}

/// All three transforms are always sent; `flags` says which hands are tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pose {
    pub seq: u16,
    pub flags: u8,
    pub head: Transform,
    pub left: Transform,
    pub right: Transform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub id: u16,
    pub name: String,
}

/// One packed world in a server's repository. The server never looks inside;
/// `sha256` is over the file bytes and the client verifies it after download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    pub slug: String,
    pub size: u32,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    ServerFull = 1,
    BadVersion = 2,
    BadName = 3,
    /// This server only lets in people signed in to an account.
    AccountRequired = 4,
    /// The join ticket did not check out, or the server could not check it.
    BadTicket = 5,
}

/// A rejected join carries its reason twice: as a `Reject` message, and in the
/// QUIC close frame as this prefix plus the reason byte. The connection closes
/// right after the message, and a client can be told about the close before it
/// has read the bytes off the stream, which would leave the player with
/// "disconnected" instead of "the server is full".
pub const REJECT_CLOSE_PREFIX: &[u8] = b"rejected:";

impl RejectReason {
    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::ServerFull),
            2 => Some(Self::BadVersion),
            3 => Some(Self::BadName),
            4 => Some(Self::AccountRequired),
            5 => Some(Self::BadTicket),
            _ => None,
        }
    }

    pub fn close_frame(self) -> Vec<u8> {
        [REJECT_CLOSE_PREFIX, &[self as u8]].concat()
    }

    pub fn from_close_frame(frame: &[u8]) -> Option<Self> {
        let code = frame.strip_prefix(REJECT_CLOSE_PREFIX)?;
        Self::from_code(*code.first()?)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Hello {
        version: u16,
        name: String,
        /// From the Fireplace, for this server only; empty for a guest.
        ticket: String,
    },
    /// `instance` is where you were put (always the hub); `flags` is `welcome::*`.
    Welcome {
        your_id: u16,
        tick_hz: u8,
        instance: u16,
        flags: u8,
    },
    Reject {
        reason: RejectReason,
    },
    PeerJoined(Peer),
    PeerLeft {
        id: u16,
    },
    /// A world the server will hand out, one message per world.
    WorldAdded(World),
    /// A running instance -- an ember -- one message per instance. `name` is
    /// what its maker called it and `kind` is `ember::*`.
    InstanceAdded {
        instance: u16,
        slug: String,
        players: u16,
        name: String,
        kind: u8,
    },
    InstanceClosed {
        instance: u16,
    },
    InstanceCount {
        instance: u16,
        players: u16,
    },
    JoinInstance {
        instance: u16,
    },
    /// Start an ember of `slug`, called `name` and open to `kind`.
    CreateInstance {
        slug: String,
        name: String,
        kind: u8,
    },
    /// The one answer to `JoinInstance` and `CreateInstance`: forget every
    /// peer, a `PeerJoined` follows for each one there. Naming the instance
    /// the asker is already in means the request failed, which keeps a
    /// refusal off the fatal `Reject` path.
    Moved {
        instance: u16,
    },
    /// Sent on a stream of its own; the reply is the raw `.hworld` bytes.
    WorldRequest {
        slug: String,
    },
    /// Take hold of an item. Always granted, and it takes the item off
    /// whoever held it: the server has never read the world, so it cannot
    /// know which items a world says may be taken.
    /// shortcut: the "no stealing" flag is enforced by the asking client, so a
    /// patched client can still take a locked item. Upgrade when the server
    /// reads world metadata, or when griefing actually happens.
    Grab {
        item: u16,
    },
    /// Let go. Ignored unless you are the holder.
    Release {
        item: u16,
    },
    /// Who holds an item now; `by` is `NOBODY` when it is on the ground.
    /// Sent to everyone in the instance, and once per held item on arrival.
    Held {
        item: u16,
        by: u16,
    },

    // --- shooting (docs/TRUST.md) ---
    /// "I hit them." A claim, not a fact: the shooting client traced the shot,
    /// because the server never reads a world and so cannot raycast. The
    /// server checks what it can -- same instance, alive, in range, not firing
    /// too fast -- and owns every consequence.
    Hit {
        target: u16,
    },
    /// The only thing that sets health. Server to everyone.
    Health {
        player: u16,
        hp: u8,
    },
    /// `by` is `NOBODY` when nobody killed them: a fall, a kill plane.
    Died {
        player: u16,
        by: u16,
    },
    Spawned {
        player: u16,
    },

    // --- a player's own stored data (docs/TRUST.md) ---
    /// Keep this against my name, in the namespace of the world I am in. A
    /// client can only ever write its own data; there is no way to address
    /// anybody else. An empty value deletes the key.
    Store {
        key: String,
        value: Vec<u8>,
    },
    Fetch {
        key: String,
    },
    /// The answer to `Fetch`. An empty value means nothing is stored there.
    Value {
        key: String,
        value: Vec<u8>,
    },

    // --- chat ---
    /// A line of chat, to everyone in the sender's instance.
    Say {
        text: String,
    },
    /// A line somebody said, sender included: the echo is how the sender
    /// knows it went out, and puts every line in the same order for all.
    Said {
        from: u16,
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datagram {
    Pose(Pose),
    PoseRelay {
        from: u16,
        pose: Pose,
    },
    Voice {
        seq: u16,
        frame: Vec<u8>,
    },
    VoiceRelay {
        from: u16,
        seq: u16,
        frame: Vec<u8>,
    },
    /// Where a held item is now, from its holder. Dropped by the server
    /// unless the sender really is the holder.
    Item {
        item: u16,
        at: Transform,
    },
    ItemRelay {
        from: u16,
        item: u16,
        at: Transform,
    },
    /// A shot leaving a muzzle: position is where from, rotation is which way.
    /// Unreliable, because a lost tracer is only a lost tracer -- the hit it
    /// may or may not cause travels separately, and reliably.
    Shot {
        at: Transform,
    },
    ShotRelay {
        from: u16,
        at: Transform,
    },
}

pub fn validate_name(name: &str) -> Result<(), DecodeError> {
    if name.is_empty() || name.len() > MAX_NAME_LEN || name.chars().any(char::is_control) {
        return Err(DecodeError::BadName);
    }
    Ok(())
}

/// A ticket is opaque to everything but the Flame checking it; this only
/// keeps it printable and bounded.
pub fn validate_ticket(ticket: &str) -> Result<(), DecodeError> {
    if ticket.len() > MAX_TICKET_LEN || !ticket.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(DecodeError::BadTicket);
    }
    Ok(())
}

/// A chat line: some text, no control characters (so no newlines and no
/// terminal escapes in the Flame's log), and not more than `MAX_CHAT_LEN`.
pub fn validate_chat(text: &str) -> Result<(), DecodeError> {
    if text.trim().is_empty() || text.len() > MAX_CHAT_LEN || text.chars().any(char::is_control) {
        return Err(DecodeError::BadChat);
    }
    Ok(())
}

/// A key names a file under `data/players/`, so it is as narrow as a slug: no
/// dots, no separators, nothing that could climb out of the directory.
pub fn validate_key(key: &str) -> Result<(), DecodeError> {
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    if key.is_empty() || key.len() > MAX_KEY_LEN || !key.chars().all(ok) {
        return Err(DecodeError::BadKey);
    }
    Ok(())
}

/// A slug names a file in the server's world directory, so it is deliberately
/// narrow: no dots, no separators, nothing that could escape the directory.
pub fn validate_slug(slug: &str) -> Result<(), DecodeError> {
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    if slug.is_empty() || slug.len() > MAX_SLUG_LEN || !slug.chars().all(ok) {
        return Err(DecodeError::BadSlug);
    }
    Ok(())
}

impl Control {
    /// Appends one framed message (`u16 body length`, then body) to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0, 0]);
        match self {
            Self::Hello { version, name, ticket } => {
                out.push(ty::HELLO);
                out.extend_from_slice(&version.to_le_bytes());
                put_name(out, name);
                debug_assert!(validate_ticket(ticket).is_ok(), "encoding invalid ticket");
                out.extend_from_slice(&(ticket.len() as u16).to_le_bytes());
                out.extend_from_slice(ticket.as_bytes());
            }
            Self::Welcome { your_id, tick_hz, instance, flags } => {
                out.push(ty::WELCOME);
                out.extend_from_slice(&your_id.to_le_bytes());
                out.push(*tick_hz);
                out.extend_from_slice(&instance.to_le_bytes());
                out.push(*flags);
            }
            Self::Reject { reason } => {
                out.push(ty::REJECT);
                out.push(*reason as u8);
            }
            Self::PeerJoined(peer) => {
                out.push(ty::PEER_JOINED);
                out.extend_from_slice(&peer.id.to_le_bytes());
                put_name(out, &peer.name);
            }
            Self::PeerLeft { id } => {
                out.push(ty::PEER_LEFT);
                out.extend_from_slice(&id.to_le_bytes());
            }
            Self::WorldAdded(world) => {
                out.push(ty::WORLD_ADDED);
                put_slug(out, &world.slug);
                out.extend_from_slice(&world.size.to_le_bytes());
                out.extend_from_slice(&world.sha256);
            }
            Self::InstanceAdded { instance, slug, players, name, kind } => {
                out.push(ty::INSTANCE_ADDED);
                out.extend_from_slice(&instance.to_le_bytes());
                put_slug(out, slug);
                out.extend_from_slice(&players.to_le_bytes());
                put_name(out, name);
                out.push(*kind);
            }
            Self::InstanceClosed { instance } => {
                out.push(ty::INSTANCE_CLOSED);
                out.extend_from_slice(&instance.to_le_bytes());
            }
            Self::InstanceCount { instance, players } => {
                out.push(ty::INSTANCE_COUNT);
                out.extend_from_slice(&instance.to_le_bytes());
                out.extend_from_slice(&players.to_le_bytes());
            }
            Self::JoinInstance { instance } => {
                out.push(ty::JOIN_INSTANCE);
                out.extend_from_slice(&instance.to_le_bytes());
            }
            Self::CreateInstance { slug, name, kind } => {
                out.push(ty::CREATE_INSTANCE);
                put_slug(out, slug);
                put_name(out, name);
                out.push(*kind);
            }
            Self::Moved { instance } => {
                out.push(ty::MOVED);
                out.extend_from_slice(&instance.to_le_bytes());
            }
            Self::WorldRequest { slug } => {
                out.push(ty::WORLD_REQUEST);
                put_slug(out, slug);
            }
            Self::Grab { item } => {
                out.push(ty::GRAB);
                out.extend_from_slice(&item.to_le_bytes());
            }
            Self::Release { item } => {
                out.push(ty::RELEASE);
                out.extend_from_slice(&item.to_le_bytes());
            }
            Self::Held { item, by } => {
                out.push(ty::HELD);
                out.extend_from_slice(&item.to_le_bytes());
                out.extend_from_slice(&by.to_le_bytes());
            }
            Self::Hit { target } => {
                out.push(ty::HIT);
                out.extend_from_slice(&target.to_le_bytes());
            }
            Self::Health { player, hp } => {
                out.push(ty::HEALTH);
                out.extend_from_slice(&player.to_le_bytes());
                out.push(*hp);
            }
            Self::Died { player, by } => {
                out.push(ty::DIED);
                out.extend_from_slice(&player.to_le_bytes());
                out.extend_from_slice(&by.to_le_bytes());
            }
            Self::Spawned { player } => {
                out.push(ty::SPAWNED);
                out.extend_from_slice(&player.to_le_bytes());
            }
            Self::Store { key, value } => {
                out.push(ty::STORE);
                put_key(out, key);
                put_value(out, value);
            }
            Self::Fetch { key } => {
                out.push(ty::FETCH);
                put_key(out, key);
            }
            Self::Value { key, value } => {
                out.push(ty::VALUE);
                put_key(out, key);
                put_value(out, value);
            }
            Self::Say { text } => {
                out.push(ty::SAY);
                put_chat(out, text);
            }
            Self::Said { from, text } => {
                out.push(ty::SAID);
                out.extend_from_slice(&from.to_le_bytes());
                put_chat(out, text);
            }
        }
        let len = (out.len() - start - 2) as u16;
        out[start..start + 2].copy_from_slice(&len.to_le_bytes());
    }

    /// Rejects an oversized frame before its body is read.
    pub fn body_len(prefix: [u8; 2]) -> Result<usize, DecodeError> {
        let len = u16::from_le_bytes(prefix) as usize;
        if len > MAX_CONTROL_LEN {
            return Err(DecodeError::TooLong);
        }
        Ok(len)
    }

    pub fn decode(body: &[u8]) -> Result<Self, DecodeError> {
        if body.len() > MAX_CONTROL_LEN {
            return Err(DecodeError::TooLong);
        }
        let mut r = Reader(body);
        let msg = match r.u8()? {
            // An older client's Hello ends at the name; it still decodes, so
            // it can be told `BadVersion` rather than dropped as garbage.
            ty::HELLO => Self::Hello {
                version: r.u16()?,
                name: r.name()?,
                ticket: if r.0.is_empty() { String::new() } else { r.ticket()? },
            },
            ty::WELCOME => {
                Self::Welcome { your_id: r.u16()?, tick_hz: r.u8()?, instance: r.u16()?, flags: r.u8()? }
            }
            ty::REJECT => {
                let code = r.u8()?;
                Self::Reject {
                    reason: RejectReason::from_code(code).ok_or(DecodeError::BadRejectReason(code))?,
                }
            }
            ty::PEER_JOINED => Self::PeerJoined(Peer { id: r.u16()?, name: r.name()? }),
            ty::PEER_LEFT => Self::PeerLeft { id: r.u16()? },
            ty::WORLD_ADDED => {
                Self::WorldAdded(World { slug: r.slug()?, size: r.world_size()?, sha256: r.take::<32>()? })
            }
            ty::INSTANCE_ADDED => Self::InstanceAdded {
                instance: r.u16()?,
                slug: r.slug()?,
                players: r.u16()?,
                name: r.name()?,
                kind: r.ember_kind()?,
            },
            ty::INSTANCE_CLOSED => Self::InstanceClosed { instance: r.u16()? },
            ty::INSTANCE_COUNT => Self::InstanceCount { instance: r.u16()?, players: r.u16()? },
            ty::JOIN_INSTANCE => Self::JoinInstance { instance: r.u16()? },
            ty::CREATE_INSTANCE => {
                Self::CreateInstance { slug: r.slug()?, name: r.name()?, kind: r.ember_kind()? }
            }
            ty::MOVED => Self::Moved { instance: r.u16()? },
            ty::WORLD_REQUEST => Self::WorldRequest { slug: r.slug()? },
            ty::GRAB => Self::Grab { item: r.item()? },
            ty::RELEASE => Self::Release { item: r.item()? },
            ty::HELD => Self::Held { item: r.item()?, by: r.u16()? },
            ty::HIT => Self::Hit { target: r.u16()? },
            ty::HEALTH => Self::Health { player: r.u16()?, hp: r.u8()? },
            ty::DIED => Self::Died { player: r.u16()?, by: r.u16()? },
            ty::SPAWNED => Self::Spawned { player: r.u16()? },
            ty::STORE => Self::Store { key: r.key()?, value: r.value()? },
            ty::FETCH => Self::Fetch { key: r.key()? },
            ty::VALUE => Self::Value { key: r.key()?, value: r.value()? },
            ty::SAY => Self::Say { text: r.chat()? },
            ty::SAID => Self::Said { from: r.u16()?, text: r.chat()? },
            other => return Err(DecodeError::UnknownType(other)),
        };
        r.finish()?;
        Ok(msg)
    }
}

impl Datagram {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(40);
        match self {
            Self::Pose(pose) => {
                out.push(ty::POSE);
                put_pose(&mut out, pose);
            }
            Self::PoseRelay { from, pose } => {
                out.push(ty::POSE_RELAY);
                out.extend_from_slice(&from.to_le_bytes());
                put_pose(&mut out, pose);
            }
            Self::Voice { seq, frame } => {
                out.push(ty::VOICE);
                out.extend_from_slice(&seq.to_le_bytes());
                out.extend_from_slice(frame);
            }
            Self::VoiceRelay { from, seq, frame } => {
                out.push(ty::VOICE_RELAY);
                out.extend_from_slice(&from.to_le_bytes());
                out.extend_from_slice(&seq.to_le_bytes());
                out.extend_from_slice(frame);
            }
            Self::Item { item, at } => {
                out.push(ty::ITEM);
                out.extend_from_slice(&item.to_le_bytes());
                put_transform(&mut out, at);
            }
            Self::ItemRelay { from, item, at } => {
                out.push(ty::ITEM_RELAY);
                out.extend_from_slice(&from.to_le_bytes());
                out.extend_from_slice(&item.to_le_bytes());
                put_transform(&mut out, at);
            }
            Self::Shot { at } => {
                out.push(ty::SHOT);
                put_transform(&mut out, at);
            }
            Self::ShotRelay { from, at } => {
                out.push(ty::SHOT_RELAY);
                out.extend_from_slice(&from.to_le_bytes());
                put_transform(&mut out, at);
            }
        }
        out
    }

    pub fn decode(buf: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader(buf);
        let msg = match r.u8()? {
            ty::POSE => Self::Pose(r.pose()?),
            ty::POSE_RELAY => Self::PoseRelay { from: r.u16()?, pose: r.pose()? },
            ty::VOICE => Self::Voice { seq: r.u16()?, frame: r.voice_frame()? },
            ty::VOICE_RELAY => Self::VoiceRelay { from: r.u16()?, seq: r.u16()?, frame: r.voice_frame()? },
            ty::ITEM => Self::Item { item: r.item()?, at: r.transform()? },
            ty::ITEM_RELAY => Self::ItemRelay { from: r.u16()?, item: r.item()?, at: r.transform()? },
            ty::SHOT => Self::Shot { at: r.transform()? },
            ty::SHOT_RELAY => Self::ShotRelay { from: r.u16()?, at: r.transform()? },
            other => return Err(DecodeError::UnknownType(other)),
        };
        r.finish()?;
        Ok(msg)
    }
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    debug_assert!(validate_name(name).is_ok(), "encoding invalid name {name:?}");
    out.push(name.len() as u8);
    out.extend_from_slice(name.as_bytes());
}

fn put_key(out: &mut Vec<u8>, key: &str) {
    debug_assert!(validate_key(key).is_ok(), "encoding invalid key {key:?}");
    out.push(key.len() as u8);
    out.extend_from_slice(key.as_bytes());
}
fn put_value(out: &mut Vec<u8>, value: &[u8]) {
    debug_assert!(value.len() <= MAX_VALUE_LEN, "encoding oversized value");
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value);
}
fn put_chat(out: &mut Vec<u8>, text: &str) {
    debug_assert!(validate_chat(text).is_ok(), "encoding invalid chat {text:?}");
    out.push(text.len() as u8);
    out.extend_from_slice(text.as_bytes());
}
fn put_slug(out: &mut Vec<u8>, slug: &str) {
    debug_assert!(validate_slug(slug).is_ok(), "encoding invalid slug {slug:?}");
    out.push(slug.len() as u8);
    out.extend_from_slice(slug.as_bytes());
}

fn put_transform(out: &mut Vec<u8>, t: &Transform) {
    for p in t.pos {
        out.extend_from_slice(&p.to_le_bytes());
    }
    out.extend_from_slice(&t.rot.to_le_bytes());
}

fn put_pose(out: &mut Vec<u8>, pose: &Pose) {
    out.extend_from_slice(&pose.seq.to_le_bytes());
    out.push(pose.flags);
    put_transform(out, &pose.head);
    put_transform(out, &pose.left);
    put_transform(out, &pose.right);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (head, rest) = self.0.split_first_chunk::<N>().ok_or(DecodeError::Truncated)?;
        self.0 = rest;
        Ok(*head)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        self.take().map(u16::from_le_bytes)
    }
    fn i16(&mut self) -> Result<i16, DecodeError> {
        self.take().map(i16::from_le_bytes)
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        self.take().map(u32::from_le_bytes)
    }
    fn name(&mut self) -> Result<String, DecodeError> {
        let len = self.u8()? as usize;
        if self.0.len() < len {
            return Err(DecodeError::Truncated);
        }
        let (bytes, rest) = self.0.split_at(len);
        self.0 = rest;
        let name = std::str::from_utf8(bytes).map_err(|_| DecodeError::BadName)?;
        validate_name(name)?;
        Ok(name.to_owned())
    }
    fn slug(&mut self) -> Result<String, DecodeError> {
        let len = self.u8()? as usize;
        if self.0.len() < len {
            return Err(DecodeError::Truncated);
        }
        let (bytes, rest) = self.0.split_at(len);
        self.0 = rest;
        let slug = std::str::from_utf8(bytes).map_err(|_| DecodeError::BadSlug)?;
        validate_slug(slug)?;
        Ok(slug.to_owned())
    }
    fn world_size(&mut self) -> Result<u32, DecodeError> {
        let size = self.u32()?;
        if size == 0 || size > MAX_WORLD_SIZE {
            return Err(DecodeError::BadWorldSize);
        }
        Ok(size)
    }
    fn ember_kind(&mut self) -> Result<u8, DecodeError> {
        let kind = self.u8()?;
        if kind >= ember::KINDS {
            return Err(DecodeError::BadEmberKind(kind));
        }
        Ok(kind)
    }
    fn item(&mut self) -> Result<u16, DecodeError> {
        let item = self.u16()?;
        if item >= MAX_ITEM {
            return Err(DecodeError::BadItem);
        }
        Ok(item)
    }
    fn key(&mut self) -> Result<String, DecodeError> {
        let len = self.u8()? as usize;
        if self.0.len() < len {
            return Err(DecodeError::Truncated);
        }
        let (bytes, rest) = self.0.split_at(len);
        self.0 = rest;
        let key = std::str::from_utf8(bytes).map_err(|_| DecodeError::BadKey)?;
        validate_key(key)?;
        Ok(key.to_owned())
    }
    fn chat(&mut self) -> Result<String, DecodeError> {
        let len = self.u8()? as usize;
        if self.0.len() < len {
            return Err(DecodeError::Truncated);
        }
        let (bytes, rest) = self.0.split_at(len);
        self.0 = rest;
        let text = std::str::from_utf8(bytes).map_err(|_| DecodeError::BadChat)?;
        validate_chat(text)?;
        Ok(text.to_owned())
    }
    fn ticket(&mut self) -> Result<String, DecodeError> {
        let len = self.u16()? as usize;
        if self.0.len() < len {
            return Err(DecodeError::Truncated);
        }
        let (bytes, rest) = self.0.split_at(len);
        self.0 = rest;
        let ticket = std::str::from_utf8(bytes).map_err(|_| DecodeError::BadTicket)?;
        validate_ticket(ticket)?;
        Ok(ticket.to_owned())
    }
    fn value(&mut self) -> Result<Vec<u8>, DecodeError> {
        let len = self.u16()? as usize;
        if len > MAX_VALUE_LEN {
            return Err(DecodeError::BadValue);
        }
        if self.0.len() < len {
            return Err(DecodeError::Truncated);
        }
        let (bytes, rest) = self.0.split_at(len);
        self.0 = rest;
        Ok(bytes.to_vec())
    }
    fn transform(&mut self) -> Result<Transform, DecodeError> {
        Ok(Transform { pos: [self.i16()?, self.i16()?, self.i16()?], rot: self.u32()? })
    }
    fn pose(&mut self) -> Result<Pose, DecodeError> {
        Ok(Pose {
            seq: self.u16()?,
            flags: self.u8()?,
            head: self.transform()?,
            left: self.transform()?,
            right: self.transform()?,
        })
    }
    fn voice_frame(&mut self) -> Result<Vec<u8>, DecodeError> {
        let frame = std::mem::take(&mut self.0);
        if frame.is_empty() || frame.len() > MAX_VOICE_FRAME {
            return Err(DecodeError::BadVoiceFrame);
        }
        Ok(frame.to_vec())
    }
    fn finish(self) -> Result<(), DecodeError> {
        if self.0.is_empty() { Ok(()) } else { Err(DecodeError::TrailingBytes) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic xorshift, so no `rand` dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn f32(&mut self) -> f32 {
            (self.next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
        }
    }

    fn sample_pose() -> Pose {
        Pose {
            seq: 0x0102,
            flags: flags::VR | flags::LEFT_HAND,
            head: Transform { pos: [1, -2, 3], rot: 0xDFF7FDFF },
            left: Transform { pos: [i16::MIN, 0, i16::MAX], rot: 0 },
            right: Transform { pos: [0, 0, 0], rot: u32::MAX },
        }
    }

    #[test]
    fn chat_round_trips_and_refuses_junk() {
        for message in [
            Control::Say { text: "hello".into() },
            Control::Said { from: 4, text: "ahoj, jak se máš? 🔥".into() },
            Control::Said { from: 1, text: "x".repeat(MAX_CHAT_LEN) },
        ] {
            let mut out = Vec::new();
            message.encode(&mut out);
            assert_eq!(Control::decode(&out[2..]).unwrap(), message);
        }
        assert_eq!(validate_chat("   "), Err(DecodeError::BadChat));
        assert_eq!(validate_chat("two\nlines"), Err(DecodeError::BadChat));
        assert_eq!(validate_chat(&"x".repeat(MAX_CHAT_LEN + 1)), Err(DecodeError::BadChat));
        // Hand-built, since `encode` refuses to write it: a newline on the wire.
        assert_eq!(Control::decode(&[ty::SAY, 3, b'a', b'\n', b'b']), Err(DecodeError::BadChat));
    }

    #[test]
    fn shooting_messages_round_trip() {
        let at = Transform { pos: [12, -4, 900], rot: 0x1234_5678 };
        datagram_round_trip(Datagram::Shot { at });
        datagram_round_trip(Datagram::ShotRelay { from: 7, at });
        for message in [
            Control::Hit { target: 3 },
            Control::Health { player: 3, hp: MAX_HEALTH },
            Control::Health { player: 3, hp: 0 },
            Control::Died { player: 3, by: 9 },
            Control::Died { player: 3, by: NOBODY },
            Control::Spawned { player: 3 },
        ] {
            let mut out = Vec::new();
            message.encode(&mut out);
            assert_eq!(Control::decode(&out[2..]).unwrap(), message);
        }
    }

    #[test]
    fn stored_values_round_trip_and_reject_what_they_should() {
        for message in [
            Control::Store { key: "score".into(), value: vec![1, 2, 3] },
            // An empty value is a delete, so it has to survive the trip.
            Control::Store { key: "score".into(), value: Vec::new() },
            Control::Fetch { key: "a_long_key_with_digits_0123456789".into() },
            Control::Value { key: "score".into(), value: vec![0xFF; MAX_VALUE_LEN] },
        ] {
            let mut out = Vec::new();
            message.encode(&mut out);
            assert_eq!(Control::decode(&out[2..]).unwrap(), message, "round trip");
        }

        // A key names a file, so anything that could climb out of a directory is
        // refused at the edge rather than sanitised later.
        for bad in ["", "../etc", "has/slash", "Has-Caps", "has.dot", &"x".repeat(MAX_KEY_LEN + 1)] {
            assert!(validate_key(bad).is_err(), "{bad:?} should not be a key");
        }
        for good in ["a", "score", "door_7", "a-b-c"] {
            assert!(validate_key(good).is_ok(), "{good:?} should be a key");
        }

        // An oversized value is refused on the way in, not truncated.
        let mut out = Vec::new();
        out.push(ty::VALUE);
        put_key(&mut out, "score");
        out.extend_from_slice(&((MAX_VALUE_LEN + 1) as u16).to_le_bytes());
        out.extend_from_slice(&vec![0u8; MAX_VALUE_LEN + 1]);
        assert_eq!(Control::decode(&out), Err(DecodeError::BadValue));
    }

    fn datagram_round_trip(msg: Datagram) {
        assert_eq!(Datagram::decode(&msg.encode()).unwrap(), msg);
    }

    fn control_round_trip(msg: Control) {
        let mut buf = Vec::new();
        msg.encode(&mut buf);
        let len = Control::body_len([buf[0], buf[1]]).unwrap();
        assert_eq!(len, buf.len() - 2);
        assert_eq!(Control::decode(&buf[2..]).unwrap(), msg);
    }

    #[test]
    fn control_messages_round_trip() {
        control_round_trip(Control::Hello { version: PROTOCOL_VERSION, name: "Ada".into(), ticket: String::new() });
        control_round_trip(Control::Hello { version: 7, name: "ž".repeat(16), ticket: "v1.abc.def".into() }); // 32 bytes of UTF-8
        control_round_trip(Control::Hello { version: 7, name: "Ada".into(), ticket: "x".repeat(MAX_TICKET_LEN) });
        control_round_trip(Control::Welcome {
            your_id: 65535,
            tick_hz: 20,
            instance: HUB_INSTANCE,
            flags: welcome::CLIENT_INSTANCING,
        });
        for reason in [
            RejectReason::ServerFull,
            RejectReason::BadVersion,
            RejectReason::BadName,
            RejectReason::AccountRequired,
            RejectReason::BadTicket,
        ] {
            control_round_trip(Control::Reject { reason });
        }
        control_round_trip(Control::PeerJoined(Peer { id: 3, name: "Bob".into() }));
        control_round_trip(Control::PeerLeft { id: 3 });
    }

    #[test]
    fn item_messages_round_trip_and_reject_a_bad_index() {
        control_round_trip(Control::Grab { item: 0 });
        control_round_trip(Control::Grab { item: MAX_ITEM - 1 });
        control_round_trip(Control::Release { item: 17 });
        control_round_trip(Control::Held { item: 17, by: 42 });
        control_round_trip(Control::Held { item: 17, by: NOBODY });

        let at = Transform { pos: [-9, 0, 9], rot: 0xDFF7FDFF };
        datagram_round_trip(Datagram::Item { item: 3, at });
        datagram_round_trip(Datagram::ItemRelay { from: 2, item: 3, at });

        // An index a world could never contain must not reach the game.
        let mut buf = Vec::new();
        Control::Held { item: 1, by: 1 }.encode(&mut buf);
        buf[3..5].copy_from_slice(&MAX_ITEM.to_le_bytes());
        assert_eq!(Control::decode(&buf[2..]), Err(DecodeError::BadItem));
    }

    #[test]
    fn reject_reasons_survive_the_close_frame() {
        for reason in [
            RejectReason::ServerFull,
            RejectReason::BadVersion,
            RejectReason::BadName,
            RejectReason::AccountRequired,
            RejectReason::BadTicket,
        ] {
            assert_eq!(RejectReason::from_close_frame(&reason.close_frame()), Some(reason));
        }
        assert_eq!(RejectReason::from_close_frame(b"rejected:"), None);
        assert_eq!(RejectReason::from_close_frame(b"rejected:\x09"), None);
        assert_eq!(RejectReason::from_close_frame(b"server stopped"), None);
        assert_eq!(RejectReason::from_close_frame(b""), None);
    }

    #[test]
    fn world_and_instance_messages_round_trip() {
        control_round_trip(Control::WorldAdded(World {
            slug: "a".repeat(MAX_SLUG_LEN),
            size: MAX_WORLD_SIZE,
            sha256: [0xAB; 32],
        }));
        control_round_trip(Control::WorldAdded(World { slug: "hub".into(), size: 1, sha256: [0; 32] }));
        control_round_trip(Control::InstanceAdded {
            instance: 7,
            slug: "court-yard_2".into(),
            players: 3,
            name: "Ada's fire".into(),
            kind: ember::FRIENDS,
        });
        control_round_trip(Control::InstanceClosed { instance: 7 });
        control_round_trip(Control::InstanceCount { instance: 7, players: 0 });
        control_round_trip(Control::JoinInstance { instance: HUB_INSTANCE });
        control_round_trip(Control::CreateInstance {
            slug: "hub".into(),
            name: "second hub".into(),
            kind: ember::INVITE,
        });
        control_round_trip(Control::Moved { instance: 65535 });
        control_round_trip(Control::WorldRequest { slug: "hub".into() });
        // Every message stays inside the frame cap, whatever the slug length.
        let mut buf = Vec::new();
        Control::WorldAdded(World { slug: "a".repeat(MAX_SLUG_LEN), size: 1, sha256: [0; 32] })
            .encode(&mut buf);
        assert!(buf.len() - 2 <= MAX_CONTROL_LEN);
    }

    #[test]
    fn slug_validation() {
        for good in ["hub", "a", "court-yard_2", &"z".repeat(MAX_SLUG_LEN)] {
            assert!(validate_slug(good).is_ok(), "{good}");
        }
        for bad in ["", "Hub", "my world", "../etc", "a.hworld", "hub/", &"z".repeat(MAX_SLUG_LEN + 1)] {
            assert_eq!(validate_slug(bad), Err(DecodeError::BadSlug), "{bad}");
        }
        // And over the wire.
        let request = |slug: &[u8]| [&[0x0D, slug.len() as u8][..], slug].concat();
        assert_eq!(Control::decode(&request(b"Hub")), Err(DecodeError::BadSlug));
        assert_eq!(Control::decode(&request(b"..")), Err(DecodeError::BadSlug));
        assert!(Control::decode(&request(b"hub")).is_ok());
        // A world that claims to be empty or enormous is refused.
        let added = |size: u32| [&[0x06, 3, b'h', b'u', b'b'][..], &size.to_le_bytes(), &[0; 32]].concat();
        assert_eq!(Control::decode(&added(0)), Err(DecodeError::BadWorldSize));
        assert_eq!(Control::decode(&added(MAX_WORLD_SIZE + 1)), Err(DecodeError::BadWorldSize));
        assert!(Control::decode(&added(MAX_WORLD_SIZE)).is_ok());
    }

    #[test]
    fn ember_kind_validation() {
        // CreateInstance: slug "hub", name "a", then the kind byte.
        let create = |kind: u8| [&[0x0B, 3, b'h', b'u', b'b', 1, b'a'][..], &[kind]].concat();
        for kind in 0..ember::KINDS {
            assert!(Control::decode(&create(kind)).is_ok(), "kind {kind} should decode");
        }
        assert_eq!(Control::decode(&create(ember::KINDS)), Err(DecodeError::BadEmberKind(ember::KINDS)));
        assert_eq!(Control::decode(&create(200)), Err(DecodeError::BadEmberKind(200)));
        assert_eq!(ember::label(ember::INVITE), "invite");
    }

    #[test]
    fn datagrams_round_trip() {
        let pose = sample_pose();
        for msg in [
            Datagram::Pose(pose),
            Datagram::PoseRelay { from: 9, pose },
            Datagram::Voice { seq: 1, frame: vec![0xAB; MAX_VOICE_FRAME] },
            Datagram::VoiceRelay { from: 9, seq: 65535, frame: vec![1] },
        ] {
            assert_eq!(Datagram::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn pose_golden_bytes() {
        // Must match the example in /docs/PROTOCOL.md.
        let bytes = Datagram::Pose(sample_pose()).encode();
        assert_eq!(bytes.len(), 34);
        assert_eq!(
            bytes,
            [
                0x10, 0x02, 0x01, 0x03, // type, seq, flags
                0x01, 0x00, 0xFE, 0xFF, 0x03, 0x00, 0xFF, 0xFD, 0xF7, 0xDF, // head
                0x00, 0x80, 0x00, 0x00, 0xFF, 0x7F, 0x00, 0x00, 0x00, 0x00, // left
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, // right
            ]
        );
        assert_eq!(Datagram::PoseRelay { from: 1, pose: sample_pose() }.encode().len(), 36);
    }

    #[test]
    fn hello_golden_bytes() {
        let mut buf = Vec::new();
        Control::Hello { version: 1, name: "Ada".into(), ticket: String::new() }.encode(&mut buf);
        assert_eq!(buf, [0x09, 0x00, 0x01, 0x01, 0x00, 0x03, b'A', b'd', b'a', 0x00, 0x00]);
        // An older client's Hello stops after the name, and still decodes,
        // so it can be refused with `BadVersion`.
        assert_eq!(
            Control::decode(&buf[2..9]),
            Ok(Control::Hello { version: 1, name: "Ada".into(), ticket: String::new() })
        );
        // A ticket with a space or a control character in it is not one.
        let mut bad = buf[2..9].to_vec();
        bad.extend_from_slice(&[2, 0, b'a', b' ']);
        assert_eq!(Control::decode(&bad), Err(DecodeError::BadTicket));
    }

    #[test]
    fn rejects_malformed_input() {
        let pose = Datagram::Pose(sample_pose()).encode();
        assert_eq!(Datagram::decode(&pose[..33]), Err(DecodeError::Truncated));
        assert_eq!(Datagram::decode(&[pose.as_slice(), &[0]].concat()), Err(DecodeError::TrailingBytes));
        assert_eq!(Datagram::decode(&[]), Err(DecodeError::Truncated));
        assert_eq!(Datagram::decode(&[0x99]), Err(DecodeError::UnknownType(0x99)));
        assert_eq!(Datagram::decode(&[0x20, 0, 0]), Err(DecodeError::BadVoiceFrame));
        let big = [&[0x20, 0, 0][..], &[0; MAX_VOICE_FRAME + 1]].concat();
        assert_eq!(Datagram::decode(&big), Err(DecodeError::BadVoiceFrame));

        assert_eq!(Control::body_len([0xFF, 0xFF]), Err(DecodeError::TooLong));
        assert_eq!(Control::decode(&[0x03, 9]), Err(DecodeError::BadRejectReason(9)));
        // Hello whose name length runs past the end.
        assert_eq!(Control::decode(&[0x01, 1, 0, 5, b'a']), Err(DecodeError::Truncated));
        // Datagram types are not valid on the control stream.
        assert_eq!(Control::decode(&[0x10]), Err(DecodeError::UnknownType(0x10)));
    }

    #[test]
    fn name_validation() {
        let hello = |name: &[u8]| [&[0x01, 1, 0, name.len() as u8][..], name].concat();
        assert_eq!(Control::decode(&hello(b"")), Err(DecodeError::BadName));
        assert_eq!(Control::decode(&hello(&[b'a'; 33])), Err(DecodeError::BadName));
        assert_eq!(Control::decode(&hello(b"a\nb")), Err(DecodeError::BadName));
        assert_eq!(Control::decode(&hello(&[0xC5])), Err(DecodeError::BadName)); // cut UTF-8
        assert!(Control::decode(&hello(&[b'a'; 32])).is_ok());
    }

    #[test]
    fn decode_never_panics_on_garbage() {
        let mut rng = Rng(0x9E3779B97F4A7C15);
        for _ in 0..100_000 {
            let len = (rng.next() % 48) as usize;
            let mut buf: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            if let Some(b) = buf.first_mut() {
                const TYPES: [u8; 17] = [
                    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x10, 0x11,
                    0x20, 0x21,
                ];
                *b = TYPES[*b as usize % TYPES.len()];
            }
            let _ = Datagram::decode(&buf);
            let _ = Control::decode(&buf);
        }
    }

    #[test]
    fn position_quantization() {
        assert_eq!(quantize_pos(0.0), 0);
        assert_eq!(quantize_pos(1.0), 512);
        assert_eq!(quantize_pos(-1.5), -768);
        assert_eq!(quantize_pos(1000.0), i16::MAX);
        assert_eq!(quantize_pos(-1000.0), i16::MIN);
        assert_eq!(quantize_pos(f32::NAN), 0);
        let mut rng = Rng(42);
        for _ in 0..10_000 {
            let m = rng.f32() * 63.0;
            assert!((dequantize_pos(quantize_pos(m)) - m).abs() <= 0.5 / POS_SCALE + 1e-6);
        }
    }

    fn angle_deg(a: [f32; 4], b: [f32; 4]) -> f64 {
        let dot: f64 = a.iter().zip(b).map(|(x, y)| *x as f64 * y as f64).sum();
        let len_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
        (2.0 * (dot.abs() / len_a).min(1.0).acos()).to_degrees()
    }

    #[test]
    fn rotation_quantization() {
        assert_eq!(quantize_rot([0.0, 0.0, 0.0, 1.0]), 0xDFF7FDFF);
        assert_eq!(dequantize_rot(0xDFF7FDFF), [0.0, 0.0, 0.0, 1.0]);
        // q and -q encode identically.
        assert_eq!(quantize_rot([0.1, -0.2, 0.3, 0.9]), quantize_rot([-0.1, 0.2, -0.3, -0.9]));
        // Degenerate input becomes identity.
        for bad in [[0.0; 4], [f32::NAN; 4], [f32::INFINITY, 0.0, 0.0, 0.0]] {
            assert_eq!(quantize_rot(bad), 0xDFF7FDFF);
        }

        let mut rng = Rng(7);
        let mut cases =
            vec![[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.5, 0.5, 0.5, 0.5], [0.5, -0.5, 0.5, -0.5]];
        cases.extend((0..50_000).map(|_| [rng.f32(), rng.f32(), rng.f32(), rng.f32()]));
        let mut worst = 0.0f64;
        for q in cases.into_iter().filter(|q| q.iter().map(|c| c * c).sum::<f32>() > 0.01) {
            worst = worst.max(angle_deg(q, dequantize_rot(quantize_rot(q))));
        }
        assert!(worst < 0.5, "worst rotation error {worst}°");
    }

    #[test]
    fn any_bits_decode_to_unit_quaternion() {
        let mut rng = Rng(99);
        for bits in [0, u32::MAX].into_iter().chain((0..10_000).map(|_| rng.next() as u32)) {
            let q = dequantize_rot(bits);
            let len: f32 = q.iter().map(|c| c * c).sum::<f32>().sqrt();
            assert!((len - 1.0).abs() < 1e-5 && q.iter().all(|c| c.is_finite()), "{bits:#x} -> {q:?}");
        }
    }
}
