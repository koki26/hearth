//! Godot 4 GDExtension: a `HearthClient` node that owns the QUIC connection
//! and turns wire poses into Godot transforms and signals.

pub mod net;
mod steamvr;
pub mod voice;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use godot::classes::Node;
use godot::prelude::*;
use hearth_protocol::{Pose, Transform, World};

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
    /// Worlds the server advertised, by slug, so GDScript can ask for one by
    /// name without carrying its size and hash around.
    worlds: HashMap<String, World>,
    cache_dir: PathBuf,
    /// Last send time per item, so carrying two things does not double the
    /// rate of either. Only ever holds the handful of items we carry.
    last_item: HashMap<u16, Instant>,
    /// Whether this server keeps what worlds store for us: we joined with an
    /// account (`welcome::SAVES`). A guest's writes go nowhere.
    saves: bool,
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
            worlds: HashMap::new(),
            cache_dir: PathBuf::from("."),
            last_item: HashMap::new(),
            saves: false,
        }
    }

    fn process(&mut self, _delta: f64) {
        while let Some(event) = self.client.as_ref().and_then(net::Client::poll) {
            match event {
                net::Event::Connected { your_id, tick_hz, instance, flags } => {
                    self.interval = Duration::from_secs_f64(1.0 / tick_hz.max(1) as f64);
                    self.saves = flags & hearth_protocol::welcome::SAVES != 0;
                    self.signals().connected().emit(
                        your_id as i32,
                        tick_hz as i32,
                        instance as i32,
                        flags as i32,
                    );
                }
                net::Event::Rejected(reason) => {
                    self.client = None;
                    self.signals().rejected().emit(&GString::from(describe(reason)));
                }
                net::Event::Disconnected(reason) => {
                    self.client = None;
                    self.signals().disconnected().emit(&GString::from(reason.as_str()));
                }
                net::Event::PeerJoined { id, name } => {
                    self.signals().peer_joined().emit(id as i32, &GString::from(name.as_str()))
                }
                net::Event::PeerLeft { id } => {
                    self.decoders.forget(id);
                    self.signals().peer_left().emit(id as i32);
                }
                net::Event::PeerVoice { from, frame } => {
                    if let Some(pcm) = self.decoders.decode(from, &frame) {
                        self.signals()
                            .peer_voice()
                            .emit(from as i32, &PackedFloat32Array::from(pcm.as_slice()));
                    }
                }
                net::Event::PeerPose { from, pose } => self.signals().peer_pose().emit(
                    from as i32,
                    to_godot(pose.head),
                    to_godot(pose.left),
                    to_godot(pose.right),
                    pose.flags as i32,
                ),
                net::Event::Held { item, by } => self.signals().item_held().emit(item as i32, by as i32),
                net::Event::ShotFired { from, at } => {
                    self.signals().shot_fired().emit(from as i32, to_godot(at))
                }
                net::Event::Health { player, hp } => self.signals().health().emit(player as i32, hp as i32),
                net::Event::Died { player, by } => self.signals().died().emit(player as i32, by as i32),
                net::Event::Said { from, text } => {
                    self.signals().chat().emit(from as i32, &GString::from(text.as_str()))
                }
                net::Event::Spawned { player } => self.signals().spawned().emit(player as i32),
                net::Event::Value { key, value } => self
                    .signals()
                    .value()
                    .emit(&GString::from(key.as_str()), &PackedByteArray::from(&value[..])),
                net::Event::ItemMoved { item, at } => {
                    self.signals().item_moved().emit(item as i32, to_godot(at))
                }
                net::Event::WorldAdded(world) => {
                    let (slug, size) = (world.slug.clone(), world.size);
                    self.worlds.insert(slug.clone(), world);
                    self.signals().world_added().emit(&GString::from(slug.as_str()), size as i64);
                }
                net::Event::InstanceAdded { instance, slug, players, name, kind } => {
                    self.signals().instance_added().emit(
                        instance as i32,
                        &GString::from(slug.as_str()),
                        players as i32,
                        &GString::from(name.as_str()),
                        kind as i32,
                    )
                }
                net::Event::InstanceClosed { instance } => {
                    self.signals().instance_closed().emit(instance as i32)
                }
                net::Event::InstanceCount { instance, players } => {
                    self.signals().instance_count().emit(instance as i32, players as i32)
                }
                net::Event::Moved { instance } => {
                    self.decoders.clear();
                    self.signals().moved().emit(instance as i32);
                }
                net::Event::WorldReady { slug, path } => self
                    .signals()
                    .world_ready()
                    .emit(&GString::from(slug.as_str()), &GString::from(path.display().to_string().as_str())),
                net::Event::WorldFailed { slug, reason } => self
                    .signals()
                    .world_failed()
                    .emit(&GString::from(slug.as_str()), &GString::from(reason.as_str())),
            }
        }
    }
}

#[godot_api]
impl HearthClient {
    /// `instance` is where the server put you (the hub); `flags` bit 0 means
    /// players may start their own instances.
    #[signal]
    fn connected(your_id: i32, tick_hz: i32, instance: i32, flags: i32);
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
    /// Who holds item `item` now, or 0 for nobody. The server decides, so
    /// this is also the answer to your own `grab_item`: do not start carrying
    /// anything until it says you hold it.
    #[signal]
    fn item_held(item: i32, by: i32);
    /// Where an item somebody else is carrying has got to.
    #[signal]
    fn item_moved(item: i32, at: Transform3D);
    /// A world this server will hand out. Ask for it with `request_world`.
    #[signal]
    fn world_added(slug: GString, size: i64);
    #[signal]
    /// A running ember: its world, how many are in it, what it is called and
    /// who it is open to (0 public, 1 friends+, 2 friends, 3 invite).
    fn instance_added(instance: i32, slug: GString, players: i32, ember_name: GString, kind: i32);
    #[signal]
    fn instance_closed(instance: i32);
    #[signal]
    fn instance_count(instance: i32, players: i32);
    /// You are now in `instance`: forget every peer, `peer_joined` follows for
    /// each one there. The instance you were already in means the move failed.
    #[signal]
    fn moved(instance: i32);
    /// Somebody fired: draw a tracer from here, pointing this way. Already
    /// rate limited by the server.
    #[signal]
    fn shot_fired(from: i32, at: Transform3D);
    /// Somebody's health changed. The server is the only thing that sets it,
    /// so this is also the answer to our own `claim_hit` (docs/TRUST.md).
    #[signal]
    fn health(player: i32, hp: i32);
    /// `by` is 0 when nobody killed them: a fall, a kill plane.
    #[signal]
    fn died(player: i32, by: i32);
    #[signal]
    fn spawned(player: i32);
    /// What the server has stored for us under this key. Empty means nothing
    /// is stored there, including when a write was refused.
    #[signal]
    fn value(key: GString, value: PackedByteArray);
    /// A verified `.hworld` file on disk, ready to unpack.
    /// A line of chat from `from`, our own lines included.
    #[signal]
    fn chat(from: i32, text: GString);

    #[signal]
    fn world_ready(slug: GString, path: GString);
    #[signal]
    fn world_failed(slug: GString, reason: GString);

    /// Where downloaded worlds are kept. Set it before connecting, from
    /// `ProjectSettings.globalize_path("user://...")`.
    #[func]
    fn set_world_cache(&mut self, path: GString) {
        self.cache_dir = PathBuf::from(path.to_string());
    }

    /// Asks for a world the server advertised. A cached copy is reported
    /// straight back through `world_ready`.
    #[func]
    fn request_world(&mut self, slug: GString) {
        let Some(client) = &self.client else { return };
        match self.worlds.get(&slug.to_string()) {
            Some(world) => client.send(net::Outgoing::RequestWorld(world.clone())),
            None => self
                .signals()
                .world_failed()
                .emit(&slug, &GString::from("the server did not offer that world")),
        }
    }

    /// Asks to hold an item, named by its index in the world's object list.
    /// The answer is an `item_held` signal. The server grants every grab,
    /// because it has never read the world: a world saying an item may not be
    /// taken is enforced here, by not asking.
    #[func]
    fn grab_item(&mut self, item: i32) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Grab(item as u16));
        }
    }

    /// Tell everyone a shot left here, pointing this way. A tracer only: it
    /// does no damage and is not what hits anybody.
    #[func]
    fn fire(&mut self, at: Transform3D) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Shot { at: from_godot(at) });
        }
    }

    /// Claim a hit on somebody. The server decides whether anything comes of
    /// it and answers with `health`; nothing here changes anybody's health,
    /// because a client that could do that is a client that cheats
    /// (docs/TRUST.md).
    #[func]
    fn claim_hit(&mut self, player: i32) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Hit { target: player as u16 });
        }
    }

    /// Ask to come back. Refused unless the server thinks we are dead.
    #[func]
    fn respawn(&mut self) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Respawn);
        }
    }

    /// Keep something against our name, in the world we are in. The server
    /// chooses the namespace, so one world cannot reach into another. An
    /// empty value deletes the key.
    #[func]
    fn store(&mut self, key: GString, value: PackedByteArray) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Store { key: key.to_string(), value: value.to_vec() });
        }
    }

    /// Say something to everyone in this ember. Trimmed, and cut to what the
    /// wire carries; an empty line is not sent. It comes back as `chat`.
    #[func]
    fn say(&mut self, text: GString) {
        let Some(client) = &self.client else { return };
        let mut line: String = text.to_string().chars().filter(|c| !c.is_control()).collect();
        line = line.trim().to_owned();
        while line.len() > hearth_protocol::MAX_CHAT_LEN {
            line.pop();
        }
        if hearth_protocol::validate_chat(&line).is_ok() {
            client.send(net::Outgoing::Say(line));
        }
    }

    /// Ask for it back. The answer is a `value` signal.
    #[func]
    fn fetch(&mut self, key: GString) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Fetch { key: key.to_string() });
        }
    }

    #[func]
    fn release_item(&mut self, item: i32) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::Release(item as u16));
        }
    }

    /// Where an item you hold is now. Call it every frame for what you carry;
    /// sends at most `tick_hz` updates per second per item.
    #[func]
    fn send_item(&mut self, item: i32, at: Transform3D) {
        let Some(client) = &self.client else { return };
        let now = Instant::now();
        let item = item as u16;
        if self.last_item.get(&item).is_some_and(|last| now.duration_since(*last) < self.interval) {
            return;
        }
        self.last_item.insert(item, now);
        client.send(net::Outgoing::Item { item, at: from_godot(at) });
    }

    /// Walks into a running instance. The answer is always a `moved` signal.
    #[func]
    fn join_instance(&mut self, instance: i32) {
        if let Some(client) = &self.client {
            client.send(net::Outgoing::JoinInstance(instance as u16));
        }
    }

    /// Asks for a fresh ember of a world, called `ember_name` and open to
    /// `kind` (0 public, 1 friends+, 2 friends, 3 invite). Refused unless the
    /// server allows it, which `connected`'s flags said.
    #[func]
    fn create_instance(&mut self, slug: GString, ember_name: GString, kind: i32) {
        if let Some(client) = &self.client {
            // Cleaned here rather than hoped for: a name the server cannot
            // decode closes the connection, and this one is typed by hand.
            // Control characters out, then cut to the wire's 32 bytes on a
            // character boundary -- truncating mid-character panics.
            let mut name = String::new();
            for c in ember_name.to_string().trim().chars().filter(|c| !c.is_control()) {
                if name.len() + c.len_utf8() > hearth_protocol::MAX_NAME_LEN {
                    break;
                }
                name.push(c);
            }
            if name.is_empty() {
                name = slug.to_string();
            }
            let kind = kind.clamp(0, hearth_protocol::ember::KINDS as i32 - 1) as u8;
            client.send(net::Outgoing::CreateInstance(slug.to_string(), name, kind));
        }
    }

    /// Replaces any existing connection. Progress arrives as signals.
    /// `fireplace` and `token` are the account to join with; leave `token`
    /// empty to join as a guest, whose saves the server does not keep.
    #[func]
    fn connect_to_server(&mut self, host: GString, port: i32, player_name: GString, fireplace: GString, token: GString) {
        self.worlds.clear();
        self.last_item.clear();
        self.saves = false;
        let account = (!token.is_empty() && !fireplace.is_empty())
            .then(|| net::Account { fireplace: fireplace.to_string(), token: token.to_string() });
        self.client = Some(net::Client::connect(
            host.to_string(),
            port as u16,
            player_name.to_string(),
            account,
            self.cache_dir.clone(),
        ));
        self.last_pose = None;
    }

    #[func]
    fn disconnect_from_server(&mut self) {
        self.client = None;
        self.saves = false;
    }

    /// For world makers: whether `store` will be kept. False for a guest --
    /// the server answers their writes with nothing -- so a world can say
    /// "sign in to keep your progress" instead of silently forgetting.
    #[func]
    fn saves_kept(&self) -> bool {
        self.saves
    }

    /// Captured stereo 48 kHz samples straight from `AudioEffectCapture`; any
    /// length. Whole 20 ms frames are downmixed, encoded and sent, the rest
    /// waits for the next call. Stereo in, because the downmix here costs
    /// nothing while the same loop in GDScript costs a slice of every frame.
    #[func]
    fn send_voice(&mut self, samples: PackedVector2Array) {
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
        let mono: Vec<f32> = samples.as_slice().iter().map(|s| (s.x + s.y) * 0.5).collect();
        for frame in encoder.push(&mono) {
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

/// What a refusal means, in words a player can act on.
fn describe(reason: hearth_protocol::RejectReason) -> &'static str {
    use hearth_protocol::RejectReason::*;
    match reason {
        ServerFull => "the server is full",
        BadVersion => "your game and the server are different versions; update whichever is older",
        BadName => "that name will not do",
        AccountRequired => "this server only lets in people signed in to an account",
        BadTicket => "the server could not check your account; try again in a moment",
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
