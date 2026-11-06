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
var _want_vr := false
var _in_vr := false
var _joined := false
var _peers := {}


func _ready() -> void:
	_xr = XRServer.find_interface("OpenXR")
	if _xr and not _xr.is_initialized():
		_xr = null
	$XROrigin3D/Camera.position.y = FLAT_EYE_HEIGHT
	$Connect.set_vr_available(_xr != null)

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
	_want_vr = use_vr and _xr != null
	$Client.connect_to_server(host, port, player_name)


func _on_connected(your_id: int, _tick_hz: int) -> void:
	_joined = true
	$Connect.hide()
	if _want_vr:
		get_viewport().use_xr = true
		DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
		$XROrigin3D/Camera.position.y = 0.0
		_in_vr = true
		_status("You are player %d. Put the headset on." % your_id)
	else:
		_status("You are player %d. No headset: click to look, WASD to move." % your_id)


func _failed(reason: String) -> void:
	_joined = false
	for id: int in _peers.keys():
		_on_peer_left(id)
	if _in_vr:
		get_viewport().use_xr = false
		$XROrigin3D/Camera.position.y = FLAT_EYE_HEIGHT
		_in_vr = false
	Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	$Connect.failed(reason)
	$Connect.show()
	print(reason)


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
