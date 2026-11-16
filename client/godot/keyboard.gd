class_name WorldKeyboard
extends FlatPanel

## Where you type an address, in two forms.
##
## In the headset it is a grid of keys: point at one and click, or move the
## highlight with the thumbstick, because there is no real keyboard you can
## reach with a headset on. Across the top is a bar you can grab to move the
## whole thing, since where it hangs by default is only a guess.
##
## On the desktop it is one line, big, in the middle of the view. You already
## have a keyboard there, so a grid of keys to click would be a worse way to
## type than the one you are sitting at: main.gd sends your key presses to
## `type` and `backspace`, and all this has to do is show you what you have
## typed so far.
##
## The line is deliberately not pointable (`_width` stays zero, which makes
## `FlatPanel.hit` miss it). It is a readout, not a control, and if it took
## clicks it would swallow the ones meant for the field behind it.
##
## A key's `value` is the character it types, or a command marked with a
## leading tab, which no key can ever produce.

signal accepted(text: String)
signal cancelled

## Key pitch and the size a key is drawn at, so keys have a gap between them.
const KEY := 0.135
const KEY_SIZE := 0.12
## The grab bar across the top, and the space above the keys for it and the
## readout.
const BAR := 0.1
const HEADROOM := 0.54
## Held stick or arrow key: this many seconds between steps after the first.
const REPEAT := 0.16
const MAX_LENGTH := 64

const LETTERS := [
	"1234567890",
	"qwertyuiop",
	"asdfghjkl:",
	"zxcvbnm.-_",
]
## The bottom row. `width` is in key pitches; eight of these at 1.2 come to
## the same width as a row of ten letters, so the panel does not grow.
const COMMANDS := [
	{"label": "Shift", "value": "\tshift", "width": 1.2},
	{"label": "Space", "value": "\tspace", "width": 1.2},
	{"label": "Back", "value": "\tback", "width": 1.2},
	{"label": "Clear", "value": "\tclear", "width": 1.2},
	{"label": "Copy", "value": "\tcopy", "width": 1.2},
	{"label": "Paste", "value": "\tpaste", "width": 1.2},
	{"label": "Cancel", "value": "\tcancel", "width": 1.2},
	{"label": "Done", "value": "\tdone", "width": 1.2},
]

var _grid: Array = []
var _row := 0
var _col := 0
var _text := ""
var _title := ""
var _shift := false
var _repeat := 0.0
var _highlight: MeshInstance3D
## The bright bar under the highlighted key.
var _marker: MeshInstance3D
## The field's name, and what you have typed into it: two pieces of text with
## two jobs, so they get two places rather than one run-on line.
var _title_label: Label3D
var _readout: Label3D
var _labels: Array = []
## The y of the top row of keys, which the key lookup measures from.
var _top := 0.0
var _showing := false
## Whether the key grid is up. False on the desktop, where it is just the line.
var _keys_shown := false
## Typing something nobody else should read.
var _secret := false


func is_open() -> bool:
	return _showing


## True when the key grid is up, which is only ever in the headset.
func has_keys() -> bool:
	return _keys_shown


## `title` says what is being typed; `text` is what is in the field already.
## `with_keys` builds the grid, which only the headset needs.
## `secret` masks what is typed in the readout -- the keys, the panel and the
## person standing next to you all see dots. `text()` still hands back what
## was really typed.
func open(title: String, text: String, with_keys := true, secret := false) -> void:
	_title = title
	_text = text.substr(0, MAX_LENGTH)
	_shift = false
	_row = 0
	_col = 0
	_repeat = 0.0
	_keys_shown = with_keys
	_secret = secret
	_showing = true
	_build()
	visible = true


func close() -> void:
	visible = false
	_showing = false
	_keys_shown = false
	_grid = []
	_labels.clear()
	_highlight = null
	_marker = null
	_readout = null
	_title_label = null
	_width = 0.0
	_height = 0.0
	for child in get_children():
		child.free()


func text() -> String:
	return _text


## Which key a local point is on, or (-1, -1) for none.
func key_at(local: Vector3) -> Vector2i:
	if _grid.is_empty():
		return Vector2i(-1, -1)
	var row := int(round((_top - local.y) / KEY))
	if row < 0 or row >= _grid.size():
		return Vector2i(-1, -1)
	for col in _grid[row].size():
		var key: Dictionary = _grid[row][col]
		if absf(local.x - float(key["x"])) <= float(key["span"]) * 0.5:
			return Vector2i(col, row)
	return Vector2i(-1, -1)


## Pointing at a key highlights it, so a click lands where you are looking.
func hover_at(local: Vector3) -> void:
	var cell := key_at(local)
	if cell.x < 0 or (cell.x == _col and cell.y == _row):
		return
	_col = cell.x
	_row = cell.y
	_place_highlight()


## Clicking a key. Returns false when the point was not on one, so the caller
## can tell a miss from a press.
func press_at(local: Vector3) -> bool:
	var cell := key_at(local)
	if cell.x < 0:
		return false
	_col = cell.x
	_row = cell.y
	_place_highlight()
	press()
	return true


## `by` is -1/0/1 on each axis. Rows have different numbers of keys, so the
## column is clamped rather than wrapped: the highlight never vanishes.
func move(by: Vector2i) -> void:
	if not is_open() or _grid.is_empty():
		return
	_row = clampi(_row + by.y, 0, _grid.size() - 1)
	_col = clampi(_col + by.x, 0, _grid[_row].size() - 1)
	_place_highlight()


## Held input: the first press steps once, then it repeats. The first wait is
## longer than the rest so a tap is one key and a hold scrolls.
func hold(by: Vector2i, delta: float) -> void:
	if by == Vector2i.ZERO:
		_repeat = 0.0
		return
	if _repeat <= 0.0:
		# No auto-repeat: one press is one step, or an off-centre stick walks the selection.
		_repeat = INF
		move(by)
	_repeat -= delta


func press() -> void:
	if not is_open() or _grid.is_empty():
		return
	var value := str(_grid[_row][_col]["value"])
	if not value.begins_with("\t"):
		type(_shift_applied(value))
		# Shift is for one character, like a phone: nobody wants to turn it off
		# again after every capital.
		if _shift:
			_shift = false
			_relabel()
		return
	match value:
		"\tshift":
			_shift = not _shift
			_relabel()
		"\tspace":
			type(" ")
		"\tback":
			backspace()
		"\tclear":
			_text = ""
			_refresh()
		"\tcopy":
			copy()
		"\tpaste":
			paste()
		"\tcancel":
			cancelled.emit()
		"\tdone":
			accepted.emit(_text)


## Finishing from outside: Enter on a real keyboard, or clicking away from the
## keyboard, both keep what you typed.
func accept() -> void:
	if is_open():
		accepted.emit(_text)


func type(character: String) -> void:
	if not is_open() or _text.length() >= MAX_LENGTH:
		return
	_text += character
	_refresh()


## The whole line, onto the system clipboard. There is one clipboard on the
## machine, so this is the same one everything else on the desktop uses --
## which is the point of it.
func copy() -> void:
	if is_open() and _text != "":
		DisplayServer.clipboard_set(_text)


## The clipboard, onto the end of the line. Cleaned on the way in: whatever
## is on a clipboard was put there by something else entirely.
func paste() -> void:
	if not is_open():
		return
	var room := MAX_LENGTH - _text.length()
	if room <= 0:
		return
	_text += clean_paste(DisplayServer.clipboard_get(), room)
	_refresh()


## What of a pasted string may go in a field: one line, no control
## characters, and no more than still fits. A clipboard can hold a whole
## document, and a newline in a field that is one line long would be a
## character nobody can see and nobody can delete.
static func clean_paste(text: String, room: int) -> String:
	var clean := ""
	for i in text.length():
		if clean.length() >= room:
			break
		if text[i].unicode_at(0) >= 32:
			clean += text[i]
	return clean


func backspace() -> void:
	if not is_open() or _text.is_empty():
		return
	_text = _text.substr(0, _text.length() - 1)
	_refresh()


func _shift_applied(value: String) -> String:
	return value.to_upper() if _shift else value


func _refresh() -> void:
	if _title_label != null:
		_title_label.text = _title
	if _readout != null:
		_readout.text = ("*".repeat(_text.length()) if _secret else _text) + "_"


## Redraws the letters when shift changes, without rebuilding the grid.
func _relabel() -> void:
	for i in _labels.size():
		var key: Dictionary = _labels[i]["key"]
		var value := str(key["value"])
		if value.begins_with("\t"):
			if value == "\tshift":
				_labels[i]["node"].modulate = UiStyle.ACCENT if _shift else UiStyle.TEXT
			continue
		_labels[i]["node"].text = _shift_applied(value)


func _build() -> void:
	for child in get_children():
		child.free()
	_labels.clear()
	_grid = []
	_width = 0.0
	_height = 0.0
	if not _keys_shown:
		_build_line()
		return
	for line: String in LETTERS:
		var row: Array = []
		for i in line.length():
			row.append({"label": line[i], "value": line[i], "width": 1.0})
		_grid.append(row)
	_grid.append(COMMANDS.duplicate(true))

	var widest := 0.0
	for row: Array in _grid:
		var total := 0.0
		for key: Dictionary in row:
			total += float(key["width"])
		widest = maxf(widest, total)
	_width = widest * KEY + 0.14
	_height = float(_grid.size()) * KEY + HEADROOM
	# The strip across the top: grab it to carry the keyboard somewhere else.
	bar_height = BAR
	_top = _height * 0.5 - 0.45

	_frame(_width, _height)

	# The handle: a pill along the top edge, the way a sheet you can drag says
	# so on a phone. The whole strip under it is what you grab.
	add_child(UiStyle.rounded(Vector2(0.2, 0.014), Color(UiStyle.EDGE_BRIGHT, 0.55),
		Vector3(0.0, _height * 0.5 - BAR * 0.45, 0.004), UiStyle.LAYER_EDGE + band, 0.007))
	# The readout sits in a sunken field, so it reads as the thing being typed
	# into rather than as a caption.
	add_child(UiStyle.rounded(Vector2(_width - 0.16, 0.13), UiStyle.BACKDROP,
		Vector3(0.0, _height * 0.5 - 0.29, 0.002), UiStyle.LAYER_FILL + band, UiStyle.RADIUS_SMALL,
		UiStyle.EDGE, UiStyle.HAIRLINE))

	_title_label = _label("", UiStyle.FONT_SMALL, UiStyle.TEXT_DIM)
	_title_label.position = Vector3(0.0, _height * 0.5 - 0.17, 0.004)
	add_child(_title_label)
	_readout = _label("", UiStyle.FONT, UiStyle.ACCENT)
	_readout.position = Vector3(0.0, _height * 0.5 - 0.29, 0.004)
	add_child(_readout)
	_refresh()

	# Over the key caps, not under them: a wash under an opaque cap is no wash.
	_highlight = UiStyle.rounded(Vector2(KEY_SIZE, KEY_SIZE), Color(UiStyle.ACCENT, UiStyle.KEY_ALPHA),
		Vector3.ZERO, UiStyle.LAYER_EDGE + band, UiStyle.RADIUS_SMALL, UiStyle.ACCENT, UiStyle.HAIRLINE * 1.5)
	add_child(_highlight)
	# The wash says "around here"; the bar says which one.
	_marker = UiStyle.rounded(Vector2(KEY_SIZE, UiStyle.MARKER * 0.6), UiStyle.ACCENT,
		Vector3.ZERO, UiStyle.LAYER_EDGE + band, UiStyle.MARKER)
	add_child(_marker)

	for r in _grid.size():
		var row: Array = _grid[r]
		var total := 0.0
		for key: Dictionary in row:
			total += float(key["width"])
		var x := -total * KEY * 0.5
		for c in row.size():
			var key: Dictionary = row[c]
			var span := float(key["width"]) * KEY
			var centre := x + span * 0.5
			x += span

			var command := str(key["value"]).begins_with("\t")
			var done := str(key["value"]) == "\tdone"
			var cap := UiStyle.rounded(Vector2(span - 0.015, KEY_SIZE),
				UiStyle.ACCENT if done else UiStyle.INK_LIFT,
				Vector3(centre, _top - r * KEY, 0.001), UiStyle.LAYER_FILL + band, UiStyle.RADIUS_SMALL)
			add_child(cap)

			var label := _label(str(key["label"]), UiStyle.FONT_SMALL if command else UiStyle.FONT,
				UiStyle.on(UiStyle.ACCENT) if done else UiStyle.TEXT)
			if done:
				label.outline_size = 0
			label.position = Vector3(centre, _top - r * KEY, 0.005)
			add_child(label)
			_labels.append({"node": label, "key": key})
			key["x"] = centre
			key["y"] = _top - r * KEY
			key["span"] = span
	_place_highlight()


## The desktop form: the line you are typing, and nothing else.
func _build_line() -> void:
	var width := 2.1
	var height := 0.44

	_frame(width, height)

	_title_label = _label("", UiStyle.FONT_SMALL, UiStyle.TEXT_DIM)
	_title_label.position = Vector3(0.0, 0.14, 0.004)
	add_child(_title_label)

	_readout = _label("", UiStyle.FONT_BIG, UiStyle.ACCENT)
	_readout.pixel_size = UiStyle.PIXEL_BIG
	_readout.position = Vector3(0.0, 0.015, 0.004)
	add_child(_readout)
	_refresh()

	var hint := _label("type on your keyboard  -  Enter when done",
		UiStyle.FONT_SMALL, UiStyle.TEXT_DIM)
	hint.position = Vector3(0.0, -0.14, 0.004)
	add_child(hint)


func _place_highlight() -> void:
	if _highlight == null or _grid.is_empty():
		return
	var key: Dictionary = _grid[_row][_col]
	UiStyle.resize(_highlight, Vector2(float(key["span"]) - 0.015, KEY_SIZE))
	_highlight.position = Vector3(float(key["x"]), float(key["y"]), 0.002)
	if _marker != null:
		UiStyle.resize(_marker, Vector2(float(key["span"]) - 0.05, UiStyle.MARKER * 0.6))
		_marker.position = Vector3(float(key["x"]),
			float(key["y"]) - KEY_SIZE * 0.5 + UiStyle.MARKER * 0.25, 0.003)
