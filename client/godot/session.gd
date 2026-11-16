extends Node

## What the game and the studio hand each other, and where the client keeps
## the worlds it downloads. An autoload, because it has to survive the scene
## change between the two.

## A world folder to load instead of the room built into the exe.
var world_dir := ""
## Studio test mode: play the world with no server and no connect screen.
var offline := false
## How the player chose to play, at the launcher. Read before the first frame,
## because OpenXR has to be started (or left alone) before anything is drawn.
var vr := false
## Where downloaded worlds land, one folder per content hash.
const CACHE := "user://worlds_cache"
## Where the studio keeps the worlds you are building.
const WORLDS := "user://worlds"
## Where "Pack" writes `.hworld` files for a server.
const PACKS := "user://packs"
## The one settings file: the connect screen's last address, and the volumes.
const SETTINGS := "user://settings.cfg"
