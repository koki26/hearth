class_name QuickMenu
extends FlatPanel

## The quick menu: hold the menu button and a row of big round buttons comes
## up beside your hand -- the camera, chat, the microphone, the full menu.
## Point at one with the other hand (or the mouse) and click.
##
## Tapping the menu button still opens the full menu; holding it is this. The
## few things you reach for while you are doing something else should not be
## three panels deep.
##
## Items come from main.gd:
##   {"id": "camera", "label": "Camera", "icon": "res://icons/camera.svg",
##    "on": true}       "on" lights the button: the camera is out, the mic is live

signal chosen(id: String)

## Each button's diameter, and the distance between their centres.
const TILE := 0.13
const PITCH := 0.17
## Room under the buttons for their names.
const CAPTION := 0.07
const MARGIN := 0.045
const GROW := 0.12

var _items: Array = []
var _pick := -1
var _body: Node3D
var _tiles: Array[MeshInstance3D] = []


func is_open() -> bool:
	return visible and not _items.is_empty()


## Shows the buttons with their middle at `at`, a global transform whose -Z
## points away from the player.
func open_at(at: Transform3D, items: Array) -> void:
	_items = items
	_pick = -1
	_build()
	global_transform = at
	visible = true
	if _body != null and is_inside_tree():
		_body.scale = Vector3(0.6, 0.6, 1.0)
		var tween := create_tween().bind_node(_body)
		tween.tween_property(_body, "scale", Vector3.ONE, GROW) \
			.set_ease(Tween.EASE_OUT).set_trans(Tween.TRANS_BACK)


func close() -> void:
	visible = false
	_items = []
	_tiles.clear()
	_pick = -1
	for child in get_children():
		child.free()
	_body = null


## The button under a local point, or -1.
func tile_at(local: Vector3) -> int:
	if _items.is_empty():
		return -1
	for i in _items.size():
		var centre := _centre(i)
		if Vector2(local.x - centre.x, local.y - centre.y).length() <= PITCH * 0.5:
			return i
	return -1


func hover_at(local: Vector3) -> void:
	_set_pick(tile_at(local))


func activate_at(local: Vector3) -> void:
	var index := tile_at(local)
	if index >= 0:
		chosen.emit(str(_items[index].get("id", "")))


func _centre(i: int) -> Vector2:
	var first := -float(_items.size() - 1) * PITCH * 0.5
	return Vector2(first + i * PITCH, CAPTION * 0.5)


func _set_pick(index: int) -> void:
	if index == _pick:
		return
	if _pick >= 0 and _pick < _tiles.size():
		UiStyle.tint(_tiles[_pick], _fill(_pick, false))
		_tiles[_pick].scale = Vector3.ONE
	_pick = index
	if _pick >= 0 and _pick < _tiles.size():
		UiStyle.tint(_tiles[_pick], _fill(_pick, true))
		_tiles[_pick].scale = Vector3(1.08, 1.08, 1.0)


func _fill(i: int, hovered: bool) -> Color:
	var on: bool = _items[i].get("on", false)
	if hovered:
		return Color(UiStyle.ACCENT, 0.45) if not on else UiStyle.ACCENT.lightened(0.15)
	return UiStyle.ACCENT if on else UiStyle.INK_LIFT


func _build() -> void:
	for child in get_children():
		child.free()
	_tiles.clear()
	_body = Node3D.new()
	add_child(_body)
	_width = PITCH * _items.size() + MARGIN * 2.0 - (PITCH - TILE)
	_height = TILE + CAPTION + MARGIN * 2.0
	for part in UiStyle.frame(_width, _height, band):
		_body.add_child(part)
	for i in _items.size():
		var item: Dictionary = _items[i]
		var centre := _centre(i)
		var on: bool = item.get("on", false)
		var tile := UiStyle.circle(TILE, _fill(i, false), Vector3(centre.x, centre.y, 0.002),
			UiStyle.LAYER_FILL + band)
		_body.add_child(tile)
		_tiles.append(tile)
		var ink := UiStyle.on(UiStyle.ACCENT) if on else UiStyle.TEXT
		var mark := UiStyle.icon(str(item.get("icon", "res://icons/menu.svg")), TILE * 0.46, ink,
			UiStyle.LAYER_TEXT + band)
		mark.position = Vector3(centre.x, centre.y, 0.006)
		_body.add_child(mark)
		var name := _label(str(item.get("label", "")), UiStyle.FONT_SMALL, UiStyle.TEXT if on else UiStyle.TEXT_DIM)
		name.position = Vector3(centre.x, centre.y - TILE * 0.5 - CAPTION * 0.45, 0.004)
		UiStyle.fit(name, PITCH - 0.01)
		_body.add_child(name)
