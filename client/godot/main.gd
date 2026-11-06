extends Node3D

## Connects to a Hearth server, streams the local head/hand poses and spawns
## an avatar per remote player. Pass settings as user args:
##   Hearth.exe -- --server 10.0.0.5 --port 7777 --name Ada

const FLAG_VR := 1
const FLAG_LEFT := 2
const FLAG_RIGHT := 4
const FLAT_SPEED := 3.0
const MOUSE_SENSITIVITY := 0.003

@export var avatar_scene: PackedScene
@export var server := "127.0.0.1"
@export var port := 7777

var _xr: XRInterface
var _peers := {}


func _ready() -> void:
	var args := _user_args()
	_xr = XRServer.find_interface("OpenXR")
	if _xr and _xr.is_initialized():
		get_viewport().use_xr = true
		DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	else:
		_xr = null
		$XROrigin3D/Camera.position.y = 1.7
		_status("No headset - flat mode. Click to look, WASD to move.")

	$Client.connected.connect(_on_connected)
	$Client.rejected.connect(func(reason: String) -> void: _status("Rejected: " + reason))
	$Client.disconnected.connect(_on_disconnected)
	$Client.peer_joined.connect(_on_peer_joined)
	$Client.peer_left.connect(_on_peer_left)
	$Client.peer_pose.connect(_on_peer_pose)

	var host: String = args.get("server", server)
	var player_port := int(args.get("port", port))
	var player_name: String = args.get("name", "Player%d" % (randi() % 1000))
	_status("Connecting to %s:%d as %s..." % [host, player_port, player_name])
	$Client.connect_to_server(host, player_port, player_name)


func _process(delta: float) -> void:
	if not _xr:
		_flat_move(delta)
	var flags := FLAG_VR if _xr else 0
	var left: XRController3D = $XROrigin3D/LeftHand
	var right: XRController3D = $XROrigin3D/RightHand
	if left.get_has_tracking_data():
		flags |= FLAG_LEFT
	if right.get_has_tracking_data():
		flags |= FLAG_RIGHT
	$Client.send_pose($XROrigin3D/Camera.global_transform, left.global_transform, right.global_transform, flags)


func _on_connected(your_id: int, tick_hz: int) -> void:
	_status("Connected as player %d (%d Hz)" % [your_id, tick_hz])


func _on_disconnected(reason: String) -> void:
	_status("Disconnected: " + reason)
	for id: int in _peers.keys():
		_on_peer_left(id)


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
	if _xr:
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
