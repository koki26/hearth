class_name Mirror
extends Node3D

## A mirror you can stand in front of, in two qualities.
##
## It renders the room a second time from the reflected point of view, which
## is the expensive part, so it is off until you ask for it and there is a
## cheap setting that only shows people:
##
##   OFF      nothing rendered at all
##   PLAYERS  a small view, player layer only -- see yourself and whoever is
##            with you, against an empty background, for about a tenth the
##            pixels
##   WORLD    a full view of everything
##
## In the headset this is a whole extra render per eye, so PLAYERS is the one
## to leave on and WORLD is for looking at yourself for a moment.

enum { OFF, PLAYERS, WORLD }

## Pixels across, per quality. Square, because the glass is.
const SIZE_LOW := 320
const SIZE_HIGH := 1024
## Everything is on layer 1; people are also on layer 2 (see main.gd), which
## is what lets the cheap setting skip the world.
const LAYER_PLAYERS := 2
const LAYER_EVERYTHING := 0xFFFFF

var quality := OFF: set = set_quality

var _viewport: SubViewport
var _camera: Camera3D
var _glass: MeshInstance3D
var _material: StandardMaterial3D
var _width := 1.6
var _height := 2.2


func _ready() -> void:
	_build()
	set_quality(quality)


## The frame and the glass. Built once; only the camera and the viewport
## change when the quality does.
func _build() -> void:
	var frame := MeshInstance3D.new()
	var box := BoxMesh.new()
	box.size = Vector3(_width + 0.14, _height + 0.14, 0.08)
	frame.mesh = box
	var metal := StandardMaterial3D.new()
	metal.albedo_color = Color(0.22, 0.23, 0.28)
	metal.metallic = 0.85
	metal.roughness = 0.3
	frame.material_override = metal
	frame.position.z = -0.05
	add_child(frame)

	var stand := MeshInstance3D.new()
	var post := CylinderMesh.new()
	post.top_radius = 0.09
	post.bottom_radius = 0.26
	post.height = 0.2
	stand.mesh = post
	stand.material_override = metal
	stand.position = Vector3(0.0, -_height * 0.5 - 0.1, -0.05)
	add_child(stand)

	var body := StaticBody3D.new()
	body.collision_layer = 1
	body.collision_mask = 1
	var shape := CollisionShape3D.new()
	var solid := BoxShape3D.new()
	solid.size = Vector3(_width + 0.14, _height + 0.14, 0.08)
	shape.shape = solid
	shape.position.z = -0.05
	body.add_child(shape)
	add_child(body)

	_viewport = SubViewport.new()
	_viewport.render_target_update_mode = SubViewport.UPDATE_DISABLED
	add_child(_viewport)
	_camera = Camera3D.new()
	_viewport.add_child(_camera)

	_glass = MeshInstance3D.new()
	var quad := QuadMesh.new()
	quad.size = Vector2(_width, _height)
	_glass.mesh = quad
	_material = StandardMaterial3D.new()
	_material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	# A reflection is laterally inverted and `looking_at` is not, so the image
	# is flipped here instead of in the maths, where it would cost a basis
	# with a negative determinant and an upside-down render.
	_material.uv1_scale = Vector3(-1.0, 1.0, 1.0)
	_glass.material_override = _material
	add_child(_glass)


func set_quality(level: int) -> void:
	quality = clampi(level, OFF, WORLD)
	if _viewport == null:
		return
	if quality == OFF:
		_viewport.render_target_update_mode = SubViewport.UPDATE_DISABLED
		_glass.visible = false
		# A dark pane, so it still reads as a mirror that is switched off.
		return
	var pixels := SIZE_HIGH if quality == WORLD else SIZE_LOW
	_viewport.size = Vector2i(pixels, int(pixels * _height / _width))
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	_camera.cull_mask = LAYER_EVERYTHING if quality == WORLD else LAYER_PLAYERS
	# Showing everything means showing the sky too, so the viewport takes the
	# world's own environment. Showing only people means there is nothing
	# behind them, and a transparent background would leave a black hole.
	_viewport.transparent_bg = quality != WORLD
	_camera.environment = null if quality == WORLD else _empty_world()
	_material.albedo_texture = _viewport.get_texture()
	_glass.visible = true


## The cheap setting draws people against nothing, so it needs an environment
## that is nothing rather than the world's sky.
func _empty_world() -> Environment:
	var environment := Environment.new()
	environment.background_mode = Environment.BG_COLOR
	environment.background_color = Color(0.02, 0.02, 0.03)
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	environment.ambient_light_color = Color(0.55, 0.58, 0.68)
	environment.ambient_light_energy = 1.4
	return environment


func what() -> String:
	match quality:
		PLAYERS:
			return "people"
		WORLD:
			return "everything"
	return "off"


## Call every frame with the player's camera. Does nothing while off, which is
## the point of off.
func look(head: Camera3D) -> void:
	if quality == OFF or _camera == null:
		return
	# The reflected eye: the player's position mirrored through the glass,
	# aimed at the mirrored point they are looking at.
	var where := _reflect(head.global_position)
	var at := _reflect(head.global_position - head.global_transform.basis.z)
	var up := _reflect_direction(head.global_transform.basis.y)
	if where.distance_to(at) < 0.001:
		return
	_camera.global_transform = Transform3D(Basis.IDENTITY, where).looking_at(at, up)
	_camera.fov = head.fov


func _reflect(point: Vector3) -> Vector3:
	var normal := global_transform.basis.z.normalized()
	return point - 2.0 * normal * normal.dot(point - global_position)


func _reflect_direction(direction: Vector3) -> Vector3:
	var normal := global_transform.basis.z.normalized()
	return direction - 2.0 * normal * normal.dot(direction)
