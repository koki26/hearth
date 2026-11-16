class_name FlatPanel
extends Node3D

## What the lift panel and the keyboard have in common: a flat rectangle in the
## world that a ray can be pointed at.
##
## The hit test is arithmetic rather than a collision shape. These panels are
## rebuilt whenever a row changes, and keeping an Area3D per row in step with
## that would be more code than intersecting one plane -- which is also exact,
## and costs no physics.

## Set by whoever builds the panel, so `hit` knows how big the rectangle is.
var _width := 0.0
var _height := 0.0
## How tall the grab strip across the top of this panel is, in metres. Zero
## for a panel you cannot pick up.
var bar_height := 0.0
## Which band this panel draws in. Draw order is global, so a panel that has to
## sit in front of another needs its whole band above it -- otherwise the one
## behind draws its text over this one's background. See UiStyle.BAND.
var band := 0


## Where a ray crosses this panel. Empty when it misses, otherwise:
##   {"local": Vector3, "distance": float}
## `local` is in the panel's own space, with (0, 0) in the middle, which is what
## the row and key lookups are written against.
func hit(from: Vector3, direction: Vector3) -> Dictionary:
	if _width <= 0.0 or _height <= 0.0 or not visible:
		return {}
	var inverse := global_transform.affine_inverse()
	var start := inverse * from
	var along := inverse.basis * direction
	# Edge on: no crossing to find.
	if absf(along.z) < 0.00001:
		return {}
	var distance := -start.z / along.z
	if distance <= 0.0:
		return {} # Behind the pointer.
	var local := start + along * distance
	if absf(local.x) > _width * 0.5 or absf(local.y) > _height * 0.5:
		return {}
	return {"local": local, "distance": distance}


## Is this local point on the grab strip? Whoever is pointing carries the
## panel while it is held (see main.gd).
func is_bar(local: Vector3) -> bool:
	return bar_height > 0.0 and _height > 0.0 and local.y >= _height * 0.5 - bar_height


## An unshaded, alpha-blended material. Panels are read, not lit. `layer` sets
## the draw order, which is what keeps a highlight behind the text it is
## highlighting -- interface ignores depth, so order is all there is. See ui.gd.
func _flat(colour: Color, layer := UiStyle.LAYER_FILL) -> StandardMaterial3D:
	return UiStyle.flat(colour, layer + band)


## A label in this panel's band. Always use this rather than `UiStyle.label`
## directly, or the text lands in another panel's order.
func _label(text: String, size: int, colour: Color = UiStyle.TEXT) -> Label3D:
	var node := UiStyle.label(text, size, colour)
	node.render_priority += band
	node.outline_render_priority += band
	return node


## The hairline and corner marks for a panel of this size (docs/UI_STYLE.md).
## Call it after adding the body, so the body covers the oversized edge quad
## and leaves only its rim showing.
func _frame(width: float, height: float) -> void:
	for part in UiStyle.frame(width, height, band):
		add_child(part)
