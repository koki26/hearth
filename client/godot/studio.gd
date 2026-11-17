extends Node3D

## Hearth Studio: Unity-inspired 3D World Builder and Avatar Creator.
## Builds worlds in docs/WORLD_FORMAT.md and custom avatars in AvatarFormat.
##
## UI Structure:
##   Top    — Unity Menu Bar (File, Edit, GameObject, Window) + Tools (Transform, Snapping, Play/Test, Mode Toggle)
##   Left   — Hierarchy Panel (Searchable Object Tree & Avatar Rig)
##   Centre — 3-D Viewport (World Flycam / Avatar Turntable + View Controls + Bottom Dock)
##   Right  — Inspector Panel (Unity-style GameObject Header, Collapsible Components + Avatar Customizer)
##   Bottom — Project Assets & Console Dock (Project, Console, Saved Avatars)

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

# ── Studio State ──────────────────────────────────────────────────────────────

var _mode := "world"                 # "world" or "avatar"
var _slug := ""
var _world: Dictionary = {}
var _selected := -1
var _undo: Array[String] = []
var _nodes: Dictionary = {}
var _loading := false
var _looking := false
var _dragging := false
var _dirty := false
var _hierarchy_filter := ""

# Transform tool mode: "move", "rotate", "scale"
var _gizmo_mode := "move"
var _snap_enabled := true
var _snap_val := 0.25

# Inspector section collapse state
var _section_collapsed := {
	"transform": false,
	"shape":     false,
	"light":     false,
	"scripts":   false,
}

# Fire# script editor state
var _editing_script_index := -1

# Avatar Creator state
var _avatar_data: Dictionary = {}
var _avatar_node: Node3D = null
var _turntable_auto_rotate := false
var _turntable_yaw := 0.0
var _turntable_pitch := 0.1
var _orbit_dist := 2.2

# Pending placement template
var _pending_place: Dictionary = {}

# Console logs
var _console_logs: Array[String] = []


# ── _ready ────────────────────────────────────────────────────────────────────

func _ready() -> void:
	DirAccess.make_dir_recursive_absolute(Session.WORLDS)
	DirAccess.make_dir_recursive_absolute(Session.PACKS)
	AvatarFormat.ensure_dirs()

	_setup_avatar_studio()
	_connect_ui()

	if _world_slugs().is_empty():
		_seed_shipped_worlds()
	_refresh_worlds()
	var worlds := _world_slugs()
	_open(worlds[0] if not worlds.is_empty() else _create("my world"))

	# Default active avatar
	_avatar_data = AvatarFormat.get_active_avatar()
	_load_avatar_ui(_avatar_data)
	_update_avatar_preview()
	_refresh_saved_avatars()
	_switch_mode("world")


func _setup_avatar_studio() -> void:
	var studio := $AvatarStudio as Node3D
	if studio == null:
		return
	var pedestal_base := studio.get_node_or_null("Pedestal/Base") as MeshInstance3D
	if pedestal_base != null:
		var mat := StandardMaterial3D.new()
		mat.albedo_color = Color(0.18, 0.2, 0.26)
		mat.roughness = 0.4
		mat.metallic = 0.2
		pedestal_base.set_surface_override_material(0, mat)


# ── UI Signal Connections ─────────────────────────────────────────────────────

func _connect_ui() -> void:
	# Mode toggle
	%ToolModeWorldBtn.pressed.connect(func() -> void: _switch_mode("world"))
	%ToolModeAvatarBtn.pressed.connect(func() -> void: _switch_mode("avatar"))

	# Gizmo tools
	%ToolGizmoMove.pressed.connect(func() -> void: _set_gizmo_mode("move"))
	%ToolGizmoRotate.pressed.connect(func() -> void: _set_gizmo_mode("rotate"))
	%ToolGizmoScale.pressed.connect(func() -> void: _set_gizmo_mode("scale"))
	%SnapToggleBtn.toggled.connect(func(t: bool) -> void: _snap_enabled = t)

	# World management
	%WorldDropdown.item_selected.connect(_on_world_dropdown_selected)
	%NewWorldBtn.pressed.connect(_new_world)
	%CopyWorldBtn.pressed.connect(_copy_world)
	%NewNameEdit.text_submitted.connect(func(_t: String) -> void: _new_world())

	# Primitive buttons
	for shape_name: String in SHAPE_SIZES:
		var btn := get_node_or_null("%Prim_" + shape_name.capitalize()) as Button
		if btn:
			btn.pressed.connect(_on_primitive_pressed.bind(shape_name))
	var light_btn: Button = %Prim_Light
	if light_btn:
		light_btn.pressed.connect(_on_primitive_pressed.bind("light"))

	%ImportBtn.pressed.connect(func() -> void: %ImportDialog.popup_centered())
	%ImportDialog.file_selected.connect(_import_model)

	# Hierarchy
	%HierarchyList.item_selected.connect(func(idx: int) -> void: _select(idx))
	%HierarchySearch.text_changed.connect(_on_hierarchy_search)
	%CreateObjectBtn.pressed.connect(_on_create_object_pressed)

	# Inspector sections
	%TransformHeader.pressed.connect(func() -> void: _toggle_section("transform"))
	%ShapeHeader.pressed.connect(func() -> void: _toggle_section("shape"))
	%LightHeader.pressed.connect(func() -> void: _toggle_section("light"))
	%ScriptsHeader.pressed.connect(func() -> void: _toggle_section("scripts"))

	# Inspector transform spinboxes
	for axis_key in ["PosX","PosY","PosZ","RotX","RotY","RotZ","ScaleX","ScaleY","ScaleZ"]:
		var sb := get_node_or_null("%Field_" + axis_key) as SpinBox
		if sb:
			sb.value_changed.connect(func(_v: float) -> void: _field_changed(false))

	# Shape fields
	(%Field_SizeX as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_SizeY as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_SizeZ as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_Color as ColorPickerButton).color_changed.connect(func(_c:Color)->void: _field_changed(true))
	(%Field_Body as OptionButton).item_selected.connect(func(_i:int)->void: _field_changed(true))
	(%Field_Mass as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))

	for choice in WorldFormat.BODIES:
		(%Field_Body as OptionButton).add_item(choice.capitalize())

	# Light fields
	(%Field_Energy as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_Range as SpinBox).value_changed.connect(func(_v:float)->void: _field_changed(true))
	(%Field_LightColor as ColorPickerButton).color_changed.connect(func(_c:Color)->void: _field_changed(true))

	# Scripts
	%AddScriptBtn.pressed.connect(_on_add_script_pressed)
	%ScriptFileDialog.file_selected.connect(_on_script_file_selected)
	%ScriptApplyBtn.pressed.connect(_on_script_apply)
	%ScriptDiscardBtn.pressed.connect(_on_script_discard)
	%ScriptCodeEdit.text_changed.connect(_on_script_code_changed)

	# Add Component menu
	%AddComponentBtn.pressed.connect(_on_add_component_pressed)

	# Actions
	%Field_Name.text_changed.connect(func(_t: String) -> void: _field_changed(false))
	%DeleteBtn.pressed.connect(_delete)
	%DuplicateBtn.pressed.connect(_duplicate)
	%FocusBtn.pressed.connect(_focus_camera)

	# Bottom bar
	%SaveBtn.pressed.connect(func() -> void: _save(true))
	%PackBtn.pressed.connect(_pack)
	%TestBtn.pressed.connect(_test)
	%SpawnHereBtn.pressed.connect(_spawn_here)

	# Bottom dock tabs
	%ConsoleClearBtn.pressed.connect(_clear_console)

	# Avatar Creator UI connections
	_connect_avatar_ui()


func _connect_avatar_ui() -> void:
	%AvatarPresetOption.item_selected.connect(_on_avatar_preset_chosen)
	%AvatarNameEdit.text_changed.connect(func(t: String) -> void: _avatar_data["name"] = t)
	%AvatarHeightSlider.value_changed.connect(func(v: float) -> void:
		_avatar_data["height"] = v
		%AvatarHeightVal.text = "%.2fx" % v
		_update_avatar_preview())

	# Colors
	%AvatarSkinColor.color_changed.connect(func(c: Color) -> void:
		_avatar_data["colors"]["skin"] = AvatarFormat.to_color_html(c)
		_avatar_data["head"]["color"] = AvatarFormat.to_color_html(c)
		_avatar_data["hands"]["color"] = AvatarFormat.to_color_html(c)
		_update_avatar_preview())
	%AvatarClothColor.color_changed.connect(func(c: Color) -> void:
		_avatar_data["colors"]["clothing"] = AvatarFormat.to_color_html(c)
		_avatar_data["body"]["color"] = AvatarFormat.to_color_html(c)
		_update_avatar_preview())
	%AvatarHairHatColor.color_changed.connect(func(c: Color) -> void:
		_avatar_data["colors"]["hair"] = AvatarFormat.to_color_html(c)
		_avatar_data["hair"]["color"] = AvatarFormat.to_color_html(c)
		_update_avatar_preview())
	%AvatarAccentColor.color_changed.connect(func(c: Color) -> void:
		_avatar_data["colors"]["accent"] = AvatarFormat.to_color_html(c)
		_update_avatar_preview())

	# Head & Face
	%AvatarHeadShape.item_selected.connect(func(idx: int) -> void:
		var shapes := AvatarFormat.SHAPES
		if idx < shapes.size():
			_avatar_data["head"]["shape"] = shapes[idx]
			_update_avatar_preview())
	%AvatarEyeStyle.item_selected.connect(func(idx: int) -> void:
		var styles := AvatarFormat.EYE_STYLES
		if idx < styles.size():
			_avatar_data["face"]["eyes"] = styles[idx]
			_update_avatar_preview())
	%AvatarNoseCheck.toggled.connect(func(t: bool) -> void:
		_avatar_data["face"]["nose"] = t
		_update_avatar_preview())
	%AvatarBlushCheck.toggled.connect(func(t: bool) -> void:
		_avatar_data["face"]["blush"] = t
		_update_avatar_preview())

	# Hats
	%AvatarHatStyle.item_selected.connect(func(idx: int) -> void:
		var hats := AvatarFormat.HAT_STYLES
		if idx < hats.size():
			_avatar_data["hair"]["type"] = hats[idx]
			_update_avatar_preview())

	# Body & Hands
	%AvatarBodyShape.item_selected.connect(func(idx: int) -> void:
		var shapes := ["capsule", "box", "cylinder"]
		if idx < shapes.size():
			_avatar_data["body"]["shape"] = shapes[idx]
			_update_avatar_preview())
	%AvatarHandShape.item_selected.connect(func(idx: int) -> void:
		var shapes := ["box", "sphere"]
		if idx < shapes.size():
			_avatar_data["hands"]["shape"] = shapes[idx]
			_update_avatar_preview())

	# Accessories
	for acc in ["backpack", "wings", "tail", "gem", "scarf"]:
		var cb := get_node_or_null("%AvatarAcc_" + acc.capitalize()) as CheckBox
		if cb:
			cb.toggled.connect(func(_t: bool) -> void: _sync_avatar_accessories())

	# Avatar actions
	%AvatarSaveBtn.pressed.connect(_on_avatar_save)
	%AvatarEquipBtn.pressed.connect(_on_avatar_equip)
	%AvatarTurntableToggle.toggled.connect(func(t: bool) -> void: _turntable_auto_rotate = t)


# ── Mode Switching ────────────────────────────────────────────────────────────

func _switch_mode(mode: String) -> void:
	_mode = mode
	var is_world := (mode == "world")

	# Toolbar highlights
	%ToolModeWorldBtn.button_pressed = is_world
	%ToolModeAvatarBtn.button_pressed = not is_world

	# 3D views
	$WorldRoot.visible = is_world
	$AvatarStudio.visible = not is_world
	$Highlight.visible = is_world and _selected >= 0

	# Left panel
	%WorldHierarchyBlock.visible = is_world
	%AvatarHierarchyBlock.visible = not is_world

	# Right inspector
	%InspectorContent.visible = is_world and _selected >= 0
	%InspectorEmpty.visible = is_world and _selected < 0
	%AvatarInspectorContent.visible = not is_world

	# Camera setup
	var camera: Camera3D = $Camera
	if is_world:
		camera.position = Vector3(0, 6, 12)
		camera.rotation = Vector3(deg_to_rad(-22), 0, 0)
		_log("info", "Switched to World Editor")
		_status("World Editor — Edit objects in 3D (WASD to fly, right-click to look)")
	else:
		_orbit_dist = 2.2
		_turntable_yaw = 0.0
		_turntable_pitch = 0.1
		_update_avatar_camera()
		_update_avatar_preview()
		_log("info", "Switched to Avatar Creator")
		_status("Avatar Creator — Customize your 3D character and set it active in-game")


func _set_gizmo_mode(m: String) -> void:
	_gizmo_mode = m
	%ToolGizmoMove.button_pressed = (m == "move")
	%ToolGizmoRotate.button_pressed = (m == "rotate")
	%ToolGizmoScale.button_pressed = (m == "scale")
	_status("Tool mode: %s" % m.capitalize())


# ── Process & Camera Controls ─────────────────────────────────────────────────

func _process(delta: float) -> void:
	if _mode == "world":
		var camera: Camera3D = $Camera
		var move := Vector3(
			float(Input.is_key_pressed(KEY_D)) - float(Input.is_key_pressed(KEY_A)),
			float(Input.is_key_pressed(KEY_E)) - float(Input.is_key_pressed(KEY_Q)),
			float(Input.is_key_pressed(KEY_S)) - float(Input.is_key_pressed(KEY_W))
		)
		if move != Vector3.ZERO:
			var speed := FLY_SPEED * (3.0 if Input.is_key_pressed(KEY_SHIFT) else 1.0)
			camera.global_position += camera.global_transform.basis * move.normalized() * speed * delta
	elif _mode == "avatar":
		if _turntable_auto_rotate:
			_turntable_yaw += delta * 0.8
			_update_avatar_camera()


func _update_avatar_camera() -> void:
	var camera: Camera3D = $Camera
	var target := Vector3(0.0, 0.45, 0.0)
	var offset := Vector3(
		sin(_turntable_yaw) * cos(_turntable_pitch) * _orbit_dist,
		sin(_turntable_pitch) * _orbit_dist,
		cos(_turntable_yaw) * cos(_turntable_pitch) * _orbit_dist
	)
	camera.global_position = target + offset
	camera.look_at(target, Vector3.UP)


# ── Focus Camera (Unity F key) ────────────────────────────────────────────────

func _focus_camera() -> void:
	if _selected < 0 or not _nodes.has(_selected):
		return
	var node: Node3D = _nodes[_selected]
	var center: Vector3 = node.global_position
	var camera: Camera3D = $Camera
	camera.global_position = center + Vector3(0, 2, 4)
	camera.look_at(center, Vector3.UP)
	_status("Focused on %s" % str(_objects()[_selected].get("name", "Object")))


# ── World Management ──────────────────────────────────────────────────────────

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
	_refresh_project_assets()


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


# ── Scene Rebuild ─────────────────────────────────────────────────────────────

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
	var filter := _hierarchy_filter.to_lower().strip_edges()
	var world_name: String = _world.get("name", _slug)
	var objects := _objects()

	for i in objects.size():
		var entry: Dictionary = objects[i]
		var obj_name := str(entry.get("name", "Object"))
		if filter != "" and not obj_name.to_lower().contains(filter):
			continue
		var kind := str(entry.get("shape", entry.get("model", "light")))
		var icon_str: String = SHAPE_ICONS.get(kind.get_file(), SHAPE_ICONS.get(kind, "📦"))
		if entry.has("model"):
			icon_str = SHAPE_ICONS["model"]
		elif entry.has("light"):
			icon_str = SHAPE_ICONS["light"]
		var label := "%s  %s" % [icon_str, obj_name]
		%HierarchyList.add_item(label)
		var item_idx: int = %HierarchyList.item_count - 1
		%HierarchyList.set_item_metadata(item_idx, i)

	%WorldNameLabel.text = world_name
	if _selected >= 0:
		for item_idx in %HierarchyList.item_count:
			if %HierarchyList.get_item_metadata(item_idx) == _selected:
				%HierarchyList.select(item_idx)
				break


func _on_hierarchy_search(text: String) -> void:
	_hierarchy_filter = text
	_refresh_hierarchy()


func _on_create_object_pressed() -> void:
	# Quick create a Box in front of the camera
	var camera: Camera3D = $Camera
	var spawn_pos := camera.global_position + camera.global_transform.basis * Vector3(0, 0, -4)
	_place({
		"name": "GameObject",
		"shape": "box",
		"size": [1.0, 1.0, 1.0],
		"color": "#9a9aa2",
		"roughness": 0.6,
		"body": "static"
	}, spawn_pos)


# ── Selection & Highlight ─────────────────────────────────────────────────────

func _select(index: int) -> void:
	_selected = index if index >= 0 and index < _objects().size() else -1
	_load_inspector()
	_highlight()
	if _selected >= 0:
		%SelectionLabel.text = "Selected: %s" % str(_objects()[_selected].get("name", "Object"))
	else:
		%SelectionLabel.text = "No selection"


func _highlight() -> void:
	var node: Node3D = _nodes.get(_selected)
	if node == null or _mode != "world":
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


# ── Input Handling ────────────────────────────────────────────────────────────

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseButton:
		var button: InputEventMouseButton = event
		if button.button_index == MOUSE_BUTTON_RIGHT:
			_looking = button.pressed
		elif button.button_index == MOUSE_BUTTON_LEFT:
			if _mode == "avatar":
				_looking = button.pressed
			else:
				if button.pressed:
					_on_click(button.position)
				else:
					_dragging = false
		elif button.button_index == MOUSE_BUTTON_WHEEL_UP:
			if _mode == "avatar":
				_orbit_dist = clampf(_orbit_dist - 0.15, 1.0, 5.0)
				_update_avatar_camera()
		elif button.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			if _mode == "avatar":
				_orbit_dist = clampf(_orbit_dist + 0.15, 1.0, 5.0)
				_update_avatar_camera()

	elif event is InputEventMouseMotion:
		var motion: InputEventMouseMotion = event
		if _mode == "avatar" and _looking:
			_turntable_yaw -= motion.relative.x * LOOK_SPEED * 1.5
			_turntable_pitch = clampf(_turntable_pitch + motion.relative.y * LOOK_SPEED * 1.5, -0.6, 0.9)
			_update_avatar_camera()
		elif _mode == "world":
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
	else:
		match key.keycode:
			KEY_DELETE: _delete()
			KEY_F: _focus_camera()
			KEY_W:
				if not _looking:
					_set_gizmo_mode("move")
			KEY_E:
				if not _looking:
					_set_gizmo_mode("rotate")
			KEY_R:
				if not _looking:
					_set_gizmo_mode("scale")


func _on_click(at: Vector2) -> void:
	var vp_rect := _viewport_rect()
	if not vp_rect.has_point(at):
		return
	if not _pending_place.is_empty():
		_place(_pending_place, _ground(at))
		return
	var hit := _pick(at)
	_select(hit)
	_dragging = hit >= 0


func _viewport_rect() -> Rect2:
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
	var snap := _snap_val if _snap_enabled else 0.01
	return Vector3(snappedf(at_ground.x, snap), 0.0, snappedf(at_ground.z, snap))


# ── Primitive Placement ───────────────────────────────────────────────────────

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
	_status("Click viewport to place %s" % shape_name)


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


# ── Transform Dragging ────────────────────────────────────────────────────────

func _drag(at: Vector2) -> void:
	if _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	var position := WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO)
	var snap := _snap_val if _snap_enabled else 0.01

	if _gizmo_mode == "rotate":
		var camera: Camera3D = $Camera
		var rot := WorldFormat.vec3(entry.get("rot", []), Vector3.ZERO)
		rot.y = snappedf(rot.y + 5.0, 15.0)
		entry["rot"] = WorldFormat.to_array(rot)
	elif _gizmo_mode == "scale":
		var sc := WorldFormat.vec3(entry.get("scale", []), Vector3.ONE)
		sc += Vector3(0.05, 0.05, 0.05)
		entry["scale"] = WorldFormat.to_array(sc)
	else:
		if Input.is_key_pressed(KEY_ALT):
			var camera: Camera3D = $Camera
			var plane := Plane(Vector3(camera.global_basis.z.x, 0.0, camera.global_basis.z.z).normalized(), position)
			var point: Variant = plane.intersects_ray(camera.project_ray_origin(at), camera.project_ray_normal(at))
			if point != null:
				position.y = snappedf(point.y, snap)
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


# ── Duplicate, Delete, Undo ───────────────────────────────────────────────────

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
	_status("Duplicated %s" % entry.get("name", "object"))


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


# ── Import ────────────────────────────────────────────────────────────────────

func _import_model(path: String) -> void:
	var result := StudioImport.into_world(path, _dir())
	if result.has("error"):
		_status(str(result["error"]))
		return
	_place({"name": str(result["model"]).get_file().get_basename(), "model": result["model"], "body": "static"},
		_ground(get_viewport().get_visible_rect().size * 0.5))
	_status("Imported %s" % result["model"])
	_refresh_project_assets()


# ── Inspector (World Mode) ────────────────────────────────────────────────────

func _toggle_section(key: String) -> void:
	_section_collapsed[key] = not _section_collapsed[key]
	_apply_section_visibility()


func _apply_section_visibility() -> void:
	var label_map := {
		"transform": "Transform",
		"shape":     "Shape / Mesh",
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
	var has := _selected >= 0 and _mode == "world"
	%InspectorEmpty.visible = not has and _mode == "world"
	%InspectorContent.visible = has and _mode == "world"

	if not has:
		_loading = false
		return

	var entry: Dictionary = _objects()[_selected]
	%Field_Name.text = str(entry.get("name", "Object"))

	# Transform
	var pos := WorldFormat.vec3(entry.get("pos", []), Vector3.ZERO)
	var rot := WorldFormat.vec3(entry.get("rot", []), Vector3.ZERO)
	var scale := WorldFormat.vec3(entry.get("scale", []), Vector3.ONE)
	_set_spin("PosX", pos.x); _set_spin("PosY", pos.y); _set_spin("PosZ", pos.z)
	_set_spin("RotX", rot.x); _set_spin("RotY", rot.y); _set_spin("RotZ", rot.z)
	_set_spin("ScaleX", scale.x); _set_spin("ScaleY", scale.y); _set_spin("ScaleZ", scale.z)

	var is_shape: bool = entry.has("shape")
	var is_light: bool = entry.has("light")
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

	_rebuild_scripts_ui(entry)
	_apply_section_visibility()
	_loading = false


func _set_spin(key: String, value: float) -> void:
	var node := get_node_or_null("%Field_" + key) as SpinBox
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


func _on_add_component_pressed() -> void:
	if _selected < 0:
		return
	var entry: Dictionary = _objects()[_selected]
	if not entry.has("light"):
		entry["light"] = {"color": "#ffb366", "energy": 2.0, "range": 8.0}
		_status("Added Light component")
		_field_changed(true)
	elif not entry.has("scripts"):
		entry["scripts"] = []
		_status("Added Scripts component")
		_field_changed(true)
	else:
		_status("Object already has all available components")


# ── Fire# Scripts UI ─────────────────────────────────────────────────────────

func _rebuild_scripts_ui(entry: Dictionary) -> void:
	for child in %ScriptRows.get_children():
		child.queue_free()

	var scripts: Array = entry.get("scripts", [])
	for i in scripts.size():
		var sc: Dictionary = scripts[i]
		_add_script_row(sc, i)


func _add_script_row(sc: Dictionary, idx: int) -> void:
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
		_:
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
		%ScriptErrorLabel.text = "✓ Script syntax valid"
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


# ── Avatar Creator UI & Logic ─────────────────────────────────────────────────

func _load_avatar_ui(data: Dictionary) -> void:
	_avatar_data = data.duplicate(true)
	%AvatarNameEdit.text = str(data.get("name", "MyAvatar"))

	var height: float = float(data.get("height", 1.0))
	%AvatarHeightSlider.value = height
	%AvatarHeightVal.text = "%.2fx" % height

	var colors: Dictionary = data.get("colors", {})
	%AvatarSkinColor.color = AvatarFormat.parse_color(colors.get("skin", AvatarFormat.DEFAULT_SKIN))
	%AvatarClothColor.color = AvatarFormat.parse_color(colors.get("clothing", AvatarFormat.DEFAULT_CLOTHING))
	%AvatarHairHatColor.color = AvatarFormat.parse_color(colors.get("hair", AvatarFormat.DEFAULT_HAIR))
	%AvatarAccentColor.color = AvatarFormat.parse_color(colors.get("accent", AvatarFormat.DEFAULT_ACCENT))

	var head: Dictionary = data.get("head", {})
	var h_shape := str(head.get("shape", "box"))
	var h_idx := AvatarFormat.SHAPES.find(h_shape)
	%AvatarHeadShape.selected = maxi(h_idx, 0)

	var face: Dictionary = data.get("face", {})
	var eye_style := str(face.get("eyes", "dots"))
	var e_idx := AvatarFormat.EYE_STYLES.find(eye_style)
	%AvatarEyeStyle.selected = maxi(e_idx, 0)
	%AvatarNoseCheck.button_pressed = bool(face.get("nose", true))
	%AvatarBlushCheck.button_pressed = bool(face.get("blush", false))

	var hair: Dictionary = data.get("hair", {})
	var hat_type := str(hair.get("type", "none"))
	var hat_idx := AvatarFormat.HAT_STYLES.find(hat_type)
	%AvatarHatStyle.selected = maxi(hat_idx, 0)

	var body: Dictionary = data.get("body", {})
	var b_shape := str(body.get("shape", "capsule"))
	var b_idx := ["capsule", "box", "cylinder"].find(b_shape)
	%AvatarBodyShape.selected = maxi(b_idx, 0)

	var hands: Dictionary = data.get("hands", {})
	var hand_shape := str(hands.get("shape", "box"))
	var hand_idx := ["box", "sphere"].find(hand_shape)
	%AvatarHandShape.selected = maxi(hand_idx, 0)

	var acc_types: Array = []
	for acc in data.get("accessories", []):
		if acc is Dictionary:
			acc_types.append(str(acc.get("type", "")))
	for acc_name in ["backpack", "wings", "tail", "gem", "scarf"]:
		var cb := get_node_or_null("%AvatarAcc_" + acc_name.capitalize()) as CheckBox
		if cb:
			cb.button_pressed = acc_types.has(acc_name)


func _sync_avatar_accessories() -> void:
	var list: Array = []
	var accent_col := AvatarFormat.to_color_html(%AvatarAccentColor.color)
	for acc_name in ["backpack", "wings", "tail", "gem", "scarf"]:
		var cb := get_node_or_null("%AvatarAcc_" + acc_name.capitalize()) as CheckBox
		if cb and cb.button_pressed:
			list.append({"type": acc_name, "color": accent_col})
	_avatar_data["accessories"] = list
	_update_avatar_preview()


func _update_avatar_preview() -> void:
	var anchor := $AvatarStudio/Pedestal/AvatarAnchor as Node3D
	if anchor == null:
		return
	for child in anchor.get_children():
		child.queue_free()
	_avatar_node = AvatarFormat.build_preview_avatar(_avatar_data)
	anchor.add_child(_avatar_node)


func _on_avatar_preset_chosen(idx: int) -> void:
	var presets := AvatarFormat.get_presets()
	var keys := presets.keys()
	if idx < keys.size():
		var chosen_name: String = keys[idx]
		_load_avatar_ui(presets[chosen_name])
		_update_avatar_preview()
		_status("Applied preset: %s" % chosen_name)


func _on_avatar_save() -> void:
	var name: String = %AvatarNameEdit.text.strip_edges()
	if name.is_empty():
		name = "MyAvatar"
	_avatar_data["name"] = name
	var err := AvatarFormat.save(_avatar_data, name)
	if err != "":
		_status("Failed to save avatar: %s" % err)
		_log("error", "Avatar save failed: %s" % err)
		return
	_status("Saved avatar '%s' ✓" % name)
	_log("info", "Avatar saved to user://avatars/%s.json" % AvatarFormat.avatar_slug(name))
	_refresh_saved_avatars()


func _on_avatar_equip() -> void:
	_on_avatar_save()
	var ok := AvatarFormat.set_active_avatar_data(_avatar_data)
	if ok:
		_status("Equipped '%s' as your active in-game avatar! ★" % str(_avatar_data["name"]))
		_log("info", "Active avatar set to: %s" % str(_avatar_data["name"]))
	else:
		_status("Could not set active avatar")


func _refresh_saved_avatars() -> void:
	%AvatarsList.clear()
	var avatars := AvatarFormat.list_avatars()
	var active_name := str(AvatarFormat.get_active_avatar().get("name", ""))
	for av_slug: String in avatars:
		var is_active := (av_slug == AvatarFormat.avatar_slug(active_name)) or (av_slug.to_lower() == active_name.to_lower())
		var label := "%s %s %s" % [
			"★" if is_active else "👤",
			av_slug.replace("-", " ").capitalize(),
			"(Active)" if is_active else ""
		]
		%AvatarsList.add_item(label)
		var idx: int = %AvatarsList.item_count - 1
		%AvatarsList.set_item_metadata(idx, av_slug)


func _refresh_project_assets() -> void:
	%ProjectList.clear()
	# Worlds
	for w in _world_slugs():
		%ProjectList.add_item("🌐 worlds/%s" % w)
	# Avatars
	for av in AvatarFormat.list_avatars():
		%ProjectList.add_item("👤 avatars/%s.json" % av)


# ── Console & Status ──────────────────────────────────────────────────────────

func _log(type: String, message: String) -> void:
	var icon := "ℹ"
	match type:
		"error": icon = "❌"
		"warn":  icon = "⚠"
		"info":  icon = "ℹ"
	var entry := "%s [%s] %s" % [icon, Time.get_time_string_from_system(), message]
	_console_logs.append(entry)
	if %ConsoleList != null:
		%ConsoleList.add_item(entry)
		%ConsoleList.ensure_current_is_visible()


func _clear_console() -> void:
	_console_logs.clear()
	if %ConsoleList != null:
		%ConsoleList.clear()


func _status(text: String) -> void:
	%StatusLabel.text = text
	print(text)


func _notification(what: int) -> void:
	if what == NOTIFICATION_WM_CLOSE_REQUEST and _dirty:
		_save(false)
