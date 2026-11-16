class_name WorldMenu
extends FlatPanel

## A panel of rows in the world: the menu that appears in front of your face,
## the friends and chat panels either side of it, and the lift panel.
##
## Point at a row and click to pick it, or move through the rows with the
## arrow keys. The pointer is the mouse on the desktop and a controller in the
## headset (see main.gd). Sliders are dragged, switches are flipped, fields
## open the keyboard.
##
## Rows come from main.gd as dictionaries. What a row has decides what it is:
##   {"id": "join:3", "label": "forest"}                 a thing to do
##   {"id": "music", "label": "Music", "value": 0.6}     a slider, 0..1
##   {"id": "mic", "label": "Microphone", "toggle": true}  a switch
##   {"id": "mirror", "label": "Mirror", "choice": "People"}  cycles when picked
##   {"id": "name", "label": "Name", "text": "Ada"}      a field: opens the keyboard
##   {"id": "chat", "label": "", "text": "", "placeholder": "Say something"}
##                                                       a field across the row
##   {"id": "new", "label": "Light it", "button": true}  a filled button
##   {"heading": "Friends"}                              a section heading
##   {"label": "nobody is here"}                         a line of text, unpickable
##   {"label": ""}                                       a gap
##   {"label": "Refused", "warn": true}                  a line of text, but red
## And these go on any row:
##   "icon": "res://icons/x.svg"   an icon on a chip before the label
##   "avatar": "Ada"               somebody's initial on their colour instead
##   "status": "online"            a dot on the avatar: online, offline
##   "sub": "in the loft"          a second, quieter line under the label
##   "note": "2 embers"            quiet text at the right-hand end
##   "action": "Join"              a small pill at the right-hand end
##   "more": true                  a chevron: picking it goes somewhere
##   "danger": true                the label in the warning colour
##   "small": true                 a short line, for chat; "accent" colours it,
##                                 "bright" makes unpickable text full white
##   "picture": Texture2D          a picture across the row
##
## Tabs run across the top under the title bar:
##   {"id": "tab:worlds", "label": "Worlds", "icon": "res://icons/globe.svg",
##    "on": true}

signal chosen(id: String)
signal changed(id: String, value: float)
## A typed field was picked: whoever owns the menu opens the keyboard.
signal edit(id: String, label: String, value: String)

const WIDTH := 1.5
const ROW_HEIGHT := UiStyle.ROW
const DISTANCE := 1.25
## The title strip across the top. Hold it to carry the menu somewhere else.
const BAR := 0.1
## Room between the header and the first row, and under the last one.
const GAP := 0.035
const FOOT := 0.05
## A picture row, edge to edge of the content.
const PICTURE := 0.5
## How far a slider moves per press.
const STEP := 0.1
## Held stick or arrow key: rows this many per second after the first.
const REPEAT := 0.18
## How long the panel takes to grow out of its own middle when it opens.
const GROW := 0.14

## Can this panel be picked up by its bar? The lift panel cannot: it is bolted
## to the car, which puts it back every frame. Nor can the side panels, which
## ride along with the menu they hang beside.
var movable := true
## Does the title bar carry an x that picks "close"?
var closable := false
## How wide this panel is. The side panels are narrower than the menu.
var width := WIDTH
## The icon before the title.
var title_icon := ""
## A quiet line at the right of the title bar: a count, a state.
var title_note := ""

var _rows: Array = []
var _tabs: Array = []
var _title := ""
var _pick := 0
## Is the pointer on this panel? Only the panel being pointed at shows its
## highlight, so three panels side by side do not each claim a row.
var _focused := true
## Everything drawn hangs off this, so opening can scale the lot out of the
## middle without touching this node's transform -- which is what `hit`
## measures against, and scaling that put the pointer somewhere else.
var _body: Node3D
var _highlight: MeshInstance3D
## The bright bar down the left of the selected row.
var _marker: MeshInstance3D
## One per row: the nodes a value change has to move.
var _parts: Array = []
## Each row's centre and height. Rows are not all one height: a heading is
## shorter than a row, a friend with a second line is taller.
var _row_y := PackedFloat32Array()
var _row_h := PackedFloat32Array()
var _repeat := 0.0
## The y of the first row's centre.
var _top := 0.0
## Where the header ends: above this is the title bar and the tabs.
var _header_bottom := 0.0


func is_open() -> bool:
	return visible and not _rows.is_empty()


## Shows the menu `DISTANCE` in front of `head`, at eye level and facing it.
##
## `head` is in this node's PARENT space, not the world. The menu hangs off the
## play space, so it rides a descending lift and walks with you.
##
## Reopening with the same number of rows keeps the cursor where it was, so
## toggling a setting from the menu does not throw you back to the top.
func open(head: Transform3D, rows: Array, tabs: Array = [], title := "") -> void:
	var was_open := visible
	_set_rows(rows, tabs, title)
	var forward := -head.basis.z
	forward.y = 0.0
	if forward.length() < 0.01:
		forward = Vector3.FORWARD
	forward = forward.normalized()
	# Upright and facing the player, never tipped to match a tilted head. A
	# quad faces +Z and culls its back, so -Z points away from them. Built
	# rather than `look_at`, which works in world space and would be wrong
	# under a parent that moves.
	transform = Transform3D(Basis.looking_at(forward, Vector3.UP), head.origin + forward * DISTANCE)
	visible = true
	if not was_open:
		_grow()


## Shows the menu wherever it already is, instead of in front of the player.
## The lift panel is bolted to the car, and the car is what moves it.
func mount(rows: Array, tabs: Array = [], title := "") -> void:
	var was_open := visible
	_set_rows(rows, tabs, title)
	visible = true
	# Only a menu that was away animates: rebuilding it to show a setting you
	# just changed should not make the whole panel jump about.
	if not was_open:
		_grow()


## Reopening with the same number of rows keeps the cursor where it was, so
## changing a setting does not throw you back to the top.
func _set_rows(rows: Array, tabs: Array = [], title := "") -> void:
	var keep := _pick if rows.size() == _rows.size() else -1
	_rows = rows
	_tabs = tabs
	_title = title
	_pick = keep if keep >= 0 else _next_pickable(0, 1)
	_build()


func close() -> void:
	visible = false
	_rows = []
	_tabs = []
	for child in get_children():
		child.free()
	_body = null
	_parts.clear()
	_highlight = null
	_marker = null


## Only the panel the pointer is on shows which row it is on.
func set_focus(on: bool) -> void:
	if _focused == on:
		return
	_focused = on
	_place_highlight()


## The centre of row `index`, in this panel's space.
func row_y(index: int) -> float:
	return _row_y[index] if index >= 0 and index < _row_y.size() else 0.0


## Where values start: fields, slider tracks and choices all line up here.
func value_column() -> float:
	return -width * 0.5 + width * UiStyle.VALUE_AT


## A slider's track, as [left x, length].
func track(index: int) -> Array:
	var left := value_column()
	return [left, width * 0.5 - UiStyle.PAD - UiStyle.READOUT - left]


## `by` is -1 for up and 1 for down; headings are skipped.
func move(by: int) -> void:
	if not is_open():
		return
	var next := _next_pickable(_pick + by, by)
	if next == _pick:
		return
	_pick = next
	_place_highlight()


## Left or right on the picked row: only sliders care.
func nudge(by: int) -> void:
	if not is_open():
		return
	var row: Dictionary = _rows[_pick]
	if not row.has("value"):
		return
	row["value"] = clampf(float(row["value"]) + STEP * by, 0.0, 1.0)
	_refresh_row(_pick)
	changed.emit(str(row.get("id", "")), float(row["value"]))


func activate() -> void:
	if not is_open():
		return
	var row: Dictionary = _rows[_pick]
	if not row.has("id"):
		return
	if row.has("value"):
		nudge(1 if float(row["value"]) < 1.0 else -10)
		return
	if row.has("text"):
		var named := str(row.get("label", "")).strip_edges()
		if named == "":
			named = str(row.get("placeholder", ""))
		edit.emit(str(row["id"]), named, str(row["text"]))
		return
	chosen.emit(str(row["id"]))


## Held input: the first press moves one row (or one notch of a slider), then
## it repeats. Call every frame with the arrow keys folded into one step. Both
## axes share the repeat, so a diagonal cannot run twice as fast as a straight
## one.
func hold(by: Vector2i, delta: float) -> void:
	if by == Vector2i.ZERO:
		_repeat = 0.0
		return
	if _repeat <= 0.0:
		# No auto-repeat: one press is one step, or an off-centre stick walks the selection.
		_repeat = INF
		if by.y != 0:
			move(by.y)
		if by.x != 0:
			nudge(by.x)
	_repeat -= delta


## Which row a local point is on, or -1 when it is not a row you can pick.
func row_at(local: Vector3) -> int:
	if _rows.is_empty() or local.y > _header_bottom:
		return -1
	for index in _rows.size():
		if absf(local.y - _row_y[index]) <= _row_h[index] * 0.5:
			return index if _rows[index].has("id") else -1
	return -1


## Which tab a local point is on, or -1. The tabs share the strip evenly.
func tab_at(local: Vector3) -> int:
	if _tabs.is_empty() or _height <= 0.0:
		return -1
	var top := _height * 0.5 - BAR
	if local.y > top or local.y < top - UiStyle.TAB:
		return -1
	var cell := _width / float(_tabs.size())
	return clampi(int(floor((local.x + _width * 0.5) / cell)), 0, _tabs.size() - 1)


## Is this point the x at the end of the title bar?
func close_at(local: Vector3) -> bool:
	if not closable or _height <= 0.0:
		return false
	var centre := Vector2(width * 0.5 - BAR * 0.6, _height * 0.5 - BAR * 0.5)
	return Vector2(local.x, local.y).distance_to(centre) <= BAR * 0.45


## The bar is for carrying, except where the x is.
func is_bar(local: Vector3) -> bool:
	return super.is_bar(local) and not close_at(local)


## Pointing at a row highlights it, so a click lands where you are looking.
func hover_at(local: Vector3) -> void:
	set_focus(true)
	var index := row_at(local)
	if index < 0 or index == _pick:
		return
	_pick = index
	_place_highlight()


## The row under this point if it is a slider, otherwise -1. Whoever is doing
## the pointing holds on to that index and keeps calling `drag`, which is what
## makes a slider a thing you pull rather than a thing you click at.
func slider_at(local: Vector3) -> int:
	var index := row_at(local)
	return index if index >= 0 and _rows[index].has("value") else -1


## Sets a slider from where along its track you are pointing.
func drag(index: int, local: Vector3) -> void:
	if index < 0 or index >= _rows.size() or not _rows[index].has("value"):
		return
	_pick = index
	_place_highlight()
	var span := track(index)
	var fraction := clampf((local.x - float(span[0])) / float(span[1]), 0.0, 1.0)
	_rows[index]["value"] = fraction
	_refresh_row(index)
	changed.emit(str(_rows[index].get("id", "")), fraction)


## Clicking a row, a tab or the x. A slider is set from where along it you
## clicked, because that is what a slider you can point at should do.
func activate_at(local: Vector3) -> void:
	if close_at(local):
		chosen.emit("close")
		return
	var tab := tab_at(local)
	if tab >= 0:
		chosen.emit(str(_tabs[tab].get("id", "")))
		return
	var index := row_at(local)
	if index < 0:
		return
	_pick = index
	_place_highlight()
	if _rows[index].has("value"):
		drag(index, local)
		return
	activate()


## How tall a row is, from what it has.
static func height_of(row: Dictionary) -> float:
	if row.has("picture"):
		return PICTURE
	if row.get("small", false):
		return UiStyle.LINE
	if row.has("heading"):
		return UiStyle.HEADING
	if row.get("button", false):
		return UiStyle.ROW_BUTTON
	if row.has("sub"):
		return UiStyle.ROW_TALL
	if not row.has("id") and str(row.get("label", "")) == "" and not row.has("avatar"):
		return UiStyle.SPACER
	return ROW_HEIGHT


func _next_pickable(from: int, by: int) -> int:
	var step := 1 if by >= 0 else -1
	var index := clampi(from, 0, _rows.size() - 1)
	while index >= 0 and index < _rows.size():
		if _rows[index].has("id"):
			return index
		index += step
	return clampi(_pick, 0, maxi(_rows.size() - 1, 0))


## What goes on the left of a row.
func _text(row: Dictionary) -> String:
	if row.has("heading"):
		return str(row["heading"]).to_upper()
	return str(row.get("label", "")).strip_edges()


## A slider's readout.
func _percent(row: Dictionary) -> String:
	return "%d%%" % int(round(float(row["value"]) * 100.0))


## What a field shows: what is in it, dots for a secret, or what it is for.
func _field(row: Dictionary) -> String:
	var typed := str(row["text"])
	if typed == "":
		return str(row.get("placeholder", "..."))
	# A password is typed on a panel hanging in a room somebody else may be
	# standing in, and on a screen somebody may be streaming.
	if row.get("secret", false):
		return "•".repeat(typed.length())
	return typed


## Moves a slider's fill, knob and readout after its value changed, which is
## the only thing that changes without the whole panel being rebuilt.
func _refresh_row(index: int) -> void:
	if index < 0 or index >= _parts.size():
		return
	var parts: Dictionary = _parts[index]
	if not parts.has("knob"):
		return
	var row: Dictionary = _rows[index]
	var span := track(index)
	var left := float(span[0])
	var length := float(span[1])
	var at := clampf(float(row["value"]), 0.0, 1.0) * length
	var y := _row_y[index]
	var fill: MeshInstance3D = parts["fill"]
	UiStyle.resize(fill, Vector2(maxf(at, UiStyle.TRACK), UiStyle.TRACK))
	fill.position = Vector3(left + maxf(at, UiStyle.TRACK) * 0.5, y, 0.003)
	(parts["knob"] as MeshInstance3D).position = Vector3(left + at, y, 0.004)
	(parts["ring"] as MeshInstance3D).position = Vector3(left + at, y, 0.0035)
	(parts["readout"] as Label3D).text = _percent(row)


## Grows the panel out of its own middle. Everything drawn hangs off `_body`,
## so this never moves the node the pointer is measured against.
func _grow() -> void:
	if _body == null or not is_inside_tree():
		return
	_body.scale = Vector3(0.92, 0.92, 1.0)
	# Bound to `_body`: closing the menu mid-animation frees it, and an
	# unbound tween would go on setting a property on nothing.
	var tween := create_tween().bind_node(_body)
	tween.tween_property(_body, "scale", Vector3.ONE, GROW) \
		.set_ease(Tween.EASE_OUT).set_trans(Tween.TRANS_BACK)


func _build() -> void:
	for child in get_children():
		child.free()
	_parts.clear()
	_body = Node3D.new()
	add_child(_body)

	var header := BAR + (UiStyle.TAB + 0.02 if not _tabs.is_empty() else 0.0)
	var content := 0.0
	_row_h.resize(_rows.size())
	_row_y.resize(_rows.size())
	for i in _rows.size():
		_row_h[i] = height_of(_rows[i])
		content += _row_h[i]
	var height := header + GAP + content + FOOT
	_width = width
	_height = height
	_header_bottom = height * 0.5 - header
	var cursor := _header_bottom - GAP
	for i in _rows.size():
		_row_y[i] = cursor - _row_h[i] * 0.5
		cursor -= _row_h[i]
	_top = _row_y[0] if not _rows.is_empty() else 0.0
	bar_height = BAR if movable else 0.0

	for part in UiStyle.frame(width, height, band):
		_body.add_child(part)
	_build_bar(height)
	_build_tabs(height)

	_highlight = UiStyle.rounded(Vector2(width - 0.05, ROW_HEIGHT), UiStyle.selection(),
		Vector3.ZERO, UiStyle.LAYER_WASH + band, UiStyle.RADIUS_SMALL)
	_body.add_child(_highlight)
	# The wash alone is too faint to say "this one", and strong enough to say it
	# would be strong enough to swallow the row's own text.
	_marker = UiStyle.rounded(Vector2(UiStyle.MARKER, ROW_HEIGHT * 0.5), UiStyle.ACCENT,
		Vector3.ZERO, UiStyle.LAYER_EDGE + band, UiStyle.MARKER)
	_body.add_child(_marker)

	for i in _rows.size():
		_parts.append(_build_row(i))
	_place_highlight()


## One row: what leads it, its label, and whatever control it ends in.
func _build_row(i: int) -> Dictionary:
	var row: Dictionary = _rows[i]
	var parts := {}
	var y := _row_y[i]
	var left := -width * 0.5 + UiStyle.PAD
	var right := width * 0.5 - UiStyle.PAD

	if row.has("picture"):
		_build_picture(row, y)
		return parts
	if row.get("button", false):
		_build_button(row, y)
		return parts

	var pickable := row.has("id")
	var colour := UiStyle.TEXT if pickable else UiStyle.TEXT_DIM
	if row.get("warn", false) or row.get("danger", false):
		colour = UiStyle.WARN
	if row.get("accent", false):
		colour = UiStyle.ACCENT
	elif row.get("bright", false):
		colour = UiStyle.TEXT
	var size := UiStyle.FONT
	if row.has("heading") or row.get("small", false):
		size = UiStyle.FONT_SMALL

	var text_left := left
	if row.has("avatar"):
		_build_avatar(str(row["avatar"]), str(row.get("status", "")), Vector3(left + UiStyle.CHIP * 0.5, y, 0.0))
		text_left = left + UiStyle.CHIP + 0.035
	elif row.has("icon"):
		var chip_at := Vector3(left + UiStyle.CHIP * 0.5, y, 0.002)
		_body.add_child(UiStyle.rounded(Vector2(UiStyle.CHIP, UiStyle.CHIP), UiStyle.INK_LIFT, chip_at,
			UiStyle.LAYER_FILL + band, UiStyle.RADIUS_SMALL))
		var mark := UiStyle.icon(str(row["icon"]), UiStyle.ICON,
			UiStyle.ACCENT if row.get("toggle", false) else colour, UiStyle.LAYER_TEXT + band)
		mark.position = chip_at + Vector3(0.0, 0.0, 0.002)
		_body.add_child(mark)
		text_left = left + UiStyle.CHIP + 0.035

	# Where the label has to stop, which depends on what the row ends in.
	var room := right - text_left
	if row.has("value"):
		room = value_column() - text_left - 0.03
		_build_slider(i, row, y, parts)
	elif row.has("toggle"):
		room -= UiStyle.SWITCH.x + 0.04
		_build_switch(bool(row["toggle"]), Vector3(right - UiStyle.SWITCH.x * 0.5, y, 0.0))
	elif row.has("text"):
		var field_left := value_column() - 0.025
		if _text(row) == "":
			field_left = text_left - 0.025
		room = field_left - text_left - 0.01
		_build_field(row, field_left, right, y)
	else:
		var end := right
		if row.get("more", false) or row.has("choice"):
			var chevron := UiStyle.icon("res://icons/chevron.svg", UiStyle.ICON * 0.7, UiStyle.TEXT_DIM,
				UiStyle.LAYER_TEXT + band)
			chevron.position = Vector3(right - UiStyle.ICON * 0.3, y, 0.004)
			_body.add_child(chevron)
			end -= UiStyle.ICON * 0.75
		if row.has("action"):
			end = _build_action(str(row["action"]), end, y) - 0.03
		var trailing := ""
		var trailing_colour := UiStyle.TEXT_DIM
		if row.has("choice"):
			trailing = str(row["choice"])
			trailing_colour = UiStyle.ACCENT
		elif row.has("note"):
			trailing = str(row["note"])
		if trailing != "":
			var note := _label(trailing, UiStyle.FONT_SMALL, trailing_colour)
			note.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
			note.position = Vector3(end, y, 0.004)
			UiStyle.fit(note, maxf(0.2, (end - text_left) * 0.5))
			_body.add_child(note)
			end -= UiStyle.text_width(note.text, note.font_size, note.pixel_size) + 0.04
		room = end - text_left

	var text := _text(row)
	if text != "":
		var label := _label(text, size, colour)
		label.horizontal_alignment = HORIZONTAL_ALIGNMENT_LEFT
		label.position = Vector3(text_left, y, 0.004)
		if row.has("sub"):
			label.position.y += 0.022
		UiStyle.fit(label, room)
		_body.add_child(label)
		parts["label"] = label
	if row.has("sub"):
		var sub := _label(str(row["sub"]), UiStyle.FONT_SMALL, UiStyle.TEXT_DIM)
		sub.horizontal_alignment = HORIZONTAL_ALIGNMENT_LEFT
		sub.position = Vector3(text_left, y - 0.032, 0.004)
		sub.pixel_size *= 0.9
		UiStyle.fit(sub, room)
		_body.add_child(sub)
	if row.has("heading"):
		# A hairline from the end of the heading to the edge: the section
		# starts here, without a box round it.
		var start := text_left + UiStyle.text_width(text, size) + 0.03
		if start < right - 0.05:
			_body.add_child(UiStyle.rounded(Vector2(right - start, UiStyle.HAIRLINE), UiStyle.EDGE,
				Vector3((start + right) * 0.5, y, 0.002), UiStyle.LAYER_EDGE + band, UiStyle.HAIRLINE))
	return parts


## A real slider: a track, the part of it that is filled, a knob, and the
## value as a number at the end.
func _build_slider(i: int, row: Dictionary, y: float, parts: Dictionary) -> void:
	var span := track(i)
	var left := float(span[0])
	var length := float(span[1])
	_body.add_child(UiStyle.rounded(Vector2(length, UiStyle.TRACK), UiStyle.INK_LIFT,
		Vector3(left + length * 0.5, y, 0.002), UiStyle.LAYER_FILL + band, UiStyle.TRACK))
	var fill := UiStyle.rounded(Vector2(UiStyle.TRACK, UiStyle.TRACK), UiStyle.ACCENT,
		Vector3(left, y, 0.003), UiStyle.LAYER_EDGE + band, UiStyle.TRACK)
	_body.add_child(fill)
	var ring := UiStyle.circle(UiStyle.KNOB + 0.014, Color(UiStyle.ACCENT, 0.35), Vector3(left, y, 0.0035),
		UiStyle.LAYER_EDGE + band)
	_body.add_child(ring)
	var knob := UiStyle.circle(UiStyle.KNOB, UiStyle.TEXT, Vector3(left, y, 0.004), UiStyle.LAYER_EDGE + band)
	_body.add_child(knob)
	var readout := _label("", UiStyle.FONT_SMALL, UiStyle.ACCENT)
	readout.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	readout.position = Vector3(width * 0.5 - UiStyle.PAD, y, 0.004)
	_body.add_child(readout)
	parts["fill"] = fill
	parts["knob"] = knob
	parts["ring"] = ring
	parts["readout"] = readout
	_parts.append(parts)
	_refresh_row(i)
	_parts.pop_back()


## A switch: a pill, lit when on, with its knob at the end it is set to.
func _build_switch(on: bool, at: Vector3) -> void:
	var size := UiStyle.SWITCH
	_body.add_child(UiStyle.rounded(size, UiStyle.ACCENT if on else UiStyle.INK_LIFT, at + Vector3(0, 0, 0.002),
		UiStyle.LAYER_FILL + band, size.y, UiStyle.EDGE, 0.0 if on else UiStyle.HAIRLINE))
	var knob := size.y - 0.014
	var x := (size.x - size.y) * 0.5 * (1.0 if on else -1.0)
	_body.add_child(UiStyle.circle(knob, UiStyle.TEXT if on else UiStyle.TEXT_DIM,
		at + Vector3(x, 0.0, 0.004), UiStyle.LAYER_EDGE + band))


## A field: a sunken box with what is in it, or what it is for in grey.
func _build_field(row: Dictionary, from: float, to: float, y: float) -> void:
	var size := Vector2(to - from, ROW_HEIGHT * 0.72)
	_body.add_child(UiStyle.rounded(size, UiStyle.BACKDROP, Vector3((from + to) * 0.5, y, 0.002),
		UiStyle.LAYER_FILL + band, UiStyle.RADIUS_SMALL, UiStyle.EDGE, UiStyle.HAIRLINE))
	var empty := str(row["text"]) == ""
	var shown := _label(_field(row), UiStyle.FONT_SMALL, UiStyle.TEXT_DIM if empty else UiStyle.ACCENT)
	shown.horizontal_alignment = HORIZONTAL_ALIGNMENT_LEFT
	shown.position = Vector3(from + 0.03, y, 0.004)
	UiStyle.fit(shown, size.x - 0.1)
	_body.add_child(shown)
	if row.has("icon") and _text(row) == "":
		return
	var pen := UiStyle.icon("res://icons/chevron.svg", UiStyle.ICON * 0.55, UiStyle.TEXT_DIM, UiStyle.LAYER_TEXT + band)
	pen.position = Vector3(to - 0.035, y, 0.004)
	_body.add_child(pen)


## A small filled pill at the end of a row -- Join, Accept -- that says what
## picking the row will do. Returns where it starts.
func _build_action(text: String, end: float, y: float) -> float:
	var wide := UiStyle.text_width(text, UiStyle.FONT_SMALL) + 0.07
	var at := Vector3(end - wide * 0.5, y, 0.002)
	_body.add_child(UiStyle.rounded(Vector2(wide, ROW_HEIGHT * 0.6), Color(UiStyle.ACCENT, 0.16), at,
		UiStyle.LAYER_FILL + band, ROW_HEIGHT, Color(UiStyle.ACCENT, 0.55), UiStyle.HAIRLINE))
	var label := _label(text, UiStyle.FONT_SMALL, UiStyle.ACCENT)
	label.position = at + Vector3(0, 0, 0.002)
	_body.add_child(label)
	return end - wide


## A row drawn as a button: a filled pill across the panel, its label in the
## middle. For the one thing a page is for: light it, take a picture, send.
func _build_button(row: Dictionary, y: float) -> void:
	var fill := UiStyle.WARN if row.get("danger", false) else UiStyle.ACCENT
	var size := Vector2(width - UiStyle.PAD * 2.0, UiStyle.ROW_BUTTON - 0.03)
	_body.add_child(UiStyle.rounded(size, fill, Vector3(0.0, y, 0.002), UiStyle.LAYER_FILL + band, size.y))
	var ink := UiStyle.on(fill)
	var text := _text(row)
	var shift := 0.0
	if row.has("icon"):
		var whole := UiStyle.ICON + 0.03 + UiStyle.text_width(text, UiStyle.FONT)
		var mark := UiStyle.icon(str(row["icon"]), UiStyle.ICON, ink, UiStyle.LAYER_TEXT + band)
		mark.position = Vector3(-whole * 0.5 + UiStyle.ICON * 0.5, y, 0.004)
		_body.add_child(mark)
		shift = (UiStyle.ICON + 0.03) * 0.5
	var label := _label(text, UiStyle.FONT, ink)
	label.outline_size = 0
	label.position = Vector3(shift, y, 0.004)
	_body.add_child(label)


## Somebody's initial on their own colour, with a dot saying whether they are
## about.
func _build_avatar(who: String, status: String, at: Vector3) -> void:
	var colour := UiStyle.avatar_colour(who)
	_body.add_child(UiStyle.circle(UiStyle.CHIP, colour, at + Vector3(0, 0, 0.002), UiStyle.LAYER_FILL + band))
	var initial := _label(who.substr(0, 1).to_upper(), UiStyle.FONT_SMALL, UiStyle.on(colour))
	initial.outline_size = 0
	initial.position = at + Vector3(0, 0, 0.004)
	_body.add_child(initial)
	if status == "":
		return
	var dot := Vector3(UiStyle.CHIP * 0.36, -UiStyle.CHIP * 0.36, 0.005)
	_body.add_child(UiStyle.circle(0.032, Color(UiStyle.INK, 1.0), at + dot, UiStyle.LAYER_EDGE + band))
	_body.add_child(UiStyle.circle(0.02, UiStyle.GOOD if status == "online" else UiStyle.TEXT_DIM,
		at + dot + Vector3(0, 0, 0.001), UiStyle.LAYER_EDGE + band))


## A picture, as wide as the content, in a rounded frame.
func _build_picture(row: Dictionary, y: float) -> void:
	var texture: Texture2D = row["picture"]
	var room := Vector2(width - UiStyle.PAD * 2.0, PICTURE - 0.04)
	var aspect := float(texture.get_width()) / maxf(1.0, float(texture.get_height()))
	var size := Vector2(room.y * aspect, room.y)
	if size.x > room.x:
		size = Vector2(room.x, room.x / aspect)
	_body.add_child(UiStyle.rounded(size + Vector2(0.02, 0.02), UiStyle.INK_LIFT, Vector3(0, y, 0.002),
		UiStyle.LAYER_FILL + band, UiStyle.RADIUS_SMALL))
	var sprite := Sprite3D.new()
	sprite.texture = texture
	sprite.pixel_size = size.y / maxf(1.0, float(texture.get_height()))
	sprite.no_depth_test = true
	sprite.render_priority = UiStyle.LAYER_TEXT + band
	sprite.texture_filter = BaseMaterial3D.TEXTURE_FILTER_LINEAR_WITH_MIPMAPS
	sprite.position = Vector3(0, y, 0.004)
	_body.add_child(sprite)


## The strip across the top: an icon and the page's name, a note at the
## right, a handle on a panel you can carry and an x on one you can close.
func _build_bar(height: float) -> void:
	var y := height * 0.5 - BAR * 0.5
	var left := -width * 0.5 + UiStyle.PAD
	var right := width * 0.5 - UiStyle.PAD * 0.6
	# The handle: a short pill along the very top edge, the way a sheet you
	# can drag says so on a phone.
	if movable:
		_body.add_child(UiStyle.rounded(Vector2(0.16, 0.012), Color(UiStyle.EDGE_BRIGHT, 0.5),
			Vector3(0.0, height * 0.5 - 0.022, 0.004), UiStyle.LAYER_EDGE + band, 0.006))
	if title_icon != "":
		var mark := UiStyle.icon(title_icon, UiStyle.ICON, UiStyle.ACCENT, UiStyle.LAYER_TEXT + band)
		mark.position = Vector3(left + UiStyle.ICON * 0.5, y - 0.004, 0.004)
		_body.add_child(mark)
		left += UiStyle.ICON + 0.03
	if closable:
		var x := UiStyle.icon("res://icons/close.svg", UiStyle.ICON * 0.7, UiStyle.TEXT_DIM, UiStyle.LAYER_TEXT + band)
		x.position = Vector3(width * 0.5 - BAR * 0.6, y - 0.004, 0.004)
		_body.add_child(x)
		right = width * 0.5 - BAR * 1.15
	if title_note != "":
		var note := _label(title_note, UiStyle.FONT_SMALL, UiStyle.TEXT_DIM)
		note.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		note.position = Vector3(right, y - 0.004, 0.004)
		_body.add_child(note)
		right -= UiStyle.text_width(title_note, UiStyle.FONT_SMALL) + 0.04
	# Under the bar, a hairline: the title is not one of the rows.
	_body.add_child(UiStyle.rounded(Vector2(width - 0.06, UiStyle.HAIRLINE), UiStyle.EDGE,
		Vector3(0.0, height * 0.5 - BAR, 0.002), UiStyle.LAYER_EDGE + band, UiStyle.HAIRLINE))
	if _title == "":
		return
	var label := _label(_title, UiStyle.FONT, UiStyle.TEXT)
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_LEFT
	label.position = Vector3(left, y - 0.004, 0.004)
	UiStyle.fit(label, right - left)
	_body.add_child(label)


## The tabs: a segmented strip, the open one lit, each an icon beside its name.
func _build_tabs(height: float) -> void:
	if _tabs.is_empty():
		return
	var inner := width - UiStyle.PAD * 1.2
	var cell := inner / float(_tabs.size())
	var middle := height * 0.5 - BAR - UiStyle.TAB * 0.5 - 0.005
	_body.add_child(UiStyle.rounded(Vector2(inner, UiStyle.TAB - 0.025), Color(UiStyle.BACKDROP, 0.6),
		Vector3(0.0, middle, 0.001), UiStyle.LAYER_FILL + band, UiStyle.TAB))
	for i in _tabs.size():
		var tab: Dictionary = _tabs[i]
		var on: bool = tab.get("on", false)
		var x := -inner * 0.5 + cell * (i + 0.5)
		var colour := UiStyle.ACCENT if on else UiStyle.TEXT_DIM
		if on:
			_body.add_child(UiStyle.rounded(Vector2(cell - 0.016, UiStyle.TAB - 0.04), Color(UiStyle.ACCENT, 0.14),
				Vector3(x, middle, 0.002), UiStyle.LAYER_FILL + band, UiStyle.TAB,
				Color(UiStyle.ACCENT, 0.5), UiStyle.HAIRLINE))
		var text := str(tab.get("label", ""))
		var whole := UiStyle.text_width(text, UiStyle.FONT_SMALL)
		if tab.has("icon"):
			whole += UiStyle.TAB_ICON + 0.022
			var mark := UiStyle.icon(str(tab["icon"]), UiStyle.TAB_ICON, colour, UiStyle.LAYER_TEXT + band)
			mark.position = Vector3(x - whole * 0.5 + UiStyle.TAB_ICON * 0.5, middle, 0.004)
			_body.add_child(mark)
		var label := _label(text, UiStyle.FONT_SMALL, UiStyle.TEXT if on else colour)
		label.horizontal_alignment = HORIZONTAL_ALIGNMENT_LEFT
		var start := x - whole * 0.5 + (UiStyle.TAB_ICON + 0.022 if tab.has("icon") else 0.0)
		label.position = Vector3(start, middle, 0.004)
		_body.add_child(label)


func _place_highlight() -> void:
	if _highlight == null or _rows.is_empty():
		return
	var show: bool = _focused and _rows[_pick].has("id") and not _rows[_pick].get("button", false)
	_highlight.visible = show
	if _marker != null:
		_marker.visible = show
	if not show:
		return
	var y := _row_y[_pick]
	var h := _row_h[_pick]
	UiStyle.resize(_highlight, Vector2(width - 0.05, h - 0.01))
	_highlight.position = Vector3(0.0, y, 0.001)
	UiStyle.resize(_marker, Vector2(UiStyle.MARKER, h * 0.5))
	_marker.position = Vector3(-width * 0.5 + 0.03, y, 0.003)
