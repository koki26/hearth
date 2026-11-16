extends Node3D

## The way between worlds: one ring per running instance, laid out in front of
## where you spawn. Walking into one asks the server to move you.
##
## This is the whole navigation UI. It works the same in VR and on the desktop,
## which no 2D menu would without a laser pointer and a keyboard nobody has in
## a headset.

signal portal_entered(instance: int)
signal create_requested(slug: String)

const TRIGGER := 0.8
const SPACING := 2.4

## True while the player is standing in a portal, so one step is one trip
## even when the list is rebuilt under their feet.
var _inside := false


## `entries` are dictionaries of {"label": String} plus either an "instance" to
## join or a "slug" to start a new instance of.
func show_portals(entries: Array, at: Transform3D) -> void:
	for child in get_children():
		child.free()
	var row := entries.size()
	for i in row:
		var entry: Dictionary = entries[i]
		var offset := (float(i) - float(row - 1) * 0.5) * SPACING
		var portal := _ring(entry)
		portal.position = at.origin + at.basis * Vector3(offset, 1.1, -4.0)
		portal.rotation.y = at.basis.get_euler().y
		portal.set_meta("entry", entry)
		add_child(portal)


func _ring(entry: Dictionary) -> Node3D:
	var holder := Node3D.new()
	var mesh := TorusMesh.new()
	mesh.inner_radius = 0.75
	mesh.outer_radius = 0.9
	# A ring you walk into is a control, so it takes the interface colours
	# (ui.gd): the accent for somewhere that exists, "good" for one you make.
	var is_new: bool = entry.has("slug")
	var material := UiStyle.glow(UiStyle.GOOD if is_new else UiStyle.ACCENT, 2.0)
	var visual := MeshInstance3D.new()
	visual.mesh = mesh
	visual.material_override = material
	visual.rotation.x = deg_to_rad(90.0)
	holder.add_child(visual)

	var label := UiStyle.label(str(entry.get("label", "")), UiStyle.FONT)
	label.position.y = 1.2
	label.pixel_size = 0.0035
	label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	label.no_depth_test = true
	holder.add_child(label)
	return holder


## Call every frame with where the player is. Walking in once is one trip:
## the next one needs stepping out and back in.
func check(player: Vector3) -> void:
	var entered: Dictionary = {}
	for portal in get_children():
		var node: Node3D = portal
		if node.global_position.distance_to(player) <= TRIGGER:
			entered = node.get_meta("entry", {})
			break
	if entered.is_empty():
		_inside = false
		return
	if _inside:
		return
	_inside = true
	if entered.has("instance"):
		portal_entered.emit(int(entered["instance"]))
	elif entered.has("slug"):
		create_requested.emit(str(entered["slug"]))
