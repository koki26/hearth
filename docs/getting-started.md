---
description: Host a Flame, join it from the client, and build your first world.
---

# Getting started

## What you need

* [Rust](https://rustup.rs) 1.95 or newer
* To build the client: [CMake](https://cmake.org), a Visual Studio C++
  toolchain and [Godot 4.7](https://godotengine.org). Players running the
  exported exe need none of these.
* For VR: a headset with any OpenXR runtime. The desktop works too.

## 1. Host a Flame

{% tabs %}
{% tab title="Windows" %}
Double-click **`start.bat`** in the repository. It builds the Flame the first
time, then opens its window.
{% endtab %}

{% tab title="Linux / macOS" %}
```sh
./start.sh
```
{% endtab %}
{% endtabs %}

Press **Start** and give your friends the address next to *Friends connect
to*. For people outside your network, forward **UDP port 7777** to your
machine.

Everyone lands in the **hub**, the world that always runs. Drop a packed
world (`.hworld`) on the window to add it. Everything the Flame does —
refusals included — is in its log, so "it will not let me in" has an answer on
screen.

{% hint style="warning" %}
The client does not verify the Flame's certificate yet. Keep a Flame to a LAN,
a VPN or a tunnel you trust until certificate pinning lands (decisions D3, D27).
{% endhint %}

## 2. Join it

Build the client once:

```powershell
client/build.ps1 -Export   # -> dist/Hearth.exe and dist/HearthStudio.exe
```

{% hint style="info" %}
Run it with `-Export` again after **any** change under `client/godot`.
Without it, `dist/Hearth.exe` stays exactly as it was — and a stale exe looks
like a change that did not work.
{% endhint %}

Start `Hearth.exe`, pick **In the headset** or **On this screen**, type the
address, and join. Sign in with a Fireplace account to keep your saves, or
play as a guest.

## 3. Build a world

Open **`HearthStudio.exe`**, next to `Hearth.exe`. Click a piece on the left,
then the ground to place it. Drag to move, hold **Alt** to lift, type exact
numbers on the right. **Import a model…** takes `.glb`, `.gltf`, `.obj` and
`.fbx`.

**Test** drops you into the world on the desktop (**F10** to come back).
**Pack .hworld** writes the one file you hand to a Flame.

The format is a zipped folder with a `world.json` and a few `.glb` files —
see [World format](WORLD_FORMAT.md).

## Next

* Run a Flame or a Fireplace for real: [Running the servers](RUNNING.md)
* Look after a Fireplace: [Operator cheatsheet](operator.md)
