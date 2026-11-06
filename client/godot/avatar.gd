extends Node3D

## One remote player: head and hands, smoothed between the 20 Hz updates.

@export var smoothing := 18.0

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
