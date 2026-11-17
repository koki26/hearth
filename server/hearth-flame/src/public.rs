//! Being listed on the Fireplace's list of public Flames.
//!
//! Off unless `[listing] public = true`. The Flame only asks to be listed;
//! the Fireplace then connects back over QUIC itself and lists it only while
//! that works and shows the certificate named here, so nobody can list an
//! address that is not theirs or a Flame that is not up.

use std::sync::{Arc, Weak};
use std::time::Duration;

use tracing::{info, warn};

use crate::Server;

/// How often the Flame asks again. The Fireplace drops one it has not heard
/// from in a few of these.
const EVERY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Listing {
    pub public: bool,
    /// What the list calls this Flame. Empty: the hub world's name.
    pub name: String,
    /// A short description of the server or world shown on public listings.
    pub description: String,
    /// A banner image URL or path shown on public listings.
    pub banner: String,
    /// Optional Fireplace to publish this Flame to (defaults to the server's accounts Fireplace if empty).
    pub fireplace: String,
    /// `host` or `host:port` players should use, when it is not the address
    /// the Fireplace sees the request come from (a domain name, a forward).
    pub address: String,
}

pub fn announce(server: &Arc<Server>, fireplace: &str, listing: &Listing, port: u16, hub: &str) {
    let target_fireplace = if !listing.fireplace.trim().is_empty() {
        listing.fireplace.trim()
    } else {
        fireplace.trim()
    };
    if !listing.public || target_fireplace.is_empty() {
        return;
    }
    let url = format!("{}/flames/announce", target_fireplace.trim_end_matches('/'));
    let name = if listing.name.trim().is_empty() { hub.to_owned() } else { listing.name.trim().to_owned() };
    let description = listing.description.trim().to_owned();
    let banner = listing.banner.trim().to_owned();
    let address = listing.address.trim().to_owned();
    let server: Weak<Server> = Arc::downgrade(server);
    std::thread::spawn(move || {
        let mut said = false;
        // Ends with the server.
        while let Some(server) = server.upgrade() {
            let body = serde_json::json!({
                "name": name,
                "description": description,
                "banner": banner,
                "address": address,
                "port": port,
                "certificate": server.tickets.certificate(),
                "players": server.state.lock().unwrap().players.len(),
                "max_players": server.max_players.0,
                "guests": server.allow_guests,
            });
            drop(server);
            let sent = ureq::post(&url)
                .config()
                .timeout_global(Some(Duration::from_secs(10)))
                .build()
                .header("Content-Type", "application/json")
                .send(body.to_string());
            match sent {
                Ok(_) if !said => {
                    info!(%url, "asked to be listed as a public Flame");
                    said = true;
                }
                Ok(_) => {}
                Err(err) => warn!(%url, "could not ask to be listed: {err}"),
            }
            std::thread::sleep(EVERY);
        }
    });
}
