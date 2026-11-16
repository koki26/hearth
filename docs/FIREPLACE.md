# The Fireplace API

The Fireplace is the one server that knows who people are: accounts, friends,
and where a friend is. Why it is separate from a Flame, and what each is
trusted with, is [TRUST.md](TRUST.md). How to run one is
[RUNNING.md](RUNNING.md); where the official one lives is
[ORACLE.md](ORACLE.md).

JSON in, JSON out, over HTTP behind a TLS terminator. Only `GET` and `POST`:
the client is a game engine, not a REST purist.

## What it is not

- **There is no password reset.** No email is collected, so there is nobody to
  send one to. A forgotten password is a new account.
- **There is no account deletion** yet, and ids are never reused — the id in a
  signed token has to keep meaning one person.
- **There is no revocation.** A token is good until it expires. Bumping an
  account's `token_epoch` in `people.json` invalidates every token it holds,
  which is the lever if one is ever needed.
- **A handle is not the name over your head in game.** That is typed on the
  lift panel and is a Flame's business. A handle is a login.
- **It never sees a world, a pose, a shot or a password in the clear.**

## Accounts

A handle is 3–20 of `A-Z a-z 0-9 - _`, starting with a letter. Two accounts
cannot differ only by case: `Ada` and `ada` are the same name, and a handle
that can be mistaken for somebody else's is a way to be them. A password is
8–128 characters, hashed with argon2id, and nothing else about it is our
business.

| | |
|---|---|
| `POST /accounts` | `{"handle": "ada", "password": "..."}` → `201` |
| `POST /sessions` | the same body → `200` |
| `POST /sessions/refresh` | `{"refresh": "v1...."}` → `200` |
| `GET /me` | → `{"id", "handle", "share_location", "friends"}` |

The first three answer with the same thing:

```json
{ "id": 7, "handle": "Ada", "access": "v1.…", "refresh": "v1.…", "expires_in": 3600 }
```

A wrong password and an unknown handle give the same `401` and the same
sentence. Whether somebody has an account here is not ours to hand out.

## Tokens

```
v1.<base64url(claims)>.<base64url(ed25519 signature))>
```

The signature covers the ASCII bytes of `v1.<claims>`. Claims:

```json
{ "sub": 7, "handle": "Ada", "typ": "access", "epoch": 1, "iat": 1762, "exp": 5362 }
```

JWT-shaped, deliberately not a JWT: no dependency, and no `alg` field for
anyone to lie about. Anything whose prefix is not exactly `v1.` is refused
rather than guessed at.

An **access** token lasts an hour and is what `Authorization: Bearer …`
carries. A **refresh** token lasts 30 days, is kept on the player's own
machine, and buys a new pair — which is why a password is typed on a keyboard
floating in mid-air about once a month. Neither is the other: a refresh token
cannot ask for anything, and an access token cannot be refreshed with.

`GET /key` publishes the public half:

```json
{ "algorithm": "ed25519", "key": "<64 hex characters>" }
```

Nothing but the Fireplace verifies a token today. That endpoint is what lets a
Flame do it offline later — holding the key, never calling here — which is the
whole point of signing them (TRUST.md).

## Friends

All of these need a Bearer token.

| | |
|---|---|
| `GET /friends` | friends with where they are, plus requests either way |
| `POST /friends/add` | `{"handle": "bob"}` → `{"state": "requested"}` |
| `POST /friends/accept` | `{"handle": "bob"}` → `{"state": "friends"}` |
| `POST /friends/remove` | `{"handle": "bob"}` → `204` |

Adding somebody who has already asked you makes you friends instead of
queueing a second request — which is what two people adding each other at the
same time expect. `remove` is one button for three states: unfriend, decline,
and take back a request, from either side. Friendship is mutual or it is not
friendship; both accounts are written in one save.

```json
{
  "friends": [{ "id": 3, "handle": "bob", "online": true,
                "flame": "1.2.3.4:7777", "ember": 3,
                "ember_name": "the loft", "world": "forest" }],
  "incoming": [{ "id": 9, "handle": "cat" }],
  "outgoing": []
}
```

An account may have 256 friends and 64 requests waiting.

## Presence

| | |
|---|---|
| `POST /presence` | `{"flame", "ember", "ember_name", "world"}`, or `{}` to clear → `204` |
| `POST /settings` | `{"share_location": false}` → `200` |

**A client says where it is; a Flame never does.** A Flame that had to tell
the Fireplace who was in it would put this server in the path of every match,
and `docs/TRUST.md` refuses that. The cost is that a client can claim to be
somewhere it is not — it can only lie about itself, and the lie is "I am in
the forest".

What it says is still checked, because `flame` ends up in a friend's client
and from there in a connection attempt: 64 bytes at most, no spaces, no
control characters, `world` and `ember_name` through the same rules the game
protocol uses.

Reports last **120 seconds**; the client sends one every 45. A client that
crashes, or a player who takes the headset off and walks away, goes quiet on
its own with no goodbye needed. Presence lives in memory only — a restart
drops it and everybody's client says where it is again within a minute.

Turning sharing off hides you **at once**, not when the last report runs out,
and reports sent while it is off are accepted and remembered nowhere. Only
accepted friends ever see any of it: `GET /friends` is the one thing that
reads presence, and it only ever walks the asker's own friends list.

## When something is refused

Always `{"error": "a short sentence"}`, never a stack trace.

| | |
|---|---|
| `400` | a handle, password, address or world that will not do |
| `401` | no token, a bad one, an expired one, or the wrong kind |
| `404` | no account by that handle |
| `409` | handle taken, already friends, too many friends or requests |
| `413` | a body over 8 KB |
| `429` | too many tries |
| `503` | this Fireplace is full |

## Limits

| | |
|---|---|
| Sign-in | 10 tries per 5 minutes, per address **and** per handle |
| New accounts | 10 an hour per address (`accounts_per_hour`), counted only for real attempts — everybody in one house shares an address |
| Friend requests | 60 an hour per account |
| Request body | 8 KB |
| Request time | 10 seconds |

Counted per address, which behind a terminator means the **last** entry of
`X-Forwarded-For`, and only when the connection itself came from loopback. A
proxy appends the peer it saw, so anything a client puts in that header is
pushed left of the truth; anywhere else the header is just something a
stranger typed.

Limits live in memory, so a restart forgives everybody. That is the right
trade here: the alternative is writing to disk on every failed password,
which is a way to be attacked rather than a defence.

## On disk

```
data/
  key.pk8        the Ed25519 signing key, 0600, made on first run
  people.json    every account and every friendship
```

Both matter. Losing `people.json` loses everybody; losing `key.pk8` signs
everybody out at once and cannot be undone. Back them up before the first
account that is not yours, and keep the backup off the box.

`people.json` carries a `version`, and an account record may **only ever gain
optional fields** — a rolled-back binary has to be able to read a file a newer
one wrote, or a bad deploy becomes lost accounts.
