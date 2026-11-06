extends Node3D

## Firelight that never sits still.

@export var energy := 3.2

var _time := 0.0


func _process(delta: float) -> void:
	_time += delta
	var flicker := sin(_time * 7.3) * 0.12 + sin(_time * 13.1) * 0.07 + sin(_time * 23.7) * 0.04
	$Light.light_energy = energy * (1.0 + flicker)
