#!/usr/bin/env bash
# Starts a Flame: one Hearth game server. The Linux and macOS twin of
# start.bat. Pass --gui for the window instead of the console -- the other way
# round from the Windows script, because a server usually has no desktop.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"

# The Flame hands out worlds from data/worlds. Pack the ones the client ships
# with, so a brand new Flame already has a hub to spawn people into. Packed
# every start, not just the first: a stale hub.hworld would otherwise be handed
# out for ever after the world was edited. It takes milliseconds.
mkdir -p data/worlds
if command -v zip >/dev/null 2>&1; then
	for world in client/godot/worlds/*/; do
		[ -f "$world/world.json" ] || continue
		name="$(basename "$world")"
		# Removed first: zip adds to an archive that is already there, which
		# would keep handing out files the world no longer has.
		rm -f "$root/data/worlds/$name.hworld"
		(
			cd "$world"
			# Named one by one rather than zipping the folder: a client
			# refuses a package holding anything but world.json and
			# assets/*.glb (docs/WORLD_FORMAT.md), and that includes the
			# "./world.json" a `zip -r .` would write.
			files=(world.json)
			for model in assets/*.glb; do
				[ -f "$model" ] && files+=("$model")
			done
			zip -q "$root/data/worlds/$name.hworld" "${files[@]}"
		)
		echo "Packed $name into data/worlds/$name.hworld"
	done
else
	echo "zip is not installed, so no worlds were packed (sudo apt install zip)."
	echo "The Flame still runs; players see their own built-in room instead."
fi

target=flame
[ "${1:-}" = "--gui" ] && target=flame-gui

# Always let cargo look: an exe left over from an older protocol cannot talk to
# a current client, and the build is a no-op when nothing has changed.
if command -v cargo >/dev/null 2>&1; then
	echo "Building $target ..."
	cargo build --release -p "hearth-$target"
elif [ ! -x "target/release/$target" ]; then
	echo "target/release/$target is missing and cargo is not installed." >&2
	exit 1
fi

exec "target/release/$target" deploy/flame.toml
