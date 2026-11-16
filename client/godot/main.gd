extends Node3D

## Connects to a Hearth server, streams the local head and hand poses and
## spawns an avatar per remote player. Args skip the launcher:
##   Hearth.exe -- --server 192.168.1.20 --port 7777 --name Ada --mode desktop
##   Hearth.exe -- --world <folder>   walk a world on your own, no server
##
## The room you see before joining is the hub that ships inside the exe
## (`worlds/hub`, the same world file the server hands out). Once a server tells
## you which world its instance is running, that one is downloaded, verified
## and built in its place.
##
## You start in the lift, and you join from a panel on its wall: see
## elevator.gd and keyboard.gd. There is no 2D connect screen any more, because
## you cannot read one with a headset already on your face.

const HUB_INSTANCE := 0
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
## How far the desktop crosshair reaches. An object may ask for more.
const VIEW_GRAB_REACH := 3.5
const MIC_BUS := "Mic"
## The player, for working out what they can walk into. The capsule itself is
## on the BodyCast node in main.tscn; this is how high to hold it, lifted by
## STEP_UP so a kerb or a step does not count as a wall.
const BODY_HEIGHT := 1.5
## The tallest thing you can walk straight up: a step, a kerb, the lip of a
## ramp. The hub's steps are 0.4, so anything under that left them unclimbable
## and the plaza fenced off from its own dais.
const STEP_UP := 0.45
## Above this much signal the microphone icon lights up.
const TALKING := 0.02
## Nobody holds this item. Matches `hearth_protocol::NOBODY`; player ids
## start at 1.
const NOBODY := 0
## A thrown item stays ours until it is slower than this, so it comes to rest
## in the same place for everybody instead of each client throwing its own.
const STILL := 0.4
## How fast a remote item catches up with where the network says it is.
const ITEM_SMOOTHING := 18.0
## Fall below the world's `kill_y` and you are put back on the spawn. Used
## when the world does not say.
const KILL_Y := -24.0
## How far out the menu opens when you are not pointing at anything nearer.
const MENU_REACH := 1.4
## Opens the menu: Y on the keyboard, the Y/B face button in the headset. Tap
## it for the menu, hold it for the quick menu.
const MENU_BUTTON := "by_button"
## How long a press has to last to count as holding rather than tapping.
const HOLD := 0.4
## The panels either side of the menu: how wide, how far round towards you,
## and the gap between them and the menu.
const WING_WIDTH := 1.0
const WING_ANGLE := 0.5
const WING_GAP := 0.05
## Chat: lines kept, and the most lines the chat panel shows at once.
const CHAT_KEEP := 60
const CHAT_LINES := 13
## How long a chat line or a notice stays at the bottom of your view.
const TOAST_TIME := 5.0
## The hub that ships inside the exe, as a world file rather than a scene: the
## same one the server hands out, so the room you stand in before joining is
## the place you are joining.
const HOME_WORLD := "res://worlds/hub"
## The menu's pages, in the order they sit across the top.
## Friends and chat are not tabs: they hang either side of the menu, always.
const MENU_TABS := [
	{"id": "tab:worlds", "label": "Worlds", "icon": "res://icons/globe.svg"},
	{"id": "tab:camera", "label": "Camera", "icon": "res://icons/camera.svg"},
	{"id": "tab:settings", "label": "Settings", "icon": "res://icons/gear.svg"},
]
## Who an ember lets in. The order is the wire's (`hearth_protocol::ember`),
## so the index is what goes out.
const EMBER_KINDS := ["public", "friends+", "friends", "invite"]
const EMBER_ICONS := [
	"res://icons/globe.svg",
	"res://icons/people.svg",
	"res://icons/people.svg",
	"res://icons/lock.svg",
]
## Where the lift panel and its keyboard hang, in metres above the car floor.
const PANEL_HEIGHT := 1.68
const KEYBOARD_HEIGHT := 1.06

@export var mic_on_icon: Texture2D
@export var mic_muted_icon: Texture2D

@export var avatar_scene: PackedScene
## Stands in for the hub only if its world file is missing or broken, so a bad
## edit still leaves a floor to stand on instead of a void.
@export var home_world: PackedScene

var _xr: XRInterface
var _in_vr := false
var _joined := false
var _xr_retry := 0.0
var _fall := 0.0
var _grounded := false
var _muted := false
var _capture: AudioEffectCapture
## How loud you are, eased down so the icon does not strobe between syllables.
var _voice_level := 0.0
var _audio: GameAudio
var _elevator: Elevator
var _menu: WorldMenu
## Either side of the menu: friends and who is here on the left, chat on the
## right. They open and close with it and ride along when it is carried.
var _friends: WorldMenu
var _chat_panel: WorldMenu
## Hold the menu button: the camera, chat, the mic, the menu.
var _quick: QuickMenu
## The camera, while it is out, and the hand holding it (null on the desktop,
## where it hangs at the corner of the view).
var _camera: PhotoCamera
var _camera_hand: Node3D
var _last_photo: Texture2D
var _photos_taken := 0
## The menu button: how long it has been held, by which hand, and whether
## holding it has already done something, so letting go does not also tap.
var _hold_time := -1.0
var _hold_hand: Node3D
var _hold_fired := false
## Chat, oldest first: {"from": id, "who": name, "text": line, "time": "14:02"}.
## A "from" of -1 is the client saying something: who came, who went.
var _chat: Array = []
## Typing that came from the menu goes back to the menu when it is done.
var _return_to_menu := false
var _toast: Label3D
var _toast_left := 0.0
## The mirror standing on the plaza, off until somebody turns it on.
var _mirror: Mirror
## The lift panel: the server address and your name, before you join.
var _panel: WorldMenu
var _keyboard: WorldKeyboard
var _details: ConnectDetails
## Accounts, friends, and where friends are (fireplace.gd).
var _fireplace: FireplaceClient
## What is typed on the sign-in page. The password is held here and nowhere
## else: it is never saved, and never leaves this file except to be sent.
var _handle_typed := ""
var _password_typed := ""
## Only for making an account: sent once, and the Fireplace does not keep it.
var _birthday_typed := ""
var _accept_terms := false
var _policy_emails := false
## What is typed into "Add a friend" on the People page.
var _friend_typed := ""
## The address typed on the account page, before it is saved.
var _email_typed := ""
## An ember to walk into the moment we land on a friend's Flame, or -1.
var _join_ember := -1
## Which page of the lift panel is showing: "main", "warning" or "advanced".
## Three pages, one `_panel_rows`, no second scene.
var _panel_page := "main"
## Which panel field the keyboard is editing, if any, and its label.
var _editing := ""
var _editing_label := ""
var _connecting := false
## What the panel says under the fields: the last error, or progress.
var _message := ""
## Holder node -> the body it carries, and that body's last travel per second.
var _held := {}
var _throw := {}
var _peers := {}
## Our own player id, so `item_held` can tell our grabs from everybody else's.
var _my_id := NOBODY
## Everything grabbable in the loaded world, by its index in the world file.
## That index is the item id on the wire; a world with no indices has no
## networked items.
var _items := {}
## Grabs the server has not answered yet: item -> the hand that asked.
var _pending := {}
## What the server says about every item: item -> the player holding it.
var _holders := {}
## Items we threw and still own until they settle: item -> body.
var _thrown := {}
## Where the network last put an item somebody else is carrying.
var _remote := {}
## What the desktop crosshair is over, if it is grabbable.
var _aimed: RigidBody3D
## The slider being pulled, and the panel it belongs to.
var _slider_menu: WorldMenu
var _slider_row := -1
## Which page of the menu is open, which world's embers are being looked at
## (empty for the list of worlds), and what the next ember will be called.
var _menu_tab := "worlds"
var _menu_world := ""
var _ember_name := ""
var _ember_kind := 0
## Carrying a panel by its grab bar: which one, and with which hand.
var _dragging := false
var _drag_ui: FlatPanel
var _drag_hand: Node3D
var _drag_distance := 0.0
var _drag_offset := Vector3.ZERO
## True once you have moved the keyboard, so it stops being pinned to the car.
var _keyboard_moved := false
## Whether the mouse was looking around before a panel took it, so it can be
## given back afterwards.
var _mouse_look := false
## Which instance we are in, and what the server told us about the others.
var _instance := HUB_INSTANCE
var _instances := {}
var _worlds := {}
var _can_create := false
## The world we are waiting for, while a download runs.
var _wanted := ""
var _loaded := ""
## The checksum of the world file the scene in front of you was built from, so
## an identical one arriving from a server costs nothing.
var _built_from := ""


func _ready() -> void:
	var args := _user_args()
	# A world on the command line, or handed over by the studio's Test button:
	# walk it alone, with no server, no lift panel and no headset.
	var local: String = args.get("world", Session.world_dir)
	_details = ConnectDetails.new()
	_xr = XRServer.find_interface("OpenXR")
	$XROrigin3D/Camera.position.y = FLAT_EYE_HEIGHT

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
	# The status line, the crosshair and the pointer beams are interface, so
	# they take the interface colours rather than carrying their own in the
	# scene file -- a reskin should never have to open a .tscn.
	for side: String in ["LeftHand", "RightHand"]:
		var beam: MeshInstance3D = get_node("XROrigin3D/%s/Pointer" % side)
		beam.material_override = UiStyle.glow(Color(UiStyle.ACCENT, 0.75), 1.6)
	$XROrigin3D/Camera/Crosshair.outline_modulate = Color(UiStyle.BACKDROP, 0.85)

	_audio = GameAudio.new()
	_audio.name = "Audio"
	add_child(_audio)
	_fireplace = FireplaceClient.new(_details)
	_fireplace.name = "Fireplace"
	_fireplace.signed_in.connect(_on_signed_in)
	_fireplace.signed_out.connect(_on_signed_out)
	_fireplace.friends_changed.connect(_refresh_menu)
	_fireplace.account_changed.connect(_refresh_panel)
	_fireplace.failed.connect(_on_fireplace_failed)
	_fireplace.said.connect(func(message: String) -> void:
		_message = message
		_email_typed = ""
		_refresh_panel())
	add_child(_fireplace)
	_elevator = Elevator.new()
	_elevator.name = "Elevator"
	add_child(_elevator)
	_elevator.arrived.connect(_on_elevator_arrived)
	_mirror = Mirror.new()
	_mirror.name = "Mirror"
	add_child(_mirror)
	# Everybody you can see is also on the player layer, so the cheap mirror
	# can draw people without drawing the world behind them.
	_on_layer($XROrigin3D/LeftHand)
	_on_layer($XROrigin3D/RightHand)

	_menu = WorldMenu.new()
	_menu.name = "Menu"
	# The settings menu opens over whatever is already there.
	_menu.band = UiStyle.BAND
	_menu.visible = false
	_menu.chosen.connect(_on_menu_chosen)
	_menu.changed.connect(_on_menu_changed)
	_menu.edit.connect(_on_menu_edit)
	# Hung off the play space, so it rides the lift and walks with you. As a
	# child of the world it stayed where it was opened while the lift carried
	# the player down past it.
	$XROrigin3D.add_child(_menu)
	_menu.closable = true
	_menu.title_icon = "res://icons/flame.svg"
	_friends = _wing("Friends", "res://icons/people.svg")
	_chat_panel = _wing("Chat", "res://icons/chat.svg")
	_quick = QuickMenu.new()
	_quick.name = "Quick"
	_quick.band = UiStyle.BAND * 2
	_quick.visible = false
	_quick.chosen.connect(_on_quick_chosen)
	add_child(_quick)
	_toast = UiStyle.label("", UiStyle.FONT_SMALL, UiStyle.TEXT)
	_toast.name = "Toast"
	_toast.fixed_size = true
	_toast.pixel_size = 0.0009
	_toast.position = Vector3(0.0, -0.33, -0.8)
	_toast.visible = false
	_toast.render_priority = UiStyle.LAYER_TEXT + UiStyle.BAND * 3
	_toast.outline_render_priority = UiStyle.LAYER_EDGE + UiStyle.BAND * 3
	$XROrigin3D/Camera.add_child(_toast)
	_panel = WorldMenu.new()
	_panel.name = "Panel"
	_panel.width = 1.7
	_panel.title_icon = "res://icons/flame.svg"
	# Bolted to the lift car, which puts it back every frame.
	_panel.movable = false
	_panel.visible = false
	_panel.chosen.connect(_on_panel_chosen)
	_panel.edit.connect(_on_panel_edit)
	add_child(_panel)
	_keyboard = WorldKeyboard.new()
	_keyboard.name = "Keyboard"
	# Always in front of the panel it is typing into.
	_keyboard.band = UiStyle.BAND
	_keyboard.visible = false
	_keyboard.accepted.connect(_on_typed)
	_keyboard.cancelled.connect(_on_typing_cancelled)
	add_child(_keyboard)

	$Client.set_world_cache(ProjectSettings.globalize_path(Session.CACHE))
	$Portals.portal_entered.connect(func(instance: int) -> void: $Client.join_instance(instance))
	# A portal says nothing about who may walk in, so its ember is public and
	# named after its world.
	$Portals.create_requested.connect(func(slug: String) -> void: $Client.create_instance(slug, slug, 0))

	$Client.connected.connect(_on_connected)
	$Client.rejected.connect(func(reason: String) -> void: _failed("Server refused the join: " + reason))
	$Client.disconnected.connect(func(reason: String) -> void: _failed(reason))
	$Client.peer_joined.connect(_on_peer_joined)
	$Client.peer_left.connect(_on_peer_left)
	$Client.peer_pose.connect(_on_peer_pose)
	$Client.peer_voice.connect(_on_peer_voice)
	$Client.chat.connect(_on_chat)
	$Client.world_added.connect(_on_world_added)
	$Client.instance_added.connect(_on_instance_added)
	$Client.instance_closed.connect(_on_instance_closed)
	$Client.instance_count.connect(_on_instance_count)
	$Client.moved.connect(_on_moved)
	$Client.item_held.connect(_on_item_held)
	$Client.item_moved.connect(_on_item_moved)
	$Client.world_ready.connect(_on_world_ready)
	$Client.world_failed.connect(func(slug: String, reason: String) -> void:
		_status("Could not get the world %s: %s" % [slug, reason]))

	if local != "":
		Session.world_dir = local
		Session.offline = true
		_joined = true
		if not _load_world(local):
			_status("Could not open the world in %s" % local)
		return

	# The launcher already asked which way to play; start the headset before
	# anything is drawn, so the first thing you see is in the right place.
	if Session.vr and not _enter_vr():
		_message = "No headset found. Is your OpenXR runtime running?"
	elif not Session.vr and _xr != null and _xr.is_initialized():
		# project.godot has openxr/enabled, so Godot opens a session before
		# anybody asks for one. An XRCamera3D is then driven by the headset
		# pose even with use_xr off -- so on a desktop with a runtime
		# installed and nothing on anybody head, the view sits at whatever
		# stale angle the runtime last reported. Let the session go.
		_xr.uninitialize()

	# The hub, with the lift standing in it. `_replace_world` starts the ride.
	_replace_world(_home())

	if args.has("server") or args.has("name"):
		_details.address = "%s:%d" % [args.get("server", "127.0.0.1"), int(args.get("port", 7777))]
		_details.player_name = args.get("name", "Player%d" % (randi() % 1000))
		_join()
		return
	_open_panel()


func _process(delta: float) -> void:
	var camera: XRCamera3D = $XROrigin3D/Camera
	var head: Vector3 = camera.global_position
	_elevator.check(head, delta)
	# The car carries you down. Left to gravity and the ground ray you hung in
	# the air while the car dropped away at a constant speed, then free-fell
	# seven metres onto it: that was the lift "throwing" you.
	if _elevator.carrying():
		$XROrigin3D.position.y = _elevator.car_y()
		_fall = 0.0
		_grounded = true
	else:
		_fall_and_land(delta)

	_sync_mouse()
	_show_pointers()
	_follow_wings()
	_hold_menu(delta)
	_fade_toast(delta)
	if _ui_open():
		_point_ui()

	if not _joined:
		_offer_vr(delta)
		_place_lift_ui()
		_ui_input(delta)
		# You can walk round the car and look out of it while you type. The
		# walls are solid, so there is nowhere to go until the doors open.
		if _in_vr:
			_vr_move(delta)
		else:
			_flat_move(delta)
		return

	# A panel takes the arrow keys, never the sticks, so you can keep walking
	# and turning while you read it.
	if _ui_open():
		_ui_input(delta)
	if _in_vr:
		_vr_move(delta)
	_carry(delta)
	_follow_items(delta)
	# Until the doors open you are not in the world yet.
	if not _elevator.descending():
		$Portals.check(head)
	_mirror.look(camera)
	if not _in_vr:
		_flat_move(delta)
		if not _ui_open():
			_aim()
	if Session.offline:
		return
	_send_voice()
	var flags := FLAG_VR if _in_vr else 0
	var left: XRController3D = $XROrigin3D/LeftHand
	var right: XRController3D = $XROrigin3D/RightHand
	if left.get_has_tracking_data():
		flags |= FLAG_LEFT
	if right.get_has_tracking_data():
		flags |= FLAG_RIGHT
	$Client.send_pose(camera.global_transform, left.global_transform, right.global_transform, flags)


# --- joining ---------------------------------------------------------------

## The panel's rows. Rebuilt whenever anything on it changes, which is cheaper
## than keeping a handle on every label.
func _panel_rows() -> Array:
	if _panel_page == "warning":
		return _warning_rows()
	if _panel_page == "advanced":
		return _advanced_rows()
	if _panel_page == "account":
		return _account_rows()
	var rows: Array = []
	rows.append({"id": "address", "label": "Server", "text": _details.address, "icon": "res://icons/globe.svg"})
	rows.append({"id": "name", "label": "Name", "text": _details.player_name, "icon": "res://icons/user.svg"})
	rows.append({
		"id": "account",
		"label": "Account",
		"note": _fireplace.handle if _fireplace.is_signed_in() else "signed out",
		"icon": "res://icons/people.svg" if _fireplace.is_signed_in() else "res://icons/lock.svg",
		"more": true,
	})
	rows.append({"label": ""})
	rows.append({"id": "join", "label": "Connecting..." if _connecting else "Join", "button": true,
		"icon": "res://icons/flame.svg"})
	rows.append({"label": ""})
	rows.append({"id": "advanced", "label": "Advanced", "icon": "res://icons/gear.svg", "more": true})
	rows.append({"id": "quit", "label": "Quit Hearth", "icon": "res://icons/exit.svg", "danger": true})
	rows.append({"label": ""})
	# The one thing nobody could guess, and the bug report that caused it.
	rows.append({"label": "Point and click, or arrow keys and Enter." if not _in_vr
		else "Point with a controller and pull the trigger."})
	if _message != "":
		rows.append({"label": _message, "warn": true})
	return rows


## Signing in to the Fireplace: the one server that knows who people are.
## Your account is what friends add; the name above is what a Flame shows
## over your head, and the two are deliberately separate (docs/TRUST.md).
func _account_rows() -> Array:
	if _fireplace.is_signed_in() and not _fireplace.recovery_codes.is_empty():
		# Straight after making an account: shown this once, never saved.
		var codes: Array = [{"heading": "Your recovery codes"},
			{"label": "Each resets your password once. Write"},
			{"label": "them down: they are not shown again."}]
		for code in _fireplace.recovery_codes:
			codes.append({"label": str(code)})
		codes.append({"id": "codes_done", "label": "I wrote them down", "button": true})
		return codes
	if _fireplace.is_signed_in():
		var mine: Array = [
			{"label": _fireplace.handle, "avatar": _fireplace.handle, "status": "online", "sub": "signed in"},
			{"label": ""},
		]
		# Only where the Fireplace sends email at all (GET /features).
		if bool(_fireplace.features.get("email", false)):
			var state := "none yet, optional" if _fireplace.email == "" else (
				"confirmed" if _fireplace.email_verified else "open the link we sent")
			mine.append({"id": "email", "label": "Email", "text": _email_typed if _email_typed != ""
				else _fireplace.email, "icon": "res://icons/globe.svg", "sub": state})
			mine.append({"id": "password", "label": "Password", "text": _password_typed, "secret": true,
				"icon": "res://icons/lock.svg"})
			mine.append({"id": "save_email", "label": "Save email", "icon": "res://icons/plus.svg"})
			mine.append({"label": "Only for resets and security notices."})
			mine.append({"label": ""})
		mine.append({"id": "signout", "label": "Sign out", "icon": "res://icons/exit.svg"})
		mine.append({"id": "back", "label": "Back", "icon": "res://icons/back.svg"})
		mine.append({"label": ""})
		mine.append({"label": "Friends are on the menu, in world."})
		mine.append({"label": _message, "warn": true} if _message != "" else {"label": ""})
		return mine
	var rows: Array = [
		{"id": "handle", "label": "Handle", "text": _handle_typed, "icon": "res://icons/user.svg"},
		{"id": "password", "label": "Password", "text": _password_typed, "secret": true,
			"icon": "res://icons/lock.svg"},
		{"label": ""},
		{"id": "signin", "label": "Sign in", "button": true},
		{"label": "New here? Also fill in:"},
		{"id": "email", "label": "Email", "text": _email_typed, "icon": "res://icons/send.svg"},
		{"id": "birthday", "label": "Birthday YYYY-MM-DD", "text": _birthday_typed, "icon": "res://icons/user.svg"},
		{"id": "accept_terms", "label": "I accept terms & privacy", "toggle": _accept_terms,
			"icon": "res://icons/check.svg"},
	]
	if bool(_fireplace.features.get("email", false)):
		rows.append({"id": "policy_emails", "label": "Email me policy changes", "toggle": _policy_emails,
			"icon": "res://icons/globe.svg"})
	rows.append_array([
		{"id": "signup", "label": "Create an account", "icon": "res://icons/plus.svg"},
		{"id": "back", "label": "Play without one", "icon": "res://icons/back.svg"},
	])
	# Opened in the system browser: the reset form and the policies are web
	# pages the Fireplace serves, and they are read once, not played in.
	if _fireplace.can_reset():
		rows.append({"id": "forgot", "label": "Forgot password?", "icon": "res://icons/lock.svg", "more": true})
	rows.append_array([
		{"label": ""},
		{"label": "An account is for friends and for finding"},
		{"label": "them. A Flame never sees your password."},
		{"label": "Your birthday only checks your age."},
		{"id": "privacy", "label": "Privacy", "more": true},
		{"id": "terms", "label": "Terms", "more": true},
		{"label": _message, "warn": true} if _message != "" else {"label": ""},
	])
	return rows


## Shown every time the advanced page is opened, and not remembered. It is
## one extra click, and it is the only thing between somebody and handing
## their sign-in to a stranger.
func _warning_rows() -> Array:
	return [
		{"heading": "Careful"},
		{"label": "For people who know what they are doing."},
		{"label": ""},
		{"label": "Direct connect skips the account server."},
		{"label": "Nothing then checks who anybody is, the"},
		{"label": "server you are joining included."},
		{"label": ""},
		{"label": "Pointing accounts somewhere else hands"},
		{"label": "whoever runs it your sign-in."},
		{"label": ""},
		{"id": "understood", "label": "I understand, show me", "danger": true, "more": true},
		{"id": "back", "label": "Go back", "icon": "res://icons/back.svg"},
	]


func _advanced_rows() -> Array:
	return [
		{"id": "accounts", "label": "Accounts", "text": _details.account_server, "icon": "res://icons/globe.svg"},
		{"id": "direct", "label": "Join as a guest", "toggle": _details.direct, "icon": "res://icons/lock.svg"},
		{"label": ""},
		{"label": "A guest plays, but servers keep no"},
		{"label": "saves for them, and some let none in."},
		{"label": ""},
		{"id": "back", "label": "Back", "icon": "res://icons/back.svg"},
	]


func _open_panel() -> void:
	_panel_page = "main"
	_panel.mount(_panel_rows(), [], _panel_title())
	_place_lift_ui()


## Rebuilds the panel in place after anything on it changed.
func _refresh_panel() -> void:
	if _panel.is_open():
		_panel.mount(_panel_rows(), [], _panel_title())


func _panel_title() -> String:
	match _panel_page:
		"account":
			return "Account"
		"warning", "advanced":
			return "Advanced"
	return "Hearth"


## The panel and the keyboard are bolted to the lift car, so they ride down
## with you. They are not children of it, because the car is rebuilt on every
## ride and freed once it has fallen away.
func _place_lift_ui() -> void:
	var camera: XRCamera3D = $XROrigin3D/Camera
	for ui: Node3D in [_panel, _keyboard]:
		if not ui.visible:
			continue
		# Once you have carried the keyboard somewhere, it stays there.
		if ui == _keyboard and (_keyboard_moved or (_dragging and _drag_ui == _keyboard)):
			continue
		# The desktop line is a readout, so it sits in the middle of the view
		# and follows it, rather than hanging on the car wall where the keys go.
		if ui == _keyboard and not _keyboard.has_keys():
			var look := -camera.global_transform.basis.z
			look.y = 0.0
			look = look.normalized() if look.length() > 0.01 else Vector3.FORWARD
			_face(ui, camera.global_position + look * 1.0)
			continue
		var height := PANEL_HEIGHT if ui == _panel else KEYBOARD_HEIGHT
		if _elevator.has_car():
			ui.global_transform = _elevator.mount_at(height)
			continue
		# No car to hang it on: put it in front of the player rather than leave
		# the only way in sitting where the lift used to be.
		var forward := -camera.global_transform.basis.z
		forward.y = 0.0
		forward = forward.normalized() if forward.length() > 0.01 else Vector3.FORWARD
		_face(ui, camera.global_position + forward * 1.3 + Vector3.UP * (height - PANEL_HEIGHT))


func _join() -> void:
	var parsed := _details.parsed()
	if str(parsed["problem"]) != "":
		_message = str(parsed["problem"])
		_refresh_panel()
		return
	_details.save()
	_connecting = true
	_message = ""
	# The lift stays put until the server answers. It used to leave the moment
	# you picked one, so a refused or unreachable server rode you all the way
	# down, opened the doors on the hub, and then snapped the car back to the
	# top when the failure arrived. `_on_connected` sends it down instead.
	_status("Connecting to %s:%d..." % [parsed["host"], int(parsed["port"])])
	_refresh_panel()
	# Signed in, the server is told who you are by a ticket from the
	# Fireplace, and keeps your saves; otherwise you join as a guest.
	var as_account := _fireplace.is_signed_in() and not _details.direct
	$Client.connect_to_server(
		str(parsed["host"]),
		int(parsed["port"]),
		_details.player_name.strip_edges(),
		_fireplace.base_url() if as_account else "",
		_fireplace.token if as_account else ""
	)


func _on_panel_chosen(id: String) -> void:
	_audio.play("click")
	match id:
		"join":
			if not _connecting:
				_join()
		"account":
			_message = ""
			_panel_page = "account"
			_refresh_panel()
		"signin":
			_message = "Signing in..."
			_fireplace.sign_in(_handle_typed, _password_typed)
			_refresh_panel()
		"signup":
			if not _accept_terms:
				_message = "Accept the terms and privacy first."
			else:
				_message = "Making an account..."
				_fireplace.create_account(_handle_typed, _password_typed, _email_typed.strip_edges(),
					_birthday_typed.strip_edges(), _accept_terms, _policy_emails)
			_refresh_panel()
		"accept_terms":
			_accept_terms = not _accept_terms
			_refresh_panel()
		"policy_emails":
			_policy_emails = not _policy_emails
			_refresh_panel()
		"forgot":
			OS.shell_open(_fireplace.page_url("reset"))
			_message = "The reset page is open in your browser."
			_refresh_panel()
		"privacy", "terms":
			OS.shell_open(_fireplace.page_url(id))
		"codes_done":
			_fireplace.recovery_codes = []
			_panel_page = "main"
			_refresh_panel()
		"save_email":
			_message = "Saving..."
			_fireplace.set_email(_password_typed, _email_typed.strip_edges())
			_password_typed = ""
			_refresh_panel()
		"signout":
			_fireplace.sign_out()
			_password_typed = ""
			_panel_page = "main"
			_refresh_panel()
		"advanced":
			_panel_page = "warning"
			_refresh_panel()
		"understood":
			_panel_page = "advanced"
			_refresh_panel()
		"back":
			_panel_page = "main"
			_refresh_panel()
		"direct":
			_details.direct = not _details.direct
			_details.save()
			_refresh_panel()
		"quit":
			get_tree().quit()


## A typed field on the menu: the ember's name. The menu goes away while the
## keyboard is up -- they draw in the same band, so one would show through the
## other -- and comes back when the typing is done.
func _on_menu_edit(id: String, label: String, value: String) -> void:
	_audio.play("click")
	_editing = id
	_editing_label = label
	_return_to_menu = true
	# What you type into chat goes out and the field is empty again, so the
	# keyboard starts empty too.
	if id == "chat":
		value = ""
	_menu.close()
	_keyboard.open(label, value, _in_vr)
	_place_lift_ui()


func _on_panel_edit(id: String, label: String, value: String) -> void:
	_audio.play("click")
	_editing = id
	_editing_label = label
	# The key grid is for the headset only. On the desktop you already have a
	# keyboard, so all that comes up is the line, to show what you are typing.
	# A password is dots wherever it is shown.
	_keyboard.open(label, value, _in_vr, id == "password")
	_place_lift_ui()


func _on_typed(text: String) -> void:
	_keyboard_moved = false
	_dragging = false
	_editing_label = ""
	var from_menu := _return_to_menu
	_return_to_menu = false
	if _editing == "chat":
		$Client.say(text)
	elif _editing == "ember_name":
		_ember_name = text
	elif _editing == "handle":
		_handle_typed = text
	elif _editing == "birthday":
		_birthday_typed = text
	elif _editing == "password":
		# Never saved and never written to the settings file: it goes to the
		# Fireplace and nowhere else.
		_password_typed = text
	elif _editing == "email":
		_email_typed = text
	elif _editing == "addfriend":
		_friend_typed = ""
		_fireplace.add_friend(text)
		_status("Asked %s to be friends." % text)
	elif _editing == "address":
		_details.address = text
	elif _editing == "name":
		_details.player_name = text
	elif _editing == "accounts":
		_details.account_server = text
		_details.save()
		_fireplace.fetch_features()
	_editing = ""
	_keyboard.close()
	_audio.play("click")
	_refresh_panel()
	if from_menu:
		_open_menu()


func _on_typing_cancelled() -> void:
	_keyboard_moved = false
	_dragging = false
	var from_menu := _return_to_menu
	_return_to_menu = false
	_editing = ""
	_keyboard.close()
	if from_menu:
		_open_menu()
	_refresh_panel()


func _on_connected(your_id: int, _tick_hz: int, instance: int, flags: int) -> void:
	_joined = true
	_connecting = false
	_my_id = your_id
	_instance = instance
	_can_create = (flags & 1) != 0
	_message = ""
	_keyboard.close()
	_panel.close()
	_show_portals()
	_elevator.descend()
	_status("Going down...")
	if _in_vr:
		_status("You are player %d. Y opens the menu." % your_id)
	else:
		_status("You are player %d. Click to look, WASD to move, Y for the menu." % your_id)
	# Following a friend: their Flame let us in, now walk into their ember.
	# `_on_moved` reports where we land; this only has to ask.
	if _join_ember >= 0 and _join_ember != instance:
		$Client.join_instance(_join_ember)
	_join_ember = -1
	_report_presence()


# --- worlds and instances --------------------------------------------------

func _on_world_added(slug: String, size: int) -> void:
	_worlds[slug] = size
	_show_portals()
	# The world of the instance we are in is the one we need right now.
	if _instances.get(_instance, {}).get("slug", "") == slug:
		_want_world(slug)


func _on_instance_added(instance: int, slug: String, players: int, ember_name: String, kind: int) -> void:
	_instances[instance] = {"slug": slug, "players": players, "name": ember_name, "kind": kind}
	_show_portals()
	if instance == _instance:
		_want_world(slug)


func _on_instance_closed(instance: int) -> void:
	_instances.erase(instance)
	_show_portals()


func _on_instance_count(instance: int, players: int) -> void:
	if _instances.has(instance):
		_instances[instance]["players"] = players
		_show_portals()


## The server moved us: every peer we knew is in another world now.
func _on_moved(instance: int) -> void:
	if instance == _instance:
		_status("That world would not open.")
		_show_portals()
		return
	_instance = instance
	_menu.close()
	# Quietly: we left them, they did not leave.
	for id: int in _peers.keys():
		_on_peer_left(id, true)
	_add_chat(-1, "", "You went to %s" % _ember_label(instance))
	for holder: Node3D in _held.keys():
		_release(holder)
	_pending.clear()
	_holders.clear()
	_thrown.clear()
	_remote.clear()
	var slug := str(_instances.get(instance, {}).get("slug", ""))
	if slug == _loaded:
		_ride_in()
	_want_world(slug)
	_show_portals()
	_report_presence()


## Asks for a world unless it is already here. `world_ready` finishes the job.
func _want_world(slug: String) -> void:
	if slug == _loaded:
		return
	if slug == "":
		_load_home()
		return
	_wanted = slug
	if not _worlds.has(slug):
		# The hub may have no world file at all; the built-in one stands in.
		_load_home()
		return
	_status("Getting the world %s..." % slug)
	$Client.request_world(slug)


func _on_world_ready(slug: String, path: String) -> void:
	if slug != _wanted:
		return
	var dir := Session.CACHE.path_join(path.get_file().get_basename())
	if not FileAccess.file_exists(dir.path_join(WorldFormat.WORLD_FILE)):
		var problem := WorldFormat.unpack(path, dir)
		if problem != "":
			_status("The world %s is not usable: %s" % [slug, problem])
			return
	# The hub the server hands out is usually the very one already standing in
	# front of you, because both came from the same world file. Rebuilding it
	# is a couple of hundred objects and their collision for no change at all,
	# which is the hitch you feel on joining.
	var built := dir.path_join(WorldFormat.WORLD_FILE)
	if _built_from != "" and FileAccess.get_md5(built) == _built_from:
		_loaded = slug
		_show_portals()
		return
	if _load_world(dir):
		_loaded = slug
		# The lift says where you are when it opens; saying it now would be
		# four seconds early and from inside a descending box.


## Replaces the room with a world from disk and puts the player at its spawn.
func _load_world(dir: String) -> bool:
	var world := WorldFormat.read(dir)
	if world.has("error"):
		push_warning("world: %s" % world["error"])
		return false
	_built_from = FileAccess.get_md5(dir.path_join(WorldFormat.WORLD_FILE))
	_replace_world(WorldFormat.build(world, dir))
	return true


## Back to the hub built into the exe: the hub of a server with no world file,
## or wherever we land after a disconnect.
func _load_home() -> void:
	if _loaded == "":
		return
	_loaded = ""
	_replace_world(_home())


## The built-in hub. Falls back to `home_world` only if the world file is
## missing or broken, so a bad edit to world.json still leaves a room to stand
## in instead of a void.
func _home() -> Node3D:
	var world := WorldFormat.read(HOME_WORLD)
	if world.has("error"):
		push_warning("built-in hub: %s" % world["error"])
		return home_world.instantiate()
	_built_from = FileAccess.get_md5(HOME_WORLD.path_join(WorldFormat.WORLD_FILE))
	return WorldFormat.build(world, HOME_WORLD)


func _replace_world(built: Node3D) -> void:
	var old := get_node_or_null("World")
	if old != null:
		remove_child(old)
		old.queue_free()
	built.name = "World"
	add_child(built)
	move_child(built, 0)
	_index_items()
	_show_portals()
	_stand_mirror()
	_ride_in()


## Puts the mirror in the world, off to one side of the spawn and turned to
## face it, so there is somewhere to go and look at yourself.
func _stand_mirror() -> void:
	var at := _spawn()
	var side := at.basis * Vector3(-5.0, 0.0, -2.0)
	var where := at.origin + side + Vector3.UP * 1.25
	_mirror.global_position = where
	_mirror.look_at(where + (at.origin - where).normalized(), Vector3.UP)
	# A mirror facing +Z has to look away from what it reflects.
	_mirror.rotate_object_local(Vector3.UP, PI)


## Every arrival is a lift ride. The car waits at the top of the shaft with its
## doors shut until there is somewhere to go: riding down before anybody had
## typed an address was four seconds of waiting for nothing.
func _ride_in() -> void:
	var at := _spawn()
	_teleport(at)
	_elevator.park(at)
	$XROrigin3D.position.y = _elevator.car_y()
	_fall = 0.0
	_audio.music("elevator")
	if _joined:
		_elevator.descend()
		_status("Going down...")
	else:
		_status("Type a server on the panel, then pick Join.")
	_place_lift_ui()


## The doors have started opening.
func _on_elevator_arrived() -> void:
	_audio.play("ding")
	_audio.music("hub" if _loaded == "" or _instance == HUB_INSTANCE else "")
	if _joined:
		_status("You are in %s" % ("the hub" if _loaded == "" else _loaded))
	else:
		_status("Type a server on the panel, then pick Join.")


## Where this world says the floor of the world is: fall past it and you are
## put back. A world that does not say gets the default.
func _kill_y() -> float:
	var world := get_node_or_null("World")
	if world == null:
		return KILL_Y
	return float(world.get_meta("kill_y", KILL_Y))


func _spawn() -> Transform3D:
	var spawn: Node3D = get_node_or_null("World/Spawn")
	return spawn.transform if spawn != null else Transform3D.IDENTITY


## Moves the play space so the player's head lands on the spawn point.
func _teleport(at: Transform3D) -> void:
	var origin: XROrigin3D = $XROrigin3D
	var camera: XRCamera3D = $XROrigin3D/Camera
	origin.position = at.origin - Vector3(camera.position.x, 0.0, camera.position.z)
	origin.rotation.y = at.basis.get_euler().y
	_fall = 0.0
	for holder: Node3D in _held.keys():
		_release(holder)


## Portals stand in the hub, one per instance; inside an instance there is one
## that takes you home. There are none before you have joined.
func _show_portals() -> void:
	var at := _spawn()
	if Session.offline or not _joined:
		$Portals.show_portals([], at)
		return
	var entries: Array = []
	if _instance == HUB_INSTANCE:
		for instance: int in _instances:
			if instance == HUB_INSTANCE:
				continue
			var running: Dictionary = _instances[instance]
			entries.append({
				"instance": instance,
				"label": "%s\n%d here" % [running.get("slug", "?"), int(running.get("players", 0))],
			})
		if _can_create:
			for slug: String in _worlds:
				entries.append({"slug": slug, "label": "new %s" % slug})
	else:
		entries.append({"instance": HUB_INSTANCE, "label": "back to the hub"})
	$Portals.show_portals(entries, at)


## Dropped out: back into the lift, with the panel up and the reason on it.
func _failed(reason: String) -> void:
	_joined = false
	_connecting = false
	_instance = HUB_INSTANCE
	_join_ember = -1
	# Nobody can follow us anywhere any more.
	_fireplace.clear_presence()
	# The page you were looking at is about a flame you have left.
	_menu_world = ""
	_instances.clear()
	_worlds.clear()
	_wanted = ""
	_my_id = NOBODY
	_chat.clear()
	_menu.close()
	_quick.close()
	_keyboard.close()
	_load_home()
	for id: int in _peers.keys():
		_on_peer_left(id)
	for holder: Node3D in _held.keys():
		_release(holder)
	_pending.clear()
	_holders.clear()
	_thrown.clear()
	_remote.clear()
	_show_portals()
	Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	_message = reason
	# `_load_home` only rebuilds when we were in a downloaded world, so the
	# ride is asked for here rather than relied on.
	_ride_in()
	_open_panel()
	print(reason)


## Rendering to the headset. A runtime can show up long after launch, so this
## is retried while you are still in the lift.
func _enter_vr() -> bool:
	if _in_vr:
		return true
	if _xr == null or (not _xr.is_initialized() and not _xr.initialize()):
		return false
	get_viewport().use_xr = true
	DisplayServer.window_set_vsync_mode(DisplayServer.VSYNC_DISABLED)
	# Flat and level: from here the headset drives it.
	$XROrigin3D/Camera.transform = Transform3D.IDENTITY
	_in_vr = true
	return true


## The headset may only show up after launch, so entering VR keeps being tried
## while you are still in the lift.
func _offer_vr(delta: float) -> void:
	if _in_vr or not Session.vr:
		return
	_xr_retry -= delta
	if _xr_retry <= 0.0:
		_xr_retry = 1.0
		if _enter_vr():
			_message = ""
			_refresh_panel()


## Puts every mesh under `node` on the player layer as well as the world one,
## which is the whole of how the cheap mirror tells people from scenery.
func _on_layer(node: Node) -> void:
	if node is VisualInstance3D:
		node.layers |= Mirror.LAYER_PLAYERS
	for child in node.get_children():
		_on_layer(child)


func _on_peer_joined(id: int, peer_name: String) -> void:
	var avatar := avatar_scene.instantiate()
	$Peers.add_child(avatar)
	_on_layer(avatar)
	avatar.set_label(peer_name)
	_peers[id] = avatar
	_status("%s joined" % peer_name)
	if _joined:
		_add_chat(-1, "", "%s came in" % peer_name)
	_refresh_menu()


func _on_peer_left(id: int, quiet := false) -> void:
	if _peers.has(id):
		var gone := _peer_name(id)
		_peers[id].queue_free()
		_peers.erase(id)
		_status("player %d left" % id)
		if _joined and not quiet:
			_add_chat(-1, "", "%s left" % gone)
		_refresh_menu()


func _on_peer_pose(id: int, head: Transform3D, left: Transform3D, right: Transform3D, flags: int) -> void:
	if _peers.has(id):
		_peers[id].set_pose(head, left, right, flags)


## Status used to hang in the world on a Label3D, which read as a sign bolted
## to the sky. It is a log line now; anything the player has to act on goes on
## the lift panel instead.
func _status(text: String) -> void:
	print(text)


# --- pointing at panels ----------------------------------------------------

## True while something in the world wants the stick and the arrow keys.
func _ui_open() -> bool:
	return _keyboard.is_open() or _menu.is_open() or _panel.is_open() or _quick.is_open()


## Every panel of rows that can be pointed at, nearest-in-band first.
func _menus() -> Array[WorldMenu]:
	return [_menu, _friends, _chat_panel, _panel]


## Where the pointer ray starts and which way it goes.
##
## In the headset it comes from a controller. On the desktop it comes from the
## camera through the mouse, so you point at a panel the way you would point at
## anything else on a screen -- or straight ahead through the crosshair while
## the mouse is captured for looking around.
func _pointer(hand: Node3D = null) -> Dictionary:
	if hand != null:
		return {"from": hand.global_position, "dir": -hand.global_transform.basis.z}
	var camera: XRCamera3D = $XROrigin3D/Camera
	if _in_vr:
		var right: XRController3D = $XROrigin3D/RightHand
		return {"from": right.global_position, "dir": -right.global_transform.basis.z}
	if Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		return {"from": camera.global_position, "dir": -camera.global_transform.basis.z}
	var mouse := get_viewport().get_mouse_position()
	return {"from": camera.project_ray_origin(mouse), "dir": camera.project_ray_normal(mouse)}


## Every frame a panel is up: highlight what the pointer is over, and carry the
## keyboard while its grab bar is held.
func _point_ui() -> void:
	if _slider_row >= 0:
		if not is_instance_valid(_slider_menu) or not _slider_menu.is_open():
			_release_slider()
			return
		var ray := _pointer(_drag_hand)
		var on := _slider_menu.hit(ray["from"], ray["dir"])
		if not on.is_empty():
			_slider_menu.drag(_slider_row, on["local"])
		return
	if _dragging:
		if not is_instance_valid(_drag_ui):
			_dragging = false
			return
		var ray := _pointer(_drag_hand)
		_face(_drag_ui, ray["from"] + Vector3(ray["dir"]) * _drag_distance + _drag_offset)
		return
	if not _in_vr:
		_hover(_pointer())
		return
	for hand: XRController3D in [$XROrigin3D/RightHand, $XROrigin3D/LeftHand]:
		if _hover(_pointer(hand)):
			return


## Highlights whatever this ray is over. True when it hit something.
func _hover(ray: Dictionary) -> bool:
	var from: Vector3 = ray["from"]
	var dir: Vector3 = ray["dir"]
	if _quick.is_open():
		var on := _quick.hit(from, dir)
		_quick.hover_at(on["local"] if not on.is_empty() else Vector3(INF, INF, 0))
		if not on.is_empty():
			return true
	if _keyboard.is_open():
		var on := _keyboard.hit(from, dir)
		if not on.is_empty():
			_keyboard.hover_at(on["local"])
			return true
	for menu: WorldMenu in _menus():
		if not menu.is_open():
			continue
		var on := menu.hit(from, dir)
		if not on.is_empty():
			# Only the panel being pointed at says which row it is on.
			for other: WorldMenu in _menus():
				other.set_focus(other == menu)
			menu.hover_at(on["local"])
			return true
	return false


## A click or a trigger pull, wherever the pointer is. True when a panel took
## it, which is what stops the same click also grabbing a crate.
func _ui_click(hand: Node3D = null) -> bool:
	var ray := _pointer(hand)
	var from: Vector3 = ray["from"]
	var dir: Vector3 = ray["dir"]
	if _quick.is_open():
		var on := _quick.hit(from, dir)
		if not on.is_empty():
			_quick.activate_at(on["local"])
			return true
		# Clicked away from it: it goes, and the click does nothing else.
		_quick.close()
		return true
	if _keyboard.is_open():
		var on := _keyboard.hit(from, dir)
		if not on.is_empty():
			var local: Vector3 = on["local"]
			if _keyboard.is_bar(local):
				_take_hold(_keyboard, hand, from, dir, float(on["distance"]))
				_keyboard_moved = true
				return true
			_keyboard.press_at(local)
			return true
	# Checked after the keyboard, so clicking another field while the keyboard
	# is up moves straight to it instead of only putting the keyboard away.
	for menu: WorldMenu in _menus():
		if not menu.is_open():
			continue
		var on := menu.hit(from, dir)
		if on.is_empty():
			continue
		if menu.is_bar(on["local"]):
			# Hold the strip across the top and the panel comes with you.
			_take_hold(menu, hand, from, dir, float(on["distance"]))
			return true
		var slider := menu.slider_at(on["local"])
		if slider >= 0:
			# Take hold of it: `_point_ui` keeps setting it until you let go.
			_slider_menu = menu
			_slider_row = slider
			menu.drag(slider, on["local"])
			return true
		menu.activate_at(on["local"])
		return true
	# Clicked clean away from everything.
	if _keyboard.is_open():
		_keyboard.accept() # Puts it away, keeping what was typed.
		return true
	if _menu.is_open():
		_toggle_menu()
		return true
	return false


## Picks a panel up by its grab bar: `_point_ui` carries it until the trigger
## or the mouse button is let go.
func _take_hold(ui: FlatPanel, hand: Node3D, from: Vector3, dir: Vector3, distance: float) -> void:
	_dragging = true
	_drag_ui = ui
	_drag_hand = hand
	_drag_distance = distance
	_drag_offset = ui.global_position - (from + dir * distance)


func _release_slider() -> void:
	_slider_menu = null
	_slider_row = -1


## Puts a panel at `at`, upright and turned towards the player. The quad faces
## +Z and culls its back, so -Z has to point away from the player.
func _face(ui: Node3D, at: Vector3) -> void:
	var head: Vector3 = $XROrigin3D/Camera.global_position
	var away := at - head
	away.y = 0.0
	away = away.normalized() if away.length() > 0.01 else Vector3.FORWARD
	ui.global_transform = Transform3D(Basis.IDENTITY, at).looking_at(at + away, Vector3.UP)


## The mouse points at panels while one is up and looks around when none is.
## Without this the click that opens a field also captures the mouse, and the
## next flick of the wrist swings the panel off the screen -- which is what
## "I cannot even type into the panel" was.
func _sync_mouse() -> void:
	if _in_vr:
		return
	var want := Input.mouse_mode
	if _ui_open():
		want = Input.MOUSE_MODE_VISIBLE
	elif _mouse_look and _joined:
		want = Input.MOUSE_MODE_CAPTURED
	if Input.mouse_mode != want:
		Input.mouse_mode = want


## A thin beam from each hand, so in the headset you can see where you are
## pointing. Only while there is something to point at.
func _show_pointers() -> void:
	var beams := _in_vr and _ui_open()
	$XROrigin3D/LeftHand/Pointer.visible = beams
	$XROrigin3D/RightHand/Pointer.visible = beams
	# On the desktop the mouse is free while a panel is up, so the crosshair
	# would be a second cursor pointing somewhere else.
	$XROrigin3D/Camera/Crosshair.visible = not _in_vr and not _ui_open()


# --- input -----------------------------------------------------------------


func _unhandled_input(event: InputEvent) -> void:
	# Cast once, so every branch below is a typed access rather than a guess.
	var key := event as InputEventKey
	# Letting go of Y: a tap if it was not held long enough to be a hold.
	if key != null and not key.pressed and key.keycode == KEY_Y and _hold_time >= 0.0 and _hold_hand == null:
		_end_hold()
		return
	if key != null and key.pressed and not key.echo:
		# The real keyboard types into the virtual one, which stays up as the
		# readout. That is the whole of desktop text entry.
		if _keyboard.is_open():
			if key.keycode == KEY_ENTER or key.keycode == KEY_KP_ENTER:
				_keyboard.accept()
			elif key.keycode == KEY_ESCAPE:
				_on_typing_cancelled()
			elif key.keycode == KEY_BACKSPACE:
				_keyboard.backspace()
			# The desktop line has no keys to click, so the shortcuts every
			# other program on the machine uses are the only way to reach the
			# clipboard from here. Checked before the character below: with
			# ctrl held, C and V arrive as control codes rather than letters.
			elif key.ctrl_pressed and key.keycode == KEY_C:
				_keyboard.copy()
			elif key.ctrl_pressed and key.keycode == KEY_V:
				_keyboard.paste()
			elif key.unicode >= 32:
				_keyboard.type(String.chr(key.unicode))
			return
		if _panel.is_open():
			if key.keycode == KEY_ENTER or key.keycode == KEY_KP_ENTER:
				_panel.activate()
			return
		if _joined:
			_world_key(key)
	if not _in_vr:
		_mouse_input(event)


func _world_key(event: InputEventKey) -> void:
	if event.keycode == KEY_Y:
		_begin_hold(null)
		return
	if _quick.is_open():
		if event.keycode == KEY_ESCAPE:
			_quick.close()
		return
	if _menu.is_open():
		# While the menu is up the keyboard belongs to it.
		if event.keycode == KEY_ENTER or event.keycode == KEY_KP_ENTER:
			_menu.activate()
		elif event.keycode == KEY_ESCAPE:
			_toggle_menu()
		return
	if event.keycode == KEY_F10 and Session.offline:
		get_tree().change_scene_to_file("res://studio.tscn")
	elif event.keycode == KEY_SPACE:
		_jump()
	elif event.keycode == KEY_V:
		_toggle_mute()
	elif event.keycode == KEY_C:
		_toggle_camera(null)
	elif event.keycode == KEY_ENTER or event.keycode == KEY_KP_ENTER or event.keycode == KEY_T:
		_start_chat(false)
	elif event.keycode == KEY_E and not _in_vr:
		_grab($XROrigin3D/Camera/HoldPoint, VIEW_GRAB_REACH)


## Click once to take the mouse, then hold the left button to pick up whatever
## the crosshair is on and let go to drop it.
func _mouse_input(event: InputEvent) -> void:
	if event is InputEventMouseButton and event.button_index == MOUSE_BUTTON_LEFT:
		if not event.pressed:
			_dragging = false
			_release_slider()
			if _joined and not _ui_open():
				_release($XROrigin3D/Camera/HoldPoint)
			return
		if _ui_open():
			_ui_click()
			return
		if Input.mouse_mode != Input.MOUSE_MODE_CAPTURED:
			Input.mouse_mode = Input.MOUSE_MODE_CAPTURED
			_mouse_look = true
		elif _camera != null and _camera_hand == null:
			_camera.shoot()
		elif _joined:
			_grab_aimed()
	elif event is InputEventKey and event.pressed and event.keycode == KEY_ESCAPE:
		Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
		_mouse_look = false
	elif event is InputEventMouseMotion:
		# Captured, or dragged with the right button: the second is how you
		# turn while a panel has the cursor for pointing.
		if Input.mouse_mode != Input.MOUSE_MODE_CAPTURED 				and (event.button_mask & MOUSE_BUTTON_MASK_RIGHT) == 0:
			return
		var camera: XRCamera3D = $XROrigin3D/Camera
		camera.rotation.y -= event.relative.x * MOUSE_SENSITIVITY
		camera.rotation.x = clampf(camera.rotation.x - event.relative.y * MOUSE_SENSITIVITY, -1.5, 1.5)


## Jump on A, mute on X, with the same handler for either hand so left-handed
## players get both. The trigger is also how you press whatever panel is in
## front of you.
func _on_hand_button(button: String, right_hand: bool) -> void:
	var hand: Node3D = $XROrigin3D/RightHand if right_hand else $XROrigin3D/LeftHand
	# Tap or hold: which one it was is only known when it is let go, or when
	# it has been down long enough (`_hold_menu`).
	if button == MENU_BUTTON:
		_begin_hold(hand)
		return
	if _ui_open():
		if button == "trigger_click":
			# Point and click, the same as on the desktop. A click that lands
			# on nothing falls back to the highlighted row, so the stick on its
			# own is still enough to drive everything.
			if not _ui_click(hand):
				if _menu.is_open():
					_menu.activate()
				elif _panel.is_open():
					_panel.activate()
		return
	if not _joined:
		return
	# The hand holding the camera takes pictures with its trigger.
	if _camera != null and hand == _camera_hand and button == "trigger_click":
		_camera.shoot()
		return
	match button:
		"ax_button":
			if right_hand:
				_jump()
			else:
				_toggle_mute()
		"grip_click", "trigger_click":
			_grab(hand, HAND_GRAB_REACH)


func _on_hand_release(button: String, right_hand: bool) -> void:
	if button == MENU_BUTTON:
		_end_hold()
		return
	if button == "trigger_click":
		_dragging = false
		_release_slider()
	if _ui_open() or not _joined:
		return
	if button == "grip_click" or button == "trigger_click":
		_release($XROrigin3D/RightHand if right_hand else $XROrigin3D/LeftHand)


## Up, down, left and right drive whatever is open; the trigger or Enter picks.
## Both sticks and the arrow keys fold into one step, so the repeat feels the
## same everywhere.
func _ui_input(delta: float) -> void:
	# The sticks are deliberately not read here: they are for walking and
	# turning, and a panel that also read them crept its selection upwards on
	# its own whenever a stick sat slightly off centre. Point and click.
	var step := Vector2i.ZERO
	if Input.is_key_pressed(KEY_UP):
		step.y = -1
	elif Input.is_key_pressed(KEY_DOWN):
		step.y = 1
	if Input.is_key_pressed(KEY_LEFT):
		step.x = -1
	elif Input.is_key_pressed(KEY_RIGHT):
		step.x = 1
	if _keyboard.is_open():
		_keyboard.hold(step, delta)
	elif _menu.is_open():
		_menu.hold(step, delta)
	elif _panel.is_open():
		_panel.hold(step, delta)


func _jump() -> void:
	if _grounded:
		_fall = JUMP_SPEED
		_grounded = false


func _toggle_mute() -> void:
	_muted = not _muted
	_audio.play("mute" if _muted else "unmute")
	_update_hud()


## What an ember is called: its own name, or its world when nobody named it.
func _ember_label(instance: int) -> String:
	var running: Dictionary = _instances.get(instance, {})
	var named := str(running.get("name", ""))
	return named if named != "" else str(running.get("slug", "?"))


## Who an ember says it lets in.
func _kind_label(kind: int) -> String:
	return str(EMBER_KINDS[clampi(kind, 0, EMBER_KINDS.size() - 1)])


func _kind_icon(kind: int) -> String:
	return str(EMBER_ICONS[clampi(kind, 0, EMBER_ICONS.size() - 1)])


## The tab strip, with the open page marked.
func _menu_tabs() -> Array:
	var tabs: Array = []
	for tab: Dictionary in MENU_TABS:
		var copy := tab.duplicate()
		copy["on"] = str(tab["id"]) == "tab:" + _menu_tab
		tabs.append(copy)
	return tabs


## What the grab bar says: where you are standing right now.
func _menu_title() -> String:
	if Session.offline or not $Client.is_connected_to_server():
		return "Hearth"
	return "Hearth  -  %s" % _ember_label(_instance)


## The menu's rows, rebuilt every time it opens so the lists are current.
func _menu_rows() -> Array:
	match _menu_tab:
		"camera":
			return _camera_rows()
		"settings":
			return _settings_rows()
	return _worlds_rows()


## The left-hand panel: your friends and where they are, then who is standing
## here with you.
func _friends_rows() -> Array:
	var rows: Array = []
	if _fireplace.is_signed_in():
		rows.append_array(FireplaceClient.friend_rows(
			_fireplace.friends, _fireplace.incoming, _friend_typed))
	else:
		rows.append({"heading": "Friends"})
		rows.append({"label": "Sign in on the lift panel", "icon": "res://icons/lock.svg",
			"sub": "to add friends and follow them"})
	rows.append({"heading": "In this ember  %d" % (_peers.size() + 1)})
	var me := _details.player_name.strip_edges()
	rows.append({"label": me, "avatar": me, "status": "online", "sub": "you"})
	for id: int in _peers:
		# Off the dictionary rather than through a typed local: an avatar has
		# no class_name, so a Node3D would not know it has a name to give.
		var peer_name := _peer_name(id)
		rows.append({"label": peer_name, "avatar": peer_name, "status": "online", "sub": "here"})
	if _peers.is_empty():
		rows.append({"label": "nobody else is here yet"})
	return rows


## Somebody's name, from their avatar.
func _peer_name(id: int) -> String:
	if _peers.has(id) and is_instance_valid(_peers[id]):
		return str(_peers[id].label())
	return "player %d" % id


## The right-hand panel: what has been said here, newest at the bottom next
## to where you say something.
func _chat_rows() -> Array:
	var rows: Array = []
	var online: bool = not Session.offline and $Client.is_connected_to_server()
	if _chat.is_empty():
		rows.append({"label": "Nothing said yet." if online else "Join a flame to chat."})
	else:
		var lines: Array = []
		var chars := int((WING_WIDTH - UiStyle.PAD * 2.0) / (UiStyle.FONT_SMALL * UiStyle.PIXEL * UiStyle.CHAR_WIDTH))
		var last_who := ""
		for entry: Dictionary in _chat:
			var who := str(entry["who"])
			if int(entry["from"]) < 0:
				lines.append({"label": str(entry["text"]), "small": true})
				last_who = ""
				continue
			# A run of lines from one person carries their name once.
			if who != last_who:
				lines.append({"label": "%s  %s" % [who, entry["time"]], "small": true, "accent": true})
				last_who = who
			for line in UiStyle.wrap(str(entry["text"]), chars):
				lines.append({"label": line, "small": true, "bright": true})
		# The newest ones: the panel grows to CHAT_LINES and no further.
		rows.append_array(lines.slice(maxi(0, lines.size() - CHAT_LINES)))
	if online:
		rows.append({"label": ""})
		rows.append({"id": "chat", "label": "", "text": "", "placeholder": "Say something...",
			"icon": "res://icons/send.svg"})
	return rows


## Worlds on this flame, and then one world's embers.
func _worlds_rows() -> Array:
	if Session.offline or not $Client.is_connected_to_server():
		return [{"label": "You are not on a flame.", "icon": "res://icons/globe.svg",
			"sub": "Worlds show up here once you join one."}]
	if _menu_world != "":
		return _ember_rows(_menu_world)
	var rows: Array = []
	if _instance != HUB_INSTANCE:
		rows.append({"id": "join:%d" % HUB_INSTANCE, "label": "Back to the hub",
			"icon": "res://icons/home.svg", "more": true})
	rows.append({"heading": "Worlds on this flame"})
	for slug: String in _worlds:
		var embers := _embers_of(slug).size()
		rows.append({
			"id": "world:%s" % slug,
			"label": slug,
			"icon": "res://icons/globe.svg",
			"note": "%d %s" % [embers, "ember" if embers == 1 else "embers"],
			"more": true,
		})
	if _worlds.is_empty():
		rows.append({"label": "this flame hands out no worlds"})
	return rows


## Which running embers are playing this world.
func _embers_of(slug: String) -> Array:
	var found: Array = []
	for instance: int in _instances:
		if str(_instances[instance].get("slug", "")) == slug:
			found.append(instance)
	return found


## One world: the embers burning in it, and how to light another.
func _ember_rows(slug: String) -> Array:
	var rows: Array = [
		{"id": "worlds", "label": "All worlds", "icon": "res://icons/back.svg"},
		{"heading": "Embers of %s" % slug},
	]
	for instance: int in _embers_of(slug):
		var running: Dictionary = _instances[instance]
		var here := int(running.get("players", 0))
		var kind := int(running.get("kind", 0))
		var row := {
			"id": "join:%d" % instance,
			"label": _ember_label(instance) + ("  (you are here)" if instance == _instance else ""),
			"icon": _kind_icon(kind),
			"note": "%d here  -  %s" % [here, _kind_label(kind)],
		}
		if instance != _instance:
			row["action"] = "Join"
		rows.append(row)
	if _embers_of(slug).is_empty():
		rows.append({"label": "none burning"})
	if not _can_create:
		rows.append({"label": "This flame lights its own embers only."})
		return rows
	rows.append({"heading": "Light a new ember"})
	rows.append({"id": "ember_name", "label": "Name", "text": _ember_name,
		"placeholder": slug, "icon": "res://icons/flame.svg"})
	rows.append({"id": "ember_kind", "label": "Who can join", "choice": _kind_label(_ember_kind),
		"icon": _kind_icon(_ember_kind)})
	rows.append({"id": "ember_new", "label": "Light it", "icon": "res://icons/plus.svg", "button": true})
	return rows


## The camera: take it out, turn it round, and the last picture you took.
func _camera_rows() -> Array:
	var rows: Array = []
	rows.append({"id": "camera", "label": "Put the camera away" if _camera != null else "Take out the camera",
		"icon": "res://icons/camera.svg", "button": true})
	rows.append({"id": "selfie", "label": "Selfie", "toggle": _camera != null and _camera.selfie,
		"icon": "res://icons/flip.svg"})
	rows.append({"label": ("Pull the trigger to take a picture." if _in_vr
		else "Click to take a picture.  C takes it out.")})
	rows.append({"heading": "Pictures"})
	if _last_photo != null:
		rows.append({"picture": _last_photo})
	rows.append({"id": "photos", "label": "Open the pictures folder", "icon": "res://icons/folder.svg",
		"note": "%d taken" % PhotoCamera.count(), "more": true})
	return rows


func _settings_rows() -> Array:
	var rows: Array = [{"heading": "Sound"}]
	rows.append({"id": "mic", "label": "Microphone", "toggle": not _muted,
		"icon": "res://icons/mic_muted.svg" if _muted else "res://icons/mic.svg"})
	rows.append({"id": "music", "label": "Music", "value": _audio.music_level,
		"icon": "res://icons/speaker.svg"})
	rows.append({"id": "sfx", "label": "Sound", "value": _audio.sfx_level,
		"icon": "res://icons/speaker.svg"})
	rows.append({"heading": "View"})
	rows.append({"id": "mirror", "label": "Mirror", "choice": _mirror.what(),
		"icon": "res://icons/mirror.svg"})
	if _fireplace.is_signed_in():
		rows.append({"heading": "Privacy"})
		rows.append({"id": "sharing", "label": "Friends can see where I am",
			"toggle": _fireplace.share_location, "icon": "res://icons/people.svg"})
	rows.append({"label": ""})
	if not Session.offline and $Client.is_connected_to_server():
		rows.append({"id": "leave", "label": "Leave this flame", "icon": "res://icons/exit.svg", "danger": true})
	rows.append({"id": "quit", "label": "Quit Hearth", "icon": "res://icons/exit.svg", "danger": true})
	return rows


## Goes and stands next to a friend: the Flame they are on, then the ember
## they are in. No new protocol and nothing new on the Flame -- it is the
## address they are reporting, put through the join we already have.
func _join_friend(whose: String) -> void:
	var found := {}
	for friend: Dictionary in _fireplace.friends:
		if str(friend.get("handle", "")) == whose:
			found = friend
	var flame := str(found.get("flame", ""))
	if flame == "":
		_status("%s is not anywhere you can follow." % whose)
		return
	var ember := int(found.get("ember", 0))
	if _joined and flame == _details.address.strip_edges():
		# Already on their Flame: it is one step, not a reconnection.
		_menu.close()
		$Client.join_instance(ember)
		return
	_menu.close()
	_details.address = flame
	# Picked up by `_on_connected` once we are through the door.
	_join_ember = ember
	_status("Following %s to %s..." % [whose, flame])
	_join()


## Tells the Fireplace where we are, so friends can follow. Only ever about
## ourselves, and only while sharing is on -- the server drops it otherwise.
func _report_presence() -> void:
	if not _fireplace.is_signed_in() or Session.offline or not $Client.is_connected_to_server():
		return
	_fireplace.report(
		_details.address.strip_edges(),
		_instance,
		_ember_label(_instance),
		str(_instances.get(_instance, {}).get("slug", "")))


func _on_signed_in(as_handle: String) -> void:
	_message = ""
	# A new account stays on the page that shows its recovery codes.
	if _panel_page == "account" and _fireplace.recovery_codes.is_empty():
		_panel_page = "main"
	_password_typed = ""
	_status("Signed in as %s." % as_handle)
	_refresh_panel()
	_refresh_menu()
	_report_presence()


func _on_signed_out(_why: String) -> void:
	_refresh_panel()
	_refresh_menu()


## Something the Fireplace would not do. Sign-in trouble belongs on the panel
## where it was typed; anything else is a line in the log.
func _on_fireplace_failed(what: String, message: String) -> void:
	if what == "signin" or what == "refresh" or what == "email":
		_message = message
		_refresh_panel()
		return
	_status(message)


## Where the pointer is aimed, an arm's length or so out: the menu opens there
## rather than always straight ahead, so it lands where you were looking.
func _pointed_at() -> Vector3:
	var ray := _pointer()
	var from: Vector3 = ray["from"]
	var dir: Vector3 = ray["dir"]
	var reach := MENU_REACH
	var sight: RayCast3D = $XROrigin3D/Camera/AimRay
	if not _in_vr:
		sight.force_raycast_update()
		if sight.is_colliding():
			reach = minf(reach, sight.get_collision_point().distance_to(from) - 0.3)
	return from + dir * clampf(reach, 0.7, MENU_REACH)


## The head, expressed in the play space the menu hangs off.
func _head_in_play_space() -> Transform3D:
	return $XROrigin3D.global_transform.affine_inverse() * $XROrigin3D/Camera.global_transform


func _toggle_menu() -> void:
	if _menu.is_open():
		_menu.close()
		_audio.play("click")
		return
	_open_menu()
	_audio.play("menu")


## Puts the menu up where you are pointing, with friends and chat either side.
## Also how it comes back after the keyboard has borrowed the view.
func _open_menu() -> void:
	_quick.close()
	_mount_menus()
	_face(_menu, _pointed_at())
	_follow_wings()


## Rebuilds the menu in place: a page change, or a setting that just moved.
func _refresh_menu() -> void:
	if _menu.is_open():
		_mount_menus()


func _mount_menus() -> void:
	_menu.title_note = ""
	_menu.mount(_menu_rows(), _menu_tabs(), _menu_title())
	var online := 0
	for friend: Dictionary in _fireplace.friends:
		online += 1 if bool(friend.get("online", false)) else 0
	_friends.title_note = "%d online" % online if _fireplace.is_signed_in() else ""
	_friends.mount(_friends_rows(), [], "Friends")
	_chat_panel.title_note = _ember_label(_instance) if $Client.is_connected_to_server() else ""
	_chat_panel.mount(_chat_rows(), [], "Chat")


## A panel that hangs beside the menu.
func _wing(title: String, icon: String) -> WorldMenu:
	var wing := WorldMenu.new()
	wing.name = title
	wing.width = WING_WIDTH
	wing.movable = false
	wing.title_icon = icon
	wing.band = UiStyle.BAND
	wing.visible = false
	wing.set_focus(false)
	wing.chosen.connect(_on_menu_chosen)
	wing.changed.connect(_on_menu_changed)
	wing.edit.connect(_on_menu_edit)
	$XROrigin3D.add_child(wing)
	return wing


## Keeps the side panels hinged to the menu's edges, turned in towards you
## like the wings of a dressing-table mirror, tops level with its top. Every
## frame, so carrying the menu by its bar carries them too.
func _follow_wings() -> void:
	if not _menu.is_open():
		if _friends.is_open():
			_friends.close()
		if _chat_panel.is_open():
			_chat_panel.close()
		return
	var half := _menu._width * 0.5
	var top := _menu._height * 0.5
	for side: float in [-1.0, 1.0]:
		var wing: WorldMenu = _friends if side < 0.0 else _chat_panel
		if not wing.is_open():
			continue
		var turn := Basis(Vector3.UP, -side * WING_ANGLE)
		var inner_edge := turn * Vector3(-side * WING_WIDTH * 0.5, 0.0, 0.0)
		var at := Vector3(side * (half + WING_GAP), top - wing._height * 0.5, 0.0) - inner_edge
		wing.transform = _menu.transform * Transform3D(turn, at)


func _on_menu_chosen(id: String) -> void:
	_audio.play("click")
	if id.begins_with("tab:"):
		_menu_tab = id.substr(4)
		_refresh_menu()
	elif id == "close":
		_menu.close()
	elif id == "mic":
		_toggle_mute()
		_refresh_menu()
	elif id == "camera":
		_toggle_camera(null)
		_refresh_menu()
	elif id == "selfie":
		if _camera == null:
			_toggle_camera(null)
		_camera.selfie = not _camera.selfie
		_refresh_menu()
	elif id == "photos":
		var folder := PhotoCamera.folder()
		DirAccess.make_dir_recursive_absolute(folder)
		OS.shell_open(folder)
	elif id == "mirror":
		# Off, people, everything, off again.
		_mirror.quality = (_mirror.quality + 1) % 3
		_refresh_menu()
	elif id == "leave":
		_menu.close()
		$Client.disconnect_from_server()
		_failed("You left the flame.")
	elif id == "quit":
		get_tree().quit()
	elif id == "worlds":
		_menu_world = ""
		_refresh_menu()
	elif id.begins_with("world:"):
		_menu_world = id.substr(6)
		_refresh_menu()
	elif id == "ember_kind":
		_ember_kind = (_ember_kind + 1) % EMBER_KINDS.size()
		_refresh_menu()
	elif id == "ember_new":
		_menu.close()
		# An unnamed ember is named after its world by the client extension.
		$Client.create_instance(_menu_world, _ember_name, _ember_kind)
	elif id == "sharing":
		_fireplace.set_sharing(not _fireplace.share_location)
		_refresh_menu()
	elif id.begins_with("friend:"):
		_join_friend(id.substr(7))
	elif id.begins_with("accept:"):
		_fireplace.accept_friend(id.substr(7))
		_status("%s is a friend now." % id.substr(7))
	elif id.begins_with("join:"):
		_menu.close()
		$Client.join_instance(int(id.substr(5)))


func _on_menu_changed(id: String, value: float) -> void:
	if id == "music":
		_audio.music_level = value
	elif id == "sfx":
		_audio.sfx_level = value
		_audio.play("click")


func _update_hud() -> void:
	var icon: Sprite3D = $XROrigin3D/Camera/MicIcon
	icon.texture = mic_muted_icon if _muted else mic_on_icon
	# Grey while nothing is going out, bright while it is. Muted stays grey,
	# because muted never sends anything.
	icon.modulate = Color(1, 1, 1) if not _muted and _voice_level > TALKING else UiStyle.TEXT_DIM


## Mic -> Opus -> the server. The capture buffer is drained even while muted,
## so unmuting does not play back a backlog.
func _send_voice() -> void:
	if _capture == null:
		return
	var available := _capture.get_frames_available()
	if available <= 0:
		return
	var captured := _capture.get_buffer(available)
	# Every sixteenth sample is plenty to know whether somebody is speaking,
	# and a loop over the whole buffer in GDScript is the cost this avoids.
	var loudest := 0.0
	for i in range(0, captured.size(), 16):
		loudest = maxf(loudest, absf(captured[i].x))
	var was := _voice_level > TALKING
	_voice_level = maxf(loudest, _voice_level * 0.85)
	if was != (_voice_level > TALKING):
		_update_hud()
	if _muted:
		return
	$Client.send_voice(captured)


func _on_peer_voice(id: int, samples: PackedFloat32Array) -> void:
	if _peers.has(id):
		_peers[id].push_voice(samples)


# --- the menu button: tap and hold ---------------------------------------------

## The menu button went down. Whether it was a tap or a hold is decided when
## it comes up again, or by `_hold_menu` once it has been down long enough.
func _begin_hold(hand: Node3D) -> void:
	if _hold_time >= 0.0:
		return
	_hold_time = 0.0
	_hold_hand = hand
	_hold_fired = false


## The menu button came up: a tap, unless holding it already did something.
func _end_hold() -> void:
	if _hold_time < 0.0:
		return
	var fired := _hold_fired
	_hold_time = -1.0
	_hold_fired = false
	if not fired:
		_menu_tap()


func _hold_menu(delta: float) -> void:
	if _hold_time < 0.0:
		return
	_hold_time += delta
	if _hold_fired or _hold_time < HOLD:
		return
	_hold_fired = true
	if _joined and not _keyboard.is_open():
		_open_quick(_hold_hand)


## A tap puts away whatever is on top, or opens the menu if nothing is.
func _menu_tap() -> void:
	if _keyboard.is_open():
		_on_typing_cancelled()
	elif _quick.is_open():
		_quick.close()
	elif _joined:
		_toggle_menu()


## The quick menu, by the hand that is holding the button -- or in the middle
## of the view on the desktop.
func _open_quick(hand: Node3D) -> void:
	_menu.close()
	var head: Vector3 = $XROrigin3D/Camera.global_position
	var at: Vector3
	if hand != null:
		var toward := head - hand.global_position
		toward.y = 0.0
		toward = toward.normalized() if toward.length() > 0.01 else Vector3.BACK
		# A little above the hand and out from it, so the hand is not in the way.
		at = hand.global_position + Vector3.UP * 0.12 - toward * 0.1
	else:
		var look: Vector3 = -$XROrigin3D/Camera.global_transform.basis.z
		at = head + look * 0.95 + Vector3.DOWN * 0.08
	var items := [
		{"id": "camera", "label": "Camera", "icon": "res://icons/camera.svg", "on": _camera != null},
		{"id": "chat", "label": "Chat", "icon": "res://icons/chat.svg"},
		{"id": "mic", "label": "Mic", "icon": "res://icons/mic_muted.svg" if _muted else "res://icons/mic.svg",
			"on": not _muted},
		{"id": "menu", "label": "Menu", "icon": "res://icons/menu.svg"},
	]
	_quick.open_at(Transform3D.IDENTITY, items)
	_face(_quick, at)
	_quick.set_meta("hand", hand)
	_audio.play("menu")


func _on_quick_chosen(id: String) -> void:
	_audio.play("click")
	var hand: Node3D = _quick.get_meta("hand", null)
	_quick.close()
	match id:
		"camera":
			_toggle_camera(hand)
		"chat":
			_start_chat(false)
		"mic":
			_toggle_mute()
		"menu":
			_open_menu()


# --- the camera -----------------------------------------------------------------

## Takes the camera out into `hand` (the right one if none is given), or puts
## it away. On the desktop it hangs at the bottom right of the view.
func _toggle_camera(hand: Node3D) -> void:
	if _camera != null:
		if _camera_hand != null:
			_camera_hand.get_node("Mesh").visible = true
		_camera.queue_free()
		_camera = null
		_camera_hand = null
		_audio.play("drop")
		return
	_camera = PhotoCamera.new()
	_camera.name = "PhotoCamera"
	_camera.taken.connect(_on_photo_taken)
	if _in_vr:
		_camera_hand = hand if hand != null else $XROrigin3D/RightHand
		_camera_hand.add_child(_camera)
		_camera.position = Vector3(0.0, 0.035, -0.075)
		# The camera is the hand now: a box poking through it looks broken.
		_camera_hand.get_node("Mesh").visible = false
	else:
		_camera_hand = null
		$XROrigin3D/Camera.add_child(_camera)
		_camera.position = Vector3(0.21, -0.15, -0.46)
		_camera.rotation = Vector3(0.12, 0.16, 0.0)
	_audio.play("grab")


func _on_photo_taken(path: String, picture: Texture2D) -> void:
	_audio.play("shutter")
	_last_photo = picture
	_photos_taken += 1
	_notify("Picture saved: %s" % path.get_file() if path != "" else "Could not save the picture.")
	_refresh_menu()


# --- chat -------------------------------------------------------------------------

func _on_chat(from: int, text: String) -> void:
	var who := "You" if from == _my_id else _peer_name(from)
	_add_chat(from, who, text)
	if from == _my_id:
		return
	if _peers.has(from) and is_instance_valid(_peers[from]):
		_peers[from].say(text)
	_audio.play("chat")
	if not _menu.is_open():
		_notify("%s: %s" % [who, text])


## One line into the log, and onto the chat panel if it is up. A `from` of -1
## is the client itself saying something.
func _add_chat(from: int, who: String, text: String) -> void:
	_chat.append({"from": from, "who": who, "text": text,
		"time": Time.get_time_string_from_system().substr(0, 5)})
	if _chat.size() > CHAT_KEEP:
		_chat = _chat.slice(_chat.size() - CHAT_KEEP)
	if _chat_panel.is_open():
		_chat_panel.mount(_chat_rows(), [], "Chat")


## Opens the keyboard on an empty chat line. `from_menu` brings the menu back
## when the line has gone.
func _start_chat(from_menu: bool) -> void:
	if Session.offline or not $Client.is_connected_to_server():
		_notify("Join a flame to chat.")
		return
	_editing = "chat"
	_editing_label = "Say something"
	_return_to_menu = from_menu
	_menu.close()
	_keyboard.open("Say something", "", _in_vr)
	_place_lift_ui()


## A line at the bottom of your view for a few seconds: a chat line while the
## menu is shut, a picture saved.
func _notify(text: String) -> void:
	_toast.text = text
	_toast.visible = true
	_toast.modulate.a = 1.0
	_toast_left = TOAST_TIME


func _fade_toast(delta: float) -> void:
	if _toast_left <= 0.0:
		return
	_toast_left -= delta
	_toast.modulate.a = clampf(_toast_left / 0.6, 0.0, 1.0)
	if _toast_left <= 0.0:
		_toast.visible = false


# --- picking things up -----------------------------------------------------

## The desktop crosshair: what it is pointing at, and what colour that makes
## it. This is the whole answer to "what am I about to pick up".
func _aim() -> void:
	var crosshair: Label3D = $XROrigin3D/Camera/Crosshair
	var ray: RayCast3D = $XROrigin3D/Camera/AimRay
	ray.force_raycast_update()
	# The ray is as long as the furthest an object may ask to be taken from, so
	# the limit is applied here per object rather than baked into its length.
	var hit := ray.get_collider() as RigidBody3D
	_aimed = null
	if hit != null and hit.is_in_group("grabbable"):
		var limit := maxf(VIEW_GRAB_REACH, float(hit.get_meta("reach", 0.0)))
		if ray.get_collision_point().distance_to(ray.global_position) <= limit:
			_aimed = hit
	# The interface colours (ui.gd): dim over nothing, accent over something you
	# can take, warn over somebody else's, good over what is in your hands.
	if _held.has($XROrigin3D/Camera/HoldPoint):
		crosshair.modulate = UiStyle.GOOD
	elif _aimed == null:
		crosshair.modulate = UiStyle.TEXT_DIM
	elif _taken_by_other(_aimed):
		crosshair.modulate = UiStyle.WARN
	else:
		crosshair.modulate = UiStyle.ACCENT


## Somebody else is holding it and it refuses to be taken out of their hands.
func _taken_by_other(body: RigidBody3D) -> bool:
	var item := int(body.get_meta("world_index", -1))
	if item < 0:
		return false
	var holder: int = _holders.get(item, NOBODY)
	return holder != NOBODY and holder != _my_id and bool(body.get_meta("locked", false))


## The desktop grab: whatever the crosshair is on. In the headset you reach for
## a thing instead, so that path stays nearest-hand-first.
func _grab_aimed() -> void:
	var hold: Node3D = $XROrigin3D/Camera/HoldPoint
	if _held.has(hold) or _aimed == null:
		return
	_request(hold, _aimed)


## Picking things up with a hand: whatever is closest to it. An object may say
## how far away it can be taken from with `reach`, and refuse to be taken out
## of somebody else's hands with `locked`.
##
## `reach` is a floor, never a limit on the hand: an object asking for less
## than the hand already has changes nothing.
func _grab(holder: Node3D, reach: float) -> void:
	if _held.has(holder):
		_release(holder)
		return
	var carried := _held.values()
	var best: RigidBody3D = null
	var nearest := 0.0
	for body: RigidBody3D in get_tree().get_nodes_in_group("grabbable"):
		if carried.has(body):
			continue
		var distance := body.global_position.distance_to(holder.global_position)
		if distance > maxf(reach, float(body.get_meta("reach", 0.0))):
			continue
		if best == null or distance < nearest:
			best = body
			nearest = distance
	if best != null:
		_request(holder, best)


## A grab is only real once the server confirms it, so two people reaching for
## the same crate cannot both end up holding it.
func _request(holder: Node3D, body: RigidBody3D) -> void:
	var item := int(body.get_meta("world_index", -1))
	# The built-in hub, a world walked offline, or no server: nothing to ask.
	if item < 0 or Session.offline or not $Client.is_connected_to_server():
		_take(holder, body)
		return
	var held_by: int = _holders.get(item, NOBODY)
	if held_by != NOBODY and bool(body.get_meta("locked", false)):
		_status("That is not yours to take.")
		return
	if held_by == _my_id:
		return # Already in our other hand.
	_pending[item] = holder
	$Client.grab_item(item)


## Start carrying something, once it is ours to carry.
func _take(holder: Node3D, body: RigidBody3D) -> void:
	body.freeze = true
	_held[holder] = body
	_audio.play("grab")


func _release(holder: Node3D) -> void:
	if not _held.has(holder):
		return
	var body: RigidBody3D = _held[holder]
	_held.erase(holder)
	body.freeze = false
	body.linear_velocity = _throw.get(body, Vector3.ZERO)
	body.angular_velocity = Vector3.ZERO
	_throw.erase(body)
	_audio.play("drop")
	var item := int(body.get_meta("world_index", -1))
	if item >= 0 and _holders.get(item, NOBODY) == _my_id:
		_thrown[item] = body


## The server decides who holds what, so this is the only place a grab becomes
## real: our own included.
func _on_item_held(item: int, by: int) -> void:
	_holders[item] = by
	var body: RigidBody3D = _items.get(item)
	var asked: Node3D = _pending.get(item)
	_pending.erase(item)
	if body == null:
		return
	if by == _my_id:
		if asked != null and not _held.has(asked):
			_take(asked, body)
		return
	# Taken off us, or put down by whoever had it.
	for carrier: Node3D in _held.keys():
		if _held[carrier] == body:
			_held.erase(carrier)
			_throw.erase(body)
			_audio.play("drop")
	_thrown.erase(item)
	# Held by somebody else: their transforms drive it, so physics must not.
	body.freeze = by != NOBODY
	if by == NOBODY:
		_remote.erase(item)


func _on_item_moved(item: int, at: Transform3D) -> void:
	if _holders.get(item, NOBODY) == _my_id:
		return # Ours; what we send is the truth.
	if _items.has(item):
		_remote[item] = at


## Items in somebody else's hands arrive at the pose rate, so they are eased
## into place the same way remote players are.
func _follow_items(delta: float) -> void:
	var weight := clampf(ITEM_SMOOTHING * delta, 0.0, 1.0)
	for item: int in _remote.keys():
		var body: RigidBody3D = _items.get(item)
		if body == null:
			_remote.erase(item)
			continue
		body.global_transform = body.global_transform.interpolate_with(_remote[item], weight)


## Everything grabbable in the world, by the index the world file gave it.
## That index is what the server relays, so a world with no indices simply has
## no networked items.
func _index_items() -> void:
	_items.clear()
	_pending.clear()
	_holders.clear()
	_thrown.clear()
	_remote.clear()
	_aimed = null
	for body: RigidBody3D in get_tree().get_nodes_in_group("grabbable"):
		var item := int(body.get_meta("world_index", -1))
		if item >= 0:
			_items[item] = body


func _carry(delta: float) -> void:
	for holder: Node3D in _held:
		var body: RigidBody3D = _held[holder]
		var was := body.global_position
		body.global_transform = holder.global_transform
		if delta > 0.0:
			_throw[body] = (body.global_position - was) / delta
		var item := int(body.get_meta("world_index", -1))
		if item >= 0 and _holders.get(item, NOBODY) == _my_id:
			$Client.send_item(item, body.global_transform)
	# A throw stays ours until it settles, so it lands in one place for
	# everybody rather than each client simulating its own arc.
	for item: int in _thrown.keys():
		var body: RigidBody3D = _thrown[item]
		if not is_instance_valid(body):
			_thrown.erase(item)
			continue
		if body.linear_velocity.length() < STILL and body.angular_velocity.length() < STILL:
			_thrown.erase(item)
			$Client.release_item(item)
			continue
		$Client.send_item(item, body.global_transform)


## Moves the play space sideways, stopped by whatever is in the way.
##
## Walking was a straight add to the play space position, with the only
## collision in the game being the ray that looks for the floor -- so every
## wall, pillar and crate in every world was walk-through. This sweeps the
## player's capsule along the step and stops at the first thing it hits, then
## retries each axis on its own so you slide along a wall instead of sticking
## to it.
func _step(move: Vector3) -> void:
	if move.is_zero_approx():
		return
	var origin: XROrigin3D = $XROrigin3D
	var camera: XRCamera3D = $XROrigin3D/Camera
	var feet := origin.global_position + Vector3(camera.position.x, 0.0, camera.position.z)
	var allowed := _sweep(feet, move)
	if allowed.is_zero_approx() and not is_zero_approx(move.x) and not is_zero_approx(move.z):
		# Blocked head on: try each axis, which is what sliding along a wall is.
		allowed = _sweep(feet, Vector3(move.x, 0.0, 0.0))
		if allowed.is_zero_approx():
			allowed = _sweep(feet, Vector3(0.0, 0.0, move.z))
	origin.position += allowed


## How much of `move` the player can actually make from `feet`.
##
## A ShapeCast3D node rather than a space-state query: `direct_space_state` is
## only valid inside physics, and called from `_process` it quietly found
## nothing at all -- walls with collision, and the player strolling through
## 5.99 m of an attempted 6. The ground ray already works this way.
##
## The node hangs off Main, which never rotates, so its local axes are the
## world's even when the play space has been turned.
func _sweep(feet: Vector3, move: Vector3) -> Vector3:
	var cast: ShapeCast3D = $BodyCast
	cast.position = feet + Vector3.UP * (BODY_HEIGHT * 0.5 + STEP_UP)
	cast.target_position = move
	cast.force_shapecast_update()
	var safe := cast.get_closest_collision_safe_fraction()
	# A cast that starts already touching reports the whole move as safe --
	# measured: resting against a door it returned 1.0 while colliding, and the
	# player strolled through. Once in contact, the surface decides: into it is
	# blocked, away from it is free, which is also what lets you leave again.
	if safe >= 1.0 and cast.is_colliding():
		for i in cast.get_collision_count():
			if cast.get_collision_normal(i).dot(move) < 0.0:
				return Vector3.ZERO
	# Back off a hair, so we never come to rest inside what we stopped against.
	return move * maxf(0.0, safe - 0.001)


## The play space floats on whatever is under the player's head, so you can
## jump, fall off a crate, and stand on one.
func _fall_and_land(delta: float) -> void:
	var origin: XROrigin3D = $XROrigin3D
	var camera: XRCamera3D = $XROrigin3D/Camera
	var ray: RayCast3D = $XROrigin3D/GroundRay
	# Fall out of the world and you are put back on the spawn. Checked before
	# the ray, because down there nothing is under you to find.
	if origin.position.y < _kill_y():
		_audio.play("drop")
		_status("You fell out of the world.")
		_teleport(_spawn())
		return
	# Started above STEP_UP, not below it. The capsule steps over anything
	# shorter than STEP_UP, so a stair of that height is walkable -- but a ray
	# starting at 0.2 could not see a floor 0.35 higher than the one you were
	# on, so you stepped up and immediately slid back off. The two have to
	# agree about how tall a step is, which is why both read the one constant.
	ray.position = Vector3(camera.position.x, STEP_UP + 0.05, camera.position.z)
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
			_step(move.normalized() * minf(stick.length(), 1.0) * VR_SPEED * delta)

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
	_step(Vector3(step.x, 0.0, step.z))


func _user_args() -> Dictionary:
	var parsed := {}
	var args := OS.get_cmdline_user_args()
	for i in args.size():
		if args[i].begins_with("--") and i + 1 < args.size():
			parsed[args[i].substr(2)] = args[i + 1]
	return parsed
