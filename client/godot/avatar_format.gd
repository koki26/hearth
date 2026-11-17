class_name AvatarFormat
extends RefCounted

## AvatarFormat — Reads, writes, validates, and builds Hearth custom avatars.
## Defines the avatar schema, built-in presets, persistence in user://avatars,
## and node building for runtime display in-game and in Hearth Studio.

const FORMAT := 1
const KIND := "avatar"
const AVATAR_EXTENSION := "json"
const MAX_ACCESSORIES := 16

const SHAPES := ["box", "sphere", "capsule", "cylinder"]
const EYE_STYLES := ["dots", "cartoon", "visor", "glasses", "none"]
const HAT_STYLES := [
	"none", "cat_ears", "bear_ears", "tophat", "cap",
	"beanie", "crown", "horns", "hair_tuft", "halo"
]
const ACCESSORY_TYPES := ["backpack", "wings", "tail", "gem", "scarf"]

const DEFAULT_SKIN := "#739ee6"
const DEFAULT_CLOTHING := "#2e4a8a"
const DEFAULT_ACCENT := "#f5c542"
const DEFAULT_HAIR := "#382414"


# ── Defaults & Presets ────────────────────────────────────────────────────────

static func default_avatar(avatar_name: String = "Classic") -> Dictionary:
	return {
		"format": FORMAT,
		"kind": KIND,
		"name": avatar_name,
		"height": 1.0,
		"colors": {
			"skin": DEFAULT_SKIN,
			"clothing": DEFAULT_CLOTHING,
			"accent": DEFAULT_ACCENT,
			"hair": DEFAULT_HAIR,
		},
		"head": {
			"shape": "box",
			"size": [0.2, 0.25, 0.22],
			"color": DEFAULT_SKIN,
		},
		"face": {
			"eyes": "dots",
			"nose": true,
			"blush": false,
		},
		"hair": {
			"type": "none",
			"color": DEFAULT_HAIR,
			"size": [0.22, 0.1, 0.24],
		},
		"body": {
			"shape": "capsule",
			"size": [0.17, 0.62, 0.17],
			"color": DEFAULT_CLOTHING,
		},
		"hands": {
			"shape": "box",
			"size": [0.08, 0.05, 0.12],
			"color": DEFAULT_SKIN,
		},
		"accessories": [],
		"model": "",
	}


static func get_presets() -> Dictionary:
	var presets := {}

	# 1. Classic Hearth
	presets["Classic"] = default_avatar("Classic")

	# 2. Cyber / Robot
	var robot := default_avatar("Cyber Robot")
	robot["colors"] = {
		"skin": "#22252e",
		"clothing": "#14161c",
		"accent": "#00f0ff",
		"hair": "#00f0ff",
	}
	robot["head"]["color"] = "#22252e"
	robot["head"]["shape"] = "box"
	robot["face"]["eyes"] = "visor"
	robot["face"]["nose"] = false
	robot["hair"]["type"] = "horns"
	robot["hair"]["color"] = "#00f0ff"
	robot["body"]["shape"] = "box"
	robot["body"]["color"] = "#14161c"
	robot["body"]["size"] = [0.26, 0.55, 0.2]
	robot["hands"]["color"] = "#22252e"
	robot["accessories"] = [{"type": "gem", "color": "#00f0ff"}]
	presets["Cyber Robot"] = robot

	# 3. Cozy Cat
	var cat := default_avatar("Cozy Cat")
	cat["colors"] = {
		"skin": "#fff2e0",
		"clothing": "#e6683b",
		"accent": "#ff99bb",
		"hair": "#e6683b",
	}
	cat["head"]["shape"] = "sphere"
	cat["head"]["size"] = [0.24, 0.24, 0.24]
	cat["head"]["color"] = "#fff2e0"
	cat["face"]["eyes"] = "cartoon"
	cat["face"]["blush"] = true
	cat["hair"]["type"] = "cat_ears"
	cat["hair"]["color"] = "#e6683b"
	cat["body"]["color"] = "#e6683b"
	cat["hands"]["color"] = "#fff2e0"
	cat["accessories"] = [{"type": "tail", "color": "#e6683b"}]
	presets["Cozy Cat"] = cat

	# 4. Wizard / Mage
	var wizard := default_avatar("Mystic Mage")
	wizard["colors"] = {
		"skin": "#e8d8c8",
		"clothing": "#3d1e6d",
		"accent": "#f5c542",
		"hair": "#6a4b9c",
	}
	wizard["head"]["shape"] = "sphere"
	wizard["head"]["color"] = "#e8d8c8"
	wizard["face"]["eyes"] = "glasses"
	wizard["hair"]["type"] = "tophat"
	wizard["hair"]["color"] = "#3d1e6d"
	wizard["body"]["shape"] = "capsule"
	wizard["body"]["color"] = "#3d1e6d"
	wizard["accessories"] = [{"type": "gem", "color": "#f5c542"}]
	presets["Mystic Mage"] = wizard

	# 5. Golden Royalty
	var royal := default_avatar("Royal Monarch")
	royal["colors"] = {
		"skin": "#f4dfcd",
		"clothing": "#8a1428",
		"accent": "#f5c542",
		"hair": "#f5c542",
	}
	royal["head"]["shape"] = "box"
	royal["head"]["color"] = "#f4dfcd"
	royal["face"]["eyes"] = "cartoon"
	royal["hair"]["type"] = "crown"
	royal["hair"]["color"] = "#f5c542"
	royal["body"]["color"] = "#8a1428"
	royal["accessories"] = [{"type": "wings", "color": "#f5c542"}]
	presets["Royal Monarch"] = royal

	# 6. Traveler / Adventurer
	var scout := default_avatar("Traveler")
	scout["colors"] = {
		"skin": "#dfc8b4",
		"clothing": "#46633b",
		"accent": "#c28938",
		"hair": "#543825",
	}
	scout["head"]["shape"] = "box"
	scout["head"]["color"] = "#dfc8b4"
	scout["face"]["eyes"] = "dots"
	scout["hair"]["type"] = "cap"
	scout["hair"]["color"] = "#46633b"
	scout["body"]["color"] = "#46633b"
	scout["accessories"] = [{"type": "backpack", "color": "#7a5435"}]
	presets["Traveler"] = scout

	# 7. Angelic / Halo
	var angel := default_avatar("Celestial")
	angel["colors"] = {
		"skin": "#f8f0ff",
		"clothing": "#e0d4f5",
		"accent": "#ffe066",
		"hair": "#ffffff",
	}
	angel["head"]["shape"] = "sphere"
	angel["head"]["color"] = "#f8f0ff"
	angel["hair"]["type"] = "halo"
	angel["hair"]["color"] = "#ffe066"
	angel["body"]["color"] = "#e0d4f5"
	angel["accessories"] = [{"type": "wings", "color": "#ffffff"}]
	presets["Celestial"] = angel

	return presets


# ── Validation ────────────────────────────────────────────────────────────────

static func validate(avatar: Dictionary) -> String:
	if int(avatar.get("format", 0)) > FORMAT:
		return "avatar requires a newer version (format %s)" % avatar.get("format")
	var kind := str(avatar.get("kind", KIND))
	if kind != KIND:
		return "this is a %s, not an avatar" % kind
	var name := str(avatar.get("name", "")).strip_edges()
	if name == "":
		return "avatar must have a name"
	if typeof(avatar.get("colors", {})) != TYPE_DICTIONARY:
		return "colors must be a dictionary"
	var height: float = float(avatar.get("height", 1.0))
	if height < 0.4 or height > 2.5:
		return "height must be between 0.4 and 2.5"
	return ""


# ── Persistence ───────────────────────────────────────────────────────────────

static func ensure_dirs() -> void:
	DirAccess.make_dir_recursive_absolute(Session.AVATARS)


static func avatar_slug(name: String) -> String:
	var clean := name.to_lower().strip_edges()
	var out := ""
	for c in clean:
		if (c >= "a" and c <= "z") or (c >= "0" and c <= "9") or c == "-" or c == "_":
			out += c
		elif c == " ":
			out += "-"
	return "avatar" if out.is_empty() else out


static func avatar_path(name: String) -> String:
	return Session.AVATARS.path_join(avatar_slug(name) + "." + AVATAR_EXTENSION)


static func save(avatar: Dictionary, custom_name: String = "") -> String:
	ensure_dirs()
	if custom_name != "":
		avatar["name"] = custom_name
	var err := validate(avatar)
	if err != "":
		return err
	avatar["format"] = FORMAT
	avatar["kind"] = KIND
	var path := avatar_path(str(avatar["name"]))
	var file := FileAccess.open(path, FileAccess.WRITE)
	if file == null:
		return "cannot open %s for writing" % path
	file.store_string(JSON.stringify(avatar, "  "))
	return ""


static func load_avatar(name: String) -> Dictionary:
	ensure_dirs()
	var path := avatar_path(name)
	if not FileAccess.file_exists(path):
		var presets := get_presets()
		if presets.has(name):
			return presets[name]
		return {"error": "avatar '%s' not found" % name}
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return {"error": "cannot read %s" % path}
	var parsed: Variant = JSON.parse_string(file.get_as_text())
	if typeof(parsed) != TYPE_DICTIONARY:
		return {"error": "invalid avatar JSON in %s" % path}
	var err := validate(parsed)
	if err != "":
		return {"error": err}
	return parsed


static func list_avatars() -> Array[String]:
	ensure_dirs()
	var result: Array[String] = []
	var dir := DirAccess.open(Session.AVATARS)
	if dir:
		dir.list_dir_begin()
		var file_name := dir.get_next()
		while file_name != "":
			if not dir.current_is_dir() and file_name.ends_with("." + AVATAR_EXTENSION):
				var basename := file_name.trim_suffix("." + AVATAR_EXTENSION)
				result.append(basename)
			file_name = dir.get_next()
	# Always ensure presets exist or are selectable
	for preset_name: String in get_presets():
		var slug := avatar_slug(preset_name)
		if not result.has(slug):
			result.append(slug)
	result.sort()
	return result


static func delete_avatar(name: String) -> bool:
	ensure_dirs()
	var path := avatar_path(name)
	if FileAccess.file_exists(path):
		return DirAccess.remove_absolute(path) == OK
	return false


static func get_active_avatar() -> Dictionary:
	ensure_dirs()
	if FileAccess.file_exists(Session.ACTIVE_AVATAR):
		var file := FileAccess.open(Session.ACTIVE_AVATAR, FileAccess.READ)
		if file != null:
			var parsed: Variant = JSON.parse_string(file.get_as_text())
			if typeof(parsed) == TYPE_DICTIONARY and validate(parsed) == "":
				return parsed
	# Fall back to Classic preset
	var def := default_avatar("Classic")
	set_active_avatar_data(def)
	return def


static func set_active_avatar_data(avatar: Dictionary) -> bool:
	ensure_dirs()
	var err := validate(avatar)
	if err != "":
		return false
	var file := FileAccess.open(Session.ACTIVE_AVATAR, FileAccess.WRITE)
	if file == null:
		return false
	file.store_string(JSON.stringify(avatar, "  "))
	return true


static func set_active_avatar_name(name: String) -> bool:
	var av := load_avatar(name)
	if av.has("error"):
		return false
	return set_active_avatar_data(av)


# ── Helpers for Color & Vectors ───────────────────────────────────────────────

static func parse_color(hex_or_color: Variant, fallback: Color = Color.WHITE) -> Color:
	if hex_or_color is Color:
		return hex_or_color
	var s := str(hex_or_color).strip_edges()
	if Color.html_is_valid(s):
		return Color.html(s)
	return fallback


static func to_color_html(col: Color) -> String:
	return "#" + col.to_html(false)


static func parse_vec3(arr: Variant, fallback: Vector3 = Vector3.ONE) -> Vector3:
	if arr is Vector3:
		return arr
	if arr is Array and (arr as Array).size() >= 3:
		return Vector3(float(arr[0]), float(arr[1]), float(arr[2]))
	return fallback


static func to_vec3_array(v: Vector3) -> Array[float]:
	return [snappedf(v.x, 0.001), snappedf(v.y, 0.001), snappedf(v.z, 0.001)]


# ── Node Generation & Visuals ─────────────────────────────────────────────────

static func create_material(color: Color, roughness: float = 0.6, metallic: float = 0.0) -> StandardMaterial3D:
	var mat := StandardMaterial3D.new()
	mat.albedo_color = color
	mat.roughness = roughness
	mat.metallic = metallic
	return mat


static func build_primitive_mesh(shape: String, size: Vector3) -> Mesh:
	match shape:
		"sphere":
			var sph := SphereMesh.new()
			sph.radius = size.x * 0.5
			sph.height = size.y
			return sph
		"capsule":
			var cap := CapsuleMesh.new()
			cap.radius = size.x * 0.5
			cap.height = size.y
			return cap
		"cylinder":
			var cyl := CylinderMesh.new()
			cyl.top_radius = size.x * 0.5
			cyl.bottom_radius = size.x * 0.5
			cyl.height = size.y
			return cyl
		_:  # "box" or default
			var box := BoxMesh.new()
			box.size = size
			return box


## Applies an avatar dictionary onto an existing Avatar scene node (avatar.tscn).
static func apply_to_avatar_node(root: Node3D, data: Dictionary) -> void:
	if root == null:
		return

	var colors: Dictionary = data.get("colors", {})
	var skin_col := parse_color(colors.get("skin", DEFAULT_SKIN))
	var cloth_col := parse_color(colors.get("clothing", DEFAULT_CLOTHING))
	var accent_col := parse_color(colors.get("accent", DEFAULT_ACCENT))
	var hair_col := parse_color(colors.get("hair", DEFAULT_HAIR))

	var height_scale: float = float(data.get("height", 1.0))

	# 1. Head
	var head_node: MeshInstance3D = root.get_node_or_null("Head")
	if head_node != null:
		var head_data: Dictionary = data.get("head", {})
		var h_shape := str(head_data.get("shape", "box"))
		var h_size := parse_vec3(head_data.get("size", [0.2, 0.25, 0.22]))
		var h_col := parse_color(head_data.get("color", skin_col))

		head_node.mesh = build_primitive_mesh(h_shape, h_size)
		head_node.set_surface_override_material(0, create_material(h_col, 0.65))

		# Clear custom features that may already exist
		for child_name in ["EyesFeature", "HatFeature", "BlushFeature"]:
			var old := head_node.get_node_or_null(child_name)
			if old != null:
				old.queue_free()

		# Face / Eyes
		var face_data: Dictionary = data.get("face", {})
		var eye_style := str(face_data.get("eyes", "dots"))
		var eyes_node := _build_eyes(eye_style, h_size, accent_col)
		if eyes_node != null:
			eyes_node.name = "EyesFeature"
			head_node.add_child(eyes_node)

		# Nose
		var nose_node: MeshInstance3D = head_node.get_node_or_null("Nose")
		if nose_node != null:
			nose_node.visible = bool(face_data.get("nose", true))
			var n_col := h_col.darkened(0.12)
			nose_node.set_surface_override_material(0, create_material(n_col, 0.7))
			nose_node.position.z = -(h_size.z * 0.5 + 0.01)

		# Blush
		if bool(face_data.get("blush", false)):
			var blush := _build_blush(h_size)
			blush.name = "BlushFeature"
			head_node.add_child(blush)

		# Hat / Hair
		var hair_data: Dictionary = data.get("hair", {})
		var hat_type := str(hair_data.get("type", "none"))
		var hat_col := parse_color(hair_data.get("color", hair_col))
		if hat_type != "none":
			var hat_node := _build_hat(hat_type, h_size, hat_col, accent_col)
			if hat_node != null:
				hat_node.name = "HatFeature"
				head_node.add_child(hat_node)

	# 2. Body
	var body_node: MeshInstance3D = root.get_node_or_null("Body")
	if body_node != null:
		var body_data: Dictionary = data.get("body", {})
		var b_shape := str(body_data.get("shape", "capsule"))
		var b_size := parse_vec3(body_data.get("size", [0.17, 0.62, 0.17]))
		var b_col := parse_color(body_data.get("color", cloth_col))

		body_node.mesh = build_primitive_mesh(b_shape, b_size)
		body_node.set_surface_override_material(0, create_material(b_col, 0.7))

		# Clear old accessories
		for child in body_node.get_children():
			if child.name.begins_with("Acc_"):
				child.queue_free()

		# Attach accessories to body
		var accs: Array = data.get("accessories", [])
		for acc_item in accs:
			if acc_item is Dictionary:
				var acc_type := str(acc_item.get("type", ""))
				var acc_col := parse_color(acc_item.get("color", accent_col))
				var acc_node := _build_accessory(acc_type, b_size, acc_col)
				if acc_node != null:
					acc_node.name = "Acc_" + acc_type
					body_node.add_child(acc_node)

	# 3. Hands
	var hand_data: Dictionary = data.get("hands", {})
	var hand_shape := str(hand_data.get("shape", "box"))
	var hand_size := parse_vec3(hand_data.get("size", [0.08, 0.05, 0.12]))
	var hand_col := parse_color(hand_data.get("color", skin_col))

	for hand_name in ["Left", "Right"]:
		var hand_node: MeshInstance3D = root.get_node_or_null(hand_name)
		if hand_node != null:
			hand_node.mesh = build_primitive_mesh(hand_shape, hand_size)
			hand_node.set_surface_override_material(0, create_material(hand_col, 0.7))

	# 4. Overall scale
	root.scale = Vector3.ONE * height_scale


static func _build_eyes(style: String, head_size: Vector3, accent_col: Color) -> Node3D:
	if style == "none":
		return null
	var parent := Node3D.new()
	var z_front := -(head_size.z * 0.5 + 0.005)
	var y_eye := 0.02

	match style:
		"visor":
			var visor := MeshInstance3D.new()
			var bm := BoxMesh.new()
			bm.size = Vector3(head_size.x * 0.85, 0.045, 0.02)
			visor.mesh = bm
			var mat := StandardMaterial3D.new()
			mat.albedo_color = accent_col
			mat.emission_enabled = true
			mat.emission = accent_col
			mat.emission_energy_multiplier = 2.0
			visor.set_surface_override_material(0, mat)
			visor.position = Vector3(0.0, y_eye, z_front)
			parent.add_child(visor)
		"glasses":
			var mat := create_material(Color(0.15, 0.15, 0.2), 0.3, 0.8)
			for x_off in [-head_size.x * 0.26, head_size.x * 0.26]:
				var rim := MeshInstance3D.new()
				var torus := TorusMesh.new()
				torus.inner_radius = 0.025
				torus.outer_radius = 0.035
				rim.mesh = torus
				rim.rotation.x = PI * 0.5
				rim.set_surface_override_material(0, mat)
				rim.position = Vector3(x_off, y_eye, z_front - 0.01)
				parent.add_child(rim)
			# Bridge
			var bridge := MeshInstance3D.new()
			var b_mesh := BoxMesh.new()
			b_mesh.size = Vector3(0.05, 0.008, 0.01)
			bridge.mesh = b_mesh
			bridge.set_surface_override_material(0, mat)
			bridge.position = Vector3(0.0, y_eye + 0.01, z_front - 0.01)
			parent.add_child(bridge)
		"cartoon":
			# White background discs + black pupil
			for x_off in [-head_size.x * 0.24, head_size.x * 0.24]:
				var white := MeshInstance3D.new()
				var wm := SphereMesh.new()
				wm.radius = 0.024
				wm.height = 0.015
				white.mesh = wm
				white.set_surface_override_material(0, create_material(Color.WHITE, 0.4))
				white.position = Vector3(x_off, y_eye, z_front)
				parent.add_child(white)

				var pupil := MeshInstance3D.new()
				var pm := SphereMesh.new()
				pm.radius = 0.013
				pm.height = 0.008
				pupil.mesh = pm
				pupil.set_surface_override_material(0, create_material(Color(0.08, 0.08, 0.12), 0.3))
				pupil.position = Vector3(x_off, y_eye, z_front - 0.007)
				parent.add_child(pupil)
		_:  # "dots"
			var dot_mat := create_material(Color(0.1, 0.1, 0.14), 0.5)
			for x_off in [-head_size.x * 0.22, head_size.x * 0.22]:
				var dot := MeshInstance3D.new()
				var sm := SphereMesh.new()
				sm.radius = 0.015
				sm.height = 0.015
				dot.mesh = sm
				dot.set_surface_override_material(0, dot_mat)
				dot.position = Vector3(x_off, y_eye, z_front)
				parent.add_child(dot)
	return parent


static func _build_blush(head_size: Vector3) -> Node3D:
	var parent := Node3D.new()
	var z_front := -(head_size.z * 0.5 + 0.004)
	var mat := create_material(Color(1.0, 0.45, 0.6, 0.8), 0.8)
	for x_off in [-head_size.x * 0.32, head_size.x * 0.32]:
		var disc := MeshInstance3D.new()
		var sm := SphereMesh.new()
		sm.radius = 0.02
		sm.height = 0.008
		disc.mesh = sm
		disc.set_surface_override_material(0, mat)
		disc.position = Vector3(x_off, -0.04, z_front)
		parent.add_child(disc)
	return parent


static func _build_hat(type: String, head_size: Vector3, hat_col: Color, accent_col: Color) -> Node3D:
	var parent := Node3D.new()
	var y_top := head_size.y * 0.5

	match type:
		"cat_ears":
			var mat := create_material(hat_col, 0.7)
			var inner_mat := create_material(Color(1.0, 0.7, 0.78), 0.8)
			for x_mult in [-1.0, 1.0]:
				var ear := MeshInstance3D.new()
				var cyl := CylinderMesh.new()
				cyl.top_radius = 0.001
				cyl.bottom_radius = 0.038
				cyl.height = 0.075
				ear.mesh = cyl
				ear.set_surface_override_material(0, mat)
				ear.position = Vector3(x_mult * head_size.x * 0.35, y_top + 0.03, -0.01)
				ear.rotation = Vector3(deg_to_rad(-8), 0, deg_to_rad(-x_mult * 18))
				parent.add_child(ear)
		"bear_ears":
			var mat := create_material(hat_col, 0.7)
			for x_mult in [-1.0, 1.0]:
				var ear := MeshInstance3D.new()
				var sph := SphereMesh.new()
				sph.radius = 0.038
				sph.height = 0.065
				ear.mesh = sph
				ear.set_surface_override_material(0, mat)
				ear.position = Vector3(x_mult * head_size.x * 0.4, y_top + 0.025, 0.0)
				parent.add_child(ear)
		"tophat":
			# Brim
			var brim := MeshInstance3D.new()
			var cyl1 := CylinderMesh.new()
			cyl1.top_radius = head_size.x * 0.65
			cyl1.bottom_radius = head_size.x * 0.65
			cyl1.height = 0.015
			brim.mesh = cyl1
			brim.set_surface_override_material(0, create_material(hat_col, 0.6))
			brim.position = Vector3(0.0, y_top + 0.008, 0.0)
			parent.add_child(brim)
			# Crown
			var crown := MeshInstance3D.new()
			var cyl2 := CylinderMesh.new()
			cyl2.top_radius = head_size.x * 0.42
			cyl2.bottom_radius = head_size.x * 0.45
			cyl2.height = 0.14
			crown.mesh = cyl2
			crown.set_surface_override_material(0, create_material(hat_col, 0.6))
			crown.position = Vector3(0.0, y_top + 0.08, 0.0)
			parent.add_child(crown)
			# Ribbon
			var ribbon := MeshInstance3D.new()
			var cyl3 := CylinderMesh.new()
			cyl3.top_radius = head_size.x * 0.455
			cyl3.bottom_radius = head_size.x * 0.46
			cyl3.height = 0.022
			ribbon.mesh = cyl3
			ribbon.set_surface_override_material(0, create_material(accent_col, 0.5))
			ribbon.position = Vector3(0.0, y_top + 0.025, 0.0)
			parent.add_child(ribbon)
		"cap":
			# Dome
			var dome := MeshInstance3D.new()
			var sph := SphereMesh.new()
			sph.radius = head_size.x * 0.52
			sph.height = head_size.y * 0.45
			dome.mesh = sph
			dome.set_surface_override_material(0, create_material(hat_col, 0.7))
			dome.position = Vector3(0.0, y_top, -0.01)
			parent.add_child(dome)
			# Visor bill
			var bill := MeshInstance3D.new()
			var bm := BoxMesh.new()
			bm.size = Vector3(head_size.x * 0.8, 0.012, 0.09)
			bill.mesh = bm
			bill.set_surface_override_material(0, create_material(hat_col.darkened(0.15), 0.7))
			bill.position = Vector3(0.0, y_top - 0.03, -(head_size.z * 0.5 + 0.04))
			bill.rotation.x = deg_to_rad(10)
			parent.add_child(bill)
		"crown":
			var crown := MeshInstance3D.new()
			var cyl := CylinderMesh.new()
			cyl.top_radius = head_size.x * 0.46
			cyl.bottom_radius = head_size.x * 0.4
			cyl.height = 0.075
			crown.mesh = cyl
			var mat := create_material(accent_col, 0.25, 0.85)
			crown.set_surface_override_material(0, mat)
			crown.position = Vector3(0.0, y_top + 0.04, 0.0)
			parent.add_child(crown)
		"halo":
			var halo := MeshInstance3D.new()
			var torus := TorusMesh.new()
			torus.inner_radius = head_size.x * 0.35
			torus.outer_radius = head_size.x * 0.44
			halo.mesh = torus
			var mat := StandardMaterial3D.new()
			mat.albedo_color = accent_col
			mat.emission_enabled = true
			mat.emission = accent_col
			mat.emission_energy_multiplier = 3.0
			halo.set_surface_override_material(0, mat)
			halo.position = Vector3(0.0, y_top + 0.12, 0.0)
			halo.rotation.x = deg_to_rad(-8)
			parent.add_child(halo)
		"horns":
			var mat := create_material(hat_col, 0.4)
			for x_mult in [-1.0, 1.0]:
				var horn := MeshInstance3D.new()
				var cyl := CylinderMesh.new()
				cyl.top_radius = 0.002
				cyl.bottom_radius = 0.028
				cyl.height = 0.11
				horn.mesh = cyl
				horn.set_surface_override_material(0, mat)
				horn.position = Vector3(x_mult * head_size.x * 0.35, y_top + 0.045, -0.02)
				horn.rotation = Vector3(deg_to_rad(-18), 0, deg_to_rad(-x_mult * 32))
				parent.add_child(horn)
		"beanie":
			var beanie := MeshInstance3D.new()
			var sph := SphereMesh.new()
			sph.radius = head_size.x * 0.54
			sph.height = head_size.y * 0.58
			beanie.mesh = sph
			beanie.set_surface_override_material(0, create_material(hat_col, 0.8))
			beanie.position = Vector3(0.0, y_top + 0.01, 0.0)
			parent.add_child(beanie)
			# Pompom
			var pom := MeshInstance3D.new()
			var p_sph := SphereMesh.new()
			p_sph.radius = 0.028
			p_sph.height = 0.056
			pom.mesh = p_sph
			pom.set_surface_override_material(0, create_material(accent_col, 0.9))
			pom.position = Vector3(0.0, y_top + head_size.y * 0.32, 0.0)
			parent.add_child(pom)
		"hair_tuft":
			var tuft := MeshInstance3D.new()
			var cap := CapsuleMesh.new()
			cap.radius = 0.03
			cap.height = 0.09
			tuft.mesh = cap
			tuft.set_surface_override_material(0, create_material(hat_col, 0.6))
			tuft.position = Vector3(0.0, y_top + 0.025, -head_size.z * 0.25)
			tuft.rotation = Vector3(deg_to_rad(35), 0, deg_to_rad(15))
			parent.add_child(tuft)
	return parent


static func _build_accessory(type: String, body_size: Vector3, color: Color) -> Node3D:
	var node := Node3D.new()
	var z_back := body_size.z * 0.5 + 0.03
	var mat := create_material(color, 0.6)

	match type:
		"backpack":
			var pack := MeshInstance3D.new()
			var bm := BoxMesh.new()
			bm.size = Vector3(body_size.x * 0.75, body_size.y * 0.45, 0.12)
			pack.mesh = bm
			pack.set_surface_override_material(0, mat)
			pack.position = Vector3(0.0, 0.02, z_back + 0.04)
			node.add_child(pack)
		"wings":
			for x_mult in [-1.0, 1.0]:
				var wing := MeshInstance3D.new()
				var bm := BoxMesh.new()
				bm.size = Vector3(0.24, 0.14, 0.015)
				wing.mesh = bm
				wing.set_surface_override_material(0, create_material(color, 0.4))
				wing.position = Vector3(x_mult * 0.18, 0.08, z_back)
				wing.rotation = Vector3(deg_to_rad(10), deg_to_rad(x_mult * -24), deg_to_rad(x_mult * 18))
				node.add_child(wing)
		"tail":
			var tail := MeshInstance3D.new()
			var cap := CapsuleMesh.new()
			cap.radius = 0.03
			cap.height = 0.22
			tail.mesh = cap
			tail.set_surface_override_material(0, mat)
			tail.position = Vector3(0.0, -body_size.y * 0.35, z_back + 0.06)
			tail.rotation = Vector3(deg_to_rad(45), 0, 0)
			node.add_child(tail)
		"gem":
			var gem := MeshInstance3D.new()
			var sph := SphereMesh.new()
			sph.radius = 0.04
			sph.height = 0.07
			gem.mesh = sph
			var g_mat := StandardMaterial3D.new()
			g_mat.albedo_color = color
			g_mat.emission_enabled = true
			g_mat.emission = color
			g_mat.emission_energy_multiplier = 2.5
			gem.set_surface_override_material(0, g_mat)
			gem.position = Vector3(body_size.x * 0.7, body_size.y * 0.3, 0.0)
			node.add_child(gem)
		"scarf":
			var scarf := MeshInstance3D.new()
			var tor := TorusMesh.new()
			tor.inner_radius = body_size.x * 0.52
			tor.outer_radius = body_size.x * 0.68
			scarf.mesh = tor
			scarf.set_surface_override_material(0, mat)
			scarf.position = Vector3(0.0, body_size.y * 0.44, 0.0)
			node.add_child(scarf)
	return node


## Builds a standalone preview avatar Node3D complete with Head, Body, and Hands.
static func build_preview_avatar(data: Dictionary) -> Node3D:
	var root := Node3D.new()
	root.name = "AvatarPreview"

	var head := MeshInstance3D.new()
	head.name = "Head"
	root.add_child(head)

	var nose := MeshInstance3D.new()
	nose.name = "Nose"
	var sm := SphereMesh.new()
	sm.radius = 0.03
	sm.height = 0.06
	nose.mesh = sm
	nose.position = Vector3(0, 0, -0.12)
	head.add_child(nose)

	var body := MeshInstance3D.new()
	body.name = "Body"
	body.position = Vector3(0, -0.5, 0)
	root.add_child(body)

	var left := MeshInstance3D.new()
	left.name = "Left"
	left.position = Vector3(-0.28, -0.35, 0.0)
	root.add_child(left)

	var right := MeshInstance3D.new()
	right.name = "Right"
	right.position = Vector3(0.28, -0.35, 0.0)
	root.add_child(right)

	apply_to_avatar_node(root, data)
	return root
