extends Node3D

## Connects to a Hearth server, streams the local head and hand poses and
## spawns an avatar per remote player. Args skip the connect screen:
##   Hearth.exe -- --server 192.168.1.20 --port 7777 --name Ada --mode desktop

const FLAG_VR := 1
const FLAG_LEFT := 2
const FLAG_RIGHT := 4
const FLAT_EYE_HEIGHT := 1.7
const FLAT_SPEED := 3.0
const MOUSE_SENSITIVITY := 0.003
const VR_SPEED := 2.5
const VR_TURN_DEG := 100.0
const STICK_DEADZONE := 0.15
const GRAVITY := 12.0
const JUMP_SPEED := 4.2
const HAND_GRAB_REACH := 0.28
const VIEW_GRAB_REACH := 1.1
const MIC_BUS := "Mic"

@export var mic_on_icon: Texture2D
@export var mic_muted_icon: Texture2D

@export var avatar_scene: PackedScene

var _xr: XRInterface
var _in_vr := false
var _joined := false
var _xr_retry := 0.0
var _fall := 0.0
var _grounded := false
var _muted := false
var _capture: AudioEffectCapture
## Holder node -> the body it carries, and that body's last travel per second.
var _held := {}
var _throw := {}
var _peers := {}


func _ready() -> void:
	_xr = XRServer.find_interface("OpenXR")
	$XROrigin3D/Camera.position.y = FLAT_EYE_HEIGHT
	# Render to the headset right away, so you can pick VR while wearing it.
	if _enter_vr():
		_status("Pull a trigger to join as %s" % $Connect.summary())

	$XROrigin3D/LeftHand.button_pressed.connect(_on_hand_button.bind(false))
	$XROrigin3D/RightHand.button_pressed.connect(_on_hand_button.bind(true))
	$XROrigin3D/LeftHand.button_released.connect(_on_hand_release.bind(false))
	$XROrigin3D/RightHand.button_released.connect(_on_hand_release.bind(true))
	var bus := AudioServer.get_bus_index(MIC_BUS)
	if bus >= 0:
		AudioServer.set_bus_mute(bus, true) # Nobody wants to hear themselves.
		_capture = AudioServer.get_bus_effect(bus, 0)
	else:
		push_warning("No Mic audio bus: voice will not be sent.")
	_update_hud()

	$Connect.connect_requested.connect(_on_connect_requested)
	$Client.connected.connect(_on_connected)
	$Client.rejected.connect(func(reason: String) -> void: _failed("Server refused the join: " + reason))
	$Client.disconnected.connect(func(reason: String) -> void: _failed(reason))
	$Client.peer_joined.connect(_on_peer_joined)
	$Client.peer_left.connect(_on_peer_left)
	$Client.peer_pose.connect(_on_peer_pose)
	$Client.peer_voice.connect(_on_peer_voice)

	var args := _user_args()
	if args.has("server") or args.has("name"):
		var host: String = args.get("server", "127.0.0.1")
		var port := int(args.get("port", 7777))
		var player_name: String = args.get("name", "Player%d" % (randi() % 1000))
		$Connect.fill(host, port, player_name)
		$Connect.busy("Connecting to %s:%d..." % [host, port])
		_on_connect_requested(host, port, player_name, args.get("mode", "vr") == "vr")


func _process(delta: float) -> void:
	if _in_vr:
		_vr_move(delta)
	if not _joined:
		_offer_vr(delta)
		return
	_fall_and_land(delta)
	_carry(delta)
	_send_voice()
	if not _in_vr:
		_flat_move(delta)
	var flags := FLAG_VR if _in_vr else 0
	var left: XRController3D = $XROrigin3D/LeftHand
	var right: XRController3D = $XROrigin3D/RightHand
	if left.get_has_tracking_data():
		flags |= FLAG_LEFT
	if right.get_has_tracking_data():
		flags |= FLAG_RIGHT
	$Client.send_pose($XROrigin3D/Camera.global_transform, left.global_transform, right.global_transform, flags)


func _on_connect_requested(host: String, port: int, player_name: String, use_vr: bool) -> void:
	if use_vr:
		if not _enter_vr():
			$Connect.failed("No headset found. Is your OpenXR runtime running?")
			return
	else:
		_leave_vr()
	$Client.connect_to_server(host, port, player_name)


func _on_connected(your_id: int, _tick_hz: int) -> void:
	_joined = true
	$Connect.hide()
	if _in_vr:
		_status("You are player %d." % your_id)
	else:
		_status("You are player %d. Click to look, WASD to move." % your_id)


func _failed(reason: String) -> void:
	_joined = false
	for id: int in _peers.keys():
		_on_peer_left(id)
	for holder: Node3D in _held.keys():
		_release(holder)
	Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	$Connect.failed(reason)
	$Connect.show()
	print(reason)


## Rendering to the headset. A runtime can show up long after launch, so this
## is retried while the connect screen is open.
func _enter_vr() -> bool:
	if _in_vr:
		return true
	if _xr == null or (not _xr.is_initialized() and not _xr.initialize()):
		return false
	get_viewport().use_xr = true
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	$XROrigin3D/Camera.position.y = 0.0
	_in_vr = true
	return true


func _leave_vr() -> void:
	if not _in_vr:
		return
	get_viewport().use_xr = false
	$XROrigin3D/Camera.position.y = FLAT_EYE_HEIGHT
	_in_vr = false


## In the headset there is no mouse, so a trigger pull joins with whatever the
## connect screen holds. Grip and the face buttons count too, because the
## default OpenXR action map binds them differently per controller.
func _offer_vr(delta: float) -> void:
	if not _in_vr:
		_xr_retry -= delta
		if _xr_retry <= 0.0:
			_xr_retry = 1.0
			if _enter_vr():
				_status("Pull a trigger to join as %s" % $Connect.summary())
		return
	for hand: XRController3D in [$XROrigin3D/LeftHand, $XROrigin3D/RightHand]:
		for button: String in ["trigger_click", "grip_click", "ax_button", "by_button"]:
			if hand.is_button_pressed(button):
				$Connect.join_in_vr()
				return
		if hand.get_float("trigger") > 0.6 or hand.get_float("grip") > 0.6:
			$Connect.join_in_vr()
			return


func _on_peer_joined(id: int, peer_name: String) -> void:
	var avatar := avatar_scene.instantiate()
	$Peers.add_child(avatar)
	avatar.set_label(peer_name)
	_peers[id] = avatar
	_status("%s joined" % peer_name)


func _on_peer_left(id: int) -> void:
	if _peers.has(id):
		_peers[id].queue_free()
		_peers.erase(id)
		_status("player %d left" % id)


func _on_peer_pose(id: int, head: Transform3D, left: Transform3D, right: Transform3D, flags: int) -> void:
	if _peers.has(id):
		_peers[id].set_pose(head, left, right, flags)


func _status(text: String) -> void:
	$Status.text = text
	print(text)


func _unhandled_input(event: InputEvent) -> void:
	if not _joined:
		return
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_SPACE:
			_jump()
		elif event.keycode == KEY_V:
			_toggle_mute()
		elif event.keycode == KEY_E and not _in_vr:
			_grab($XROrigin3D/Camera/HoldPoint, VIEW_GRAB_REACH)
	if _in_vr:
		return
	if event is InputEventMouseButton and event.pressed:
		Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	elif event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	elif event is InputEventMouseMotion and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		var camera: XRCamera3D = $XROrigin3D/Camera
		camera.rotation.y -= event.relative.x * MOUSE_SENSITIVITY
		camera.rotation.x = clampf(camera.rotation.x - event.relative.y * MOUSE_SENSITIVITY, -1.5, 1.5)


## Jump on A, mute on X, with the same handler for either hand so left-handed
## players get both.
func _on_hand_button(button: String, right_hand: bool) -> void:
	if not _joined:
		return # A button press is how you join from inside the headset.
	var hand: Node3D = $XROrigin3D/RightHand if right_hand else $XROrigin3D/LeftHand
	match button:
		"ax_button":
			if right_hand:
				_jump()
			else:
				_toggle_mute()
		"grip_click", "trigger_click":
			_grab(hand, HAND_GRAB_REACH)


func _on_hand_release(button: String, right_hand: bool) -> void:
	if button == "grip_click" or button == "trigger_click":
		_release($XROrigin3D/RightHand if right_hand else $XROrigin3D/LeftHand)


func _jump() -> void:
	if _grounded:
		_fall = JUMP_SPEED
		_grounded = false


func _toggle_mute() -> void:
	_muted = not _muted
	_update_hud()


func _update_hud() -> void:
	$XROrigin3D/Camera/MicIcon.texture = mic_muted_icon if _muted else mic_on_icon


## Mic -> Opus -> the server. The capture buffer is drained even while muted,
## so unmuting does not play back a backlog.
func _send_voice() -> void:
	if _capture == null:
		return
	var available := _capture.get_frames_available()
	if available <= 0:
		return
	var captured := _capture.get_buffer(available)
	if _muted:
		return
	var mono := PackedFloat32Array()
	mono.resize(captured.size())
	for i in captured.size():
		mono[i] = (captured[i].x + captured[i].y) * 0.5
	$Client.send_voice(mono)


func _on_peer_voice(id: int, samples: PackedFloat32Array) -> void:
	if _peers.has(id):
		_peers[id].push_voice(samples)


## Picking things up: whatever is closest to the hand, or to a point in front
## of the camera on the desktop. Held objects are frozen and follow the holder;
## letting go hands them the holder's speed, so they can be thrown. None of
## this is networked, so what you move does not move for anybody else.
func _grab(holder: Node3D, reach: float) -> void:
	if _held.has(holder):
		_release(holder)
		return
	var carried := _held.values()
	var best: RigidBody3D = null
	var nearest := reach
	for body: RigidBody3D in get_tree().get_nodes_in_group("grabbable"):
		var distance := body.global_position.distance_to(holder.global_position)
		if distance < nearest and not carried.has(body):
			best = body
			nearest = distance
	if best == null:
		return
	best.freeze = true
	_held[holder] = best


func _release(holder: Node3D) -> void:
	if not _held.has(holder):
		return
	var body: RigidBody3D = _held[holder]
	_held.erase(holder)
	body.freeze = false
	body.linear_velocity = _throw.get(body, Vector3.ZERO)
	body.angular_velocity = Vector3.ZERO
	_throw.erase(body)


func _carry(delta: float) -> void:
	for holder: Node3D in _held:
		var body: RigidBody3D = _held[holder]
		var was := body.global_position
		body.global_transform = holder.global_transform
		if delta > 0.0:
			_throw[body] = (body.global_position - was) / delta


## The play space floats on whatever is under the player's head, so you can
## jump, fall off a crate, and stand on one.
func _fall_and_land(delta: float) -> void:
	var origin: XROrigin3D = $XROrigin3D
	var camera: XRCamera3D = $XROrigin3D/Camera
	var ray: RayCast3D = $XROrigin3D/GroundRay
	ray.position = Vector3(camera.position.x, 0.2, camera.position.z)
	ray.force_raycast_update()

	_fall -= GRAVITY * delta
	origin.position.y += _fall * delta
	if not ray.is_colliding():
		_grounded = false
		return
	var ground: float = ray.get_collision_point().y
	if origin.position.y <= ground and _fall <= 0.0:
		origin.position.y = ground
		_fall = 0.0
		_grounded = true
	else:
		_grounded = false


## Left stick walks where you are looking, right stick turns smoothly around
## your head so the view does not swing around the play space origin.
func _vr_move(delta: float) -> void:
	var origin: XROrigin3D = $XROrigin3D
	var camera: XRCamera3D = $XROrigin3D/Camera

	var stick := _stick($XROrigin3D/LeftHand)
	if stick.length() > STICK_DEADZONE:
		var move: Vector3 = camera.global_transform.basis * Vector3(stick.x, 0.0, -stick.y)
		move.y = 0.0
		if move.length() > 0.001:
			origin.position += move.normalized() * minf(stick.length(), 1.0) * VR_SPEED * delta

	var turn := _stick($XROrigin3D/RightHand).x
	if absf(turn) > STICK_DEADZONE:
		var spin := Basis(Vector3.UP, -turn * deg_to_rad(VR_TURN_DEG) * delta)
		var head := camera.global_position
		origin.global_transform = Transform3D(spin, head - spin * head) * origin.global_transform


## Thumbstick, or the trackpad on controllers that have one instead.
func _stick(hand: XRController3D) -> Vector2:
	var stick: Vector2 = hand.get_vector2("primary")
	return stick if stick.length() > STICK_DEADZONE else hand.get_vector2("secondary")


func _flat_move(delta: float) -> void:
	var move := Vector3(
		float(Input.is_key_pressed(KEY_D)) - float(Input.is_key_pressed(KEY_A)),
		0.0,
		float(Input.is_key_pressed(KEY_S)) - float(Input.is_key_pressed(KEY_W))
	)
	if move == Vector3.ZERO:
		return
	var camera: XRCamera3D = $XROrigin3D/Camera
	var step: Vector3 = camera.global_transform.basis * move.normalized() * FLAT_SPEED * delta
	$XROrigin3D.position += Vector3(step.x, 0.0, step.z)


func _user_args() -> Dictionary:
	var parsed := {}
	var args := OS.get_cmdline_user_args()
	for i in args.size():
		if args[i].begins_with("--") and i + 1 < args.size():
			parsed[args[i].substr(2)] = args[i + 1]
	return parsed
