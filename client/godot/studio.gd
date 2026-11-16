extends Node3D

## Hearth Studio: builds worlds in the format the game loads
## (docs/WORLD_FORMAT.md). Shipped as its own exe from this same project, so
## the loader, the materials and the test-play rig are the game's own.

const SNAP := 0.25
const FLY_SPEED := 8.0
const LOOK_SPEED := 0.004
const UNDO_DEPTH := 50
const SHAPE_SIZES := {
	"box": [1.0, 1.0, 1.0],
	"sphere": [1.0, 1.0, 1.0],
	"cylinder": [1.0, 2.0, 1.0],
	"plane": [4.0, 1.0, 4.0],
	"torus": [1.2, 1.8, 1.2],
}

var _slug := ""
var _world: Dictionary = {}
var _selected := -1
var _undo: Array[String] = []
var _nodes: Dictionary = {}
var _fields: Dictionary = {}
var _rows: Dictionary = {}
var _loading := false
var _looking := false
var _dragging := false
var _dirty := false


func _ready() -> void:
	DirAccess.make_dir_recursive_absolute(Session.WORLDS)
	DirAccess.make_dir_recursive_absolute(Session.PACKS)
	_build_fields()
	_fill_palette()
	%WorldList.item_selected.connect(func(index: int) -> void: _open(%WorldList.get_item_text(index)))
	%NewWorld.pressed.connect(_new_world)
	%CopyWorld.pressed.connect(_copy_world)
	%Palette.item_selected.connect(func(_index: int) -> void: _status("Click the ground to place it"))
	%Import.pressed.connect(func() -> void: %ImportDialog.popup_centered())
	%ImportDialog.file_selected.connect(_import_model)
	%ObjectList.item_selected.connect(func(index: int) -> void: _select(index))
	%Duplicate.pressed.connect(_duplicate)
	%Delete.pressed.connect(_delete)
	%Save.pressed.connect(func() -> void: _save(true))
	%Pack.pressed.connect(_pack)
	%Test.pressed.connect(_test)
	%SpawnHere.pressed.connect(_spawn_here)
	if _world_slugs().is_empty():
		_seed_shipped_worlds()
	_refresh_worlds()
	var worlds := _world_slugs()
	_open(worlds[0] if not worlds.is_empty() else _create("my world"))


## First run: copy the world that ships in the exe into your folder, so there
## is something to walk around in and take apart.
func _seed_shipped_worlds() -> void:
	for slug in DirAccess.get_directories_at("res://worlds"):
		var world := WorldFormat.read("res://worlds".path_join(slug))
		if world.has("error"):
			continue
		WorldFormat.write(world, _dir(slug))
		for asset in WorldFormat.list_assets("res://worlds".path_join(slug)):
			DirAccess.make_dir_recursive_absolute(_dir(slug).path_join(WorldFormat.ASSETS))
			DirAccess.copy_absolute(
				"res://worlds".path_join(slug).path_join(WorldFormat.ASSETS).path_join(asset),
				_dir(slug).path_join(WorldFormat.ASSETS).path_join(asset))


func _process(delta: float) -> void:
	var camera: Camera3D = $Camera
	var move := Vector3(
		float(Input.is_key_pressed(KEY_D)) - float(Input.is_key_pressed(KEY_A)),
		float(Input.is_key_pressed(KEY_E)) - float(Input.is_key_pressed(KEY_Q)),
		float(Input.is_key_pressed(KEY_S)) - float(Input.is_key_pressed(KEY_W))
	)
	if move == Vector3.ZERO:
		return
	var speed := FLY_SPEED * (3.0 if Input.is_key_pressed(KEY_SHIFT) else 1.0)
	var step := camera.global_transform.basis * move.normalized() * speed * delta
	camera.global_position += step


# --- worlds ----------------------------------------------------------------

func _world_slugs() -> PackedStringArray:
	var slugs := DirAccess.get_directories_at(Session.WORLDS)
	slugs.sort()
	return slugs


func _refresh_worlds() -> void:
	%WorldList.clear()
	for slug in _world_slugs():
		%WorldList.add_item(slug)
		if slug == _slug:
			%WorldList.select(%WorldList.item_count - 1)


func _dir(slug := "") -> String:
	return Session.WORLDS.path_join(slug if slug != "" else _slug)


func _create(wanted_name: String) -> String:
	var slug := WorldFormat.slugify(wanted_name)
	var unique := slug
	var n := 2
	while DirAccess.dir_exists_absolute(_dir(unique)):
		unique = "%s-%d" % [slug, n]
		n += 1
	var world := WorldFormat.new_world(wanted_name.strip_edges())
	var problem := WorldFormat.write(world, _dir(unique))
	if problem != "":
		_status(problem)
	return unique


func _new_world() -> void:
	var wanted: String = %NewName.text.strip_edges()
	if wanted == "":
		_status("Type a name for the new world first")
		return
	%NewName.text = ""
	var slug := _create(wanted)
	_refresh_worlds()
	_open(slug)


func _copy_world() -> void:
	if _slug == "":
		return
	_save(false)
	var slug := _create("%s copy" % _world.get("name", _slug))
	var target := _dir(slug)
	var copy := _world.duplicate(true)
	copy["name"] = "%s copy" % _world.get("name", _slug)
	WorldFormat.write(copy, target)
	DirAccess.make_dir_recursive_absolute(target.path_join(WorldFormat.ASSETS))
	for asset in WorldFormat.list_assets(_dir()):
		var from := _dir().path_join(WorldFormat.ASSETS).path_join(asset)
		DirAccess.copy_absolute(from, target.path_join(WorldFormat.ASSETS).path_join(asset))
	_refresh_worlds()
	_open(slug)


func _open(slug: String) -> void:
	if _slug != "" and _slug != slug:
		_save(false)
	_slug = slug
	var world := WorldFormat.read(_dir())
	if world.has("error"):
		_status(str(world["error"]))
		return
	_world = world
	if typeof(_world.get("objects")) != TYPE_ARRAY:
		_world["objects"] = []
	_undo.clear()
	_selected = -1
	_dirty = false
	_rebuild()
	_refresh_worlds()
	_fill_palette()
	_status("Opened %s" % _world.get("name", slug))


func _save(loud: bool) -> void:
	if _slug == "" or _world.is_empty():
		return
	var problem := WorldFormat.write(_world, _dir())
	_dirty = problem != ""
	if loud or problem != "":
		_status(problem if problem != "" else "Saved %s" % _slug)


func _pack() -> void:
	_save(false)
	var out := Session.PACKS.path_join("%s.%s" % [_slug, WorldFormat.PACK_EXTENSION])
	var problem := WorldFormat.pack(_dir(), out)
	if problem != "":
		_status(problem)
		return
	_status("Packed to %s" % ProjectSettings.globalize_path(out))
	OS.shell_show_in_file_manager(ProjectSettings.globalize_path(out))


func _test() -> void:
	_save(false)
	Session.world_dir = _dir()
	Session.offline = true
	get_tree().change_scene_to_file("res://main.tscn")


func _spawn_here() -> void:
	var camera: Camera3D = $Camera
	_push_undo()
	_world["spawn"] = {
		"pos": WorldFormat.to_array(Vector3(camera.global_position.x, 0.0, camera.global_position.z)),
		"yaw": snappedf(rad_to_deg(camera.global_rotation.y), 1.0),
	}
	_rebuild()
	_status("Players will spawn here")


# --- the scene -------------------------------------------------------------

func _rebuild() -> void:
	for child in $WorldRoot.get_children():
		child.free()
	var built := WorldFormat.build(_world, _dir())
	$WorldRoot.add_child(built)
	_nodes.clear()
	for node in built.get_children():
		if node.has_meta("world_index"):
			_nodes[int(node.get_meta("world_index"))] = node
	_refresh_objects()
	_highlight()


func _objects() -> Array:
	return _world.get("objects", [])


func _refresh_objects() -> void:
	%ObjectList.clear()
	for entry: Dictionary in _objects():
		var kind := str(entry.get("shape", entry.get("model", "light")))
		%ObjectList.add_item("%s  (%s)" % [entry.get("name", "Object"), kind.get_file()])
	if _selected >= 0 and _selected < %ObjectList.item_count:
		%ObjectList.select(_selected)


func _select(index: int) -> void:
	_selected = index if index >= 0 and index < _objects().size() else -1
	if _selected >= 0:
		%ObjectList.select(_selected)
	else:
		%ObjectList.deselect_all()
	_load_fields()
	_highlight()


## A translucent box around the selection: cheaper than a gizmo, and enough to
## see what you are about to move.
func _highlight() -> void:
	var node: Node3D = _nodes.get(_selected)
	if node == null:
		$Highlight.visible = false
		return
	var box := WorldFormat.bounds(node)
	var mesh := BoxMesh.new()
	mesh.size = box.size + Vector3.ONE * 0.04
	var material := StandardMaterial3D.new()
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	# A selection is interface, so it is the interface accent (ui.gd).
	material.albedo_color = Color(UiStyle.ACCENT, 0.22)
	mesh.material = material
	$Highlight.mesh = mesh
	$Highlight.global_transform = node.global_transform.translated_local(box.get_center())
	$Highlight.visible = true


# --- placing, picking, moving ---------------------------------------------

func _fill_palette() -> void:
	%Palette.clear()
	for shape: String in SHAPE_SIZES:
		%Palette.add_item(shape.capitalize())
		%Palette.set_item_metadata(%Palette.item_count - 1, {
			"name": shape.capitalize(), "shape": shape, "size": SHAPE_SIZES[shape].duplicate(),
			"color": "#9a9aa2", "roughness": 0.6, "body": "static",
		})
	%Palette.add_item("Light")
	%Palette.set_item_metadata(%Palette.item_count - 1, {
		"name": "Light", "light": {"color": "#ffb366", "energy": 2.0, "range": 8.0},
	})
	if _slug == "":
		return
	for asset in WorldFormat.list_assets(_dir()):
		%Palette.add_item(asset.get_basename())
		%Palette.set_item_metadata(%Palette.item_count - 1, {
			"name": asset.get_basename(), "model": "%s/%s" % [WorldFormat.ASSETS, asset], "body": "static",
		})


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var button: InputEventMouseButton = event
		if button.button_index == MOUSE_BUTTON_RIGHT:
			_looking = button.pressed
		elif button.button_index == MOUSE_BUTTON_LEFT:
			if button.pressed:
				_on_click(button.position)
			else:
				_dragging = false
	elif event is InputEventMouseMotion:
		var motion: InputEventMouseMotion = event
		if _looking:
			var camera: Camera3D = $Camera
			camera.rotation.y -= motion.relative.x * LOOK_SPEED
			camera.rotation.x = clampf(camera.rotation.x - motion.relative.y * LOOK_SPEED, -1.5, 1.5)
		elif _dragging:
			_drag(motion.position)


func _unhandled_key_input(event: InputEvent) -> void:
	var key := event as InputEventKey
	if key == null or not key.pressed or key.echo:
		return
	if key.ctrl_pressed:
		match key.keycode:
			KEY_S: _save(true)
			KEY_D: _duplicate()
			KEY_Z: _undo_last()
	elif key.keycode == KEY_DELETE:
		_delete()


func _on_click(at: Vector2) -> void:
	var chosen: PackedInt32Array = %Palette.get_selected_items()
	if not chosen.is_empty():
		var entry: Dictionary = %Palette.get_item_metadata(chosen[0])
		%Palette.deselect_all()
		_place(entry, _ground(at))
		return
	var hit := _pick(at)
	_select(hit)
	_dragging = hit >= 0


## Ray against every object box: no physics layers, no picker nodes.
func _pick(at: Vector2) -> int:
	var camera: Camera3D = $Camera
	var from := camera.project_ray_origin(at)
	var direction := camera.project_ray_normal(at)
	var best := -1
	var nearest := INF
	for index: int in _nodes:
		var node: Node3D = _nodes[index]
		var box: AABB = node.global_transform * WorldFormat.bounds(node)
		var point: Variant = box.intersects_ray(from, direction)
		if point == null:
			continue
		var distance: float = from.distance_to(point)
		if distance < nearest:
			nearest = distance
			best = index
	return best


func _ground(at: Vector2) -> Vector3:
	var camera: Camera3D = $Camera
	var from := camera.project_ray_origin(at)
	var direction := camera.project_ray_normal(at)
	var point: Variant = Plane(Vector3.UP, 0.0).intersects_ray(from, direction)
	if point == null:
		point = from + direction * 10.0
	var at_ground: Vector3 = point
	return Vector3(snappedf(at_ground.x, SNAP), 0.0, snappedf(at_ground.z, SNAP))


func _place(template: Dictionary, at: Vector3) -> void:
	_push_undo()
	var entry := template.duplicate(true)
	entry["pos"] = WorldFormat.to_array(at + Vector3.UP * _resting_height(entry))
	entry["rot"] = [0.0, 0.0, 0.0]
	entry["scale"] = [1.0, 1.0, 1.0]
	_objects().append(entry)
	_rebuild()
	_select(_objects().size() - 1)
	_status("Placed %s" % entry.get("name", "object"))


## Drops a piece on the floor rather than half inside it.
func _resting_height(entry: Dictionary) -> float:
	if entry.has("light"):
		return 2.0
	var size: Array = entry.get("size", [1.0, 1.0, 1.0])
	return float(size[1]) * 0.5 if entry.has("shape") else 0.0


func _drag(at: Vector2) -> void:
	if _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	var position := WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO)
	if Input.is_key_pressed(KEY_ALT):
		var camera: Camera3D = $Camera
		# Height follows the mouse against a vertical plane through the object.
		var plane := Plane(Vector3(camera.global_basis.z.x, 0.0, camera.global_basis.z.z).normalized(), position)
		var point: Variant = plane.intersects_ray(camera.project_ray_origin(at), camera.project_ray_normal(at))
		if point != null:
			var lifted: Vector3 = point
			position.y = snappedf(lifted.y, SNAP)
	else:
		var ground := _ground(at)
		position = Vector3(ground.x, position.y, ground.z)
	entry["pos"] = WorldFormat.to_array(position)
	var node: Node3D = _nodes.get(_selected)
	if node != null:
		node.position = position
	_highlight()
	_load_fields()
	_dirty = true


func _duplicate() -> void:
	if _selected < 0:
		return
	_push_undo()
	var entry: Dictionary = _objects()[_selected].duplicate(true)
	var at := WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO) + Vector3(SNAP * 2, 0.0, SNAP * 2)
	entry["pos"] = WorldFormat.to_array(at)
	_objects().append(entry)
	_rebuild()
	_select(_objects().size() - 1)


func _delete() -> void:
	if _selected < 0:
		return
	_push_undo()
	var name := str(_objects()[_selected].get("name", "object"))
	_objects().remove_at(_selected)
	_selected = -1
	_rebuild()
	_select(-1)
	_status("Removed %s" % name)


func _push_undo() -> void:
	_undo.append(JSON.stringify(_world))
	if _undo.size() > UNDO_DEPTH:
		_undo.remove_at(0)
	_dirty = true


func _undo_last() -> void:
	if _undo.is_empty():
		_status("Nothing left to undo")
		return
	var previous: Variant = JSON.parse_string(_undo.pop_back())
	if typeof(previous) != TYPE_DICTIONARY:
		return
	_world = previous
	_selected = -1
	_rebuild()
	_select(-1)
	_status("Undone")


# --- the inspector ---------------------------------------------------------

func _build_fields() -> void:
	_fields["name"] = _row("Name", LineEdit.new())
	_fields["name"].text_changed.connect(func(_text: String) -> void: _field_changed(false))
	for group in ["pos", "rot", "scale", "size"]:
		var boxes: Array[SpinBox] = []
		var row := HBoxContainer.new()
		for axis in 3:
			var spin := SpinBox.new()
			spin.step = 0.05
			spin.min_value = -4096.0
			spin.max_value = 4096.0
			spin.custom_minimum_size.x = 64
			spin.size_flags_horizontal = Control.SIZE_EXPAND_FILL
			spin.value_changed.connect(func(_v: float) -> void: _field_changed(group == "size"))
			row.add_child(spin)
			boxes.append(spin)
		_fields[group] = boxes
		_add_row(group.capitalize(), row, group)
	_fields["color"] = _row("Colour", ColorPickerButton.new())
	_fields["color"].color_changed.connect(func(_c: Color) -> void: _field_changed(true))
	var body := OptionButton.new()
	for choice in WorldFormat.BODIES:
		body.add_item(choice.capitalize())
	_fields["body"] = _row("Body", body)
	body.item_selected.connect(func(_i: int) -> void: _field_changed(true))
	_fields["mass"] = _row("Mass", _spin(0.01, 1000.0, 0.1))
	_fields["mass"].value_changed.connect(func(_v: float) -> void: _field_changed(true))
	_fields["energy"] = _row("Energy", _spin(0.0, 16.0, 0.1))
	_fields["energy"].value_changed.connect(func(_v: float) -> void: _field_changed(true))
	_fields["range"] = _row("Range", _spin(0.1, 200.0, 0.5))
	_fields["range"].value_changed.connect(func(_v: float) -> void: _field_changed(true))


func _spin(low: float, high: float, step: float) -> SpinBox:
	var spin := SpinBox.new()
	spin.min_value = low
	spin.max_value = high
	spin.step = step
	return spin


func _row(label: String, control: Control) -> Control:
	control.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_add_row(label, control, label.to_lower())
	return control


func _add_row(label: String, control: Control, key: String) -> void:
	var row := HBoxContainer.new()
	var text := Label.new()
	text.text = label
	text.custom_minimum_size.x = 54
	row.add_child(text)
	row.add_child(control)
	%Fields.add_child(row)
	_rows[key] = row


func _load_fields() -> void:
	_loading = true
	var has := _selected >= 0
	for key: String in _rows:
		_rows[key].visible = has
	if not has:
		%SelectionLabel.text = "Nothing selected"
		_loading = false
		return
	var entry: Dictionary = _objects()[_selected]
	%SelectionLabel.text = str(entry.get("name", "Object"))
	_fields["name"].text = str(entry.get("name", "Object"))
	_set_vec("pos", WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO))
	_set_vec("rot", WorldFormat.vec3(entry.get("rot", []), Vector3.ZERO))
	_set_vec("scale", WorldFormat.vec3(entry.get("scale", []), Vector3.ONE))
	_set_vec("size", WorldFormat.vec3(entry.get("size", []), Vector3.ONE))
	_fields["color"].color = Color.html(str(entry.get("color", "#9a9aa2"))) \
		if Color.html_is_valid(str(entry.get("color", ""))) else Color(0.6, 0.6, 0.62)
	_fields["body"].selected = maxi(WorldFormat.BODIES.find(str(entry.get("body", "static"))), 0)
	_fields["mass"].value = float(entry.get("mass", 1.0))
	var light: Dictionary = entry.get("light", {})
	_fields["energy"].value = float(light.get("energy", 2.0))
	_fields["range"].value = float(light.get("range", 8.0))

	# Only what this kind of object has.
	var is_light: bool = entry.has("light")
	var is_shape: bool = entry.has("shape")
	_rows["size"].visible = is_shape
	_rows["colour"].visible = is_shape
	_rows["body"].visible = not is_light
	_rows["mass"].visible = not is_light and str(entry.get("body", "")) == "grabbable"
	_rows["energy"].visible = is_light
	_rows["range"].visible = is_light
	_loading = false


func _set_vec(key: String, value: Vector3) -> void:
	var boxes: Array[SpinBox] = _fields[key]
	boxes[0].value = value.x
	boxes[1].value = value.y
	boxes[2].value = value.z


func _vec_of(key: String) -> Array:
	var boxes: Array[SpinBox] = _fields[key]
	return [boxes[0].value, boxes[1].value, boxes[2].value]


## `structural` edits (size, colour, body) need the object built again;
## a move only needs its transform.
func _field_changed(structural: bool) -> void:
	if _loading or _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	entry["name"] = _fields["name"].text
	entry["pos"] = _vec_of("pos")
	entry["rot"] = _vec_of("rot")
	entry["scale"] = _vec_of("scale")
	if entry.has("shape"):
		entry["size"] = _vec_of("size")
		entry["color"] = "#" + _fields["color"].color.to_html(false)
	if not entry.has("light"):
		entry["body"] = WorldFormat.BODIES[_fields["body"].selected]
		if entry["body"] == "grabbable":
			entry["mass"] = _fields["mass"].value
	else:
		var light: Dictionary = entry.get("light", {})
		light["energy"] = _fields["energy"].value
		light["range"] = _fields["range"].value
		entry["light"] = light
	_dirty = true
	if structural:
		_rebuild()
		_select(_selected)
	else:
		var node: Node3D = _nodes.get(_selected)
		if node != null:
			node.position = WorldFormat.vec3(entry["pos"], Vector3.ZERO)
			var rot: Vector3 = WorldFormat.vec3(entry["rot"], Vector3.ZERO)
			node.rotation = Vector3(deg_to_rad(rot.x), deg_to_rad(rot.y), deg_to_rad(rot.z))
			node.scale = WorldFormat.vec3(entry["scale"], Vector3.ONE)
		_refresh_objects()
		_highlight()


func _import_model(path: String) -> void:
	var result := StudioImport.into_world(path, _dir())
	if result.has("error"):
		_status(str(result["error"]))
		return
	_fill_palette()
	_place({"name": str(result["model"]).get_file().get_basename(), "model": result["model"], "body": "static"},
		_ground(get_viewport().get_visible_rect().size * 0.5))
	_status("Imported %s" % result["model"])


func _status(text: String) -> void:
	%Status.text = text
	print(text)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST and _dirty:
		_save(false)
