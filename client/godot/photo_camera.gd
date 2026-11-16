class_name PhotoCamera
extends Node3D

## A camera you hold: a body, a lens, and a screen on the back showing what
## the lens sees. Pull the trigger (or click, on the desktop) and it takes a
## picture, which lands as a PNG in Pictures/Hearth.
##
## The picture is the lens's own render at full HD, not a crop of your view:
## you frame it with the screen on the back the way you would a real camera,
## and what you see in the headset has nothing to do with it.
##
## Colours here are the camera's own -- it is a thing in the world, not
## interface -- except the accent ring, which says whose camera it is.

## Taken and saved. `path` is empty when saving failed.
signal taken(path: String, picture: Texture2D)

## The live screen, and the picture.
const PREVIEW := Vector2i(480, 270)
const PHOTO := Vector2i(1920, 1080)
## The camera's own body is drawn on this layer only, and its lens leaves the
## layer out: otherwise the first thing in every picture is the inside of the
## lens.
const BODY_LAYER := 1 << 19
const SIZE := Vector3(0.15, 0.092, 0.044)
const FOV := 62.0

## Where pictures go. Empty means Pictures/Hearth; tests point it elsewhere.
var into := ""
## Turned round: the lens looks back at whoever holds it.
var selfie := false

var _viewport: SubViewport
var _eye: Camera3D
var _lens: Marker3D
var _back: Marker3D
var _screen: StandardMaterial3D
var _flash := 0.0
var _busy := false


## Where pictures are kept.
static func folder(override := "") -> String:
	if override != "":
		return override
	var pictures := OS.get_system_dir(OS.SYSTEM_DIR_PICTURES)
	if pictures == "":
		return ProjectSettings.globalize_path("user://photos")
	return pictures.path_join("Hearth")


## How many pictures are in `folder`.
static func count(override := "") -> int:
	var found := 0
	for file in DirAccess.get_files_at(folder(override)):
		if file.get_extension().to_lower() == "png":
			found += 1
	return found


func _ready() -> void:
	_build_body()
	_viewport = SubViewport.new()
	_viewport.size = PREVIEW
	_viewport.msaa_3d = Viewport.MSAA_4X
	_viewport.render_target_update_mode = SubViewport.UPDATE_ALWAYS
	add_child(_viewport)
	_eye = Camera3D.new()
	_eye.fov = FOV
	_eye.near = 0.03
	_eye.cull_mask = 0xFFFFF & ~BODY_LAYER
	_viewport.add_child(_eye)
	_eye.current = true
	_screen.albedo_texture = _viewport.get_texture()


func _process(delta: float) -> void:
	_eye.global_transform = (_back if selfie else _lens).global_transform
	if _flash > 0.0:
		_flash = maxf(0.0, _flash - delta * 3.0)
		_screen.emission_energy_multiplier = _flash * 4.0


## Takes a picture. Asynchronous: the lens renders once more at full size
## before anything is read back, and `taken` says when it is done.
func shoot() -> void:
	if _busy:
		return
	_busy = true
	_viewport.size = PHOTO
	# One frame to draw at the new size, one to be sure it was read from.
	await RenderingServer.frame_post_draw
	await RenderingServer.frame_post_draw
	if not is_inside_tree():
		return
	var image := _viewport.get_texture().get_image()
	_viewport.size = PREVIEW
	_flash = 1.0
	var path := _save(image)
	_busy = false
	taken.emit(path, ImageTexture.create_from_image(image))


func _save(image: Image) -> String:
	var dir := folder(into)
	DirAccess.make_dir_recursive_absolute(dir)
	var stamp := Time.get_datetime_string_from_system().replace(":", "-").replace("T", "_")
	var path := dir.path_join("Hearth_%s.png" % stamp)
	var again := 2
	while FileAccess.file_exists(path):
		path = dir.path_join("Hearth_%s_%d.png" % [stamp, again])
		again += 1
	if image == null or image.save_png(path) != OK:
		return ""
	return path


func _build_body() -> void:
	var shell := _material(Color(0.1, 0.095, 0.095), 0.45, 0.25)
	var grip := _material(Color(0.045, 0.042, 0.042), 0.9, 0.0)
	var metal := _material(Color(0.55, 0.53, 0.52), 0.3, 0.9)
	var glass := _material(Color(0.02, 0.025, 0.04), 0.05, 0.8)

	_add(_box(SIZE), shell, Vector3.ZERO)
	# A leatherette band round the middle, the thing that makes a box a camera.
	_add(_box(Vector3(SIZE.x + 0.002, SIZE.y * 0.42, SIZE.z + 0.002)), grip, Vector3(0, -0.008, 0))
	# The viewfinder hump and the shutter button on top.
	_add(_box(Vector3(0.042, 0.016, 0.032)), shell, Vector3(-0.034, SIZE.y * 0.5 + 0.008, 0))
	_add(_cylinder(0.009, 0.008), _material(UiStyle.ACCENT, 0.4, 0.3), Vector3(0.045, SIZE.y * 0.5 + 0.004, 0))

	# The lens, out of the front (-Z).
	var front := -SIZE.z * 0.5
	var barrel := _add(_cylinder(0.03, 0.034), metal, Vector3(0.012, -0.002, front - 0.017))
	barrel.rotation.x = PI * 0.5
	var ring := _add(_cylinder(0.031, 0.005), _material(UiStyle.ACCENT, 0.35, 0.4), Vector3(0.012, -0.002, front - 0.03))
	ring.rotation.x = PI * 0.5
	var lens := _add(_cylinder(0.022, 0.003), glass, Vector3(0.012, -0.002, front - 0.0345))
	lens.rotation.x = PI * 0.5
	_lens = Marker3D.new()
	_lens.position = Vector3(0.012, -0.002, front - 0.04)
	add_child(_lens)
	# Turned round for a selfie: above the screen, looking back.
	_back = Marker3D.new()
	_back.position = Vector3(0.0, SIZE.y * 0.5 + 0.02, SIZE.z * 0.5 + 0.03)
	_back.rotation.y = PI
	add_child(_back)

	# The screen on the back (+Z), facing whoever holds it, in a bezel.
	var back := SIZE.z * 0.5
	_add(_box(Vector3(0.124, 0.074, 0.002)), grip, Vector3(0, 0.002, back + 0.001))
	_screen = StandardMaterial3D.new()
	_screen.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	_screen.emission_enabled = true
	_screen.emission = Color.WHITE
	_screen.emission_energy_multiplier = 0.0
	var screen := MeshInstance3D.new()
	var quad := QuadMesh.new()
	quad.size = Vector2(0.116, 0.065)
	screen.mesh = quad
	screen.material_override = _screen
	screen.position = Vector3(0, 0.002, back + 0.0025)
	screen.layers = BODY_LAYER
	screen.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(screen)


func _material(colour: Color, roughness: float, metallic: float) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = colour
	material.roughness = roughness
	material.metallic = metallic
	return material


func _box(size: Vector3) -> BoxMesh:
	var mesh := BoxMesh.new()
	mesh.size = size
	return mesh


func _cylinder(radius: float, height: float) -> CylinderMesh:
	var mesh := CylinderMesh.new()
	mesh.top_radius = radius
	mesh.bottom_radius = radius
	mesh.height = height
	mesh.radial_segments = 32
	return mesh


func _add(mesh: Mesh, material: Material, at: Vector3) -> MeshInstance3D:
	var node := MeshInstance3D.new()
	node.mesh = mesh
	node.material_override = material
	node.position = at
	node.layers = BODY_LAYER
	add_child(node)
	return node
