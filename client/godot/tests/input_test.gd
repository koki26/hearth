extends SceneTree

## Drives the real main.tscn with real input events, without a window:
##   godot --headless --path client/godot --script res://tests/input_test.gd
##
## This exists because "I cannot type into the panel on desktop" got shipped:
## every piece underneath had its own test and the routing between them had
## none. It loads the actual scene and pushes actual key presses through it.

var _failed := 0
var _checks := 0


func _initialize() -> void:
	_run()


func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)


func _key(code: Key, unicode: int = 0) -> void:
	for pressed in [true, false]:
		var event := InputEventKey.new()
		event.keycode = code
		event.physical_keycode = code
		event.unicode = unicode
		event.pressed = pressed
		Input.parse_input_event(event)


func _run() -> void:
	var main: Node3D = load("res://main.tscn").instantiate()
	root.add_child(main)
	await process_frame
	await process_frame

	var panel: WorldMenu = main.get_node("Panel")
	var keys: WorldKeyboard = main.get_node("Keyboard")

	_check(panel.is_open(), "the lift panel is up before you have joined")
	_check(not keys.is_open(), "and the keyboard is not")

	# Enter on the first row, which is the server address.
	_key(KEY_ENTER)
	await process_frame
	_check(keys.is_open(), "Enter on a field opens the keyboard")
	var started: String = keys.text()
	_check(started != "", "the keyboard starts from what the field held")

	# Type into it with a real keyboard.
	_key(KEY_X, "x".unicode_at(0))
	await process_frame
	_check(keys.text() == started + "x", "a letter typed on a real keyboard reaches the field")

	_key(KEY_BACKSPACE)
	await process_frame
	_check(keys.text() == started, "backspace reaches it too")

	# Enter accepts, which writes the value back and closes the keyboard.
	_key(KEY_ENTER)
	await process_frame
	_check(not keys.is_open(), "Enter closes the keyboard")
	_check(panel.is_open(), "and leaves the panel up")

	_the_menu_pages(main)

	await _no_connect_keeps_the_lift_up(main)

	await _walls_are_solid(main)

	await _stairs(main)

	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)


## The menu's pages: picking a tab changes which rows come out, and a world's
## page offers its embers plus the one you are about to light.
func _the_menu_pages(main: Node3D) -> void:
	var ids := func(rows: Array) -> Array: return rows.map(func(row): return str(row.get("id", "")))

	main._on_menu_chosen("tab:camera")
	_check(main._menu_tab == "camera", "a tab pick changes the page")
	_check(ids.call(main._menu_rows()).has("camera"), "the camera page takes the camera out")
	main._on_peer_joined(7, "Bob")
	var text := func(row: Dictionary) -> String: return str(row.get("heading", row.get("label", "")))
	var people: Array = main._friends_rows().map(text)
	_check(people.any(func(label): return label.contains("Friends")), "the friends panel starts with friends")
	_check(people.any(func(label): return label.contains("In this ember")), "and then says where you are")
	_check(people.any(func(label): return label.contains("Bob")), "listing who is here by name")
	# Signed out, it says where to sign in rather than pretending to have friends.
	_check(people.any(func(label): return label.contains("Sign in")), "and how to get some")
	main._on_peer_left(7)

	# Chat: a line from somebody here lands in the log and on the chat panel,
	# under their name; our own echo is "You".
	main._my_id = 3
	main._on_peer_joined(8, "Cat")
	main._on_chat(8, "ahoj, kde jste?")
	main._on_chat(3, "u ohne")
	var said: Array = main._chat.filter(func(entry): return int(entry["from"]) >= 0)
	_check(said.size() == 2 and str(said[0]["who"]) == "Cat" and str(said[1]["who"]) == "You",
		"chat lines carry who said them")
	var said_rows: Array = main._chat_rows().map(text)
	_check(said_rows.any(func(line): return line.contains("kde jste")), "and what they said")
	_check(said_rows.any(func(line): return line.begins_with("Cat")), "under their name")
	main._on_peer_left(8)
	main._chat.clear()
	main._my_id = 0

	main._on_menu_chosen("tab:settings")
	_check(ids.call(main._menu_rows()).has("mic"), "the settings page has the microphone")

	# An ember somebody else lit, in a world this flame hands out.
	main._can_create = true
	main._worlds["forest"] = 1
	main._instances[5] = {"slug": "forest", "players": 2, "name": "Ada's fire", "kind": 2}
	var rows: Array = main._ember_rows("forest")
	var on_page: Array = ids.call(rows)
	_check(on_page.has("join:5"), "a burning ember is on its world's page")
	_check(on_page.has("ember_name") and on_page.has("ember_kind") and on_page.has("ember_new"),
		"and so is the way to light another")
	var theirs: Array = rows.filter(func(row): return str(row.get("id", "")) == "join:5")
	_check(str(theirs[0]["label"]).contains("Ada's fire"), "an ember is listed by its own name")
	_check(str(theirs[0]["note"]).contains("friends"), "with who it lets in")
	_check(not theirs[0].has("text"), "and picking it joins it instead of opening the keyboard")

	# Who can join cycles through all four and comes back round.
	var seen := {}
	for i in 4:
		seen[main._ember_kind] = true
		main._on_menu_chosen("ember_kind")
	_check(seen.size() == 4 and main._ember_kind == 0, "every kind of ember can be picked")

	main._instances.erase(5)
	main._worlds.erase("forest")
	main._can_create = false
	main._menu_tab = "worlds"

	# The menu button: a tap opens the menu, a hold opens the quick menu and
	# letting go of it then does nothing more.
	main._joined = true
	main._begin_hold(null)
	main._hold_menu(0.1)
	main._end_hold()
	_check(main._menu.is_open() and not main._quick.is_open(), "tapping the menu button opens the menu")
	main._end_hold()
	main._begin_hold(null)
	main._end_hold()
	_check(not main._menu.is_open(), "and tapping it again shuts it")
	main._begin_hold(null)
	main._hold_menu(0.25)
	main._hold_menu(0.25)
	_check(main._quick.is_open(), "holding it brings up the quick menu")
	main._end_hold()
	_check(main._quick.is_open() and not main._menu.is_open(), "which stays when the button is let go")
	main._on_quick_chosen("camera")
	_check(main._camera != null and not main._quick.is_open(), "the quick menu takes the camera out")
	_check(main._camera_rows()[0]["label"].contains("away"), "and the camera page offers to put it away")
	main._toggle_camera(null)
	_check(main._camera == null, "and it goes away again")
	main._joined = false


## A step you can walk up, and a crate you cannot walk through. The two pull
## against each other: the capsule rides STEP_UP off the ground so a stair does
## not stop it, and the ground ray has to start above STEP_UP or it cannot see
## the floor it just climbed onto and slides straight back off.
func _stairs(main: Node3D) -> void:
	var origin: Node3D = main.get_node("XROrigin3D")
	main._elevator.clear()
	origin.position = Vector3(0.0, -1.2, 9.0)
	for i in 30:
		await process_frame
	_check(is_equal_approx(origin.position.y, -1.2), "standing on the plaza")

	# North, up the two 0.4 m steps and onto the dais, whose top is y = 0.
	for i in 400:
		main._step(Vector3(0.0, 0.0, -0.03))
		await process_frame
	_check(is_equal_approx(origin.position.y, 0.0),
		"walked up the steps onto the dais (y=%.2f)" % origin.position.y)

	# A crate is waist high, so it is a wall and not a stair.
	origin.position = Vector3(2.2, -1.2, 6.0)
	for i in 30:
		await process_frame
	for i in 250:
		main._step(Vector3(0.0, 0.0, -0.03))
		await process_frame
	_check(origin.position.z > 3.6, "a crate still stops you (z=%.2f)" % origin.position.z)


## A server that never answers used to ride you all the way down, open the
## doors on the hub, and then snap the car back to the top when the failure
## finally arrived. Nothing should move until the server has said yes.
func _no_connect_keeps_the_lift_up(main: Node3D) -> void:
	var lift = main._elevator
	# descending() is "the doors are not open yet", which is also true of a
	# parked car, so motion is measured by how far up the shaft it still is.
	var top: float = lift.height()
	_check(top > 0.0, "the lift starts up the shaft")

	# _join saves what you typed, so the real settings file is put back after.
	var was: String = main._details.address
	# Nothing listens on port 1. The attempt is made and the answer is not
	# waited for, because the point is what happens before one arrives.
	main._details.address = "127.0.0.1:1"
	main._join()
	await process_frame
	await process_frame
	_check(is_equal_approx(lift.height(), top), "an unanswered join leaves it up")
	_check(not lift.doors_open(), "with the doors shut")

	# And when the refusal does land it says so in red, in the lift.
	main._failed("Server refused the join: nope")
	var shouted: Array = main._panel_rows().filter(func(row): return row.get("warn", false))
	_check(shouted.size() == 1, "the failure is one row on the panel")
	_check(str(shouted[0]["label"]).contains("refused"), "and the row is the reason why")
	_check(is_equal_approx(lift.height(), top) and not lift.doors_open(), "and the lift still has not moved")

	main._details.address = was
	main._details.save()


## Walking used to be a straight add to the play space position, so every wall
## in every world was walk-through. The shut lift is the tightest box there is
## to prove it against: four walls and nowhere to go.
func _walls_are_solid(main: Node3D) -> void:
	var origin: Node3D = main.get_node("XROrigin3D")
	var lift = main._elevator
	_check(lift.has_car() and not lift.doors_open(), "the lift is shut while you type")

	var start := origin.position
	# Shove hard at every wall in turn, far further than the car is wide.
	for way: Vector3 in [Vector3.RIGHT, Vector3.LEFT, Vector3.FORWARD, Vector3.BACK]:
		for i in 120:
			main._step(way * 0.1)
			await process_frame
	# Shoved at all four in turn you end up in a corner, so each axis is
	# checked on its own: the diagonal out of the middle of a 1.5 m box is
	# 2.1 m and proves nothing.
	var strayed := origin.position - start
	_check(absf(strayed.x) < Elevator.HALF and absf(strayed.z) < Elevator.HALF,
		"a shut lift keeps you in it (ended %.2f, %.2f from the middle)" % [strayed.x, strayed.z])
	_check(absf(strayed.x) > 0.5, "but you can still walk about inside it")

	# And once the doors are open there is a way out. Frames here are real
	# frames, so the four second ride needs a lot more of them than it looks.
	lift.descend()
	for i in 20000:
		await process_frame
		if lift.doors_open():
			break
	_check(lift.doors_open(), "the doors do open")
	var before := origin.position
	for i in 120:
		main._step(Vector3.RIGHT * 0.1)
		await process_frame
	_check(origin.position.x - before.x > Elevator.HALF, "and then you can walk out of them")
