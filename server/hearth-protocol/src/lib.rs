//! Hearth wire protocol. The byte-level spec is in /protocol/PROTOCOL.md;
//! keep the two in sync.

use std::fmt;

pub const PROTOCOL_VERSION: u16 = 1;
pub const ALPN: &[u8] = b"hearth/1";
pub const MAX_NAME_LEN: usize = 32;
/// Upper bound for a control message body; current messages are < 40 bytes.
pub const MAX_CONTROL_LEN: usize = 256;
/// Keeps a voice datagram under the 1200-byte QUIC minimum path MTU.
pub const MAX_VOICE_FRAME: usize = 1000;
/// Position units per metre: i16 covers ±64 m at ~2 mm resolution.
pub const POS_SCALE: f32 = 512.0;

pub mod flags {
    pub const VR: u8 = 1 << 0;
    pub const LEFT_HAND: u8 = 1 << 1;
    pub const RIGHT_HAND: u8 = 1 << 2;
}

mod ty {
    pub const HELLO: u8 = 0x01;
    pub const WELCOME: u8 = 0x02;
    pub const REJECT: u8 = 0x03;
    pub const PEER_JOINED: u8 = 0x04;
    pub const PEER_LEFT: u8 = 0x05;
    pub const POSE: u8 = 0x10;
    pub const POSE_RELAY: u8 = 0x11;
    pub const VOICE: u8 = 0x20;
    pub const VOICE_RELAY: u8 = 0x21;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    TrailingBytes,
    UnknownType(u8),
    TooLong,
    BadName,
    BadVoiceFrame,
    BadRejectReason(u8),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("message truncated"),
            Self::TrailingBytes => f.write_str("trailing bytes after message"),
            Self::UnknownType(t) => write!(f, "unknown message type 0x{t:02x}"),
            Self::TooLong => f.write_str("message too long"),
            Self::BadName => f.write_str("invalid display name"),
            Self::BadVoiceFrame => f.write_str("voice frame empty or too large"),
            Self::BadRejectReason(r) => write!(f, "unknown reject reason {r}"),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    ServerFull = 1,
    BadVersion = 2,
    BadName = 3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Hello { version: u16, name: String },
    Welcome { your_id: u16, tick_hz: u8 },
    Reject { reason: RejectReason },
    PeerJoined(Peer),
    PeerLeft { id: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datagram {
    Pose(Pose),
    PoseRelay { from: u16, pose: Pose },
    Voice { seq: u16, frame: Vec<u8> },
    VoiceRelay { from: u16, seq: u16, frame: Vec<u8> },
}

pub fn validate_name(name: &str) -> Result<(), DecodeError> {
    if name.is_empty() || name.len() > MAX_NAME_LEN || name.chars().any(char::is_control) {
        return Err(DecodeError::BadName);
    }
    Ok(())
}

impl Control {
    /// Appends one framed message (`u16 body length`, then body) to `out`.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0, 0]);
        match self {
            Self::Hello { version, name } => {
                out.push(ty::HELLO);
                out.extend_from_slice(&version.to_le_bytes());
                put_name(out, name);
            }
            Self::Welcome { your_id, tick_hz } => {
                out.push(ty::WELCOME);
                out.extend_from_slice(&your_id.to_le_bytes());
                out.push(*tick_hz);
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
            ty::HELLO => Self::Hello { version: r.u16()?, name: r.name()? },
            ty::WELCOME => Self::Welcome { your_id: r.u16()?, tick_hz: r.u8()? },
            ty::REJECT => Self::Reject {
                reason: match r.u8()? {
                    1 => RejectReason::ServerFull,
                    2 => RejectReason::BadVersion,
                    3 => RejectReason::BadName,
                    other => return Err(DecodeError::BadRejectReason(other)),
                },
            },
            ty::PEER_JOINED => Self::PeerJoined(Peer { id: r.u16()?, name: r.name()? }),
            ty::PEER_LEFT => Self::PeerLeft { id: r.u16()? },
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

    fn control_round_trip(msg: Control) {
        let mut buf = Vec::new();
        msg.encode(&mut buf);
        let len = Control::body_len([buf[0], buf[1]]).unwrap();
        assert_eq!(len, buf.len() - 2);
        assert_eq!(Control::decode(&buf[2..]).unwrap(), msg);
    }

    #[test]
    fn control_messages_round_trip() {
        control_round_trip(Control::Hello { version: PROTOCOL_VERSION, name: "Ada".into() });
        control_round_trip(Control::Hello { version: 7, name: "ž".repeat(16) }); // 32 bytes of UTF-8
        control_round_trip(Control::Welcome { your_id: 65535, tick_hz: 20 });
        for reason in [RejectReason::ServerFull, RejectReason::BadVersion, RejectReason::BadName] {
            control_round_trip(Control::Reject { reason });
        }
        control_round_trip(Control::PeerJoined(Peer { id: 3, name: "Bob".into() }));
        control_round_trip(Control::PeerLeft { id: 3 });
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
        // Must match the example in /protocol/PROTOCOL.md.
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
        Control::Hello { version: 1, name: "Ada".into() }.encode(&mut buf);
        assert_eq!(buf, [0x07, 0x00, 0x01, 0x01, 0x00, 0x03, b'A', b'd', b'a']);
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
                *b = [0x01, 0x02, 0x03, 0x04, 0x05, 0x10, 0x11, 0x20, 0x21][*b as usize % 9];
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
        let mut cases = vec![[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.5, 0.5, 0.5, 0.5], [0.5, -0.5, 0.5, -0.5]];
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
