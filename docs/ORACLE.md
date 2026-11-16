# The official Fireplace, on Oracle Cloud Always Free

How `fireplace.example.org` is set up: one Always Free instance
running the Fireplace, behind Caddy, with a certificate it renews itself.

**Only the Fireplace goes here.** A Flame — the game server — is hosted by
anybody, including you at home, and needs no cloud at all: see
[RUNNING.md](RUNNING.md). That split is the point of the whole design
([TRUST.md](TRUST.md)): one account server, many game servers, and a game
that keeps working when this box does not.

Budget an hour, most of it waiting for Oracle.

## What Always Free gives you

Checked against Oracle's own
[Always Free resources](https://docs.oracle.com/en-us/iaas/Content/FreeTier/freetier_topic-Always_Free_Resources.htm)
page in November 2026:

| | |
|---|---|
| Arm (Ampere A1) | 1,500 OCPU-hours and 9,000 GB-hours a month — **2 OCPUs and 12 GB** running all month |
| x86 | two `VM.Standard.E2.1.Micro`: ⅛ OCPU and 1 GB each |
| Storage | 200 GB of block and boot volume in the home region |
| Outbound traffic | 10 TB a month |

The Fireplace is tiny: a few hundred kilobytes of accounts, and one small
request every 45 seconds per player signed in. The reason to take the Arm
instance anyway is that **argon2id costs 19 MiB and real CPU per sign-in, on
purpose**, and that building Rust on a 1 GB micro needs swap to finish at all.

Three things to know before leaning on it:

- **The Arm allowance was halved during 2026** (it was 4 OCPUs and 24 GB). Do
  not build on numbers from an older guide.
- **A1 capacity is often full** in popular regions. "Out of host capacity" is
  not your mistake: try another availability domain, another region, or again
  later. Pick your home region carefully at signup — it cannot be moved.
- **Always Free is not a promise.** Oracle reclaims idle instances on free
  accounts. Keep the data backed up elsewhere and be able to rebuild this in
  an hour, which is what this page is for.

## 1. The instance

Console → **Compute → Instances → Create instance**.

- **Image:** Canonical Ubuntu, 24.04 LTS or newer.
- **Shape:** `VM.Standard.A1.Flex`, **2 OCPUs, 12 GB**. Check that it says
  *Always Free-eligible*.
- **Networking:** a new VCN with a public subnet, and **assign a public IPv4
  address**.
- **SSH keys:** paste your public key. There is no password login.

```sh
ssh ubuntu@<your-public-ip>
```

## 2. The name, on Cloudflare

In your DNS provider's dashboard for `example.org`, add an `A` record:

| Type | Name | Content | Proxy |
|---|---|---|---|
| A | `fireplace` | your instance's public IP | **DNS only** |

**DNS only — the grey cloud, not the orange one.** A proxied record puts
Cloudflare's edge between Let's Encrypt and your box, and the edge terminates
TLS there, which breaks both of the challenges Caddy can use to get a
certificate.

If you later want the orange cloud for its own sake, the way through is a
Caddy built with the Cloudflare DNS module (`xcaddy`) and an API token scoped
to DNS edit, using the DNS-01 challenge instead. That is a variant, not the
default, and nothing here needs it.

```sh
dig +short fireplace.example.org    # your instance's IP
```

## 3. Open the ports — in **both** places

This is where everyone loses an afternoon. Oracle filters traffic in the
cloud, *and* the Ubuntu image ships its own `iptables` rules that drop
everything except SSH. A port has to be open in both or it is closed.

Only **TCP 80 and 443**. No UDP — that is a Flame's business, and there is no
Flame here. **Never open 7800:** the Fireplace speaks plain HTTP and is
reached only by the terminator on its own machine.

**a) The VCN security list.** Networking → Virtual Cloud Networks → your VCN →
the public subnet → its Security List → **Add Ingress Rules**: source
`0.0.0.0/0`, TCP, destination ports `80` and `443`, stateful.

**b) The host's iptables.** The default `INPUT` chain ends in a REJECT, and a
rule appended after it never runs — so insert, do not append:

```sh
sudo iptables -L INPUT --line-numbers      # find the REJECT line number
sudo iptables -I INPUT 6 -p tcp --dport 80 -j ACCEPT
sudo iptables -I INPUT 6 -p tcp --dport 443 -j ACCEPT
sudo netfilter-persistent save             # survives a reboot
```

Use the line number *of the REJECT rule* in place of `6`, so the new rules
land above it. Do not turn on `ufw` as well: on these images it fights
Oracle's own rules and can lock you out of SSH.

## 4. Build it

```sh
sudo apt update
sudo apt install -y build-essential git
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"

git clone <your-fork-or-this-repo> hearth
cd hearth
cargo build --release -p hearth-fireplace
cargo test -p hearth-fireplace             # a few seconds, and worth it here
```

Pure Rust: no CMake, no libopus, nothing Arm-specific. A few minutes on two
OCPUs.

## 5. Install it as a service

```sh
sudo useradd --system --home /opt/fireplace --shell /usr/sbin/nologin hearth
sudo install -d -o hearth -g hearth /opt/fireplace /opt/fireplace/data
sudo install -o hearth -g hearth target/release/fireplace /opt/fireplace/
sudo install -o hearth -g hearth deploy/fireplace.toml /opt/fireplace/
# Email: the SMTP password, one line, readable by the service alone
# (docs/SELFHOST-MAIL.md). Fill in [operator] in fireplace.toml as well:
# with email on, the Fireplace will not start until it says who runs it.
sudo install -d -m 700 -o hearth -g hearth /opt/fireplace/secrets
sudo install -m 600 -o hearth -g hearth /dev/null /opt/fireplace/secrets/smtp-password
sudo -u hearth nano /opt/fireplace/secrets/smtp-password
sudo -u hearth /opt/fireplace/fireplace admin test-mail you@example.org /opt/fireplace/fireplace.toml
sudo cp -r web /opt/fireplace/ && sudo chown -R hearth:hearth /opt/fireplace/web
sudo cp deploy/fireplace.service /etc/systemd/system/
sudo systemctl enable --now fireplace

journalctl -u fireplace -f
curl -s http://127.0.0.1:7800/health       # ok
```

It listens on `127.0.0.1` and prints its public signing key in the first log
line. Leave `bind` alone: a port that is never open is a better last line than
a firewall rule that might be wrong.

## 6. HTTPS, with Caddy in front

```sh
sudo apt install -y caddy
sudo tee /etc/caddy/Caddyfile >/dev/null <<'EOF'
fireplace.example.org {
	reverse_proxy 127.0.0.1:7800
}
EOF
sudo systemctl restart caddy
journalctl -u caddy -f        # watch it fetch the certificate
```

That is the whole of it. Caddy owns 80 and 443 and renews the certificate on
its own; the Fireplace stays on localhost, speaking the plain HTTP it says it
speaks. Caddy also appends the real client address to `X-Forwarded-For`, which
is what the Fireplace's rate limiting reads — and it believes that header only
on a connection that came from loopback, so nothing a stranger puts in it
counts.

From your own machine:

```sh
curl -s https://fireplace.example.org/health      # ok
curl -s https://fireplace.example.org/key         # the signing key
```

Then make the first account — yours, before anybody else's:

```sh
curl -s -XPOST https://fireplace.example.org/accounts \
  -H 'content-type: application/json' \
  -d '{"handle":"you","password":"something you wrote down"}'
```

It answers with eight `recovery_codes`. Keep them with the password: they
are how you get back in without email ([FIREPLACE.md](FIREPLACE.md)).

## 7. Back it up before anybody else signs up

Two files matter, both in `/opt/fireplace/data`. Losing `people.json` loses
everybody; losing `key.pk8` signs everybody out at once and cannot be undone.

```sh
sudo tee /etc/systemd/system/fireplace-backup.service >/dev/null <<'EOF'
[Unit]
Description=Pack up the Fireplace's data
[Service]
Type=oneshot
# Root-only: the archive holds email addresses.
UMask=0077
ExecStart=/usr/bin/tar czf /opt/fireplace/backup.tar.gz -C /opt/fireplace data
EOF
sudo tee /etc/systemd/system/fireplace-backup.timer >/dev/null <<'EOF'
[Unit]
Description=Nightly Fireplace backup
[Timer]
OnCalendar=daily
Persistent=true
[Install]
WantedBy=timers.target
EOF
sudo systemctl daemon-reload
sudo systemctl enable --now fireplace-backup.timer
```

A tarball on the same box is half a backup. Pull it down from your own
machine. **It holds email addresses, so it is personal data: encrypt it the
moment it lands, and keep only the last 14 days** -- that is the
`backup_days` `/privacy` promises, and a deleted account must really be gone
from backups by then. With [age](https://age-encryption.org):

```sh
ssh ubuntu@<ip> sudo cat /opt/fireplace/backup.tar.gz \
  | age -r <your age public key> > fireplace-$(date +%F).tar.gz.age
find . -name 'fireplace-*.tar.gz.age' -mtime +14 -delete
```

Or with 7-Zip, AES-256 with the file names hidden too (`-mhe=on`), then
delete the plain copy:

```sh
ssh ubuntu@<ip> sudo cat /opt/fireplace/backup.tar.gz > backup.tar.gz
7z a -p -mhe=on fireplace-$(date +%F).7z backup.tar.gz && rm backup.tar.gz
```

Keep the age key or the 7-Zip password somewhere other than next to the
backups.

**The restore drill**, worth doing once now rather than reading for the first
time in an emergency: stop the service, unpack the archive over
`/opt/fireplace`, `chown -R hearth:hearth /opt/fireplace/data`, start it, and
check that a token issued *before* the restore still works (`GET /me` with
it). If it does, the key came back with the accounts. Decrypt first:
`age -d -i key.txt fireplace-<date>.tar.gz.age > backup.tar.gz`.

**A restore brings deleted accounts back.** After restoring, go through the
erasure requests answered since that backup was taken and delete those
accounts again ([PRIVACY-OPS.md](PRIVACY-OPS.md)).

## 8. Afterwards

```sh
systemctl status fireplace caddy
journalctl -u fireplace --since "1 hour ago"
```

**Updating:**

```sh
cd ~/hearth && git pull
cargo build --release -p hearth-fireplace
sudo install -o hearth -g hearth target/release/fireplace /opt/fireplace/
sudo cp -r web /opt/fireplace/ && sudo chown -R hearth:hearth /opt/fireplace/web
sudo systemctl restart fireplace
```

A change to the account page alone (`web/`) needs only the `cp` line -- no
build and no restart:

```sh
sudo cp -r web /opt/fireplace/ && sudo chown -R hearth:hearth /opt/fireplace/web
```

A restart signs nobody out: tokens are signed, not stored, and the key stays
on disk. Presence is forgotten, and every client says where it is again
within a minute.

**Pointing the game at it** is one line — `ConnectDetails.DEFAULT_FIREPLACE`
in `client/godot/connect.gd`, which already reads
`https://fireplace.example.org`. A player can point their own client
somewhere else, behind the warning page that says what that means
(`docs/decisions.md` D54).

**Watch** the Always Free usage page for OCPU-hours and egress, and set a
budget alert at $0.01, so a change in the free allowance arrives as an email
rather than a bill.
