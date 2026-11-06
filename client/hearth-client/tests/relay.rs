//! The real client against an in-process server.

use std::time::Duration;

use hearth_client::net::{Client, Event};
use hearth_protocol::{Pose, RejectReason, Transform, flags};

async fn next_event(client: &Client) -> Event {
    for _ in 0..1500 {
        if let Some(event) = client.poll() {
            return event;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("no event within 15 s");
}

fn start(max_players: u16) -> u16 {
    let (cert, key) = hearth_server::generate_cert().unwrap();
    let endpoint = hearth_server::endpoint(0, cert, key).unwrap();
    let port = endpoint.local_addr().unwrap().port();
    tokio::spawn(hearth_server::serve(endpoint, max_players));
    port
}

#[tokio::test]
async fn two_clients_see_each_other_and_exchange_poses() {
    let port = start(2);

    let ada = Client::connect("127.0.0.1".into(), port, "Ada".into());
    assert!(matches!(next_event(&ada).await, Event::Connected { your_id: 1, tick_hz: 20 }));

    let bob = Client::connect("localhost".into(), port, "Bob".into());
    assert!(matches!(next_event(&bob).await, Event::Connected { your_id: 2, .. }));
    assert!(matches!(next_event(&bob).await, Event::PeerJoined { id: 1, ref name } if name == "Ada"));
    assert!(matches!(next_event(&ada).await, Event::PeerJoined { id: 2, ref name } if name == "Bob"));

    let head = Transform::from_f32([0.0, 1.7, -0.5], [0.0, 0.0, 0.0, 1.0]);
    let sent = Pose { seq: 1, flags: flags::VR, head, left: head, right: head };
    bob.send_pose(sent);
    let Event::PeerPose { from, pose } = next_event(&ada).await else { panic!("expected a pose") };
    assert_eq!((from, pose), (2, sent));

    drop(bob);
    assert!(matches!(next_event(&ada).await, Event::PeerLeft { id: 2 }));

    // Third player with a free slot again, and a fourth that is one too many.
    let cy = Client::connect("127.0.0.1".into(), port, "Cy".into());
    assert!(matches!(next_event(&cy).await, Event::Connected { .. }));
    let dee = Client::connect("127.0.0.1".into(), port, "Dee".into());
    assert!(matches!(next_event(&dee).await, Event::Rejected(RejectReason::ServerFull)));
}

#[tokio::test]
async fn bad_input_fails_without_a_connection() {
    let port = start(1);
    let empty = Client::connect("127.0.0.1".into(), port, String::new());
    assert!(matches!(next_event(&empty).await, Event::Disconnected(_)));
    let nowhere = Client::connect("no.such.host.hearth.invalid".into(), 7777, "Ada".into());
    assert!(matches!(next_event(&nowhere).await, Event::Disconnected(_)));
}
