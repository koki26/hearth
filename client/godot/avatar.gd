extends Node3D

## One remote player: a head, hands, and a body levitating under the head.
## Smoothed between the 20 Hz pose updates.

@export var smoothing := 18.0

const BODY_DROP := 0.5

var _head := Transform3D.IDENTITY
var _left := Transform3D.IDENTITY
var _right := Transform3D.IDENTITY


func set_label(text: String) -> void:
	$Head/Label.text = text


func set_pose(head: Transform3D, left: Transform3D, right: Transform3D, flags: int) -> void:
	_head = head
	_left = left
	_right = right
	$Left.visible = flags & 2 != 0
	$Right.visible = flags & 4 != 0


func _process(delta: float) -> void:
	var weight := clampf(smoothing * delta, 0.0, 1.0)
	$Head.transform = $Head.transform.interpolate_with(_head, weight)
	$Left.transform = $Left.transform.interpolate_with(_left, weight)
	$Right.transform = $Right.transform.interpolate_with(_right, weight)
	# The body hangs below the head and only follows its yaw, so a player
	# looking down does not tip their whole body over.
	$Body.position = $Head.position + Vector3.DOWN * BODY_DROP
	$Body.rotation.y = $Head.transform.basis.get_euler().y
