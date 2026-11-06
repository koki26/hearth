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

@export var avatar_scene: PackedScene

var _xr: XRInterface
var _in_vr := false
var _joined := false
var _xr_retry := 0.0
var _peers := {}


func _ready() -> void:
	_xr = XRServer.find_interface("OpenXR")
	$XROrigin3D/Camera.position.y = FLAT_EYE_HEIGHT
	# Render to the headset right away, so you can pick VR while wearing it.
	if _enter_vr():
		_status("Pull a trigger to join as %s" % $Connect.summary())

	$Connect.connect_requested.connect(_on_connect_requested)
	$Client.connected.connect(_on_connected)
	$Client.rejected.connect(func(reason: String) -> void: _failed("Server refused the join: " + reason))
	$Client.disconnected.connect(func(reason: String) -> void: _failed(reason))
	$Client.peer_joined.connect(_on_peer_joined)
	$Client.peer_left.connect(_on_peer_left)
	$Client.peer_pose.connect(_on_peer_pose)

	var args := _user_args()
	if args.has("server") or args.has("name"):
		var host: String = args.get("server", "127.0.0.1")
		var port := int(args.get("port", 7777))
		var player_name: String = args.get("name", "Player%d" % (randi() % 1000))
		$Connect.fill(host, port, player_name)
		$Connect.busy("Connecting to %s:%d..." % [host, port])
		_on_connect_requested(host, port, player_name, args.get("mode", "vr") == "vr")


func _process(delta: float) -> void:
	if not _joined:
		_offer_vr(delta)
		return
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
	if _in_vr or not _joined:
		return
	if event is InputEventMouseButton and event.pressed:
		Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
	elif event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	elif event is InputEventMouseMotion and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		var camera: XRCamera3D = $XROrigin3D/Camera
		camera.rotation.y -= event.relative.x * MOUSE_SENSITIVITY
		camera.rotation.x = clampf(camera.rotation.x - event.relative.y * MOUSE_SENSITIVITY, -1.5, 1.5)


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
