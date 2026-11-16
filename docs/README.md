---
description: A small social VR space. Host a Flame, build a world, meet your friends in it.
---

# Welcome to Hearth

Hearth is a small social VR space. Somebody runs a server — a **Flame** —
everyone joins it, and you see each other's head and hands move in real time.
You build the places you meet in with **Hearth Studio**, drop them on a Flame,
and walk between them through portals.

Rust and Godot 4, QUIC over the wire, MIT licensed.

{% hint style="info" %}
New here? Start with **[Getting started](getting-started.md)**: host a Flame,
join it, and build your first world in a few minutes.
{% endhint %}

## Two servers, two jobs

|                | **A Flame**                          | **The Fireplace**                          |
| -------------- | ------------------------------------ | ------------------------------------------ |
| What it is     | the game server: worlds, instances, voice | accounts, friends, and where a friend is |
| Who runs it    | anybody — you, a friend, a club      | one, for everybody                         |
| Needed to play | yes                                  | no — guests play without an account        |
| Speaks         | QUIC ([protocol](PROTOCOL.md))       | HTTPS JSON ([API](FIREPLACE.md))           |

A Flame never learns your password or your email. It only gets your display
name and an id that means nothing anywhere else. Why it is split this way is
[Trust, data and accounts](TRUST.md).

## Find your way

{% content-ref url="getting-started.md" %}
[getting-started.md](getting-started.md)
{% endcontent-ref %}

{% content-ref url="RUNNING.md" %}
[RUNNING.md](RUNNING.md)
{% endcontent-ref %}

{% content-ref url="operator.md" %}
[operator.md](operator.md)
{% endcontent-ref %}

{% content-ref url="WORLD_FORMAT.md" %}
[WORLD_FORMAT.md](WORLD_FORMAT.md)
{% endcontent-ref %}

## The code

| Piece                                          | Where                          |
| ---------------------------------------------- | ------------------------------ |
| A Flame: game server, worlds and instances     | `server/hearth-flame`          |
| The window a Flame runs in                     | `server/hearth-flame-gui`      |
| Wire protocol, used by all three               | `shared/hearth-protocol`       |
| The Fireplace: accounts, friends, presence     | `main-server/hearth-fireplace` |
| Godot extension (`HearthClient` node)          | `client/hearth-client`         |
| The VR scene, its scripts, and Hearth Studio   | `client/godot`                 |

Source: [github.com/koki26/hearth](https://github.com/koki26/hearth).
Every design choice and the reason for it: [Design decisions](decisions.md).
