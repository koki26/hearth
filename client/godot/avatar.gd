extends Node3D

## One remote player: a head, hands, and a body levitating under the head.
## Smoothed between the 20 Hz pose updates.

@export var smoothing := 18.0

const BODY_DROP := 0.5
## How long a chat bubble hangs over somebody's head, and how wide it gets.
const BUBBLE_TIME := 7.0
const BUBBLE_CHARS := 28
## The nameplate's picture and badge icons, in metres.
const PICTURE := 0.09
const BADGE_ICON := 0.045
const OWNER_COLOUR := Color("f5c542")
const STAFF_COLOUR := Color("3ccf6e")

## Their public profile from the Fireplace (`/profiles/...`), or empty for a
## guest. Only the Fireplace fills it: a Flame passes on an id, nothing more.
var profile := {}
## Custom avatar styling (AvatarFormat dictionary).
var avatar_data := {}

var _voice: AudioStreamGeneratorPlayback
var _head := Transform3D.IDENTITY
var _left := Transform3D.IDENTITY
var _right := Transform3D.IDENTITY
var _bubble: Label3D
var _bubble_left := 0.0


func _ready() -> void:
	$Head/Voice.play()
	_voice = $Head/Voice.get_stream_playback()
	set_picture(null)


## Customizes the visual mesh, materials, and accessories of this avatar.
func set_avatar_data(data: Dictionary) -> void:
	avatar_data = data
	AvatarFormat.apply_to_avatar_node(self, data)



func _exit_tree() -> void:
	_voice = null # Otherwise Godot reports the playback still in use at exit.


## Mono 48 kHz samples from this player, straight from the Opus decoder.
func push_voice(samples: PackedFloat32Array) -> void:
	if _voice == null:
		return
	var room := mini(samples.size(), _voice.get_frames_available())
	if room <= 0:
		return
	var frames := PackedVector2Array()
	frames.resize(room)
	for i in room:
		frames[i] = Vector2(samples[i], samples[i])
	_voice.push_buffer(frames)


func set_label(text: String) -> void:
	$Head/Label.text = text
	var initial: Label3D = $Head/Plate.get_node_or_null("Initial")
	if initial != null and initial.visible:
		initial.text = text.substr(0, 1).to_upper()
	_lay_out_plate()


## The Fireplace's word on who this is: the name it vouches for, and the badge
## over it -- a yellow crown for the Fireplace owner, a green shield for
## anybody the owner put in a group.
func set_profile(found: Dictionary) -> void:
	profile = found
	var badge = found.get("badge")
	var plate := $Head/Plate
	for child in plate.get_children():
		if child.name.begins_with("Badge"):
			# Out of the tree now, so the new ones get these names.
			plate.remove_child(child)
			child.queue_free()
	if badge is Dictionary:
		var is_owner := str(badge.get("kind", "")) == "owner"
		var colour := OWNER_COLOUR if is_owner else STAFF_COLOUR
		var parts := {
			"BadgeIcon": UiStyle.icon("res://icons/crown.svg" if is_owner else "res://icons/shield.svg", BADGE_ICON, colour),
			"BadgeWords": UiStyle.label(str(badge.get("label", "")), UiStyle.FONT_SMALL, colour),
			"BadgeCheck": UiStyle.icon("res://icons/check.svg", BADGE_ICON, colour),
		}
		for part: String in parts:
			var node: GeometryInstance3D = parts[part]
			node.name = part
			node.set("billboard", BaseMaterial3D.BILLBOARD_ENABLED)
			plate.add_child(node)
	_lay_out_plate()


## Their picture beside their name; circles are enforced even in nametags.
func set_picture(texture: Texture2D) -> void:
	var sprite: Sprite3D = $Head/Plate.get_node_or_null("Picture")
	if sprite == null:
		sprite = Sprite3D.new()
		sprite.name = "Picture"
		sprite.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		sprite.no_depth_test = true
		sprite.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		$Head/Plate.add_child(sprite)
	if texture != null:
		var img := texture.get_image()
		if img != null:
			var masked := img.duplicate()
			FireplaceClient.circle_mask(masked)
			sprite.texture = ImageTexture.create_from_image(masked)
		else:
			sprite.texture = texture
		sprite.visible = true
		sprite.pixel_size = PICTURE / float(maxi(sprite.texture.get_height(), 1))
		var initial: Label3D = $Head/Plate.get_node_or_null("Initial")
		if initial != null:
			initial.visible = false
	else:
		var col := UiStyle.avatar_colour(label())
		var img := Image.create(64, 64, false, Image.FORMAT_RGBA8)
		img.fill(Color(0, 0, 0, 0))
		var r := 30.0
		for y in 64:
			for x in 64:
				var dx := float(x) - 31.5
				var dy := float(y) - 31.5
				var d := sqrt(dx * dx + dy * dy)
				if d <= r:
					var a := clampf(r - d + 0.5, 0.0, 1.0)
					img.set_pixel(x, y, Color(col.r, col.g, col.b, a))
		sprite.texture = ImageTexture.create_from_image(img)
		sprite.visible = true
		sprite.pixel_size = PICTURE / 64.0
		var initial: Label3D = $Head/Plate.get_node_or_null("Initial")
		if initial == null:
			initial = Label3D.new()
			initial.name = "Initial"
			initial.billboard = BaseMaterial3D.BILLBOARD_ENABLED
			initial.no_depth_test = true
			initial.font_size = UiStyle.FONT_SMALL
			initial.pixel_size = 0.0018
			$Head/Plate.add_child(initial)
		initial.text = label().substr(0, 1).to_upper()
		initial.visible = true
	_lay_out_plate()


## Billboards turn on their own, so the parts of the plate sit side by side
## through their offsets, which are in each one's own pixels.
func _lay_out_plate() -> void:
	var name_label: Label3D = $Head/Label
	var name_width := UiStyle.text_width(name_label.text, name_label.font_size, name_label.pixel_size)
	var picture: Sprite3D = $Head/Plate.get_node_or_null("Picture")
	if picture != null and picture.visible:
		var gap := 0.015
		var off_x := -(name_width * 0.5 + gap + PICTURE * 0.5)
		picture.offset = Vector2(off_x / picture.pixel_size, 0.0)
		var initial: Label3D = $Head/Plate.get_node_or_null("Initial")
		if initial != null and initial.visible:
			initial.offset = Vector2(off_x / initial.pixel_size, 0.0)
	var words: Label3D = $Head/Plate.get_node_or_null("BadgeWords")
	if words == null:
		return
	var rise := 0.06
	var words_width := UiStyle.text_width(words.text, words.font_size, words.pixel_size)
	var row := BADGE_ICON * 2.0 + 0.02 + words_width
	var left := -row * 0.5
	for part: Array in [["BadgeIcon", BADGE_ICON], ["BadgeWords", words_width], ["BadgeCheck", BADGE_ICON]]:
		var node: Node3D = $Head/Plate.get_node(str(part[0]))
		var middle: float = left + float(part[1]) * 0.5
		var pixel: float = node.get("pixel_size")
		node.set("offset", Vector2(middle / pixel, rise / pixel))
		left += float(part[1]) + 0.01


## Something they said in chat, over their head for a few seconds. Facing
## whoever looks at it, so it can be read from anywhere in the room.
func say(text: String) -> void:
	if _bubble == null:
		_bubble = UiStyle.label("", UiStyle.FONT_SMALL, UiStyle.TEXT)
		_bubble.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		_bubble.pixel_size = 0.0018
		_bubble.vertical_alignment = VERTICAL_ALIGNMENT_BOTTOM
		_bubble.position = Vector3(0.0, 0.38, 0.0)
		$Head.add_child(_bubble)
	_bubble.text = "\n".join(UiStyle.wrap(text, BUBBLE_CHARS).slice(0, 4))
	_bubble.modulate.a = 1.0
	_bubble.visible = true
	_bubble_left = BUBBLE_TIME


## Their name, for the menu's list of who is here.
func label() -> String:
	return $Head/Label.text


func set_pose(head: Transform3D, left: Transform3D, right: Transform3D, flags: int) -> void:
	_head = head
	_left = left
	_right = right
	$Left.visible = flags & 2 != 0
	$Right.visible = flags & 4 != 0


func _process(delta: float) -> void:
	if _bubble_left > 0.0:
		_bubble_left -= delta
		_bubble.modulate.a = clampf(_bubble_left / 0.8, 0.0, 1.0)
		_bubble.visible = _bubble_left > 0.0
	var weight := clampf(smoothing * delta, 0.0, 1.0)
	$Head.transform = $Head.transform.interpolate_with(_head, weight)
	$Left.transform = $Left.transform.interpolate_with(_left, weight)
	$Right.transform = $Right.transform.interpolate_with(_right, weight)
	# The body hangs below the head and only follows its yaw, so a player
	# looking down does not tip their whole body over.
	$Body.position = $Head.position + Vector3.DOWN * BODY_DROP
	$Body.rotation.y = $Head.transform.basis.get_euler().y
