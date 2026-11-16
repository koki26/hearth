extends SceneTree

## Checks the pointer maths on the panels, without a window:
##   godot --headless --path client/godot --script res://tests/panel_test.gd
##
## "Point at a row and click it" is arithmetic against a plane rather than a
## collision shape (panel.gd), and getting it a row out is the kind of thing
## that looks like the panel ignoring you.

var _failed := 0
var _checks := 0


func _initialize() -> void:
	_run()


## A frame has to pass before added nodes are really in the tree, and
## `global_transform` complains loudly about being asked too early.
func _run() -> void:
	await process_frame
	await _menu_rows()
	await _menu_sliders()
	await _controls()
	await _menu_tabs()
	await _keyboard_keys()
	await _misses()
	_contrast()
	# It waits for a frame, so it is a coroutine: without the await, print and
	# quit below run first and its checks never happen.
	await _text_has_a_place()
	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)


func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)


## A ray straight down -Z at (x, y), which crosses a panel sitting at the
## origin exactly at its local (x, y).
func _at(x: float, y: float) -> Array:
	return [Vector3(x, y, 1.0), Vector3(0.0, 0.0, -1.0)]


func _menu_rows() -> void:
	var menu := WorldMenu.new()
	root.add_child(menu)
	await process_frame
	var edited := []
	var chosen := []
	menu.edit.connect(func(id: String, _label: String, value: String) -> void: edited.append([id, value]))
	menu.chosen.connect(func(id: String) -> void: chosen.append(id))
	menu.mount([
		{"heading": "Heading"},
		{"id": "address", "label": "Server", "text": "1.2.3.4"},
		{"id": "name", "label": "Name", "text": "Ada"},
		{"id": "join", "label": "Join"},
	])

	# Dead centre of the panel is on a row, and every row is reachable at the y
	# its own label sits at.
	var ray := _at(0.0, 0.0)
	var on := menu.hit(ray[0], ray[1])
	_check(not on.is_empty(), "a ray through the middle of the panel hits it")

	var rows := {1: "address", 2: "name", 3: "join"}
	var found := 0
	for index: int in rows:
		var y := menu.row_y(index)
		var row_hit := menu.hit(_at(0.0, y)[0], _at(0.0, y)[1])
		if not row_hit.is_empty() and menu.row_at(row_hit["local"]) == index:
			found += 1
	_check(found == rows.size(), "every pickable row is found at its own height")
	# Pointing at a heading does nothing at all, rather than jumping to some
	# other row: a click where there is nothing to click must not act.
	_check(menu.row_at(Vector3(0.0, menu._top, 0.0)) == -1, "a heading is not pickable")

	# Clicking a typed field asks for the keyboard; clicking a button picks it.
	var field := menu.hit(_at(0.0, menu.row_y(1))[0], Vector3(0, 0, -1))
	menu.activate_at(field["local"])
	_check(edited.size() == 1 and edited[0][0] == "address",
		"clicking a typed field asks for the keyboard with that field's value")
	_check(edited[0][1] == "1.2.3.4", "and hands over what is already in it")

	var button := menu.hit(_at(0.0, menu.row_y(3))[0], Vector3(0, 0, -1))
	menu.activate_at(button["local"])
	_check(chosen == ["join"], "clicking a plain row picks it")
	menu.free()


func _menu_sliders() -> void:
	var menu := WorldMenu.new()
	root.add_child(menu)
	await process_frame
	var changes := []
	menu.changed.connect(func(id: String, value: float) -> void: changes.append([id, value]))
	menu.mount([{"id": "music", "label": "Music", "value": 0.5}])

	var y := menu._top
	# Hard left and hard right of the one row.
	menu.activate_at(Vector3(-WorldMenu.WIDTH * 0.5 + 0.07, y, 0.0))
	_check(changes.size() == 1 and is_zero_approx(changes[0][1]),
		"clicking the left end of a slider sets it to nothing")
	menu.activate_at(Vector3(WorldMenu.WIDTH * 0.5 - 0.07, y, 0.0))
	_check(changes.size() == 2 and is_equal_approx(changes[1][1], 1.0),
		"and the right end sets it full")

	# Pulling it: the row is taken hold of once, then set over and over from
	# wherever the pointer has got to.
	_check(menu.slider_at(Vector3(0.0, y, 0.0)) == 0, "a slider row says it is one")
	_check(menu.slider_at(Vector3(0.0, y - WorldMenu.ROW_HEIGHT, 0.0)) == -1,
		"and a row that is not a slider says it is not")
	var span := menu.track(0)
	var seen: Array = []
	for along: float in [0.1, 0.35, 0.6, 0.9]:
		menu.drag(0, Vector3(float(span[0]) + float(span[1]) * along, y, 0.0))
		seen.append(changes[changes.size() - 1][1])
	var climbing := true
	for i in range(1, seen.size()):
		climbing = climbing and seen[i] > seen[i - 1]
	_check(climbing, "dragging right walks the value up all the way along")
	_check(seen.size() == 4, "and every step of the drag is reported")
	_check(is_equal_approx(seen[1], 0.35), "the knob lands where the pointer is along the track")
	# A real slider: a track, a fill, a knob -- not a row of characters.
	var knob: MeshInstance3D = menu._parts[0]["knob"]
	_check(is_equal_approx(knob.position.x, float(span[0]) + float(span[1]) * 0.9),
		"and the knob is drawn there")
	_check(str(menu._parts[0]["readout"].text) == "90%", "with the value read out at the end")
	menu.free()


## Switches, choices, the x in the title bar, and the quick menu's buttons.
func _controls() -> void:
	var menu := WorldMenu.new()
	menu.closable = true
	root.add_child(menu)
	await process_frame
	var chosen := []
	menu.chosen.connect(func(id: String) -> void: chosen.append(id))
	menu.mount([
		{"id": "mic", "label": "Microphone", "toggle": true, "icon": "res://icons/mic.svg"},
		{"id": "mirror", "label": "Mirror", "choice": "People"},
		{"id": "go", "label": "Light it", "button": true},
	], [], "Hearth")
	menu.activate_at(Vector3(0.0, menu.row_y(0), 0.0))
	menu.activate_at(Vector3(0.0, menu.row_y(1), 0.0))
	menu.activate_at(Vector3(0.0, menu.row_y(2), 0.0))
	_check(chosen == ["mic", "mirror", "go"], "a switch, a choice and a button are each picked, got %s" % str(chosen))
	var x := Vector3(menu.width * 0.5 - WorldMenu.BAR * 0.6, menu._height * 0.5 - WorldMenu.BAR * 0.5, 0.0)
	_check(not menu.is_bar(x), "the x is not part of the handle")
	menu.activate_at(x)
	_check(chosen.back() == "close", "and clicking it closes")
	menu.free()

	var quick := QuickMenu.new()
	root.add_child(quick)
	await process_frame
	var picked := []
	quick.chosen.connect(func(id: String) -> void: picked.append(id))
	quick.open_at(Transform3D.IDENTITY, [
		{"id": "camera", "label": "Camera", "icon": "res://icons/camera.svg"},
		{"id": "chat", "label": "Chat", "icon": "res://icons/chat.svg"},
	])
	var right := quick._centre(1)
	var on := quick.hit(_at(right.x, right.y)[0], _at(right.x, right.y)[1])
	_check(not on.is_empty() and quick.tile_at(on["local"]) == 1, "each quick button is where it is drawn")
	quick.activate_at(on["local"])
	_check(picked == ["chat"], "and clicking one picks it")
	_check(quick.tile_at(Vector3(0.0, -9.0, 0.0)) == -1, "a point off the buttons picks nothing")
	quick.free()

	# Text that does not fit is made to, rather than running off the panel.
	var long := UiStyle.label("x".repeat(80), UiStyle.FONT)
	UiStyle.fit(long, 0.5)
	_check(UiStyle.text_width(long.text, long.font_size, long.pixel_size) <= 0.5 + 0.001,
		"a long label is fitted to its room")
	_check(long.text.ends_with("\u2026"), "and says it was cut short")
	long.free()
	var lines := UiStyle.wrap("the quick brown fox jumps over the lazy dog", 12)
	var fits := true
	for line in lines:
		fits = fits and line.length() <= 12
	_check(fits and lines.size() == 4, "chat wraps at spaces into lines that fit, got %s" % str(lines))


## The tab strip across the top: each cell picks its own page, and nothing in
## the strip also counts as the first row.
func _menu_tabs() -> void:
	var menu := WorldMenu.new()
	root.add_child(menu)
	await process_frame
	var chosen := []
	menu.chosen.connect(func(id: String) -> void: chosen.append(id))
	var tabs := [
		{"id": "tab:people", "label": "People", "icon": "res://icons/people.svg"},
		{"id": "tab:worlds", "label": "Worlds", "icon": "res://icons/globe.svg", "on": true},
		{"id": "tab:settings", "label": "Settings", "icon": "res://icons/gear.svg"},
	]
	menu.mount([{"id": "join", "label": "Join", "icon": "res://icons/home.svg"}], tabs, "Hearth")

	var strip := menu._height * 0.5 - WorldMenu.BAR - UiStyle.TAB * 0.5
	var cell := WorldMenu.WIDTH / 3.0
	for i in 3:
		var x := -WorldMenu.WIDTH * 0.5 + cell * (i + 0.5)
		var on := menu.hit(_at(x, strip)[0], _at(x, strip)[1])
		_check(not on.is_empty() and menu.tab_at(on["local"]) == i, "tab %d is where it is drawn" % i)
		_check(menu.row_at(on["local"]) == -1, "a point on the tabs is not a row")
	menu.activate_at(Vector3(0.0, strip, 0.0))
	_check(chosen == ["tab:worlds"], "clicking a tab picks that page, got %s" % str(chosen))
	_check(menu.tab_at(Vector3(0.0, menu._top, 0.0)) == -1, "a row is not a tab")
	_check(menu.is_bar(Vector3(0.0, menu._height * 0.5 - 0.01, 0.0)), "the top strip is the handle")
	_check(not menu.is_bar(Vector3(0.0, strip, 0.0)), "the tabs are not the handle")
	# An icon per row and per tab, all of them loaded.
	var icons := menu.find_children("*", "Sprite3D", true, false)
	_check(icons.size() == 4, "every icon is drawn, got %d" % icons.size())
	var loaded := true
	for sprite: Sprite3D in icons:
		loaded = loaded and sprite.texture != null
	_check(loaded, "every icon has its texture")
	menu.free()


func _keyboard_keys() -> void:
	var keys := WorldKeyboard.new()
	root.add_child(keys)
	await process_frame
	keys.open("Server", "")

	var ray := _at(0.0, 0.0)
	_check(not keys.hit(ray[0], ray[1]).is_empty(), "the keyboard is hittable")

	# The top row is the digits, so the key under the far left of it is "1".
	var top_left := keys.key_at(Vector3(-WorldKeyboard.KEY * 4.5, keys._top, 0.0))
	_check(top_left == Vector2i(0, 0), "the first key is where it is drawn")
	_check(keys.press_at(Vector3(-WorldKeyboard.KEY * 4.5, keys._top, 0.0)), "and can be pressed")
	_check(keys.text() == "1", "pressing it types it")

	# Second row, second key: "w".
	var w := Vector3(-WorldKeyboard.KEY * 3.5, keys._top - WorldKeyboard.KEY, 0.0)
	_check(keys.key_at(w) == Vector2i(1, 1), "a key one row down and one across is found")
	keys.press_at(w)
	_check(keys.text() == "1w", "and types the right letter")

	# The grab bar is the strip across the very top, above the readout.
	_check(keys.is_bar(Vector3(0.0, keys._height * 0.5 - 0.01, 0.0)), "the top strip is the grab bar")
	_check(not keys.is_bar(Vector3(0.0, keys._top, 0.0)), "the keys are not the grab bar")
	_check(keys.key_at(Vector3(0.0, keys._height * 0.5 - 0.01, 0.0)).x < 0,
		"and the grab bar is not a key")

	# The command row, found by what each key does rather than by counting
	# pitches: the row's keys have been renumbered once already.
	var command_row := keys._top - 4 * WorldKeyboard.KEY
	var where := func(value: String) -> Vector3:
		for key: Dictionary in keys._grid[4]:
			if str(key["value"]) == value:
				return Vector3(float(key["x"]), command_row, 0.0)
		return Vector3(99.0, 99.0, 0.0)

	# Paste with nothing to paste changes nothing and breaks nothing: in a
	# headless run there is no clipboard at all.
	_check(keys.press_at(where.call("\tcopy")), "Copy is a key on the bottom row")
	_check(keys.press_at(where.call("\tpaste")), "and so is Paste")
	_check(keys.text() == "1w", "pasting an empty clipboard types nothing")

	# What a paste is allowed to bring in. The clipboard itself is the
	# machine's, so only the cleaning is ours to check.
	_check(WorldKeyboard.clean_paste("hearth.example.com", 64) == "hearth.example.com",
		"an address pastes as itself")
	_check(WorldKeyboard.clean_paste("two\nlines\there", 64) == "twolineshere",
		"a newline or a tab never reaches a one-line field")
	_check(WorldKeyboard.clean_paste("abcdef", 3) == "abc", "and a paste is cut to what fits")
	_check(WorldKeyboard.clean_paste("whatever", 0) == "", "a full field takes nothing")

	var accepted := [""]
	keys.accepted.connect(func(text: String) -> void: accepted[0] = text)
	_check(keys.press_at(where.call("\tdone")), "Done is a key on the bottom row")
	_check(accepted[0] == "1w", "and hands back what was typed")
	keys.free()


func _misses() -> void:
	var menu := WorldMenu.new()
	root.add_child(menu)
	await process_frame
	menu.mount([{"id": "join", "label": "Join"}])

	_check(menu.hit(Vector3(9.0, 0.0, 1.0), Vector3(0, 0, -1)).is_empty(),
		"a ray wide of the panel misses it")
	_check(menu.hit(Vector3(0.0, 0.0, 1.0), Vector3(0, 0, 1)).is_empty(),
		"a ray pointing away from the panel misses it")
	_check(menu.hit(Vector3(0.0, 0.0, 1.0), Vector3(1, 0, 0)).is_empty(),
		"a ray along the panel misses it")
	_check(menu.row_at(Vector3(0.0, 99.0, 0.0)) == -1, "a point off the top is no row")

	menu.close()
	_check(menu.hit(Vector3(0.0, 0.0, 1.0), Vector3(0, 0, -1)).is_empty(),
		"a closed panel cannot be pointed at")
	menu.free()


## Every text colour must be readable on every surface it can land on. WCAG
## calls 4.5 the floor for text and 7.0 comfortable for body text; a reskin
## that drops below those is a reskin that shipped unreadable text.
func _contrast() -> void:
	var ink := UiStyle.over(UiStyle.INK, UiStyle.BACKDROP)
	var lift := UiStyle.over(UiStyle.INK_LIFT, UiStyle.BACKDROP)

	for surface: Color in [ink, lift, UiStyle.BACKDROP]:
		_check(UiStyle.contrast(UiStyle.TEXT, surface) >= 7.0,
			"body text is comfortable on every surface (%.1f)" % UiStyle.contrast(UiStyle.TEXT, surface))
	for colour: Color in [UiStyle.TEXT_DIM, UiStyle.ACCENT, UiStyle.WARN, UiStyle.GOOD]:
		_check(UiStyle.contrast(colour, ink) >= 4.5,
			"a meaning colour is readable on a panel (%.1f)" % UiStyle.contrast(colour, ink))
	# A selected row: the wash goes over the panel, and the row's own label and
	# value still have to be read on top of it. This is the pair that shipped
	# unreadable, so it is the pair most worth pinning down.
	var selected := UiStyle.over(UiStyle.selection(), ink)
	_check(UiStyle.contrast(UiStyle.TEXT, selected) >= 7.0,
		"a selected row's label stays comfortable (%.1f)" % UiStyle.contrast(UiStyle.TEXT, selected))
	_check(UiStyle.contrast(UiStyle.ACCENT, selected) >= 4.5,
		"a selected row's value stays readable (%.1f)" % UiStyle.contrast(UiStyle.ACCENT, selected))
	var key_selected := UiStyle.over(Color(UiStyle.ACCENT, UiStyle.KEY_ALPHA), lift)
	_check(UiStyle.contrast(UiStyle.TEXT, key_selected) >= 7.0,
		"a highlighted key stays readable (%.1f)" % UiStyle.contrast(UiStyle.TEXT, key_selected))

	# Blending happens in linear space on the GPU. Composite in sRGB instead
	# and a wash reads far darker on paper than it does on screen.
	var half := UiStyle.over(Color(Color.WHITE, 0.5), Color.BLACK)
	_check(half.r > 0.7, "a half-alpha white over black is bright, as the GPU draws it (%.2f)" % half.r)

	# A button is filled with the accent, and its label sits straight on it.
	_check(UiStyle.contrast(UiStyle.on(UiStyle.ACCENT), UiStyle.ACCENT) >= 7.0,
		"a button's label is comfortable on the accent (%.1f)" % UiStyle.contrast(UiStyle.on(UiStyle.ACCENT), UiStyle.ACCENT))

	# The one light background in the set: a pressed button.
	var pressed := UiStyle.ACCENT.darkened(0.7)
	_check(UiStyle.contrast(UiStyle.TEXT, pressed) >= 4.5,
		"button text is readable while pressed (%.1f)" % UiStyle.contrast(UiStyle.TEXT, pressed))
	# The edge has to be visible against its own panel, or there is no edge.
	_check(UiStyle.contrast(UiStyle.over(UiStyle.EDGE, ink), ink) >= 1.3, "the hairline is visible")
	_check(UiStyle.contrast(UiStyle.EDGE_BRIGHT, ink) >= 4.5, "the corner marks are visible")

	# `on` has to pick the readable one, whatever a world throws at it.
	_check(UiStyle.on(Color.WHITE) == UiStyle.BACKDROP, "dark text goes on a white surface")
	_check(UiStyle.on(Color.BLACK) == UiStyle.TEXT, "light text goes on a black one")
	_check(is_equal_approx(UiStyle.contrast(Color.BLACK, Color.WHITE), 21.0),
		"black on white is the full range")


## Text sits in its own place: inset from the edge, lined up in a column, and
## centred in the row that highlights it.
func _text_has_a_place() -> void:
	var menu := WorldMenu.new()
	root.add_child(menu)
	await process_frame
	menu.mount([
		{"id": "address", "label": "Server", "text": "1.2.3.4"},
		{"id": "name", "label": "Name", "text": "Ada"},
		{"id": "music", "label": "Music", "value": 0.5},
	])

	var labels: Array[Label3D] = []
	var values: Array[Label3D] = []
	var left := -WorldMenu.WIDTH * 0.5 + UiStyle.PAD
	# Recursive: everything a panel draws hangs off one node it scales when it
	# opens, so none of it is a direct child.
	for child in menu.find_children("*", "Label3D", true, false):
		if absf(child.position.x - left) < 0.001:
			labels.append(child)
		elif child.horizontal_alignment == HORIZONTAL_ALIGNMENT_LEFT:
			values.append(child)

	_check(labels.size() == 3, "every row has a label in the label column")
	_check(values.size() == 2, "and every field shows what is in it")
	var column := values[0].position.x
	var aligned := true
	for value: Label3D in values:
		aligned = aligned and is_equal_approx(value.position.x, column)
	_check(aligned, "field values line up with each other instead of trailing their labels")
	_check(column > left, "the value column is right of the labels")
	_check(absf(float(menu.track(2)[0]) - menu.value_column()) < 0.001,
		"and a slider's track starts in that same column")

	for text: Label3D in labels + values:
		_check_once(absf(text.position.x) <= WorldMenu.WIDTH * 0.5 - UiStyle.PAD * 0.5,
			"no text starts outside the panel")
		_check_once(text.outline_size > 0, "every glyph carries its own outline")
	menu.free()


var _seen := {}


## One check for a whole loop, not one per item.
func _check_once(ok: bool, what: String) -> void:
	if _seen.has(what):
		if not ok and _seen[what]:
			_seen[what] = false
			_failed += 1
			printerr("FAILED (later item): %s" % what)
		return
	_seen[what] = ok
	_check(ok, what)
