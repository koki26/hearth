//! Godot 4 GDExtension: a `HearthClient` node that owns the QUIC connection
//! and turns wire poses into Godot transforms and signals.

pub mod net;
mod steamvr;
pub mod voice;

use std::time::{Duration, Instant};

use godot::classes::Node;
use godot::prelude::*;
use hearth_protocol::{Pose, Transform};

struct HearthExtension;

#[gdextension]
unsafe impl ExtensionLibrary for HearthExtension {
    // Core runs before the engine brings up OpenXR.
    fn min_level() -> InitLevel {
        InitLevel::Core
    }

    fn on_stage_init(stage: InitStage) {
        if stage == InitStage::Core {
            steamvr::prefer_if_running();
        }
    }
}

#[derive(GodotClass)]
#[class(base = Node)]
pub struct HearthClient {
    base: Base<Node>,
    client: Option<net::Client>,
    seq: u16,
    /// Minimum gap between poses, from the server's `tick_hz`.
    interval: Duration,
    last_pose: Option<Instant>,
    encoder: Option<voice::Encoder>,
    decoders: voice::Decoders,
}

#[godot_api]
impl INode for HearthClient {
    fn init(base: Base<Node>) -> Self {
        Self {
            base,
            client: None,
            seq: 0,
            interval: Duration::from_millis(50),
            last_pose: None,
            encoder: None,
            decoders: voice::Decoders::default(),
        }
    }

    fn process(&mut self, _delta: f64) {
        while let Some(event) = self.client.as_ref().and_then(net::Client::poll) {
            match event {
                net::Event::Connected { your_id, tick_hz } => {
                    self.interval = Duration::from_secs_f64(1.0 / tick_hz.max(1) as f64);
                    self.signals().connected().emit(your_id as i32, tick_hz as i32);
                }
                net::Event::Rejected(reason) => {
                    self.client = None;
                    self.signals().rejected().emit(&GString::from(format!("{reason:?}").as_str()));
                }
                net::Event::Disconnected(reason) => {
                    self.client = None;
                    self.signals().disconnected().emit(&GString::from(reason.as_str()));
                }
                net::Event::PeerJoined { id, name } => self.signals().peer_joined().emit(id as i32, &GString::from(name.as_str())),
                net::Event::PeerLeft { id } => {
                    self.decoders.forget(id);
                    self.signals().peer_left().emit(id as i32);
                }
                net::Event::PeerVoice { from, frame } => {
                    if let Some(pcm) = self.decoders.decode(from, &frame) {
                        self.signals().peer_voice().emit(from as i32, &PackedFloat32Array::from(pcm.as_slice()));
                    }
                }
                net::Event::PeerPose { from, pose } => self.signals().peer_pose().emit(
                    from as i32,
                    to_godot(pose.head),
                    to_godot(pose.left),
                    to_godot(pose.right),
                    pose.flags as i32,
                ),
            }
        }
    }
}

#[godot_api]
impl HearthClient {
    #[signal]
    fn connected(your_id: i32, tick_hz: i32);
    #[signal]
    fn rejected(reason: GString);
    #[signal]
    fn disconnected(reason: GString);
    #[signal]
    fn peer_joined(id: i32, name: GString);
    #[signal]
    fn peer_left(id: i32);
    /// `flags` bit 0 = peer is in VR, bits 1/2 = left/right hand tracked.
    #[signal]
    fn peer_pose(id: i32, head: Transform3D, left: Transform3D, right: Transform3D, flags: i32);
    /// Decoded mono 48 kHz samples from one speaker.
    #[signal]
    fn peer_voice(id: i32, samples: PackedFloat32Array);

    /// Replaces any existing connection. Progress arrives as signals.
    #[func]
    fn connect_to_server(&mut self, host: GString, port: i32, player_name: GString) {
        self.client = Some(net::Client::connect(host.to_string(), port as u16, player_name.to_string()));
        self.last_pose = None;
    }

    #[func]
    fn disconnect_from_server(&mut self) {
        self.client = None;
    }

    /// Captured mono 48 kHz samples; any length. Whole 20 ms frames are
    /// encoded and sent, the rest waits for the next call.
    #[func]
    fn send_voice(&mut self, samples: PackedFloat32Array) {
        let Some(client) = &self.client else { return };
        if self.encoder.is_none() {
            match voice::Encoder::new() {
                Ok(encoder) => self.encoder = Some(encoder),
                Err(err) => {
                    godot_error!("no voice: {err:#}");
                    return;
                }
            }
        }
        let encoder = self.encoder.as_mut().expect("just created");
        for frame in encoder.push(samples.as_slice()) {
            client.send(net::Outgoing::Voice(frame));
        }
    }

    #[func]
    fn voice_sample_rate(&self) -> i32 {
        voice::SAMPLE_RATE as i32
    }

    #[func]
    fn is_connected_to_server(&self) -> bool {
        self.client.is_some()
    }

    /// Call every frame; sends at most `tick_hz` poses per second.
    #[func]
    fn send_pose(&mut self, head: Transform3D, left: Transform3D, right: Transform3D, flags: i32) {
        let Some(client) = &self.client else { return };
        let now = Instant::now();
        if self.last_pose.is_some_and(|last| now.duration_since(last) < self.interval) {
            return;
        }
        self.last_pose = Some(now);
        self.seq = self.seq.wrapping_add(1);
        client.send(net::Outgoing::Pose(Pose {
            seq: self.seq,
            flags: flags as u8,
            head: from_godot(head),
            left: from_godot(left),
            right: from_godot(right),
        }));
    }
}

fn from_godot(t: Transform3D) -> Transform {
    let q = t.basis.orthonormalized().get_quaternion();
    Transform::from_f32([t.origin.x, t.origin.y, t.origin.z], [q.x, q.y, q.z, q.w])
}

fn to_godot(t: Transform) -> Transform3D {
    let [x, y, z, w] = t.rot_f32();
    let [px, py, pz] = t.pos_f32();
    Transform3D::new(Basis::from_quaternion(Quaternion::new(x, y, z, w)), Vector3::new(px, py, pz))
}
