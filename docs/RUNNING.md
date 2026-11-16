# Running the servers

There are two, and you almost certainly want only one of them.

| | what it is | who runs it | needed to play? |
|---|---|---|---|
| **Flame** | one game server: worlds, embers, poses, voice | anybody | yes |
| **Fireplace** | accounts, friends, and where your friends are | us, centrally | no |

Both are plain Rust binaries with no system dependencies, and both run on
Windows, Linux and macOS. Building them needs [rustup](https://rustup.rs) and
a linker (`build-essential` on Debian and Ubuntu). The CMake and libopus in
the README's *What you need* are for the **client**; neither server has any C
in it.

```sh
cargo build --release -p hearth-flame       # target/release/flame
cargo build --release -p hearth-fireplace   # target/release/fireplace
cargo build --release -p hearth-flame-gui   # target/release/flame-gui (needs a desktop)
```

Putting the official Fireplace on a free cloud box, with HTTPS in front of
it: [ORACLE.md](ORACLE.md). A Flame does not need one — it is hosted by
whoever is playing on it.

---

## A Flame

### Windows

Double-click **`start.bat`**: it packs the worlds the client ships with,
builds, and opens the window. `start.bat --noGUI` gives you the console one
instead. The window and its commands are in the [README](https://github.com/koki26/hearth/blob/main/README.md).

### Linux and macOS

```sh
bash start.sh          # the console Flame, which is what a server wants
bash start.sh --gui    # the window, if there is a desktop to put it on
```

Same script, the other way round from Windows: the console is the default
because a server has no screen. It needs `zip` to pack the worlds
(`sudo apt install zip`); without it the Flame still runs, and players see
their own built-in room instead of your hub.

Or do it by hand, which is all the script does:

```sh
cargo build --release -p hearth-flame
mkdir -p data/worlds
# Named file by file on purpose: a client refuses a package holding anything
# but world.json and assets/*.glb, the "./world.json" of `zip -r .` included.
(cd client/godot/worlds/hub && zip -q ../../../../data/worlds/hub.hworld world.json)
./target/release/flame deploy/flame.toml
```

### What it needs

- **UDP**, not TCP — it is QUIC. Port `7777` by default. Forward or open
  exactly that, and check you opened *UDP*: this is the single most common
  reason a Flame nobody can reach looks like it is working fine.
- A **data directory** (`data/` beside the config) for its TLS certificate,
  its worlds and the players' stored data. Keep it across restarts: the
  certificate is what clients will pin.
- Nothing else. No database, no account server.

Every key it takes, with its default and what it means, is in
[`deploy/flame.toml`](https://github.com/koki26/hearth/blob/main/deploy/flame.toml). Pass the config as the only
argument; with no argument it looks for `flame.toml` in the directory it was
started from, and with no file at all it runs on the defaults and says so.

Worlds are `.hworld` files in `data/worlds` (really zips — see
[WORLD_FORMAT.md](WORLD_FORMAT.md)). Drop one in and type `rescan`, or drag it
onto the window.

---

## The Fireplace

It holds **accounts, friends, and where a friend is** — which Flame and which
ember — so you can go and stand next to them. Every endpoint, the token
format and the limits are in [FIREPLACE.md](FIREPLACE.md); the design behind
the split is [TRUST.md](TRUST.md).

You do not need one to play. A Flame works on its own, and a client with no
account joins under a typed name exactly as before.

```sh
cargo run --release -p hearth-fireplace -- deploy/fireplace.toml
```

```powershell
cargo run --release -p hearth-fireplace -- deploy\fireplace.toml
```

```sh
curl -s localhost:7800/health                      # ok
curl -s -XPOST localhost:7800/accounts -H 'content-type: application/json'   -d '{"handle":"ada","password":"a long one"}'    # an account, and two tokens
```

- **TCP** `7800`, and **plain HTTP on purpose**. It belongs behind a TLS
  terminator (Caddy, nginx, a load balancer) that owns the certificate.
- It binds **`127.0.0.1`** by default, so nothing off the machine can reach
  it. Set `bind` only when your terminator is on another box — and then put a
  firewall in front of it.
- A **data directory** holding two files, both of which matter:
  `key.pk8`, the Ed25519 key it signs tokens with, made on first run and
  `0600`; and `people.json`, every account and every friendship. Losing the
  first signs everybody out at once; losing the second loses everybody.
- Rate limits and presence live in memory, so a restart forgives everybody and
  forgets where they were. It signs nobody out: tokens are signed, not stored.

Every key it takes is in [`deploy/fireplace.toml`](https://github.com/koki26/hearth/blob/main/deploy/fireplace.toml).

Pointing a client at one is a single constant,
`ConnectDetails.DEFAULT_FIREPLACE` in `client/godot/connect.gd`; a player can
point their own somewhere else from the panel, behind the warning that says
what that means.

---

## As a service (systemd)

Two unit files are in [`deploy/`](https://github.com/koki26/hearth/tree/main/deploy). They expect a binary, a config
and a `data/` directory under `/opt/flame` and `/opt/fireplace`, owned by a
user that cannot log in:

```sh
sudo useradd --system --home /opt/flame --shell /usr/sbin/nologin hearth
sudo install -d -o hearth -g hearth /opt/flame /opt/flame/data
sudo install -o hearth -g hearth target/release/flame /opt/flame/
sudo install -o hearth -g hearth deploy/flame.toml /opt/flame/
sudo cp deploy/flame.service /etc/systemd/system/
sudo systemctl enable --now flame
```

```sh
systemctl status flame
journalctl -u flame -f          # everything it does, refusals included
sudo systemctl restart flame    # after dropping in a new binary
```

The Fireplace is the same with every `flame` swapped for `fireplace`, and
[ORACLE.md](ORACLE.md) walks through exactly that, with the backups it wants
once it is holding real accounts. Both
stop cleanly on SIGTERM, so `systemctl stop` is not a kill, and neither reads
stdin when there is no console attached — the typed commands are for an
interactive Flame.

`RUST_LOG=debug` turns up the detail: `sudo systemctl edit flame` and add

```ini
[Service]
Environment=RUST_LOG=debug
```

---

## Updating

A Flame and a client must agree on the protocol version
([PROTOCOL.md](PROTOCOL.md)); a mismatch is refused with *bad version* rather
than failing obscurely, so **update both together**.

```sh
git pull
cargo build --release -p hearth-flame
sudo install -o hearth -g hearth target/release/flame /opt/flame/
sudo systemctl restart flame
```

Back up `data/` — in particular `data/cert.der` and `data/key.der`. A new certificate
is not the end of the world today (clients do not verify it yet, see the
README's *Heads up*), but it will be once they pin it.
