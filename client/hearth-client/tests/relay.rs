//! The real client against an in-process server.

use std::path::PathBuf;
use std::time::Duration;

use hearth_client::net::{Client, Event, Outgoing, cached_path};
use hearth_flame::{Config, Roster, Server};
use hearth_protocol::{HUB_INSTANCE, Pose, RejectReason, Transform, flags, welcome};

async fn next_event(client: &Client) -> Event {
    for _ in 0..1500 {
        if let Some(event) = client.poll() {
            return event;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no event within 15 s");
}

/// Skips the world list, the instance list and player counts.
async fn next_interesting(client: &Client) -> Event {
    loop {
        match next_event(client).await {
            Event::WorldAdded(_) | Event::InstanceAdded { .. } | Event::InstanceCount { .. } => {
                continue;
            }
            other => return other,
        }
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hearth-client-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Harness {
    port: u16,
    server: std::sync::Arc<Server>,
    dir: PathBuf,
}

impl Harness {
    fn start(tag: &str, max_players: u16, worlds: &[(&str, &str)]) -> Self {
        let dir = temp_dir(tag);
        for (slug, contents) in worlds {
            std::fs::write(dir.join(format!("{slug}.hworld")), contents).unwrap();
        }
        let config = Config {
            max_players,
            worlds_dir: Some(dir.clone()),
            client_instancing: true,
            ..Config::default()
        };
        let (cert, key) = hearth_flame::generate_cert().unwrap();
        let endpoint = hearth_flame::endpoint(0, cert, key).unwrap();
        let port = endpoint.local_addr().unwrap().port();
        let server = Server::new(&config, tokio::sync::watch::channel(Roster::new()).0);
        tokio::spawn(hearth_flame::serve_with(endpoint, server.clone()));
        Self { port, server, dir }
    }

    fn client(&self, name: &str) -> Client {
        Client::connect("127.0.0.1".into(), self.port, name.into(), self.dir.join("cache"))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn two_clients_see_each_other_and_exchange_poses() {
    let h = Harness::start("relay", 2, &[]);

    let ada = h.client("Ada");
    assert!(matches!(
        next_event(&ada).await,
        Event::Connected {
            your_id: 1,
            tick_hz: 20,
            instance: HUB_INSTANCE,
            flags: welcome::CLIENT_INSTANCING
        }
    ));

    let bob = Client::connect("localhost".into(), h.port, "Bob".into(), h.dir.join("cache"));
    assert!(matches!(next_event(&bob).await, Event::Connected { your_id: 2, .. }));
    assert!(matches!(next_interesting(&bob).await, Event::PeerJoined { id: 1, ref name } if name == "Ada"));
    assert!(matches!(next_interesting(&ada).await, Event::PeerJoined { id: 2, ref name } if name == "Bob"));

    let head = Transform::from_f32([0.0, 1.7, -0.5], [0.0, 0.0, 0.0, 1.0]);
    let sent = Pose { seq: 1, flags: flags::VR, head, left: head, right: head };
    bob.send(Outgoing::Pose(sent));
    let Event::PeerPose { from, pose } = next_interesting(&ada).await else { panic!("expected a pose") };
    assert_eq!((from, pose), (2, sent));

    // Voice goes the same way, round trip through the server's relay.
    bob.send(Outgoing::Voice(vec![9, 8, 7]));
    let Event::PeerVoice { from, frame } = next_interesting(&ada).await else { panic!("expected voice") };
    assert_eq!((from, frame), (2, vec![9, 8, 7]));

    drop(bob);
    assert!(matches!(next_interesting(&ada).await, Event::PeerLeft { id: 2 }));

    // Third player with a free slot again, and a fourth that is one too many.
    let cy = h.client("Cy");
    assert!(matches!(next_event(&cy).await, Event::Connected { .. }));
    let dee = h.client("Dee");
    let event = next_event(&dee).await;
    assert!(matches!(event, Event::Rejected(RejectReason::ServerFull)), "got {event:?}");
}

#[tokio::test]
async fn bad_input_fails_without_a_connection() {
    let h = Harness::start("badinput", 1, &[]);
    let empty = Client::connect("127.0.0.1".into(), h.port, String::new(), h.dir.clone());
    assert!(matches!(next_event(&empty).await, Event::Disconnected(_)));
    let nowhere = Client::connect("no.such.host.hearth.invalid".into(), 7777, "Ada".into(), h.dir.clone());
    assert!(matches!(next_event(&nowhere).await, Event::Disconnected(_)));
}

#[tokio::test]
async fn a_world_downloads_into_the_cache_and_is_reused() {
    let h = Harness::start("download", 2, &[("forest", "packed world bytes")]);
    let ada = h.client("Ada");
    assert!(matches!(next_event(&ada).await, Event::Connected { .. }));

    let Event::WorldAdded(world) = next_event(&ada).await else { panic!("expected the world list") };
    assert_eq!((world.slug.as_str(), world.size), ("forest", 18));

    ada.send(Outgoing::RequestWorld(world.clone()));
    let Event::WorldReady { slug, path } = next_interesting(&ada).await else { panic!("expected a world") };
    assert_eq!(slug, "forest");
    assert_eq!(path, cached_path(&h.dir.join("cache"), &world), "the cache is addressed by hash");
    assert_eq!(std::fs::read(&path).unwrap(), b"packed world bytes");

    // Asking again is answered from the cache, so the file stays put.
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    ada.send(Outgoing::RequestWorld(world.clone()));
    let Event::WorldReady { path: again, .. } = next_interesting(&ada).await else {
        panic!("expected a world")
    };
    assert_eq!(again, path);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), before);

    // A hash that does not match what the server has is refused, and nothing
    // is written: this is the only defence against a server (or whoever is on
    // the path) swapping a world for another (docs/decisions.md D3, D27).
    let lie = hearth_protocol::World { sha256: [0xAA; 32], ..world };
    ada.send(Outgoing::RequestWorld(lie.clone()));
    let Event::WorldFailed { slug, reason } = next_interesting(&ada).await else {
        panic!("expected a refusal")
    };
    assert_eq!(slug, "forest");
    assert!(reason.contains("hash"), "{reason}");
    assert!(!cached_path(&h.dir.join("cache"), &lie).exists());
}

#[tokio::test]
async fn walking_into_an_instance_moves_only_that_player() {
    let h = Harness::start("instances", 2, &[("forest", "packed world bytes")]);
    let ada = h.client("Ada");
    assert!(matches!(next_event(&ada).await, Event::Connected { .. }));
    let bob = h.client("Bob");
    assert!(matches!(next_event(&bob).await, Event::Connected { .. }));
    assert!(matches!(next_interesting(&bob).await, Event::PeerJoined { id: 1, .. }));
    assert!(matches!(next_interesting(&ada).await, Event::PeerJoined { id: 2, .. }));

    // Ada starts her own instance of the forest and lands in it alone.
    ada.send(Outgoing::CreateInstance("forest".into(), "Ada's fire".into(), hearth_protocol::ember::FRIENDS));
    let instance = loop {
        match next_event(&ada).await {
            Event::Moved { instance } => break instance,
            _ => continue,
        }
    };
    assert_ne!(instance, HUB_INSTANCE);
    assert!(matches!(next_interesting(&bob).await, Event::PeerLeft { id: 1 }));

    // Bob follows her in and they see each other again.
    bob.send(Outgoing::JoinInstance(instance));
    assert!(matches!(next_interesting(&bob).await, Event::Moved { instance: i } if i == instance));
    assert!(matches!(next_interesting(&bob).await, Event::PeerJoined { id: 1, .. }));
    assert!(matches!(next_interesting(&ada).await, Event::PeerJoined { id: 2, .. }));

    // An instance that does not exist leaves them where they are.
    bob.send(Outgoing::JoinInstance(9999));
    assert!(matches!(next_interesting(&bob).await, Event::Moved { instance: i } if i == instance));
    assert_eq!(h.server.instances().len(), 2);
}
