class_name StudioImport
extends RefCounted

## Brings a model into a world. Only the studio does this, and everything
## leaves as a `.glb`, so the game's loader never parses obj or fbx and a
## world from a stranger's server cannot feed those parsers
## (docs/decisions.md D26).

const MAX_SOURCE_BYTES := 200 * 1024 * 1024
const MAX_VERTICES := 2_000_000
const FBX_HELP := "FBX needs the FBX2glTF converter. Put %s next to this program or in %s, then import again."


## Copies or converts `source` into `<world_dir>/assets` and returns
## {"model": "assets/<name>.glb"}, or {"error": ...}.
static func into_world(source: String, world_dir: String) -> Dictionary:
	if not FileAccess.file_exists(source):
		return {"error": "no file at %s" % source}
	var size := FileAccess.open(source, FileAccess.READ).get_length()
	if size > MAX_SOURCE_BYTES:
		return {"error": "%s is over the %d MB import limit" % [source.get_file(), MAX_SOURCE_BYTES / 1048576]}

	var extension := source.get_extension().to_lower()
	var assets := world_dir.path_join(WorldFormat.ASSETS)
	DirAccess.make_dir_recursive_absolute(assets)
	var target := assets.path_join("%s.glb" % _asset_name(source.get_file().get_basename(), assets))

	var problem := ""
	match extension:
		"glb":
			problem = _copy_glb(source, target)
		"gltf":
			problem = _reexport(source, target)
		"obj":
			problem = _from_obj(source, target)
		"fbx":
			problem = _from_fbx(source, target)
		_:
			problem = "%s files are not supported: use glb, gltf, obj or fbx" % extension
	if problem != "":
		DirAccess.remove_absolute(target)
		return {"error": problem}
	# Nothing enters a world until the loader itself can read it.
	if WorldFormat.load_model(target) == null:
		DirAccess.remove_absolute(target)
		return {"error": "%s did not survive the import" % source.get_file()}
	return {"model": "%s/%s" % [WorldFormat.ASSETS, target.get_file()]}


## A slug-safe name that does not already exist in the folder.
static func _asset_name(wanted: String, assets: String) -> String:
	var base := ""
	for character in wanted.to_lower():
		base += character if character.is_valid_identifier() or character.is_valid_int() else "_"
	base = base.lstrip("_")
	if base == "":
		base = "model"
	var name := base
	var n := 2
	while FileAccess.file_exists(assets.path_join("%s.glb" % name)):
		name = "%s_%d" % [base, n]
		n += 1
	return name


static func _copy_glb(source: String, target: String) -> String:
	if DirAccess.copy_absolute(source, target) != OK:
		return "could not copy %s" % source.get_file()
	return ""


## Loads a model Godot understands and writes it back out as one `.glb`.
static func _reexport(source: String, target: String) -> String:
	var scene := WorldFormat.load_model(source)
	if scene == null:
		return "%s is not a readable glTF file" % source.get_file()
	var problem := _write_glb(scene, target)
	scene.queue_free()
	return problem


static func _write_glb(scene: Node, target: String) -> String:
	var document := GLTFDocument.new()
	var state := GLTFState.new()
	if document.append_from_scene(scene, state) != OK:
		return "could not convert the model"
	if document.write_to_filesystem(state, target) != OK:
		return "could not write %s" % target.get_file()
	return ""


static func _from_fbx(source: String, target: String) -> String:
	var tool := _find_fbx_tool()
	if tool == "":
		return FBX_HELP % [fbx_tool_name(), ProjectSettings.globalize_path("user://tools")]
	# The converter writes <out>.glb, so hand it the name without one.
	var out := target.get_basename()
	var output: Array = []
	var code := OS.execute(tool, ["--binary", "--embed-textures", "--input",
		ProjectSettings.globalize_path(source), "--output", ProjectSettings.globalize_path(out)], output, true)
	if code != 0:
		return "the FBX converter failed: %s" % "\n".join(output).substr(0, 300)
	if not FileAccess.file_exists(target):
		return "the FBX converter wrote no %s" % target.get_file()
	return ""


## facebookincubator/FBX2glTF, the converter Godot's own FBX importer uses.
static func fbx_tool_name() -> String:
	return "FBX2glTF.exe" if OS.get_name() == "Windows" else "FBX2glTF"


static func _find_fbx_tool() -> String:
	for directory in [OS.get_executable_path().get_base_dir(), ProjectSettings.globalize_path("user://tools")]:
		var path: String = directory.path_join(fbx_tool_name())
		if FileAccess.file_exists(path):
			return path
	return ""


# --- Wavefront OBJ ---------------------------------------------------------

## A small obj reader: vertices, normals, UVs and faces, with the albedo
## colour and texture from the .mtl beside it. No animation, no groups, no
## smoothing - a prop made in any modeller comes through, which is the point.
static func _from_obj(source: String, target: String) -> String:
	var file := FileAccess.open(source, FileAccess.READ)
	if file == null:
		return "could not read %s" % source.get_file()
	var positions: Array[Vector3] = []
	var normals: Array[Vector3] = []
	var uvs: Array[Vector2] = []
	var surfaces: Dictionary = {} # material name -> Array of [p, uv, n] index triples
	var current := ""
	var materials := {}
	while not file.eof_reached():
		var parts := file.get_line().strip_edges().split(" ", false)
		if parts.is_empty():
			continue
		match parts[0]:
			"v":
				positions.append(_obj_vec3(parts))
			"vn":
				normals.append(_obj_vec3(parts))
			"vt":
				uvs.append(Vector2(_obj_float(parts, 1), 1.0 - _obj_float(parts, 2)))
			"usemtl":
				current = parts[1] if parts.size() > 1 else ""
			"mtllib":
				if parts.size() > 1:
					materials = _read_mtl(source.get_base_dir().path_join(parts[1]))
			"f":
				if positions.size() > MAX_VERTICES:
					return "%s has more than %d vertices" % [source.get_file(), MAX_VERTICES]
				var face: Array = []
				for i in range(1, parts.size()):
					face.append(_obj_corner(parts[i], positions.size(), uvs.size(), normals.size()))
				# Fan the polygon into triangles.
				for i in range(1, face.size() - 1):
					var triangle: Array = surfaces.get(current, [])
					triangle.append_array([face[0], face[i], face[i + 1]])
					surfaces[current] = triangle
	if positions.is_empty() or surfaces.is_empty():
		return "%s holds no faces" % source.get_file()

	var mesh := ArrayMesh.new()
	for material_name: String in surfaces:
		var tool := SurfaceTool.new()
		tool.begin(Mesh.PRIMITIVE_TRIANGLES)
		for corner: Array in surfaces[material_name]:
			if corner[1] >= 0 and corner[1] < uvs.size():
				tool.set_uv(uvs[corner[1]])
			if corner[2] >= 0 and corner[2] < normals.size():
				tool.set_normal(normals[corner[2]])
			if corner[0] < 0 or corner[0] >= positions.size():
				return "%s names a vertex it does not have" % source.get_file()
			tool.add_vertex(positions[corner[0]])
		if normals.is_empty():
			tool.generate_normals()
		tool.index()
		tool.set_material(materials.get(material_name, StandardMaterial3D.new()))
		tool.commit(mesh)

	var holder := MeshInstance3D.new()
	holder.mesh = mesh
	holder.name = target.get_file().get_basename()
	var problem := _write_glb(holder, target)
	holder.queue_free()
	return problem


static func _obj_vec3(parts: PackedStringArray) -> Vector3:
	return Vector3(_obj_float(parts, 1), _obj_float(parts, 2), _obj_float(parts, 3))


static func _obj_float(parts: PackedStringArray, index: int) -> float:
	if index >= parts.size() or not parts[index].is_valid_float():
		return 0.0
	return float(parts[index])


## "3/4/5", "3//5" and "3" all appear; obj counts from 1 and allows negatives.
static func _obj_corner(text: String, points: int, uvs: int, normals: int) -> Array:
	var bits := text.split("/")
	return [
		_obj_index(bits[0] if bits.size() > 0 else "", points),
		_obj_index(bits[1] if bits.size() > 1 else "", uvs),
		_obj_index(bits[2] if bits.size() > 2 else "", normals),
	]


static func _obj_index(text: String, count: int) -> int:
	if not text.is_valid_int():
		return -1
	var index := text.to_int()
	return index + count if index < 0 else index - 1


static func _read_mtl(path: String) -> Dictionary:
	var materials := {}
	var file := FileAccess.open(path, FileAccess.READ)
	if file == null:
		return materials
	var current := ""
	while not file.eof_reached():
		var parts := file.get_line().strip_edges().split(" ", false)
		if parts.is_empty():
			continue
		match parts[0]:
			"newmtl":
				current = parts[1] if parts.size() > 1 else ""
				materials[current] = StandardMaterial3D.new()
			"Kd":
				if materials.has(current):
					materials[current].albedo_color = Color(
						_obj_float(parts, 1), _obj_float(parts, 2), _obj_float(parts, 3))
			"map_Kd":
				if materials.has(current) and parts.size() > 1:
					var texture := _load_texture(path.get_base_dir().path_join(parts[parts.size() - 1]))
					if texture != null:
						materials[current].albedo_texture = texture
	return materials


static func _load_texture(path: String) -> ImageTexture:
	var image := Image.new()
	if not FileAccess.file_exists(path) or image.load(path) != OK:
		return null
	return ImageTexture.create_from_image(image)
