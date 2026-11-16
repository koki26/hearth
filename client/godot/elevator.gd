class_name Elevator
extends Node3D

## The way into a world: a lift car with doors.
##
## It does not move until you ask it to. You start in the car at the top of the
## shaft with the doors shut and the panel on the wall beside you; once you
## have picked a server, `descend` sends it down, and the doors open on the
## world when it lands. Riding down before anybody had typed an address was a
## four second wait for nothing.
##
## The car carries the player: main.gd pins the play space to `car_y()` while
## `carrying()` is true. Left to gravity and the ground ray you hung in the air
## while the car dropped away at a constant speed, then free-fell onto it.
##
## The doors are on +X and the panel is on the -Z wall, so the one never covers
## the other. The rails run outside the other three sides, because inside they
## came up through the floor you were standing on.

signal arrived

## Waiting at the top, on the way down, landed and opening, open, falling away.
enum { PARKED, DESCENDING, ARRIVED, OPEN, LEAVING }

## How far above the spawn the car waits, and how fast it comes down: about
## four seconds, long enough to feel like a descent and short enough to wait.
const DROP := 26.0
const SPEED := 6.5
## The car is a box: half its width and depth, and its inside height.
const HALF := 1.5
const HEIGHT := 2.7
## How long the doors take to open, and how far each leaf travels.
const DOOR_TIME := 1.4
const DOOR_TRAVEL := 1.5
## The rails run outside the car, clear of the doorway.
const RAIL_OUT := HALF + 0.4
const RAIL_HEIGHT := 140.0
const RAIL_FADE := 4.0
## Once you are off, the car falls, speeding up, fading as it goes so it does
## not visibly sink through the floor of the world.
const FALL_GRAVITY := 14.0
const GONE_BELOW := 60.0

var _car: Node3D
var _deck: StaticBody3D
var _doors: Array[Node3D] = []
## The collision on the shut doors, thrown away once they open.
var _doorway: Array[CollisionShape3D] = []
var _fading: Array[StandardMaterial3D] = []
var _state := PARKED
var _floor_y := 0.0
var _opened := 0.0
var _fall := 0.0
var _fade := 0.0


## Puts the car at the top of the shaft over `at`, doors shut, and waits.
##
## Called again while it is already on its way, it moves to the new spawn and
## carries on from the height it had reached: that happens on every join,
## because the lift leaves as soon as you are connected and the world it is
## taking you to only finishes downloading a moment later.
func park(at: Transform3D) -> void:
	var moving := _car != null and _state <= DESCENDING
	var resume := _car.position.y - _floor_y if moving else DROP
	var was := _state if moving else PARKED
	clear()
	_floor_y = at.origin.y
	_state = was
	_opened = 0.0
	_fall = 0.0
	_fade = 0.0

	_car = Node3D.new()
	_car.name = "Car"
	_car.position = at.origin + Vector3.UP * resume
	_car.rotation.y = at.basis.get_euler().y
	add_child(_car)
	_build_car()
	_build_rails(at)


## Send it down. Does nothing if it is already going, or already there.
func descend() -> void:
	if _state == PARKED:
		_state = DESCENDING


func clear() -> void:
	for child in get_children():
		child.free()
	_car = null
	_deck = null
	_doors.clear()
	_doorway.clear()
	_fading.clear()
	_state = PARKED


func has_car() -> bool:
	return _car != null


## True while the car is the thing holding the player up, so main.gd pins the
## play space to it instead of letting gravity have a go.
func carrying() -> bool:
	return _car != null and _state < OPEN


## True until the doors are open: there is no world to walk into yet.
func descending() -> bool:
	return _car != null and _state < OPEN


func doors_open() -> bool:
	return _car != null and _state >= OPEN


## The world height of the car's floor: what the player stands on.
func car_y() -> float:
	return _car.position.y if _car != null else _floor_y


## How far above the spawn the car is.
func height() -> float:
	return _car.position.y - _floor_y if _car != null else 0.0


## Where to hang something on the car's -Z wall, `y` metres above its floor.
## Faces the middle of the car, which is where the player is standing.
func mount_at(y: float) -> Transform3D:
	if _car == null:
		return Transform3D.IDENTITY
	return _car.global_transform * Transform3D(Basis.IDENTITY, Vector3(0.0, y, -HALF + 0.09))


## Call every frame with where the player's head is: that is how stepping off
## is noticed.
func check(player: Vector3, delta: float) -> void:
	if _car == null:
		return
	match _state:
		DESCENDING:
			_car.position.y = maxf(_floor_y, _car.position.y - SPEED * delta)
			if is_equal_approx(_car.position.y, _floor_y):
				_state = ARRIVED
				arrived.emit()
		ARRIVED:
			_opened = minf(_opened + delta / DOOR_TIME, 1.0)
			_slide()
			if _opened >= 1.0:
				_state = OPEN
				# The way out stops being a wall.
				for shape in _doorway:
					shape.queue_free()
				_doorway.clear()
		OPEN:
			# Most of a metre of slack, so leaning out is not getting off.
			var flat := Vector2(player.x - _car.position.x, player.z - _car.position.z)
			if flat.length() > HALF + 0.9:
				_state = LEAVING
				# Nothing should stand on it, or be clipped by it, on the way.
				if _deck != null:
					_deck.collision_layer = 0
				# Alpha blending is a second pass in both eyes, so it is
				# switched on only now, for the few seconds the fade lasts.
				for material in _fading:
					material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		LEAVING:
			_fall += FALL_GRAVITY * delta
			_car.position.y -= _fall * delta
			_fade = minf(_fade + delta / RAIL_FADE, 1.0)
			for material in _fading:
				var colour := material.albedo_color
				colour.a = 1.0 - _fade
				material.albedo_color = colour
				if material.emission_enabled:
					material.emission_energy_multiplier = material.get_meta("energy", 1.0) * (1.0 - _fade)
			if _car.position.y < _floor_y - GONE_BELOW or _fade >= 1.0:
				clear()


## Slides the two leaves apart by however far they have opened.
func _slide() -> void:
	for i in _doors.size():
		var side := 1.0 if i == 0 else -1.0
		_doors[i].position.z = side * (HALF * 0.5 + DOOR_TRAVEL * _opened)


func _build_car() -> void:
	var metal := _metal(Color(0.19, 0.2, 0.24), 0.85, 0.3)
	var panelling := _metal(Color(0.26, 0.28, 0.34), 0.4, 0.55)
	var trim := _glow(UiStyle.ACCENT, 2.4)

	# The floor is the only part with collision: you ride on it, and walls with
	# collision would trap you when you try to walk out.
	_deck = StaticBody3D.new()
	_deck.collision_layer = 1
	_deck.collision_mask = 1
	_car.add_child(_deck)
	var slab := _box(Vector3(HALF * 2.0, 0.22, HALF * 2.0), panelling)
	slab.position.y = -0.11
	_deck.add_child(slab)
	var shape := CollisionShape3D.new()
	var box := BoxShape3D.new()
	box.size = Vector3(HALF * 2.0, 0.22, HALF * 2.0)
	shape.shape = box
	shape.position.y = -0.11
	_deck.add_child(shape)

	# A lit line round the floor: the edge you look for when deciding whether
	# you are off it.
	var strip := _box(Vector3(HALF * 2.0 + 0.05, 0.03, HALF * 2.0 + 0.05), trim)
	strip.position.y = 0.015
	_car.add_child(strip)

	# Three walls. The fourth side, +X, is the doorway. They are solid, which
	# is what lets you walk round the car while you type without walking out
	# of it.
	for wall: Array in [
		[Vector3(HALF * 2.0, HEIGHT, 0.1), Vector3(0.0, HEIGHT * 0.5, -HALF)],
		[Vector3(HALF * 2.0, HEIGHT, 0.1), Vector3(0.0, HEIGHT * 0.5, HALF)],
		[Vector3(0.1, HEIGHT, HALF * 2.0), Vector3(-HALF, HEIGHT * 0.5, 0.0)],
	]:
		var panel := _box(wall[0], panelling)
		panel.position = wall[1]
		_car.add_child(panel)
		_solid(wall[0], wall[1])

	var roof := _box(Vector3(HALF * 2.0, 0.12, HALF * 2.0), panelling)
	roof.position.y = HEIGHT
	_car.add_child(roof)

	# A frame round the doorway, so the opening reads as a door and not a hole.
	for post: Vector3 in [Vector3(HALF, HEIGHT * 0.5, -HALF), Vector3(HALF, HEIGHT * 0.5, HALF)]:
		var jamb := _box(Vector3(0.16, HEIGHT, 0.16), metal)
		jamb.position = post
		_car.add_child(jamb)
	var lintel := _box(Vector3(0.16, 0.16, HALF * 2.0 + 0.16), metal)
	lintel.position = Vector3(HALF, HEIGHT - 0.08, 0.0)
	_car.add_child(lintel)

	# Two leaves that slide apart along Z.
	for i in 2:
		var leaf := _box(Vector3(0.09, HEIGHT - 0.2, HALF), metal)
		leaf.position = Vector3(HALF, (HEIGHT - 0.2) * 0.5, 0.0)
		_car.add_child(leaf)
		_doors.append(leaf)
		# The shut doors are a wall too, until they are not.
		_doorway.append(_solid(Vector3(0.12, HEIGHT - 0.2, HALF), Vector3.ZERO, leaf))
		var seam := _box(Vector3(0.11, HEIGHT - 0.35, 0.025), trim)
		seam.position = Vector3(0.0, 0.0, (HALF * 0.5 - 0.02) * (1.0 if i == 0 else -1.0))
		leaf.add_child(seam)
	_slide()

	var lamp := OmniLight3D.new()
	lamp.light_color = Color(0.74, 0.87, 1.0)
	lamp.light_energy = 2.6
	lamp.omni_range = 8.0
	lamp.position.y = HEIGHT - 0.35
	_car.add_child(lamp)


func _build_rails(at: Transform3D) -> void:
	var rail_metal := _metal(Color(0.3, 0.33, 0.4), 0.9, 0.25)
	var glow := _glow(UiStyle.ACCENT, 2.6)
	# Clear of the doorway on +X, so they never cross the way out.
	for offset: Vector3 in [
		Vector3(0.0, 0.0, RAIL_OUT), Vector3(0.0, 0.0, -RAIL_OUT), Vector3(-RAIL_OUT, 0.0, 0.0),
	]:
		var rail := _cylinder(0.055, RAIL_HEIGHT, rail_metal)
		rail.position = at.origin + offset + Vector3.UP * (RAIL_HEIGHT * 0.5)
		add_child(rail)
		var lit := _cylinder(0.022, RAIL_HEIGHT, glow)
		lit.position = rail.position + offset.normalized() * 0.07
		add_child(lit)


## A static body of this size, so the player cannot walk through it. Parented
## to the car (or to a door leaf, which slides).
func _solid(size: Vector3, at: Vector3, parent: Node3D = null) -> CollisionShape3D:
	var body := StaticBody3D.new()
	body.collision_layer = 1
	body.collision_mask = 1
	body.position = at
	var shape := CollisionShape3D.new()
	var box := BoxShape3D.new()
	box.size = size
	shape.shape = box
	body.add_child(shape)
	(parent if parent != null else _car).add_child(body)
	return shape


func _box(size: Vector3, material: StandardMaterial3D) -> MeshInstance3D:
	var node := MeshInstance3D.new()
	var mesh := BoxMesh.new()
	mesh.size = size
	node.mesh = mesh
	node.material_override = material
	return node


func _cylinder(radius: float, height: float, material: StandardMaterial3D) -> MeshInstance3D:
	var node := MeshInstance3D.new()
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	node.mesh = mesh
	node.material_override = material
	return node


## Every material the lift uses is registered for the fade, and each one is a
## fresh copy: they are mutated on the way down.
func _metal(colour: Color, metallic: float, roughness: float) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = colour
	material.metallic = metallic
	material.roughness = roughness
	_fading.append(material)
	return material


func _glow(colour: Color, energy: float) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = colour
	material.emission_enabled = true
	material.emission = colour
	material.emission_energy_multiplier = energy
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.set_meta("energy", energy)
	_fading.append(material)
	return material
