class_name WorldFormat
extends RefCounted

## Reads, writes, packs and builds a Hearth world. The spec is in
## docs/WORLD_FORMAT.md; keep the two in sync.
##
## The same functions serve the studio, which authors worlds, and the game,
## which loads them from a server. A world from a server is untrusted input:
## every limit below is a trust boundary, not a nicety.

const FORMAT := 1
const WORLD_FILE := "world.json"
const ASSETS := "assets"
const PACK_EXTENSION := "hworld"

const MAX_JSON_BYTES := 8 * 1024 * 1024
const MAX_OBJECTS := 5000
const MAX_ENTRIES := 2000
const MAX_UNPACKED_BYTES := 256 * 1024 * 1024
const MAX_MODEL_BYTES := 200 * 1024 * 1024
const MAX_EXTENT := 4096.0
const MAX_SCALE := 256.0
## How far a hand may be from a grabbable before it can take it, when the
## object does not say. Metres.
const DEFAULT_REACH := 0.35
const MAX_REACH := 8.0
## Fall below the floor by this much and you are put back at the spawn, unless
## the world names its own `kill_y`.
const DEFAULT_KILL_DROP := 24.0

const SHAPES := ["box", "sphere", "cylinder", "plane", "torus"]
const BODIES := ["none", "static", "grabbable"]
## Only models the studio has already converted, so the game never parses obj
## or fbx, whatever a server claims to be sending (docs/decisions.md D26).
const MODEL_PATTERN := "^assets/[A-Za-z0-9_. -]+\\.glb$"


## The world a "New world" starts from: a floor you can stand on.
static func new_world(world_name: String) -> Dictionary:
	return {
		"format": FORMAT,
		"kind": "world",
		"name": world_name,
		"spawn": {"pos": [0.0, 0.0, 0.0], "yaw": 0.0},
		"sky": default_sky(),
		"objects": [
			{
				"name": "Floor", "shape": "box", "size": [16.0, 0.5, 16.0],
				"color": "#424248", "roughness": 0.55,
				"pos": [0.0, -0.25, 0.0], "rot": [0.0, 0.0, 0.0], "scale": [1.0, 1.0, 1.0],
				"body": "static",
			},
		],
	}


static func default_sky() -> Dictionary:
	return {
		"top": "#171c38", "horizon": "#806166", "ground": "#0d0d14",
		"ambient": "#666c99", "fog": 0.012,
		"sun": {"color": "#aec2ff", "energy": 0.35, "angles": [-40.0, 30.0]},
	}


## Reads `<dir>/world.json`. The result holds "error" when it cannot be used.
static func read(dir: String) -> Dictionary:
	var path := dir.path_join(WORLD_FILE)
	if not FileAccess.file_exists(path):
		return {"error": "no %s in %s" % [WORLD_FILE, dir]}
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return {"error": "cannot read %s" % path}
	if file.get_length() > MAX_JSON_BYTES:
		return {"error": "%s is over %d bytes" % [WORLD_FILE, MAX_JSON_BYTES]}
	var parsed: Variant = JSON.parse_string(file.get_as_text())
	if typeof(parsed) != TYPE_DICTIONARY:
		return {"error": "%s is not a JSON object" % WORLD_FILE}
	var world: Dictionary = parsed
	var problem := validate(world)
	if problem != "":
		return {"error": problem}
	return world


static func write(world: Dictionary, dir: String) -> String:
	var problem := validate(world)
	if problem != "":
		return problem
	var error := DirAccess.make_dir_recursive_absolute(dir)
	if error != OK and not DirAccess.dir_exists_absolute(dir):
		return "cannot create %s" % dir
	world["format"] = FORMAT
	var file := FileAccess.open(dir.path_join(WORLD_FILE), FileAccess.WRITE)
	if file == null:
		return "cannot write %s in %s" % [WORLD_FILE, dir]
	file.store_string(JSON.stringify(world, "  "))
	return ""


## "" when the world is usable. Only the parts a loader relies on are
## checked: unknown keys are left alone, so a newer studio still works here.
static func validate(world: Dictionary) -> String:
	if int(world.get("format", 0)) > FORMAT:
		return "this world needs a newer Hearth (format %s)" % world.get("format")
	var kind := str(world.get("kind", "world"))
	if kind != "world":
		return "this is a %s, not a world" % kind
	if typeof(world.get("objects", [])) != TYPE_ARRAY:
		return "objects must be a list"
	var objects: Array = world.get("objects", [])
	if objects.size() > MAX_OBJECTS:
		return "%d objects is over the limit of %d" % [objects.size(), MAX_OBJECTS]
	return ""


## Builds the scene. Objects that make no sense are skipped with a warning:
## one bad entry must not cost you the whole world.
static func build(world: Dictionary, dir: String) -> Node3D:
	var root := Node3D.new()
	root.name = "World"
	root.add_child(_sky(world.get("sky", {}) as Dictionary))
	root.add_child(_sun(world.get("sky", {}) as Dictionary))

	var spawn := Marker3D.new()
	spawn.name = "Spawn"
	var at: Dictionary = world.get("spawn", {})
	spawn.position = vec3(at.get("pos", []), Vector3.ZERO)
	spawn.rotation.y = deg_to_rad(_number(at.get("yaw", 0.0), 0.0))
	root.add_child(spawn)
	# Fall past this and you are put back at the spawn. Relative to the spawn
	# by default, so a world never has to think about it.
	root.set_meta("kill_y", _number(
		world.get("kill_y", spawn.position.y - DEFAULT_KILL_DROP),
		spawn.position.y - DEFAULT_KILL_DROP,
	))

	var objects: Array = world.get("objects", [])
	for index in objects.size():
		if typeof(objects[index]) != TYPE_DICTIONARY:
			push_warning("world: skipped an object that is not an object")
			continue
		var node := build_object(objects[index], dir)
		if node != null:
			# The studio maps its edits back onto the nodes with this.
			node.set_meta("world_index", index)
			root.add_child(node)
	return root


static func build_object(entry: Dictionary, dir: String) -> Node3D:
	var node: Node3D = null
	if entry.has("light"):
		node = _light(entry.get("light", {}) as Dictionary)
	elif entry.has("shape"):
		node = _shape(entry)
	elif entry.has("model"):
		node = _model(entry, dir)
	else:
		push_warning("world: an object has no shape, model or light")
		return null
	if node == null:
		return null
	node.name = StringName(str(entry.get("name", "Object")).validate_node_name())
	node.position = vec3(entry.get("pos", []), Vector3.ZERO).clampf(-MAX_EXTENT, MAX_EXTENT)
	var rot := vec3(entry.get("rot", []), Vector3.ZERO)
	node.rotation = Vector3(deg_to_rad(rot.x), deg_to_rad(rot.y), deg_to_rad(rot.z))
	var scale := vec3(entry.get("scale", []), Vector3.ONE).clampf(0.001, MAX_SCALE)
	node.scale = scale
	return node


static func _shape(entry: Dictionary) -> Node3D:
	var kind := str(entry.get("shape", ""))
	var size := vec3(entry.get("size", []), Vector3.ONE).clampf(0.001, MAX_EXTENT)
	var mesh: Mesh = null
	match kind:
		"box":
			var box := BoxMesh.new()
			box.size = size
			mesh = box
		"sphere":
			var sphere := SphereMesh.new()
			sphere.radius = size.x * 0.5
			sphere.height = size.y
			mesh = sphere
		"cylinder":
			var cylinder := CylinderMesh.new()
			cylinder.top_radius = size.x * 0.5
			cylinder.bottom_radius = size.z * 0.5
			cylinder.height = size.y
			mesh = cylinder
		"plane":
			var plane := PlaneMesh.new()
			plane.size = Vector2(size.x, size.z)
			mesh = plane
		"torus":
			var torus := TorusMesh.new()
			torus.inner_radius = size.x * 0.5
			torus.outer_radius = maxf(size.z * 0.5, size.x * 0.5 + 0.01)
			mesh = torus
		_:
			push_warning("world: unknown shape %s" % kind)
			return null
	var visual := MeshInstance3D.new()
	visual.mesh = mesh
	visual.material_override = _material(entry)
	return _with_body(visual, entry)


static func _model(entry: Dictionary, dir: String) -> Node3D:
	var relative := str(entry.get("model", ""))
	if not RegEx.create_from_string(MODEL_PATTERN).search(relative):
		push_warning("world: %s is not an allowed model path" % relative)
		return null
	var scene := load_model(dir.path_join(relative))
	if scene == null:
		return null
	return _with_body(scene, entry)


## Loads one glTF binary at runtime. Returns null rather than failing loudly:
## a world may name a model that did not survive the trip.
static func load_model(path: String) -> Node3D:
	if not FileAccess.file_exists(path):
		push_warning("world: no model at %s" % path)
		return null
	if FileAccess.open(path, FileAccess.READ).get_length() > MAX_MODEL_BYTES:
		push_warning("world: %s is over the model size limit" % path)
		return null
	var document := GLTFDocument.new()
	var state := GLTFState.new()
	# Keep textures in memory; the default wants to write them beside the file.
	state.handle_binary_image = GLTFState.HANDLE_BINARY_EMBED_AS_UNCOMPRESSED
	if document.append_from_file(path, state) != OK:
		push_warning("world: %s is not a readable glTF file" % path)
		return null
	var scene := document.generate_scene(state)
	if scene == null:
		return null
	var node := scene as Node3D
	if node == null:
		scene.queue_free()
		return null
	return node


## Wraps a visual in whatever body its `body` asks for, with collision shapes
## taken from its meshes.
static func _with_body(visual: Node3D, entry: Dictionary) -> Node3D:
	var body := str(entry.get("body", "static"))
	if not BODIES.has(body):
		body = "static"
	if body == "none":
		return visual
	var holder: PhysicsBody3D
	if body == "grabbable":
		var rigid := RigidBody3D.new()
		rigid.mass = clampf(_number(entry.get("mass", 1.0), 1.0), 0.01, 1000.0)
		rigid.add_to_group("grabbable")
		# Read back by main.gd when deciding what a hand can take, and whether
		# it may be taken out of somebody else's hands.
		rigid.set_meta("reach", clampf(_number(entry.get("reach", DEFAULT_REACH), DEFAULT_REACH), 0.0, MAX_REACH))
		rigid.set_meta("locked", bool(entry.get("locked", false)))
		holder = rigid
	else:
		holder = StaticBody3D.new()
	holder.collision_layer = 1
	holder.collision_mask = 1
	holder.add_child(visual)
	visual.position = Vector3.ZERO
	for shape in _shapes_of(visual, body == "grabbable"):
		holder.add_child(shape)
	return holder


static func _shapes_of(visual: Node3D, convex: bool) -> Array[CollisionShape3D]:
	var shapes: Array[CollisionShape3D] = []
	for mesh_instance in _meshes(visual):
		var mesh := mesh_instance.mesh
		if mesh == null:
			continue
		var shape := CollisionShape3D.new()
		shape.shape = _shape_for(mesh, convex)
		if shape.shape == null:
			continue
		shape.transform = relative_transform(visual, mesh_instance)
		shapes.append(shape)
	return shapes


## The cheapest shape that still matches the mesh.
##
## A concave triangle mesh is the most expensive shape there is, so: a box,
## a sphere and a cylinder have exact primitive shapes, anything else convex
## gets a hull. Only a torus, a plane and a glTF model -- which can be
## any shape at all, including concave -- still need the real triangles, and a
## grabbable never does, because you carry it rather than walk through it.
static func _shape_for(mesh: Mesh, convex: bool) -> Shape3D:
	if mesh is BoxMesh:
		var box := BoxShape3D.new()
		box.size = (mesh as BoxMesh).size
		return box
	if mesh is SphereMesh:
		var ball := mesh as SphereMesh
		if is_equal_approx(ball.height, ball.radius * 2.0):
			var sphere := SphereShape3D.new()
			sphere.radius = ball.radius
			return sphere
		# Squashed: still convex, so a hull is exact enough and stays cheap.
		return mesh.create_convex_shape()
	if mesh is CylinderMesh:
		var tube := mesh as CylinderMesh
		if is_equal_approx(tube.top_radius, tube.bottom_radius):
			var cylinder := CylinderShape3D.new()
			cylinder.radius = tube.top_radius
			cylinder.height = tube.height
			return cylinder
		return mesh.create_convex_shape() # A cone.
	return mesh.create_convex_shape() if convex else mesh.create_trimesh_shape()


## Where `node` sits inside `root`, walking the parents rather than reading
## global transforms: during a build nothing is in the tree yet.
static func relative_transform(root: Node3D, node: Node3D) -> Transform3D:
	var combined := Transform3D.IDENTITY
	var current := node
	while current != null and current != root:
		combined = current.transform * combined
		current = current.get_parent() as Node3D
	return combined


static func _meshes(node: Node) -> Array[MeshInstance3D]:
	var found: Array[MeshInstance3D] = []
	if node is MeshInstance3D:
		found.append(node)
	for child in node.get_children():
		found.append_array(_meshes(child))
	return found


static func _material(entry: Dictionary) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = _color(entry.get("color", ""), Color(0.6, 0.6, 0.62))
	material.roughness = clampf(_number(entry.get("roughness", 0.6), 0.6), 0.0, 1.0)
	material.metallic = clampf(_number(entry.get("metallic", 0.0), 0.0), 0.0, 1.0)
	if entry.has("emission"):
		material.emission_enabled = true
		material.emission = _color(entry.get("emission", ""), material.albedo_color)
		material.emission_energy_multiplier = clampf(_number(entry.get("emission_energy", 1.0), 1.0), 0.0, 32.0)
	return material


static func _light(light: Dictionary) -> Node3D:
	var omni := OmniLight3D.new()
	omni.light_color = _color(light.get("color", ""), Color(1.0, 0.8, 0.6))
	omni.light_energy = clampf(_number(light.get("energy", 1.0), 1.0), 0.0, 16.0)
	omni.omni_range = clampf(_number(light.get("range", 6.0), 6.0), 0.1, 200.0)
	omni.shadow_enabled = bool(light.get("shadow", false))
	return omni


static func _sky(sky: Dictionary) -> WorldEnvironment:
	var material := ProceduralSkyMaterial.new()
	material.sky_top_color = _color(sky.get("top", ""), Color(0.09, 0.11, 0.22))
	material.sky_horizon_color = _color(sky.get("horizon", ""), Color(0.5, 0.38, 0.4))
	material.ground_bottom_color = _color(sky.get("ground", ""), Color(0.05, 0.05, 0.08))
	material.ground_horizon_color = material.sky_horizon_color
	var environment := Environment.new()
	environment.background_mode = Environment.BG_SKY
	environment.sky = Sky.new()
	environment.sky.sky_material = material
	environment.ambient_light_source = Environment.AMBIENT_SOURCE_SKY
	environment.ambient_light_color = _color(sky.get("ambient", ""), Color(0.4, 0.44, 0.6))
	environment.ambient_light_sky_contribution = 0.6
	environment.ambient_light_energy = 0.7
	environment.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	var fog := clampf(_number(sky.get("fog", 0.0), 0.0), 0.0, 1.0)
	environment.fog_enabled = fog > 0.0
	environment.fog_density = fog
	environment.fog_light_color = material.sky_horizon_color
	environment.fog_sky_affect = 0.3
	var holder := WorldEnvironment.new()
	holder.name = "WorldEnvironment"
	holder.environment = environment
	return holder


static func _sun(sky: Dictionary) -> DirectionalLight3D:
	var sun_data: Dictionary = sky.get("sun", {})
	var angles := _vec2(sun_data.get("angles", []), Vector2(-40.0, 30.0))
	var sun := DirectionalLight3D.new()
	sun.name = "Sun"
	sun.rotation = Vector3(deg_to_rad(angles.x), deg_to_rad(angles.y), 0.0)
	sun.light_color = _color(sun_data.get("color", ""), Color(0.68, 0.76, 1.0))
	sun.light_energy = clampf(_number(sun_data.get("energy", 0.5), 0.5), 0.0, 16.0)
	sun.shadow_enabled = true
	# Godot's default is four shadow splits over 100 m, which is four shadow
	# renders per eye. Two over 70 m is half the cost and no visible difference
	# in a world you walk across.
	sun.directional_shadow_mode = DirectionalLight3D.SHADOW_PARALLEL_2_SPLITS
	sun.directional_shadow_max_distance = 70.0
	return sun


# --- packing ---------------------------------------------------------------

## Zips a world folder into one `.hworld` file.
static func pack(dir: String, out_file: String) -> String:
	var world := read(dir)
	if world.has("error"):
		return str(world["error"])
	var packer := ZIPPacker.new()
	if packer.open(out_file) != OK:
		return "cannot write %s" % out_file
	var error := _put(packer, dir.path_join(WORLD_FILE), WORLD_FILE)
	if error == "":
		for asset in list_assets(dir):
			error = _put(packer, dir.path_join(ASSETS).path_join(asset), "%s/%s" % [ASSETS, asset])
			if error != "":
				break
	packer.close()
	if error != "":
		DirAccess.remove_absolute(out_file)
	return error


static func _put(packer: ZIPPacker, source: String, name: String) -> String:
	var file := FileAccess.open(source, FileAccess.READ)
	if file == null:
		return "cannot read %s" % source
	if packer.start_file(name) != OK:
		return "cannot add %s" % name
	packer.write_file(file.get_buffer(file.get_length()))
	packer.close_file()
	return ""


## Unpacks a `.hworld` into `dir`. A world file comes off the network, so each
## entry is checked before anything is written: no paths outside the folder, no
## file types the loader does not read, and a cap on how much it may expand to.
static func unpack(file: String, dir: String) -> String:
	var reader := ZIPReader.new()
	if reader.open(file) != OK:
		return "%s is not a readable world package" % file.get_file()
	var names := reader.get_files()
	if names.size() > MAX_ENTRIES:
		reader.close()
		return "the package holds %d files, over the limit of %d" % [names.size(), MAX_ENTRIES]
	var model := RegEx.create_from_string(MODEL_PATTERN)
	var total := 0
	var wanted: Array[String] = []
	for name in names:
		if name.ends_with("/"):
			continue
		if name != WORLD_FILE and not model.search(name):
			reader.close()
			return "the package holds %s, which a world may not contain" % name
		total += reader.read_file(name).size()
		if total > MAX_UNPACKED_BYTES:
			reader.close()
			return "the package unpacks to more than %d bytes" % MAX_UNPACKED_BYTES
		wanted.append(name)
	if not wanted.has(WORLD_FILE):
		reader.close()
		return "the package has no %s" % WORLD_FILE

	DirAccess.make_dir_recursive_absolute(dir.path_join(ASSETS))
	for name in wanted:
		var out := FileAccess.open(dir.path_join(name), FileAccess.WRITE)
		if out == null:
			reader.close()
			return "cannot write %s into %s" % [name, dir]
		out.store_buffer(reader.read_file(name))
	reader.close()
	var world := read(dir)
	return str(world.get("error", ""))


static func list_assets(dir: String) -> PackedStringArray:
	var found := PackedStringArray()
	if not DirAccess.dir_exists_absolute(dir.path_join(ASSETS)):
		return found
	var model := RegEx.create_from_string(MODEL_PATTERN)
	for name in DirAccess.get_files_at(dir.path_join(ASSETS)):
		if model.search("%s/%s" % [ASSETS, name]):
			found.append(name)
	return found


# --- small helpers ---------------------------------------------------------

static func _number(value: Variant, fallback: float) -> float:
	if typeof(value) != TYPE_FLOAT and typeof(value) != TYPE_INT:
		return fallback
	var number := float(value)
	return fallback if is_nan(number) or is_inf(number) else number


static func vec3(value: Variant, fallback: Vector3) -> Vector3:
	if typeof(value) != TYPE_ARRAY or (value as Array).size() < 3:
		return fallback
	var list: Array = value
	return Vector3(_number(list[0], fallback.x), _number(list[1], fallback.y), _number(list[2], fallback.z))


static func _vec2(value: Variant, fallback: Vector2) -> Vector2:
	if typeof(value) != TYPE_ARRAY or (value as Array).size() < 2:
		return fallback
	var list: Array = value
	return Vector2(_number(list[0], fallback.x), _number(list[1], fallback.y))


static func _color(value: Variant, fallback: Color) -> Color:
	var text := str(value)
	return Color.html(text) if Color.html_is_valid(text) else fallback


static func to_array(v: Vector3) -> Array:
	return [snappedf(v.x, 0.001), snappedf(v.y, 0.001), snappedf(v.z, 0.001)]


## A folder and package name a server will accept as a world slug.
static func slugify(text: String) -> String:
	var slug := ""
	for character in text.strip_edges().to_lower():
		if character >= "a" and character <= "z" or character >= "0" and character <= "9" or character == "_":
			slug += character
		elif character == " " or character == "-":
			slug += "-"
	slug = slug.lstrip("-").rstrip("-").substr(0, 32)
	return slug if slug != "" else "world"


## The visible extent of a built object, for the studio's pick and highlight.
static func bounds(node: Node3D) -> AABB:
	var box := AABB()
	var first := true
	for mesh_instance in _meshes(node):
		var transformed := relative_transform(node, mesh_instance) * mesh_instance.get_aabb()
		box = transformed if first else box.merge(transformed)
		first = false
	if first:
		box = AABB(Vector3(-0.25, -0.25, -0.25), Vector3.ONE * 0.5) # lights have no mesh
	return box
