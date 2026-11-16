extends Node

## The launcher: the first thing either exe shows.
##
## It asks how you want to play before the game starts, because OpenXR has to
## be running (or left alone) before the first frame is drawn, and because a
## flat screen is unreadable once a headset is already on your face.
##
## Which is why the question is asked twice, in two places at once: on the
## monitor, and -- if a runtime answers -- on a panel inside the headset. Both
## stay live, so it does not matter which one you are looking at. A headset
## that only shows up a few seconds after launch is picked up too.
##
## It also sends the studio build straight to the world builder, so the studio
## never starts OpenXR.

const STICK_DEADZONE := 0.3
## How far in front of you the panel hangs, and how far out of place it has to
## get before it is put back. Both distance and angle, because the first pose
## the headset reports is usually the origin: placed from a guessed eye height
## the panel ended up above the view, and a check on horizontal distance alone
## never noticed.
const DISTANCE := 1.25
const STRAY := 0.7
const STRAY_ANGLE := 0.6

var _asking := false
var _xr: XRInterface
var _xr_retry := 0.0
## Give up after this many tries. A machine with no headset should not spend
## the whole launcher re-initialising OpenXR once a second for ever.
var _xr_tries := 12
var _menu: WorldMenu
var _origin: XROrigin3D
var _camera: XRCamera3D
var _hands: Array[XRController3D] = []
## A runtime can start a session with no headset in it. Until one reports real
## tracking the display is not handed over for good.
var _tracking_wait := 0.0
var _confirmed := false


func _ready() -> void:
	set_process(false)
	if OS.has_feature("studio") or OS.get_cmdline_user_args().has("--studio"):
		# Deferred: the tree is still adding this node when _ready runs.
		get_tree().change_scene_to_file.call_deferred("res://studio.tscn")
		return
	var args := _user_args()
	# A world or a mode on the command line answers the question already, which
	# is what the studio's Test button and every scripted run rely on.
	if args.has("world") or args.has("mode"):
		Session.vr = args.get("mode", "") == "vr"
		_start()
		return
	_ask()


## Two buttons and nothing else. Built in code rather than a scene because a
## scene file for three nodes is three places to keep in step. Nothing here
## styles anything: `ui_theme.tres` is the project theme, so every Control
## already looks right (docs/UI_STYLE.md).
func _ask() -> void:
	_asking = true
	set_process(true)

	var screen := ColorRect.new()
	screen.color = UiStyle.BACKDROP
	screen.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(screen)

	var middle := CenterContainer.new()
	middle.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(middle)

	var column := VBoxContainer.new()
	column.alignment = BoxContainer.ALIGNMENT_CENTER
	column.add_theme_constant_override("separation", 14)
	middle.add_child(column)

	var title := Label.new()
	title.text = "Hearth"
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	title.add_theme_font_size_override("font_size", UiStyle.FONT_TITLE_2D)
	column.add_child(title)

	var hint := Label.new()
	hint.text = "How do you want to play?"
	hint.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	hint.modulate = UiStyle.TEXT_DIM
	column.add_child(hint)

	var spacer := Control.new()
	spacer.custom_minimum_size = Vector2(0, 18)
	column.add_child(spacer)

	# The headset first and focused, so Enter or a gamepad picks the one you
	# are most likely wearing while you reach for the keyboard.
	var vr := _button("In the headset", "VR")
	vr.pressed.connect(_pick.bind(true))
	column.add_child(vr)

	var desktop := _button("On this screen", "Mouse and keyboard")
	desktop.pressed.connect(_pick.bind(false))
	column.add_child(desktop)

	var note := Label.new()
	note.text = "Put the headset on and the same question is in there too."
	note.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	note.modulate = UiStyle.TEXT_DIM
	column.add_child(note)

	vr.grab_focus()


func _button(label: String, under: String) -> Button:
	var button := Button.new()
	button.text = "%s\n%s" % [label, under]
	button.custom_minimum_size = Vector2(340, 74)
	return button


func _process(delta: float) -> void:
	if not _asking:
		return
	if _menu == null:
		# A runtime can take a few seconds to come up, so keep asking.
		if _xr_tries <= 0:
			return
		_xr_retry -= delta
		if _xr_retry <= 0.0:
			_xr_retry = 1.0
			_xr_tries -= 1
			_try_headset()
		return
	if not _confirmed and not _confirm_headset(delta):
		return
	_keep_in_view()
	var step := Vector2i.ZERO
	var stick := Vector2.ZERO
	for hand in _hands:
		stick += hand.get_vector2("primary") + hand.get_vector2("secondary")
	if absf(stick.y) > STICK_DEADZONE:
		step.y = -1 if stick.y > 0.0 else 1
	_menu.hold(step, delta)


## OpenXR will happily start a session with nothing on anybody's head, and a
## session with no headset renders black -- which, with the viewport handed
## over, is all a desktop player would ever see. So the handover is
## provisional: if no real tracking turns up, take it back.
func _confirm_headset(delta: float) -> bool:
	if _xr.get_tracking_status() == XRInterface.XR_NORMAL_TRACKING:
		_confirmed = true
		return true
	_tracking_wait += delta
	if _tracking_wait < 3.0:
		return false
	get_viewport().use_xr = false
	# Shut the session down too, or it complains once a frame for ever that
	# nothing is rendering it.
	_xr.uninitialize()
	_origin.queue_free()
	_origin = null
	_camera = null
	_hands.clear()
	_menu = null
	# Do not keep trying: the runtime is there, the headset is not.
	_xr_tries = 0
	print("OpenXR started but nothing is tracking: staying on the screen.")
	return false


## The panel sits in the world rather than stuck to your face, but it comes
## back if you lose it -- which includes the first frames, before the headset
## has reported a pose worth placing anything against.
func _keep_in_view() -> void:
	var head := _camera.transform
	var forward := -head.basis.z
	forward.y = 0.0
	forward = forward.normalized() if forward.length() > 0.01 else Vector3.FORWARD
	var want := head.origin + forward * DISTANCE
	var to_panel := _menu.position - head.origin
	var strayed := _menu.position.distance_to(want) > STRAY
	var behind := to_panel.length() < 0.05 or forward.angle_to(to_panel.normalized()) > STRAY_ANGLE
	if not strayed and not behind:
		return
	# Moved, not rebuilt: `open` makes every mesh again, and this can run on
	# any frame.
	_menu.transform = Transform3D(Basis.looking_at(forward, Vector3.UP), want)


## Brings up the same question on a panel in the headset. Does nothing until a
## runtime answers, and is retried every second until one does.
func _try_headset() -> void:
	_xr = XRServer.find_interface("OpenXR")
	if _xr == null or (not _xr.is_initialized() and not _xr.initialize()):
		return
	get_viewport().use_xr = true

	_origin = XROrigin3D.new()
	add_child(_origin)
	_camera = XRCamera3D.new()
	_origin.add_child(_camera)
	for tracker: String in ["left_hand", "right_hand"]:
		var hand := XRController3D.new()
		hand.tracker = tracker
		hand.button_pressed.connect(_on_hand_button)
		_origin.add_child(hand)
		_hands.append(hand)

	_menu = WorldMenu.new()
	# The launcher's panel is driven by the stick, and it puts itself back in
	# front of you, so there is nothing to carry it by.
	_menu.movable = false
	_menu.chosen.connect(_on_chosen)
	_origin.add_child(_menu)
	_menu.open(Transform3D(Basis.IDENTITY, Vector3.ZERO), _rows())
	_keep_in_view()


func _rows() -> Array:
	return [
		{"label": "-- Hearth --"},
		{"label": "How do you want to play?"},
		{"id": "vr", "label": "In the headset"},
		{"id": "desktop", "label": "On this screen"},
		{"label": ""},
		{"label": "Stick to move, trigger to pick."},
	]


func _on_hand_button(_button: String) -> void:
	# Any button: there is nothing else on this screen to press by mistake.
	if _menu != null:
		_menu.activate()


func _on_chosen(id: String) -> void:
	_pick(id == "vr")


func _pick(use_vr: bool) -> void:
	Session.vr = use_vr
	if not use_vr:
		# Picked the monitor with a headset already rendering: hand the
		# viewport back and release the session, or the game draws to an HMD
		# nobody is looking at while the player waits at a black screen.
		get_viewport().use_xr = false
		if _xr != null and _xr.is_initialized():
			_xr.uninitialize()
	_start()


func _start() -> void:
	_asking = false
	set_process(false)
	get_tree().change_scene_to_file.call_deferred("res://main.tscn")


func _user_args() -> Dictionary:
	var parsed := {}
	var args := OS.get_cmdline_user_args()
	for i in args.size():
		if args[i].begins_with("--") and i + 1 < args.size():
			parsed[args[i].substr(2)] = args[i + 1]
	return parsed
