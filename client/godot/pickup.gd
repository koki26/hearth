extends Node3D

## A floating collectible. Touch it with a hand, or walk into it. Picking up is
## not networked yet, so every player has their own.

signal collected

const HAND_REACH := 0.3
const BODY_REACH := 0.6
const RESPAWN := 8.0

var _hidden_for := 0.0


func _process(delta: float) -> void:
	rotate_y(delta * 1.5)
	if _hidden_for > 0.0:
		_hidden_for -= delta
		if _hidden_for <= 0.0:
			$Mesh.visible = true
		return

	for hand: Node3D in get_tree().get_nodes_in_group("hand"):
		if hand.global_position.distance_to(global_position) < HAND_REACH:
			_take()
			return
	var camera := get_viewport().get_camera_3d()
	if camera and camera.global_position.distance_to(global_position) < BODY_REACH:
		_take()


func _take() -> void:
	$Mesh.visible = false
	_hidden_for = RESPAWN
	collected.emit()
