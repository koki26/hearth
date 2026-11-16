# Trust, data and accounts

Where Hearth is going: one **Fireplace** that knows who people are, and many
**Flames** that other people host. This is the design the three things in
flight share -- shooting, stored data, and accounts -- because all three are
the same question wearing different hats: *who is allowed to say that?*

## The rule

**The client is a renderer and an input device. It is not a source of truth.**

Anything a client says is a *claim*. The server decides what actually
happened, tells everybody, and writes down anything that has to survive.
A patched client must only ever be able to hurt itself.

That is not where the code is today, and the gap is worth being honest about:

| | today | wanted |
|---|---|---|
| where you are | client says, server relays | fine as is -- it is a pose, not a fact |
| what you hold | **server decides** (D29) | as is |
| `locked` items | client enforces (D29) | the Flame, once it reads world metadata |
| health, death | — | the Flame |
| stored data | **the Flame**, on disk, per account; guests get none | as is |
| who you are | **a join ticket** from the Fireplace, checked by the Flame; a guest is a typed name | as is |

## Shooting

The hard constraint: **the server never reads a world** (D3, D27). It hands
out `.hworld` files without parsing them, which is what makes hosting one
safe. So the server cannot raycast. It does not know where the walls are.

That rules out the obvious design, where the server traces every shot. What
is left is the split most shooters actually use:

* The shooter's client traces the shot locally and **claims** a hit.
* The server owns **health, death, respawn and score**, and nobody else
  writes them.
* The server **sanity-checks** every claim it can without a world: the target
  exists, is in the same instance, is alive, is within the weapon's range of
  the shooter's last known pose, and the shooter has not fired faster than
  the weapon allows.

What the server cannot check is line of sight -- a patched client can claim a
hit through a wall. That is a known hole, written down rather than pretended
away, and it is the price of a server that never parses a world.

`shortcut: no line-of-sight check on hit claims, because the server does not
read worlds. Close it by having the server load a collision-only digest of
each world (a list of boxes), which is a format change, not a protocol one.`

Everything else a cheater might try is already covered: you cannot damage
somebody in another instance, you cannot fire faster than the rate limit, you
cannot set your own health, and you cannot report your own death or somebody
else's kill.

### On the wire (v4)

| message | who sends | what it means |
|---|---|---|
| `Shot { origin, direction }` | client → server | I fired. Unreliable: a lost tracer is a lost tracer. |
| `ShotRelay { from, origin, direction }` | server → all | so everybody draws the tracer |
| `Hit { target }` | client → server | I claim I hit this player. Reliable. |
| `Health { player, hp }` | server → all | the only thing that sets health |
| `Died { player, by }` | server → all | |
| `Spawned { player }` | server → all | |

## Stored data

Creators need somewhere to keep things: a score, a key, a door somebody
opened, what somebody built. It cannot live on the client, or the first
patched client rewrites it.

So: **a key/value store on the Flame, per player, namespaced by world.**

```
Store { key, value }     client → server, reliable
Fetch { key }            client → server, reliable
Value { key, value }     server → client
```

The rules that make it safe:

* **A client may only write its own data.** There is no "write to this other
  player". It is not addressable, so it cannot be abused.
* **Namespaced by the world you are in**, so one creator's world cannot read
  or clobber another's. The key a client sends is suffixed onto the world
  slug by the Flame; the client never names the namespace.
* **Quotas**, enforced on the Flame: a cap on key length, value size, number
  of keys per player per world, and a write rate limit. A client that goes
  over is told, not disconnected -- going over is usually a bug, not an
  attack.
* **Written to disk** under `data/players/`, so it survives a restart.

What it deliberately is not: a database creators can query across players.
That needs an index, a query language and a whole new set of ways to be
abused. One player's own data, fetched by key, is the thing that covers the
cases people actually have.

Until accounts exist the key is the player's **name**, which is as weak as it
sounds -- anybody can claim any name. That is exactly the gap accounts close,
and why the store is being built now and keyed on an identity later.

## Accounts

Two kinds of server, and they are not the same thing:

**The Fireplace.** One of them, run centrally. It knows who people are
and nothing else. It never sees a world, a pose or a shot. It issues a signed
token saying "this is player 1234, display name Ada, issued at T, good for an
hour".

**Flames.** Many, hosted by anybody. They verify the token's signature
against the Fireplace's public key, which they hold a copy of, and now
have a stable player id they did not have to trust the client for. They never
see a password and never talk to the Fireplace to check a login, so the
Fireplace is not in the path of a match and cannot take everybody down
with it.

```
      player                 the Fireplace              a Flame
        |  sign in (once)         |                        |
        |------------------------>|                        |
        |<-- token, signed -------|                        |
        |                                                  |
        |  Hello { version, name, token }                  |
        |------------------------------------------------->|
        |                                  verify signature |
        |                                  (public key, offline)
        |<-- Welcome { your stable id } --------------------|
```

Why signed tokens rather than a Flame asking the Fireplace: a Flame somebody
else hosts should not be able to harvest credentials, should not need
to be online to the central service mid-match, and should not be able to
impersonate a player to another Flame. A short-lived signed token gives all
three. Ed25519, because the signature is 64 bytes and verification needs no
state.

What stays authoritative where:

* **The Fireplace:** identity, display name, the public key, and anything
  that must mean the same thing everywhere.
* **A Flame:** everything about its own worlds -- health, items, stored
  data, who is in which instance. A Flame owns its own game and
  cannot write to your account.

That split is the point. A server somebody else hosts should be able to run a
whole game without being trusted with your identity, and your identity should
not depend on that server staying up.

### What exists now

The Fireplace holds **accounts, friends and presence**, and issues Ed25519
tokens. Every endpoint is in `docs/FIREPLACE.md`. What that buys today: a
handle people can add you by, a friends list, and where your friends are --
which Flame and which ember -- so you can go and stand next to them.

**Flames check who you are.** A signed-in client, after the TLS handshake,
asks the Fireplace for a *join ticket* naming the certificate that Flame just
showed it, and sends it in `Hello`. The Flame checks the signature offline with
the key it fetched from its Fireplace once (`fireplace` in `flame.toml`), and
learns a display name and a player id that is **only meaningful on that
Flame** -- never the account, never the handle (`docs/FIREPLACE.md`).

That answers the replay problem without waiting for pinning (D3): a ticket
names one certificate, so a Flame handed one cannot use it on another, and a
man in the middle showing its own certificate gets a ticket for itself and
nobody else.

**Saves belong to accounts.** The per-player store is keyed on that per-Flame
id. A guest -- no ticket -- still plays, if the Flame lets guests in
(`allow_guests`), but nothing is kept for them, `Welcome` tells their client
so (`welcome::SAVES`), and a world can check `saves_kept()` to say "sign in to
keep your progress". Typing somebody's name no longer gets you their saves.

Two consequences worth stating plainly:

* **Presence is reported by the player's own client**, never by a Flame. A
  Flame that had to call the Fireplace mid-match would put one server in the
  path of everybody's game. The cost is that a client can claim to be
  somewhere it is not; it can only ever lie about itself.
* **`friends` and `friends+` embers are still unenforced** (D58). A Flame
  knows a per-Flame id now, but not who anybody's friends are, and asking the
  Fireplace mid-match is exactly what this design refuses. That needs the
  ticket to carry something friend-shaped; it does not yet.
