extends Node3D

## Hearth Studio: builds worlds in the format the game loads
## (docs/WORLD_FORMAT.md). Shipped as its own exe from this same project, so
## the loader, the materials and the test-play rig are the game's own.
##
## UI: Unity-style 3-column layout.
##   Left  — Hierarchy (object tree)
##   Centre — 3-D viewport
##   Right  — Inspector (collapsible component sections + Fire# script editor)
##   Top    — Toolbar (world management, primitive palette, import)
##   Bottom — Action bar (Save, Pack, Test, Spawn Here, status)

const SNAP := 0.25
const FLY_SPEED := 8.0
const LOOK_SPEED := 0.004
const UNDO_DEPTH := 50
const SHAPE_SIZES := {
	"box":      [1.0, 1.0, 1.0],
	"sphere":   [1.0, 1.0, 1.0],
	"cylinder": [1.0, 2.0, 1.0],
	"plane":    [4.0, 1.0, 4.0],
	"torus":    [1.2, 1.8, 1.2],
}
const SHAPE_ICONS := {
	"box":      "📦",
	"sphere":   "🔮",
	"cylinder": "🌀",
	"plane":    "▬",
	"torus":    "⭕",
	"light":    "💡",
	"model":    "🗿",
}

var _slug   := ""
var _world: Dictionary = {}
var _selected := -1
var _undo: Array[String] = []
var _nodes: Dictionary = {}
var _loading := false
var _looking := false
var _dragging := false
var _dirty := false

# Inspector section collapse state
var _section_collapsed := {
	"transform": false,
	"shape":     false,
	"light":     false,
	"scripts":   false,
}

# Script editor state
var _editing_script_index := -1   # index in entry["scripts"]


# ── ready ─────────────────────────────────────────────────────────────────────

func _ready() -> void:
	DirAccess.make_dir_recursive_absolute(Session.WORLDS)
	DirAccess.make_dir_recursive_absolute(Session.PACKS)
	_connect_ui()
	if _world_slugs().is_empty():
		_seed_shipped_worlds()
	_refresh_worlds()
	var worlds := _world_slugs()
	_open(worlds[0] if not worlds.is_empty() else _create("my world"))


func _connect_ui() -> void:
	# Toolbar – world management
	%WorldDropdown.item_selected.connect(_on_world_dropdown_selected)
	%NewWorldBtn.pressed.connect(_new_world)
	%CopyWorldBtn.pressed.connect(_copy_world)
	%NewNameEdit.text_submitted.connect(func(_t: String) -> void: _new_world())

	# Toolbar – primitives
	for shape_name: String in SHAPE_SIZES:
		var btn: Button = %("Prim_" + shape_name.capitalize())
		if btn:
			btn.pressed.connect(_on_primitive_pressed.bind(shape_name))
	var light_btn: Button = %Prim_Light
	if light_btn:
		light_btn.pressed.connect(_on_primitive_pressed.bind("light"))

	%ImportBtn.pressed.connect(func() -> void: %ImportDialog.popup_centered())
	%ImportDialog.file_selected.connect(_import_model)

	# Hierarchy
	%HierarchyList.item_selected.connect(func(idx: int) -> void: _select(idx))

	# Inspector sections – collapse buttons
	%TransformHeader.pressed.connect(func() -> void: _toggle_section("transform"))
	%ShapeHeader.pressed.connect(func() -> void: _toggle_section("shape"))
	%LightHeader.pressed.connect(func() -> void: _toggle_section("light"))
	%ScriptsHeader.pressed.connect(func() -> void: _toggle_section("scripts"))

	# Inspector – transform spinboxes
	for axis_key in ["PosX","PosY","PosZ","RotX","RotY","RotZ","ScaleX","ScaleY","ScaleZ"]:
		(%("Field_" + axis_key) as SpinBox).value_changed.connect(
			func(_v: float) -> void: _field_changed(false))

	# Inspector – shape fields
	(%Field_SizeX as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_SizeY as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_SizeZ as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_Color as ColorPickerButton).color_changed.connect(func(_c:Color)->void: _field_changed(true))
	(%Field_Body as OptionButton).item_selected.connect(func(_i:int)->void: _field_changed(true))
	(%Field_Mass as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))

	# Populate body options from WorldFormat
	for choice in WorldFormat.BODIES:
		(%Field_Body as OptionButton).add_item(choice.capitalize())

	# Inspector – light fields
	(%Field_Energy as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_Range as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_LightColor as ColorPickerButton).color_changed.connect(func(_c:Color)->void: _field_changed(true))

	# Inspector – scripts
	%AddScriptBtn.pressed.connect(_on_add_script_pressed)
	%ScriptFileDialog.file_selected.connect(_on_script_file_selected)

	# Script editor popup
	%ScriptApplyBtn.pressed.connect(_on_script_apply)
	%ScriptDiscardBtn.pressed.connect(_on_script_discard)
	%ScriptCodeEdit.text_changed.connect(_on_script_code_changed)

	# Bottom bar
	%SaveBtn.pressed.connect(func() -> void: _save(true))
	%PackBtn.pressed.connect(_pack)
	%TestBtn.pressed.connect(_test)
	%SpawnHereBtn.pressed.connect(_spawn_here)

	# Object rename
	%Field_Name.text_changed.connect(func(_t: String) -> void: _field_changed(false))
	%DeleteBtn.pressed.connect(_delete)
	%DuplicateBtn.pressed.connect(_duplicate)


# ── process ───────────────────────────────────────────────────────────────────

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


# ── worlds ─────────────────────────────────────────────────────────────────────

func _world_slugs() -> PackedStringArray:
	var slugs := DirAccess.get_directories_at(Session.WORLDS)
	slugs.sort()
	return slugs


func _refresh_worlds() -> void:
	%WorldDropdown.clear()
	var slugs := _world_slugs()
	for s in slugs:
		%WorldDropdown.add_item(s)
	var idx := slugs.find(_slug)
	if idx >= 0:
		%WorldDropdown.select(idx)


func _on_world_dropdown_selected(index: int) -> void:
	var slugs := _world_slugs()
	if index < slugs.size():
		_open(slugs[index])


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
	var wanted: String = %NewNameEdit.text.strip_edges()
	if wanted == "":
		_status("Type a name for the new world first")
		return
	%NewNameEdit.text = ""
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
		DirAccess.copy_absolute(
			_dir().path_join(WorldFormat.ASSETS).path_join(asset),
			target.path_join(WorldFormat.ASSETS).path_join(asset))
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
	_status("Opened %s" % _world.get("name", slug))


func _save(loud: bool) -> void:
	if _slug == "" or _world.is_empty():
		return
	var problem := WorldFormat.write(_world, _dir())
	_dirty = problem != ""
	if loud or problem != "":
		_status(problem if problem != "" else "Saved %s ✓" % _slug)


func _pack() -> void:
	_save(false)
	var out := Session.PACKS.path_join("%s.%s" % [_slug, WorldFormat.PACK_EXTENSION])
	var problem := WorldFormat.pack(_dir(), out)
	if problem != "":
		_status(problem)
		return
	_status("Packed → %s" % ProjectSettings.globalize_path(out))
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
	_status("Spawn point set here ✓")


# ── seed shipped worlds ────────────────────────────────────────────────────────

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


# ── scene rebuild ─────────────────────────────────────────────────────────────

func _rebuild() -> void:
	for child in $WorldRoot.get_children():
		child.free()
	var built := WorldFormat.build(_world, _dir())
	$WorldRoot.add_child(built)
	_nodes.clear()
	for node in built.get_children():
		if node.has_meta("world_index"):
			_nodes[int(node.get_meta("world_index"))] = node
	_refresh_hierarchy()
	_highlight()


func _objects() -> Array:
	return _world.get("objects", [])


func _refresh_hierarchy() -> void:
	%HierarchyList.clear()
	var world_name: String = _world.get("name", _slug)
	for entry: Dictionary in _objects():
		var kind := str(entry.get("shape", entry.get("model", "light")))
		var icon_str: String = SHAPE_ICONS.get(kind.get_file(), SHAPE_ICONS.get(kind, "📦"))
		if entry.has("model"):
			icon_str = SHAPE_ICONS["model"]
		elif entry.has("light"):
			icon_str = SHAPE_ICONS["light"]
		var label := "%s  %s" % [icon_str, entry.get("name", "Object")]
		%HierarchyList.add_item(label)
	%WorldNameLabel.text = world_name
	if _selected >= 0 and _selected < %HierarchyList.item_count:
		%HierarchyList.select(_selected)


# ── selection ─────────────────────────────────────────────────────────────────

func _select(index: int) -> void:
	_selected = index if index >= 0 and index < _objects().size() else -1
	if _selected >= 0:
		%HierarchyList.select(_selected)
	else:
		%HierarchyList.deselect_all()
	_load_inspector()
	_highlight()


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
	material.albedo_color = Color(UiStyle.ACCENT, 0.22)
	mesh.material = material
	$Highlight.mesh = mesh
	$Highlight.global_transform = node.global_transform.translated_local(box.get_center())
	$Highlight.visible = true


# ── input / mouse ─────────────────────────────────────────────────────────────

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
	# Ignore clicks that land in UI panels
	var vp_rect := _viewport_rect()
	if not vp_rect.has_point(at):
		return
	# If a primitive is pending placement, place it on click
	if not _pending_place.is_empty():
		_place(_pending_place, _ground(at))
		return
	var hit := _pick(at)
	_select(hit)
	_dragging = hit >= 0


func _viewport_rect() -> Rect2:
	# The viewport fills between the left and right panels; approximate it.
	var left_w: float = %HierarchyPanel.size.x
	var right_w: float = %InspectorPanel.size.x
	var full := get_viewport().get_visible_rect()
	return Rect2(left_w, 48.0, full.size.x - left_w - right_w, full.size.y - 48.0 - 44.0)


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


# ── primitive toolbar ─────────────────────────────────────────────────────────

var _pending_place: Dictionary = {}

func _on_primitive_pressed(shape_name: String) -> void:
	if shape_name == "light":
		_pending_place = {"name": "Light", "light": {"color": "#ffb366", "energy": 2.0, "range": 8.0}}
	else:
		_pending_place = {
			"name": shape_name.capitalize(),
			"shape": shape_name,
			"size": SHAPE_SIZES[shape_name].duplicate(),
			"color": "#9a9aa2", "roughness": 0.6, "body": "static",
		}
	_status("Click the viewport to place %s" % shape_name)
	# Override next left click in viewport to place instead of select
	set_process_unhandled_input(true)


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
	_pending_place = {}


func _resting_height(entry: Dictionary) -> float:
	if entry.has("light"):
		return 2.0
	var size: Array = entry.get("size", [1.0, 1.0, 1.0])
	return float(size[1]) * 0.5 if entry.has("shape") else 0.0


# ── drag / move ───────────────────────────────────────────────────────────────

func _drag(at: Vector2) -> void:
	if _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	var position := WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO)
	if Input.is_key_pressed(KEY_ALT):
		var camera: Camera3D = $Camera
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
	_load_inspector()
	_dirty = true


# ── duplicate / delete / undo ─────────────────────────────────────────────────

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


# ── import ────────────────────────────────────────────────────────────────────

func _import_model(path: String) -> void:
	var result := StudioImport.into_world(path, _dir())
	if result.has("error"):
		_status(str(result["error"]))
		return
	_fill_import_palette()
	_place({"name": str(result["model"]).get_file().get_basename(), "model": result["model"], "body": "static"},
		_ground(get_viewport().get_visible_rect().size * 0.5))
	_status("Imported %s" % result["model"])


func _fill_import_palette() -> void:
	# Refresh any import-based primitive buttons — currently handled via place directly
	pass


# ── inspector ─────────────────────────────────────────────────────────────────

func _toggle_section(key: String) -> void:
	_section_collapsed[key] = not _section_collapsed[key]
	_apply_section_visibility()


func _apply_section_visibility() -> void:
	var label_map := {
		"transform": "Transform",
		"shape":     "Shape",
		"light":     "Light",
		"scripts":   "Fire# Scripts",
	}
	var content_map := {
		"transform": %Section_Transform,
		"shape":     %ShapeSection,
		"light":     %LightSection,
		"scripts":   %Section_Scripts,
	}
	var header_map := {
		"transform": %TransformHeader,
		"shape":     %ShapeHeader,
		"light":     %LightHeader,
		"scripts":   %ScriptsHeader,
	}
	for key: String in _section_collapsed:
		var content_node: Control = content_map.get(key)
		var header_btn: Button = header_map.get(key)
		if content_node:
			content_node.visible = not _section_collapsed[key]
		if header_btn:
			header_btn.text = "%s %s" % [
				"▶" if _section_collapsed[key] else "▼",
				label_map.get(key, key.capitalize())
			]


func _load_inspector() -> void:
	_loading = true
	var has := _selected >= 0
	%InspectorEmpty.visible = not has
	%InspectorContent.visible = has

	if not has:
		_loading = false
		return

	var entry: Dictionary = _objects()[_selected]

	# Name / header
	%Field_Name.text = str(entry.get("name", "Object"))

	# Transform
	var pos := WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO)
	var rot := WorldFormat.vec3(entry.get("rot", []), Vector3.ZERO)
	var scale := WorldFormat.vec3(entry.get("scale", []), Vector3.ONE)
	_set_spin("PosX", pos.x); _set_spin("PosY", pos.y); _set_spin("PosZ", pos.z)
	_set_spin("RotX", rot.x); _set_spin("RotY", rot.y); _set_spin("RotZ", rot.z)
	_set_spin("ScaleX", scale.x); _set_spin("ScaleY", scale.y); _set_spin("ScaleZ", scale.z)

	# Shape / light section visibility
	var is_shape: bool = entry.has("shape")
	var is_light: bool = entry.has("light")
	# Always show headers for the present component type, let collapse state govern content
	%ShapeHeader.visible = is_shape
	%ShapeSection.visible = is_shape and not _section_collapsed.get("shape", false)
	%LightHeader.visible = is_light
	%LightSection.visible = is_light and not _section_collapsed.get("light", false)

	if is_shape:
		var size := WorldFormat.vec3(entry.get("size", []), Vector3.ONE)
		_set_spin("SizeX", size.x); _set_spin("SizeY", size.y); _set_spin("SizeZ", size.z)
		var color_str := str(entry.get("color", "#9a9aa2"))
		(%Field_Color as ColorPickerButton).color = Color.html(color_str) if Color.html_is_valid(color_str) else Color(0.6, 0.6, 0.62)
		(%Field_Body as OptionButton).selected = maxi(WorldFormat.BODIES.find(str(entry.get("body", "static"))), 0)
		(%Field_Mass as SpinBox).value = float(entry.get("mass", 1.0))
		%MassRow.visible = str(entry.get("body", "")) == "grabbable"

	if is_light:
		var light: Dictionary = entry.get("light", {})
		(%Field_Energy as SpinBox).value = float(light.get("energy", 2.0))
		(%Field_Range as SpinBox).value = float(light.get("range", 8.0))
		var lc := str(light.get("color", "#ffb366"))
		(%Field_LightColor as ColorPickerButton).color = Color.html(lc) if Color.html_is_valid(lc) else Color(1.0, 0.7, 0.4)

	# Scripts section
	_rebuild_scripts_ui(entry)
	_apply_section_visibility()

	_loading = false


func _set_spin(key: String, value: float) -> void:
	var node := %("Field_" + key) as SpinBox
	if node:
		node.value = value


func _field_changed(structural: bool) -> void:
	if _loading or _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	entry["name"] = (%Field_Name as LineEdit).text

	entry["pos"]   = [(%Field_PosX as SpinBox).value, (%Field_PosY as SpinBox).value, (%Field_PosZ as SpinBox).value]
	entry["rot"]   = [(%Field_RotX as SpinBox).value, (%Field_RotY as SpinBox).value, (%Field_RotZ as SpinBox).value]
	entry["scale"] = [(%Field_ScaleX as SpinBox).value, (%Field_ScaleY as SpinBox).value, (%Field_ScaleZ as SpinBox).value]

	if entry.has("shape"):
		entry["size"]  = [(%Field_SizeX as SpinBox).value, (%Field_SizeY as SpinBox).value, (%Field_SizeZ as SpinBox).value]
		entry["color"] = "#" + (%Field_Color as ColorPickerButton).color.to_html(false)
		entry["body"]  = WorldFormat.BODIES[(%Field_Body as OptionButton).selected]
		if entry["body"] == "grabbable":
			entry["mass"] = (%Field_Mass as SpinBox).value
		%MassRow.visible = entry["body"] == "grabbable"

	if entry.has("light"):
		var light: Dictionary = entry.get("light", {})
		light["energy"] = (%Field_Energy as SpinBox).value
		light["range"]  = (%Field_Range as SpinBox).value
		light["color"]  = "#" + (%Field_LightColor as ColorPickerButton).color.to_html(false)
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
		_refresh_hierarchy()
		_highlight()


# ── Fire# Scripts UI ─────────────────────────────────────────────────────────

func _rebuild_scripts_ui(entry: Dictionary) -> void:
	# Clear existing script rows
	for child in %ScriptRows.get_children():
		child.queue_free()

	var scripts: Array = entry.get("scripts", [])
	for i in scripts.size():
		var sc: Dictionary = scripts[i]
		_add_script_row(sc, i)


func _add_script_row(sc: Dictionary, idx: int) -> void:
	# Script header row: [🔥 name.fs] [Edit] [×]
	var header := HBoxContainer.new()
	header.add_theme_constant_override("separation", 6)

	var icon_lbl := Label.new()
	icon_lbl.text = "🔥"
	header.add_child(icon_lbl)

	var name_lbl := Label.new()
	name_lbl.text = str(sc.get("name", "script.fs"))
	name_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	header.add_child(name_lbl)

	var edit_btn := Button.new()
	edit_btn.text = "✏ Edit"
	edit_btn.pressed.connect(_on_edit_script.bind(idx))
	header.add_child(edit_btn)

	var remove_btn := Button.new()
	remove_btn.text = "×"
	remove_btn.pressed.connect(_on_remove_script.bind(idx))
	header.add_child(remove_btn)

	%ScriptRows.add_child(header)

	# Exposed fields from public vars
	var source: String = sc.get("source", "")
	var values: Dictionary = sc.get("values", {})
	var fields := FireSharp.get_exposed_fields(source)
	for field: Dictionary in fields:
		var field_row := HBoxContainer.new()
		field_row.add_theme_constant_override("separation", 4)

		var field_lbl := Label.new()
		field_lbl.text = str(field.get("name", ""))
		field_lbl.custom_minimum_size.x = 80
		field_row.add_child(field_lbl)

		var field_val: Variant = values.get(str(field.get("name", "")), field.get("default"))
		var field_type: String = str(field.get("type", "var"))
		var ctrl: Control = _make_field_control(field_type, field_val, sc, str(field.get("name", "")))
		ctrl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		field_row.add_child(ctrl)

		%ScriptRows.add_child(field_row)

	# Separator
	var sep := HSeparator.new()
	%ScriptRows.add_child(sep)


func _make_field_control(field_type: String, value: Variant, sc: Dictionary, field_name: String) -> Control:
	match field_type:
		"bool":
			var cb := CheckBox.new()
			cb.button_pressed = bool(value) if value != null else false
			cb.toggled.connect(func(v: bool) -> void: _set_script_value(sc, field_name, v))
			return cb
		"int":
			var spin := SpinBox.new()
			spin.step = 1
			spin.min_value = -99999
			spin.max_value = 99999
			spin.value = int(value) if value != null else 0
			spin.value_changed.connect(func(v: float) -> void: _set_script_value(sc, field_name, int(v)))
			return spin
		"Color":
			var cpb := ColorPickerButton.new()
			cpb.color = value if value is Color else Color.WHITE
			cpb.color_changed.connect(func(c: Color) -> void: _set_script_value(sc, field_name, c))
			return cpb
		"Vector3":
			var hb := HBoxContainer.new()
			var v3: Vector3 = value if value is Vector3 else Vector3.ZERO
			for axis_idx in 3:
				var sp := SpinBox.new()
				sp.step = 0.01
				sp.min_value = -9999
				sp.max_value = 9999
				sp.value = [v3.x, v3.y, v3.z][axis_idx]
				sp.size_flags_horizontal = Control.SIZE_EXPAND_FILL
				sp.value_changed.connect(func(_v: float) -> void:
					var nx := (hb.get_child(0) as SpinBox).value
					var ny := (hb.get_child(1) as SpinBox).value
					var nz := (hb.get_child(2) as SpinBox).value
					_set_script_value(sc, field_name, Vector3(nx, ny, nz)))
				hb.add_child(sp)
			return hb
		"string":
			var le := LineEdit.new()
			le.text = str(value) if value != null else ""
			le.text_changed.connect(func(v: String) -> void: _set_script_value(sc, field_name, v))
			return le
		_:  # float, var
			var spin := SpinBox.new()
			spin.step = 0.01
			spin.min_value = -99999.0
			spin.max_value = 99999.0
			spin.value = float(value) if value != null else 0.0
			spin.value_changed.connect(func(v: float) -> void: _set_script_value(sc, field_name, v))
			return spin


func _set_script_value(sc: Dictionary, field_name: String, value: Variant) -> void:
	if not sc.has("values"):
		sc["values"] = {}
	sc["values"][field_name] = value
	_dirty = true


func _on_add_script_pressed() -> void:
	%ScriptFileDialog.popup_centered()


func _on_script_file_selected(path: String) -> void:
	if _selected < 0:
		return
	var source := ""
	var fa := FileAccess.open(path, FileAccess.READ)
	if fa:
		source = fa.get_as_text()
	var entry: Dictionary = _objects()[_selected]
	if not entry.has("scripts"):
		entry["scripts"] = []
	entry["scripts"].append({"name": path.get_file(), "source": source, "values": {}})
	_dirty = true
	_rebuild_scripts_ui(entry)


func _on_remove_script(idx: int) -> void:
	if _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	var scripts: Array = entry.get("scripts", [])
	if idx < scripts.size():
		scripts.remove_at(idx)
	_dirty = true
	_rebuild_scripts_ui(entry)


func _on_edit_script(idx: int) -> void:
	if _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	var scripts: Array = entry.get("scripts", [])
	if idx >= scripts.size():
		return
	_editing_script_index = idx
	var sc: Dictionary = scripts[idx]
	(%ScriptEditorTitle as Label).text = "🔥 Editing: %s" % sc.get("name", "script.fs")
	(%ScriptCodeEdit as CodeEdit).text = str(sc.get("source", ""))
	%ScriptErrorLabel.text = ""
	%ScriptEditorPopup.visible = true
	(%ScriptCodeEdit as CodeEdit).grab_focus()


func _on_script_code_changed() -> void:
	var src: String = (%ScriptCodeEdit as CodeEdit).text
	var result := FireSharp.validate(src)
	if result.get("ok", true):
		%ScriptErrorLabel.text = ""
		%ScriptErrorLabel.add_theme_color_override("font_color", Color(0.5, 0.92, 0.62))
	else:
		%ScriptErrorLabel.text = "⚠ Line %d: %s" % [result.get("line", 0), result.get("error", "")]
		%ScriptErrorLabel.add_theme_color_override("font_color", Color(1.0, 0.45, 0.42))


func _on_script_apply() -> void:
	if _selected < 0 or _editing_script_index < 0:
		%ScriptEditorPopup.visible = false
		return
	var entry: Dictionary = _objects()[_selected]
	var scripts: Array = entry.get("scripts", [])
	if _editing_script_index < scripts.size():
		scripts[_editing_script_index]["source"] = (%ScriptCodeEdit as CodeEdit).text
	_dirty = true
	_editing_script_index = -1
	%ScriptEditorPopup.visible = false
	_rebuild_scripts_ui(entry)


func _on_script_discard() -> void:
	_editing_script_index = -1
	%ScriptEditorPopup.visible = false


# ── status ────────────────────────────────────────────────────────────────────

func _status(text: String) -> void:
	%StatusLabel.text = text
	print(text)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST and _dirty:
		_save(false)
