extends SceneTree

## Checks the world format without a window:
##   godot --headless --path client/godot --script res://tests/world_format_test.gd
## Run by `client/build.ps1 -Test`. The nasty-package cases are the point:
## worlds arrive from servers nobody verified (docs/decisions.md D3, D27).

var _failed := 0
var _checks := 0


func _initialize() -> void:
	var dir := "user://test-worlds"
	_rm(dir)
	_round_trip(dir)
	_validation()
	_builds(dir)
	_packaging(dir)
	_bad_packages(dir)
	_slugs()
	_shipped_hub()
	_shipped_worlds()
	_rm(dir)
	_cheap_collision()
	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)


func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)


func _round_trip(root: String) -> void:
	var dir := root.path_join("round-trip")
	var world := WorldFormat.new_world("My Place")
	_check(WorldFormat.write(world, dir) == "", "a new world saves")
	var read := WorldFormat.read(dir)
	_check(not read.has("error"), "and reads back: %s" % read.get("error", ""))
	_check(read.get("name") == "My Place", "with its name")
	_check((read.get("objects") as Array).size() == 1, "and its floor")
	_check(WorldFormat.read(root.path_join("nope")).has("error"), "a missing world is an error, not a crash")


func _validation() -> void:
	_check(WorldFormat.validate(WorldFormat.new_world("ok")) == "", "a fresh world is valid")
	_check(WorldFormat.validate({"format": 99, "kind": "world"}) != "", "a newer format is refused")
	_check(WorldFormat.validate({"kind": "avatar"}) != "", "an avatar is not a world")
	_check(WorldFormat.validate({"objects": "lots"}) != "", "objects must be a list")
	var crowd: Array = []
	for i in WorldFormat.MAX_OBJECTS + 1:
		crowd.append({"shape": "box"})
	_check(WorldFormat.validate({"objects": crowd}) != "", "too many objects is refused")


func _builds(root: String) -> void:
	var dir := root.path_join("built")
	var world := WorldFormat.new_world("Built")
	world["objects"] = [
		{"name": "Floor", "shape": "box", "size": [8, 0.5, 8], "body": "static", "pos": [0, 0, 0]},
		{"name": "Crate", "shape": "box", "size": [0.5, 0.5, 0.5], "body": "grabbable", "mass": 3.0},
		{"name": "Lamp", "light": {"energy": 2.0}, "pos": [1, 2, 1]},
		{"name": "Decor", "shape": "sphere", "body": "none"},
		{"name": "Nonsense", "shape": "dodecahedron"},
		{"name": "Hostile", "model": "assets/../../etc/passwd.glb"},
		{"name": "AlsoHostile", "model": "/etc/shadow.glb"},
		"not even an object",
	]
	WorldFormat.write(world, dir)
	var built := WorldFormat.build(world, dir)
	_check(built.get_node_or_null("WorldEnvironment") != null, "the sky is built")
	_check(built.get_node_or_null("Sun") != null, "the sun is built")
	_check(built.get_node_or_null("Spawn") != null, "the spawn point is built")
	# Four good objects; the unknown shape, both bad model paths and the
	# string are skipped without taking the world down with them.
	var objects := 0
	for child in built.get_children():
		if child.has_meta("world_index"):
			objects += 1
	_check(objects == 4, "bad objects are skipped, good ones built (got %d)" % objects)

	var crate := built.get_node_or_null("Crate")
	_check(crate is RigidBody3D and crate.is_in_group("grabbable"), "a grabbable is a rigid body in the group")
	_check(crate != null and crate.mass == 3.0, "with the mass it asked for")
	_check(built.get_node_or_null("Floor") is StaticBody3D, "a static object is a static body")
	_check(built.get_node_or_null("Decor") is MeshInstance3D, "a body of none is just a mesh")
	_check(built.get_node_or_null("Lamp") is OmniLight3D, "a light is a light")
	var shapes := 0
	for child in built.get_node("Floor").get_children():
		if child is CollisionShape3D and child.shape != null:
			shapes += 1
	_check(shapes == 1, "and has collision taken from its mesh")

	# The studio picks and highlights objects by these bounds.
	var floor_box: AABB = WorldFormat.bounds(built.get_node("Floor"))
	_check(floor_box.size.is_equal_approx(Vector3(8, 0.5, 8)), "bounds cover the whole object (%s)" % floor_box.size)
	_check(WorldFormat.bounds(built.get_node("Lamp")).size.length() > 0.0, "and a light still has some")
	built.free()


func _packaging(root: String) -> void:
	var dir := root.path_join("packed")
	var out := root.path_join("packed.hworld")
	var world := WorldFormat.new_world("Packed")
	WorldFormat.write(world, dir)
	_check(WorldFormat.pack(dir, out) == "", "a world packs")
	var back := root.path_join("unpacked")
	var problem := WorldFormat.unpack(out, back)
	_check(problem == "", "and unpacks: %s" % problem)
	_check(WorldFormat.read(back).get("name") == "Packed", "with the same world inside")


## Everything a hostile package could try.
func _bad_packages(root: String) -> void:
	var escape := root.path_join("escape.hworld")
	_write_zip(escape, {"world.json": "{}", "../escaped.json": "x"})
	_check(WorldFormat.unpack(escape, root.path_join("out1")) != "", "a path that climbs out is refused")
	_check(not FileAccess.file_exists(root.path_join("escaped.json")), "and nothing is written outside")

	var absolute := root.path_join("absolute.hworld")
	_write_zip(absolute, {"world.json": "{}", "/tmp/hearth-escaped.json": "x"})
	_check(WorldFormat.unpack(absolute, root.path_join("out2")) != "", "an absolute path is refused")

	var program := root.path_join("program.hworld")
	_write_zip(program, {"world.json": "{}", "assets/nasty.exe": "MZ"})
	_check(WorldFormat.unpack(program, root.path_join("out3")) != "", "a file type a world cannot hold is refused")

	var crowded := root.path_join("crowded.hworld")
	var many := {"world.json": "{}"}
	for i in WorldFormat.MAX_ENTRIES + 1:
		many["assets/%d.glb" % i] = "x"
	_write_zip(crowded, many)
	_check(WorldFormat.unpack(crowded, root.path_join("out4")) != "", "too many entries is refused")

	var empty := root.path_join("empty.hworld")
	_write_zip(empty, {"assets/thing.glb": "x"})
	_check(WorldFormat.unpack(empty, root.path_join("out5")) != "", "a package with no world.json is refused")

	_check(WorldFormat.unpack(root.path_join("not-a-zip.hworld"), root.path_join("out6")) != "",
		"a file that is not a package is refused")


func _write_zip(path: String, entries: Dictionary) -> void:
	var packer := ZIPPacker.new()
	if packer.open(path) != OK:
		_check(false, "could not write the test package %s" % path)
		return
	for name: String in entries:
		packer.start_file(name)
		packer.write_file(str(entries[name]).to_utf8_buffer())
		packer.close_file()
	packer.close()


func _slugs() -> void:
	_check(WorldFormat.slugify("My Place") == "my-place", "names become slugs")
	_check(WorldFormat.slugify("../etc/passwd") == "etcpasswd", "and cannot climb out")
	_check(WorldFormat.slugify("!!!") == "world", "and are never empty")
	_check(WorldFormat.slugify("A".repeat(80)).length() <= 32, "and stay short")


## The world that ships in the exe, which also becomes the server's hub.
func _shipped_hub() -> void:
	var hub := WorldFormat.read("res://worlds/hub")
	_check(not hub.has("error"), "the shipped hub world reads: %s" % hub.get("error", ""))
	if hub.has("error"):
		return
	var built := WorldFormat.build(hub, "res://worlds/hub")
	var objects := 0
	var grabbable := 0
	for child in built.get_children():
		if child.has_meta("world_index"):
			objects += 1
		if child.is_in_group("grabbable"):
			grabbable += 1
	_check(objects == (hub["objects"] as Array).size(), "and every object in it builds (%d)" % objects)
	# A count would break on every edit to the world; what has to hold is that
	# there is something to pick up and that the item flags reach the nodes.
	_check(grabbable >= 4, "with things to pick up (%d)" % grabbable)
	_check(built.get_node_or_null("Spawn") != null, "and a spawn point")
	_check(built.has_meta("kill_y"), "and a height you cannot fall below")

	var reaches := 0
	var locked := 0
	for child in built.get_children():
		if not child.is_in_group("grabbable"):
			continue
		_check(child.has_meta("reach") and child.has_meta("locked"), "%s carries its item flags" % child.name)
		if float(child.get_meta("reach")) > WorldFormat.DEFAULT_REACH:
			reaches += 1
		if bool(child.get_meta("locked")):
			locked += 1
	_check(reaches > 0, "something can be reached from further away (%d)" % reaches)
	_check(locked > 0, "and something cannot be taken out of your hands (%d)" % locked)
	built.free()


func _rm(path: String) -> void:
	if not DirAccess.dir_exists_absolute(path):
		return
	for name in DirAccess.get_files_at(path):
		DirAccess.remove_absolute(path.path_join(name))
	for name in DirAccess.get_directories_at(path):
		_rm(path.path_join(name))
	DirAccess.remove_absolute(path)


## Primitives must get primitive collision shapes. A concave triangle mesh per
## box cost thirty seconds of five-second frames on the first load of a world,
## so this is the regression that matters most in the whole file.
func _cheap_collision() -> void:
	var world := WorldFormat.new_world("shapes")
	world["objects"] = [
		{"name": "Box", "shape": "box", "size": [2, 1, 2], "pos": [0, 0, 0], "body": "static"},
		{"name": "Ball", "shape": "sphere", "size": [1, 1, 1], "pos": [4, 0, 0], "body": "static"},
		{"name": "Squashed", "shape": "sphere", "size": [4, 1, 4], "pos": [8, 0, 0], "body": "static"},
		{"name": "Pipe", "shape": "cylinder", "size": [1, 3, 1], "pos": [12, 0, 0], "body": "static"},
		{"name": "Ring", "shape": "torus", "size": [2, 1, 3], "pos": [16, 0, 0], "body": "static"},
	]
	var built := WorldFormat.build(world, "user://nowhere")
	var found := {}
	for child in built.get_children():
		for shape in child.get_children():
			if shape is CollisionShape3D:
				found[str(child.name)] = shape.shape
	_check(found.get("Box") is BoxShape3D, "a box gets a box shape")
	_check(found.get("Ball") is SphereShape3D, "a sphere gets a sphere shape")
	_check(found.get("Pipe") is CylinderShape3D, "a cylinder gets a cylinder shape")
	_check(found.get("Squashed") is ConvexPolygonShape3D, "a squashed sphere gets a convex hull")
	# A torus is the one primitive you can walk through the middle of.
	_check(found.get("Ring") is ConcavePolygonShape3D, "a torus still gets its triangles")
	built.free()


## Every world the client ships is packed into a brand new server by
## start.bat, so a broken one is a broken server rather than a broken level.
func _shipped_worlds() -> void:
	var worlds := DirAccess.get_directories_at("res://worlds")
	_check(worlds.size() >= 2, "more than one world ships with the client")
	for slug: String in worlds:
		var dir := "res://worlds/%s" % slug
		var world := WorldFormat.read(dir)
		_check(not world.has("error"), "%s reads" % slug)
		if world.has("error"):
			continue
		_check(WorldFormat.validate(world) == "", "%s validates" % slug)
		var built := WorldFormat.build(world, dir)
		_check(built.get_node_or_null("Spawn") != null, "%s has somewhere to stand" % slug)
		built.free()
