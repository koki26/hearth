extends Node3D

## One remote player: a head, hands, and a body levitating under the head.
## Smoothed between the 20 Hz pose updates.

@export var smoothing := 18.0

const BODY_DROP := 0.5
## How long a chat bubble hangs over somebody's head, and how wide it gets.
const BUBBLE_TIME := 7.0
const BUBBLE_CHARS := 28

var _voice: AudioStreamGeneratorPlayback
var _head := Transform3D.IDENTITY
var _left := Transform3D.IDENTITY
var _right := Transform3D.IDENTITY
var _bubble: Label3D
var _bubble_left := 0.0


func _ready() -> void:
	$Head/Voice.play()
	_voice = $Head/Voice.get_stream_playback()


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
