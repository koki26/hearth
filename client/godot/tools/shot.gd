extends SceneTree

## Renders the real scene to PNGs so the interface can be looked at instead of
## reasoned about:
##
##   godot --path client/godot --script res://tools/shot.gd --resolution 1280x720
##
## Not --headless: there has to be a renderer for there to be a picture. A
## window appears for a few seconds and closes itself.

const OUT := "D:/Hearth/shots"
const SETTLE := 24


func _initialize() -> void:
	_run()


func _run() -> void:
	DirAccess.make_dir_recursive_absolute(OUT)
	var main: Node3D = load("res://main.tscn").instantiate()
	root.add_child(main)
	for i in SETTLE:
		await process_frame

	await _shot("panel")

	# A refused join: the one row on this panel that is not white, and the whole
	# point of it is being readable at a glance from inside the lift.
	main._message = "Server refused the join: wrong version"
	main._refresh_panel()
	for i in 6:
		await process_frame
	await _shot("refused")
	main._message = ""
	main._refresh_panel()

	# Open the keyboard on the first field, which is what a click does.
	var panel: WorldMenu = main.get_node("Panel")
	panel.activate()
	for i in 8:
		await process_frame
	await _shot("typing")

	# And the settings menu, which is the other panel. Not by faking a join:
	# that starts gravity, the player drops, and the menu is left behind where
	# the camera used to be -- which looks like a bug in the menu and is not.
	var keyboard: WorldKeyboard = main.get_node("Keyboard")

	# The key grid, which only the headset ever shows. Asked for directly
	# rather than by faking a headset, because there is not one here.
	keyboard.open("Server", "127.0.0.1:7777", true)
	main._place_lift_ui()
	for i in 8:
		await process_frame
	await _shot("keys")

	keyboard.close()
	# Closed, not hidden, so the menu is the only panel in the picture.
	panel.close()
	# Somebody to have on the friends panel and something said in chat.
	main._on_peer_joined(7, "Bob")
	main._on_peer_joined(8, "Cat")
	main._add_chat(7, "Bob", "hey, anyone up for the forest?")
	main._add_chat(8, "Cat", "give me a minute, just grabbing a drink and I will be right there")
	main._add_chat(-1, "", "Dan came in")
	main._add_chat(7, "Bob", "nice")
	main._toggle_menu()
	# Straight ahead and far enough back that both side panels are in the
	# picture; the menu opens wherever the mouse happens to be otherwise.
	var eye: Node3D = main.get_node("XROrigin3D/Camera")
	var ahead := -eye.global_transform.basis.z
	ahead.y = 0.0
	main._face(main._menu, eye.global_position + ahead.normalized() * 2.1 + Vector3.DOWN * 0.15)
	for i in 8:
		await process_frame
	await _shot("menu")
	main._on_menu_chosen("tab:settings")
	for i in 6:
		await process_frame
	await _shot("settings")
	main._on_menu_chosen("tab:camera")
	for i in 6:
		await process_frame
	await _shot("camera_page")
	main._menu.close()
	main._open_quick(null)
	for i in 10:
		await process_frame
	await _shot("quick")
	main._quick.close()
	main._toggle_camera(null)
	for i in 20:
		await process_frame
	await _shot("camera")
	main._toggle_camera(null)
	main._on_peer_left(7)
	main._on_peer_left(8)

	# The two advanced pages, which are the only way to turn accounts off or
	# move them, and so the two most worth looking at before shipping.
	main._menu.close() # Left open by the shot before this one.
	panel.mount(main._panel_rows())
	main._panel_page = "warning"
	main._refresh_panel()
	for i in 6:
		await process_frame
	await _shot("warning")
	main._panel_page = "advanced"
	main._refresh_panel()
	for i in 6:
		await process_frame
	await _shot("advanced")
	main._panel_page = "main"
	main._refresh_panel()

	# The doors. Send the lift down and wait for it to land and open.
	main._menu.close()
	main._elevator.descend()
	for i in 600:
		await process_frame
		if main._elevator.doors_open():
			break
	for i in 4:
		await process_frame
	await _shot("arrived")

	# The mirror, from in front of it, showing everything.
	main._menu.close()
	main._elevator.clear()
	main._mirror.quality = Mirror.WORLD
	var origin: Node3D = main.get_node("XROrigin3D")
	var camera: Node3D = main.get_node("XROrigin3D/Camera")
	var glass: Vector3 = main._mirror.global_position
	# +Z is the glass; -Z is its back. Standing behind it shows you the plaza.
	var facing: Vector3 = main._mirror.global_transform.basis.z
	origin.position = glass + facing * 2.2 - Vector3.UP * 1.25
	camera.rotation.y = atan2(-facing.x, -facing.z) + PI
	for i in 20:
		await process_frame
	await _shot("mirror")

	print("shots in %s" % OUT)
	quit(0)


func _shot(name: String) -> void:
	await process_frame
	var image := root.get_texture().get_image()
	image.save_png("%s/%s.png" % [OUT, name])
	print("  %s.png %dx%d" % [name, image.get_width(), image.get_height()])
