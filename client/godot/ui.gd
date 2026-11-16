class_name UiStyle
extends RefCounted

## Every colour, size and material the interface uses. One file, so skinning
## the whole game is editing this one and nothing else.
##
## The look: warm charcoal panels with round corners, a hairline edge, a rim
## of light along the top and a soft shadow under them. Controls are drawn as
## controls -- a slider has a track and a knob, a switch is a pill -- and every
## shape is one rounded rectangle from one tiny shader, which keeps it cheap
## enough to draw twice, once per eye, and keeps a reskin to a few colours.
##
## Content keeps its own colours. An icon, an avatar, a world, a flame: those
## are the things you are looking at, not the frame around them.
##
## 2D controls (the launcher, the world builder) are styled by `ui_theme.tres`,
## which `tools/make_theme.gd` generates from the tokens below and
## `project.godot` applies to every Control in the project. After changing a
## token, regenerate it:
##
##   godot --headless --path client/godot --script res://tools/make_theme.gd

# --- colour ----------------------------------------------------------------

## The palette is a fire seen at night: warm charcoal surfaces, one ember-amber
## accent, and text the colour of ash-white paper. Cool blues read as
## "computer"; Hearth is a place people sit around.

## Behind everything: the launcher, and any full-screen fill.
const BACKDROP := Color(0.022, 0.019, 0.02)
## A panel. Charcoal, slightly see-through so the world stays present.
const INK := Color(0.052, 0.046, 0.046, 0.94)
## A surface sitting on a panel: a key cap, a button, a field, an icon chip.
const INK_LIFT := Color(0.118, 0.106, 0.104, 0.97)
## The hairline around a panel. Low alpha on purpose: it should read as an edge
## catching the firelight, not as a drawn border.
const EDGE := Color(0.98, 0.86, 0.74, 0.13)
## The rim along the top of a panel, a switch's knob, the edge of focus.
const EDGE_BRIGHT := Color(1.0, 0.94, 0.88, 0.86)
## The live thing: what is selected, what is being pointed at, what is on.
const ACCENT := Color(1.0, 0.63, 0.3)
const TEXT := Color(0.965, 0.945, 0.925)
## Headings, hints, and anything you are not meant to act on.
const TEXT_DIM := Color(0.63, 0.59, 0.57)
## Refused, blocked, leaving.
const WARN := Color(1.0, 0.45, 0.42)
## Yours, done, online.
const GOOD := Color(0.5, 0.92, 0.62)
## Under every panel: a soft dark pool that lifts it off the world.
const SHADOW := Color(0.0, 0.0, 0.0, 0.42)

# --- size, in metres, for panels in the world ------------------------------

## The hairline's width.
const HAIRLINE := 0.004
## Kept for the 2D theme's sake; panels in the world have round corners now.
const CORNER := 0.1
## How round things are: a panel, and anything sitting on one.
const RADIUS := 0.05
const RADIUS_SMALL := 0.024
## How far a panel's shadow spreads past its edge, and how soft it is.
const SHADOW_SPREAD := 0.05
## Space between a panel's edge and its content. Text never touches an edge.
const PAD := 0.07
## One row of a list, and the shorter kinds of row.
const ROW := 0.115
## A section heading, and the gap a blank row leaves.
const HEADING := 0.085
const SPACER := 0.04
## A line of chat, or any row marked "small".
const LINE := 0.066
## A row with a second line under its label.
const ROW_TALL := 0.15
## A row drawn as a button: a filled pill across the panel.
const ROW_BUTTON := 0.135
## Where a row's value starts, as a fraction of the panel's width from its left
## edge. Labels go left of it, values right of it, so a column of values lines
## up instead of each one starting wherever its label happened to end.
const VALUE_AT := 0.46
## A slider: the track's thickness, and the knob that rides it.
const TRACK := 0.014
const KNOB := 0.046
## A switch: its pill, and the room its readout keeps at the end of a slider.
const SWITCH := Vector2(0.11, 0.058)
const READOUT := 0.12
## The rounded square an icon sits on, at the start of a row.
const CHIP := 0.084
## Every glyph gets a dark outline, as a fraction of its own font size: that
## is what makes text readable whatever ends up behind it -- a lit brazier, a
## white crate, somebody's avatar.
## It scales with the font: a fixed thickness bleeds into small letters.
const OUTLINE_RATIO := 0.085
## Roughly how wide one character is, as a fraction of its font size in
## pixels. Label3D cannot be measured before it is in the tree, so text that
## has to fit is fitted against this.
const CHAR_WIDTH := 0.5

## Draw order for the transparent surfaces an interface is made of. Depth
## testing does not settle it -- interface ignores depth on purpose -- so
## without these the highlight behind a row can be painted over the row's own
## text, which is exactly what it did.
const LAYER_BODY := -4
## The selection wash: over the panel, under every surface on it, so a field
## on the selected row is still a field.
const LAYER_WASH := -3
const LAYER_FILL := -2
const LAYER_EDGE := -1
const LAYER_TEXT := 0
## Draw order is global, so the layers above only order one panel against
## itself. A panel that must sit in front of another adds a band: without it
## the panel behind has its text drawn over the nearer panel's background.
const BAND := 10

## How strongly a selected row is washed with the accent. Kept low because the
## text on that row still has to be read. The selection is carried by the
## marker bar instead, so this only has to say "around here".
##
## Measured, not guessed: see the contrast table in docs/UI_STYLE.md, which
## tests/panel_test.gd holds every token to.
const SELECT_ALPHA := 0.08
## A highlighted key is washed harder, because the outline round it does the
## rest; 0.14 is as far as it goes before white on it drops under 7:1.
const KEY_ALPHA := 0.14
## The bright bar that says "this one": down the left of a row, under a key.
const MARKER := 0.012
## An icon on a row: a little under the row height, so it never crowds it.
const ICON := 0.056
## The strip of tabs across the top of a panel, and the icon in a tab.
const TAB := 0.13
const TAB_ICON := 0.056
const FONT_BIG := 72
const FONT := 44
const FONT_SMALL := 34
## Label3D world size per pixel of font. Two labels at different font sizes and
## the same pixel size are in proportion, which is the whole point of having it
## here rather than per call site.
const PIXEL := 0.0013
const PIXEL_BIG := 0.0016

# --- size, in pixels, for 2D controls --------------------------------------

const BORDER_2D := 1
const RADIUS_2D := 8
const PAD_2D := 10
const FONT_2D := 18
const FONT_TITLE_2D := 44


# --- materials and meshes --------------------------------------------------

## One rounded rectangle, drawn exactly by a signed distance rather than built
## out of triangles: crisp at any distance, anti-aliased by its own screen
## derivative, and the same cost as the flat quad it replaced. A border, a
## soft edge (for shadows) and a sheen down from the top are all uniforms, so
## every surface in the interface is this one shader.
const ROUNDED_SHADER := """
shader_type spatial;
render_mode unshaded, blend_mix, depth_draw_never, depth_test_disabled, cull_back, shadows_disabled, fog_disabled;

uniform vec4 fill : source_color = vec4(0.0);
uniform vec4 border : source_color = vec4(0.0);
uniform vec2 size = vec2(1.0);
uniform float radius = 0.0;
uniform float border_width = 0.0;
uniform float feather = 0.0;
uniform float sheen = 0.0;

float box(vec2 p, vec2 half_size, float r) {
	vec2 q = abs(p) - half_size + r;
	return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

void fragment() {
	vec2 p = (UV - 0.5) * size;
	float r = min(radius, min(size.x, size.y) * 0.5);
	float d = box(p, size * 0.5, r);
	float aa = max(fwidth(d), 0.00001);
	float coverage = clamp(0.5 - d / aa, 0.0, 1.0);
	if (feather > 0.0) {
		coverage = 1.0 - smoothstep(-feather, feather, d);
	}
	// The border is laid over the fill, not swapped for it: a faint hairline
	// on a dark panel is a faint hairline, not a bright seam where the two
	// colours cross-fade at full strength.
	float ring = border_width > 0.0 ? 1.0 - clamp(0.5 - (d + border_width) / aa, 0.0, 1.0) : 0.0;
	float over = border.a * ring;
	vec4 colour = vec4(mix(fill.rgb, border.rgb, over), fill.a + (1.0 - fill.a) * over);
	colour.rgb += sheen * (1.0 - UV.y) * (1.0 - UV.y);
	ALBEDO = colour.rgb;
	ALPHA = colour.a * coverage;
}
"""

static var _rounded: Shader


## Unshaded and alpha-blended. Interface is read, never lit: a panel that
## responds to a passing torch is a panel you cannot read.
static func flat(colour: Color, layer := LAYER_FILL) -> StandardMaterial3D:
	var material := StandardMaterial3D.new()
	material.albedo_color = colour
	material.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	material.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	material.render_priority = layer
	return material


## For the few parts that should look lit from inside: the accent edge, a
## pointer beam.
static func glow(colour: Color, energy := 1.8) -> StandardMaterial3D:
	var material := flat(colour)
	material.emission_enabled = true
	material.emission = colour
	material.emission_energy_multiplier = energy
	return material


static func quad(size: Vector2, colour: Color, at: Vector3, layer := LAYER_FILL) -> MeshInstance3D:
	var node := MeshInstance3D.new()
	var mesh := QuadMesh.new()
	mesh.size = size
	node.mesh = mesh
	var material := flat(colour, layer)
	# Interface wins the depth test, the same as its text does.
	material.no_depth_test = true
	node.material_override = material
	node.position = at
	node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	return node


## A rounded rectangle `size` metres across, centred on `at`. `radius` is
## clamped to half the short side, so a big radius makes a pill and a square
## with a big radius makes a circle.
static func rounded(size: Vector2, colour: Color, at: Vector3, layer := LAYER_FILL,
		radius := RADIUS_SMALL, border := Color(0, 0, 0, 0), border_width := 0.0) -> MeshInstance3D:
	if _rounded == null:
		_rounded = Shader.new()
		_rounded.code = ROUNDED_SHADER
	var material := ShaderMaterial.new()
	material.shader = _rounded
	material.render_priority = layer
	material.set_shader_parameter("fill", colour)
	material.set_shader_parameter("border", border)
	material.set_shader_parameter("radius", radius)
	material.set_shader_parameter("border_width", border_width)
	var node := MeshInstance3D.new()
	node.mesh = QuadMesh.new()
	node.material_override = material
	node.position = at
	node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	resize(node, size)
	return node


## A circle `diameter` across: a knob, a status dot, an avatar.
static func circle(diameter: float, colour: Color, at: Vector3, layer := LAYER_FILL) -> MeshInstance3D:
	return rounded(Vector2(diameter, diameter), colour, at, layer, diameter)


## Changes a `rounded` shape's size in place, which is how a slider's fill
## follows the knob without rebuilding the row.
static func resize(node: MeshInstance3D, size: Vector2) -> void:
	(node.mesh as QuadMesh).size = size
	(node.material_override as ShaderMaterial).set_shader_parameter("size", size)


## Recolours a `rounded` shape in place.
static func tint(node: MeshInstance3D, colour: Color) -> void:
	(node.material_override as ShaderMaterial).set_shader_parameter("fill", colour)


static func label(text: String, size: int, colour: Color = TEXT) -> Label3D:
	var node := Label3D.new()
	node.text = text
	node.font_size = size
	node.pixel_size = PIXEL
	node.modulate = colour
	# Interface always wins against the world behind it, otherwise a crate can
	# eat half a row.
	node.no_depth_test = true
	# Contrast, guaranteed rather than hoped for: a panel is translucent and
	# the world behind it is any colour at all, so every glyph carries its own
	# dark edge -- thin, and in proportion to the glyph.
	node.outline_size = maxi(2, int(round(float(size) * OUTLINE_RATIO)))
	node.outline_modulate = BACKDROP
	# Text is the last thing drawn, and its own outline goes behind it.
	node.render_priority = LAYER_TEXT
	node.outline_render_priority = LAYER_EDGE
	node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	return node


## How wide `text` comes out at this font size, in metres. An estimate (see
## CHAR_WIDTH), good enough to fit a line into the room it has.
static func text_width(text: String, size: int, pixel := PIXEL) -> float:
	return float(text.length()) * float(size) * pixel * CHAR_WIDTH


## Makes a label fit `room` metres: first by shrinking it a little, then by
## cutting it short with an ellipsis. Text never runs off a panel or into the
## control beside it.
static func fit(node: Label3D, room: float) -> void:
	var wide := text_width(node.text, node.font_size, node.pixel_size)
	if wide <= room or room <= 0.0:
		return
	var shrink := maxf(0.8, room / wide)
	node.pixel_size *= shrink
	wide *= shrink
	if wide <= room:
		return
	var keep := maxi(1, int(floor(float(node.text.length()) * room / wide)) - 1)
	node.text = node.text.substr(0, keep).strip_edges() + "\u2026"


## Splits `text` into lines of at most `chars` characters, at spaces where it
## can. Label3D can wrap on its own, but then nothing knows how tall the result
## is, and a list needs to.
static func wrap(text: String, chars: int) -> PackedStringArray:
	var lines := PackedStringArray()
	var line := ""
	for word in text.split(" ", false):
		while word.length() > chars:
			if line != "":
				lines.append(line)
				line = ""
			lines.append(word.substr(0, chars))
			word = word.substr(chars)
		if word == "":
			continue
		if line == "":
			line = word
		elif line.length() + 1 + word.length() <= chars:
			line += " " + word
		else:
			lines.append(line)
			line = word
	if line != "":
		lines.append(line)
	return lines


## An icon from `icons/`, `size` metres tall and tinted like text. A Sprite3D
## rather than a textured quad: it is the one node that already does a texture
## with alpha, and it is what the HUD mic icon is.
static func icon(path: String, size: float, colour: Color = TEXT, layer := LAYER_TEXT) -> Sprite3D:
	var node := Sprite3D.new()
	node.texture = load(path)
	# Measured rather than assumed: an icon re-exported at another resolution
	# would otherwise change size on the panel.
	var pixels := float(node.texture.get_height()) if node.texture != null else 64.0
	node.pixel_size = size / maxf(pixels, 1.0)
	node.modulate = colour
	node.no_depth_test = true
	node.render_priority = layer
	# Icons are imported at twice their drawn size with mipmaps, so they stay
	# sharp up close and do not shimmer from across the room.
	node.texture_filter = BaseMaterial3D.TEXTURE_FILTER_LINEAR_WITH_MIPMAPS
	node.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	return node


## The colour of somebody's avatar disc where there is no picture. It comes
## from the name, so it is the same everywhere and for everybody. Content, not
## interface, which is why it is not a token.
static func avatar_colour(who: String) -> Color:
	var hue := float(absi(who.to_lower().hash()) % 360) / 360.0
	return Color.from_hsv(hue, 0.5, 0.78)


# --- contrast --------------------------------------------------------------

## WCAG relative luminance.
static func luminance(colour: Color) -> float:
	var total := 0.0
	var weights := [0.2126, 0.7152, 0.0722]
	var channels := [colour.r, colour.g, colour.b]
	for i in 3:
		var c: float = channels[i]
		var linear := c / 12.92 if c <= 0.04045 else pow((c + 0.055) / 1.055, 2.4)
		total += float(weights[i]) * linear
	return total


## WCAG contrast ratio: 1.0 for two identical colours, 21.0 for black on white.
## 4.5 is the floor for text you must be able to read, 7.0 for body text.
static func contrast(a: Color, b: Color) -> float:
	var la := luminance(a)
	var lb := luminance(b)
	return (maxf(la, lb) + 0.05) / (minf(la, lb) + 0.05)


## Flattens a translucent colour onto what is behind it, so a panel's real
## contrast can be reasoned about rather than guessed.
##
## The mix happens in linear space, because that is where the GPU does it.
## Lerping the sRGB values instead badly under-reads how bright a translucent
## overlay comes out.
static func over(colour: Color, under: Color) -> Color:
	return under.srgb_to_linear().lerp(colour.srgb_to_linear(), colour.a).linear_to_srgb()


## The text colour that reads best on `surface`. For backgrounds that are not
## one of the tokens: a world colour, a swatch, a player's chosen tint.
static func on(surface: Color) -> Color:
	return TEXT if contrast(TEXT, surface) >= contrast(BACKDROP, surface) else BACKDROP


## The wash over whatever is selected.
static func selection() -> Color:
	return Color(ACCENT, SELECT_ALPHA)


## A panel's body: a soft shadow, the rounded charcoal surface with its
## hairline, and a rim of light along the top edge. Returns the meshes rather
## than adding them, so the caller controls order. Draw the panel's content
## from z = 0 forwards; this sits behind it.
static func frame(width: float, height: float, band := 0) -> Array[MeshInstance3D]:
	var parts: Array[MeshInstance3D] = []
	var shadow := rounded(Vector2(width + SHADOW_SPREAD * 2.0, height + SHADOW_SPREAD * 2.0),
		SHADOW, Vector3(0.0, -SHADOW_SPREAD * 0.35, -0.02), LAYER_BODY + band, RADIUS + SHADOW_SPREAD)
	(shadow.material_override as ShaderMaterial).set_shader_parameter("feather", SHADOW_SPREAD * 0.9)
	parts.append(shadow)
	var body := rounded(Vector2(width, height), INK, Vector3(0.0, 0.0, -0.012), LAYER_BODY + band,
		RADIUS, EDGE, HAIRLINE)
	(body.material_override as ShaderMaterial).set_shader_parameter("sheen", 0.035)
	parts.append(body)
	# The rim: light catching the top edge, which is what makes a flat panel
	# read as a thing with thickness.
	parts.append(rounded(Vector2(width - RADIUS * 2.2, HAIRLINE * 0.8), Color(EDGE_BRIGHT, 0.18),
		Vector3(0.0, height * 0.5 - HAIRLINE * 1.2, -0.004), LAYER_EDGE + band, HAIRLINE))
	return parts


# --- 2D controls -----------------------------------------------------------

## The theme behind every Control in the project. Saved to `ui_theme.tres` by
## tools/make_theme.gd, never built at runtime, so the launcher and the world
## builder need no styling code at all.
static func build_theme() -> Theme:
	var theme := Theme.new()
	theme.default_font_size = FONT_2D

	theme.set_stylebox("normal", "Button", _box(INK_LIFT, EDGE))
	theme.set_stylebox("hover", "Button", _box(INK_LIFT.lightened(0.06), ACCENT))
	theme.set_stylebox("pressed", "Button", _box(ACCENT.darkened(0.7), ACCENT))
	theme.set_stylebox("focus", "Button", _box(Color(0, 0, 0, 0), EDGE_BRIGHT))
	theme.set_stylebox("disabled", "Button", _box(INK, EDGE.darkened(0.4)))
	theme.set_color("font_color", "Button", TEXT)
	theme.set_color("font_hover_color", "Button", TEXT)
	theme.set_color("font_pressed_color", "Button", TEXT)
	theme.set_color("font_disabled_color", "Button", TEXT_DIM)

	theme.set_stylebox("panel", "PanelContainer", _box(INK, EDGE))
	theme.set_stylebox("panel", "Panel", _box(INK, EDGE))
	theme.set_stylebox("panel", "PopupPanel", _box(INK, EDGE_BRIGHT))

	theme.set_color("font_color", "Label", TEXT)

	theme.set_stylebox("normal", "LineEdit", _box(BACKDROP, EDGE))
	theme.set_stylebox("focus", "LineEdit", _box(BACKDROP, ACCENT))
	theme.set_color("font_color", "LineEdit", TEXT)
	theme.set_color("font_placeholder_color", "LineEdit", TEXT_DIM)
	theme.set_color("caret_color", "LineEdit", ACCENT)
	theme.set_color("selection_color", "LineEdit", ACCENT * Color(1, 1, 1, 0.35))

	theme.set_stylebox("panel", "ItemList", _box(BACKDROP, EDGE))
	theme.set_stylebox("focus", "ItemList", _box(Color(0, 0, 0, 0), ACCENT))
	theme.set_stylebox("selected", "ItemList", _box(ACCENT.darkened(0.68), ACCENT))
	theme.set_stylebox("selected_focus", "ItemList", _box(ACCENT.darkened(0.6), ACCENT))
	theme.set_stylebox("hovered", "ItemList", _box(INK_LIFT, Color(0, 0, 0, 0)))
	theme.set_color("font_color", "ItemList", TEXT)
	theme.set_color("font_selected_color", "ItemList", TEXT)

	theme.set_stylebox("normal", "OptionButton", _box(INK_LIFT, EDGE))
	theme.set_stylebox("hover", "OptionButton", _box(INK_LIFT.lightened(0.06), ACCENT))
	theme.set_color("font_color", "OptionButton", TEXT)

	theme.set_stylebox("normal", "CheckBox", _box(Color(0, 0, 0, 0), Color(0, 0, 0, 0)))
	theme.set_color("font_color", "CheckBox", TEXT)

	theme.set_stylebox("slider", "HSlider", _box(BACKDROP, EDGE))
	theme.set_stylebox("grabber_area", "HSlider", _box(ACCENT.darkened(0.3), ACCENT))

	theme.set_stylebox("panel", "TabContainer", _box(INK, EDGE))
	theme.set_stylebox("tab_selected", "TabContainer", _box(INK_LIFT, ACCENT))
	theme.set_stylebox("tab_unselected", "TabContainer", _box(INK, EDGE))
	theme.set_color("font_selected_color", "TabContainer", TEXT)
	theme.set_color("font_unselected_color", "TabContainer", TEXT_DIM)
	return theme


## A near-black fill with a hairline border and a small radius: the 2D half of
## the same idea as `frame`.
static func _box(fill: Color, border: Color) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = fill
	box.border_color = border
	box.set_border_width_all(BORDER_2D)
	box.set_corner_radius_all(RADIUS_2D)
	box.content_margin_left = PAD_2D
	box.content_margin_right = PAD_2D
	box.content_margin_top = PAD_2D * 0.6
	box.content_margin_bottom = PAD_2D * 0.6
	return box
