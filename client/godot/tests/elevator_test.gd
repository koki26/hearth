extends SceneTree

## Checks the lift and the keyboard on its panel, without a window:
##   godot --headless --path client/godot --script res://tests/elevator_test.gd
##
## The lift is the first thing anybody sees and the only way into a world, and
## the panel on its wall is the only way to reach a server. The things that
## strand a player are worth a test: a car that never lands, a car that drops
## you, and a keyboard that cannot spell an address.

var _failed := 0
var _checks := 0


func _initialize() -> void:
	_lands()
	_resumes_mid_ride()
	_falls_away_once_you_step_off()
	_types()
	_types_on_the_desktop()
	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)


func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)


## A fresh lift, parented so global transforms work.
func _lift() -> Elevator:
	var lift := Elevator.new()
	root.add_child(lift)
	return lift


func _lands() -> void:
	var lift := _lift()
	var spawn := Transform3D(Basis.IDENTITY, Vector3(0.0, 0.0, 0.0))
	lift.park(spawn)
	_check(is_equal_approx(lift.car_y(), Elevator.DROP), "it waits a full drop above the spawn")
	_check(lift.carrying(), "and it is holding the player up while it waits")
	_check(not lift.doors_open(), "with the doors shut")
	# Parked means parked: a hundred frames must not move it.
	for i in 100:
		lift.check(Vector3.ZERO, 1.0 / 60.0)
	_check(is_equal_approx(lift.car_y(), Elevator.DROP),
		"it does not budge until there is somewhere to go")

	lift.descend()
	_check(lift.descending(), "picking a server sends it down")

	# Longer than the ride plus the doors, at 60 frames a second.
	var arrived := [false]
	lift.arrived.connect(func() -> void: arrived[0] = true)
	var last := lift.car_y()
	var monotonic := true
	for i in 520:
		lift.check(Vector3.ZERO, 1.0 / 60.0)
		monotonic = monotonic and lift.car_y() <= last + 0.0001
		last = lift.car_y()
		if not lift.descending():
			break
	_check(monotonic, "the car only ever goes down")
	_check(not lift.descending(), "the car lands and the doors finish opening")
	_check(arrived[0], "landing is announced")
	_check(lift.doors_open(), "the doors are open at the end of it")
	_check(not lift.carrying(), "and it has let go of the player")
	_check(is_equal_approx(lift.car_y(), 0.0), "it stops level with the spawn, not above or below it")
	lift.free()


## Joining starts the ride before the world has finished downloading, so `ride`
## is called a second time while the car is already on its way.
func _resumes_mid_ride() -> void:
	var lift := _lift()
	lift.park(Transform3D(Basis.IDENTITY, Vector3.ZERO))
	lift.descend()
	for i in 60:
		lift.check(Vector3.ZERO, 1.0 / 60.0)
	var halfway := lift.car_y()
	_check(halfway < Elevator.DROP and halfway > 0.0, "the car is part way down")

	lift.park(Transform3D(Basis.IDENTITY, Vector3(5.0, 2.0, 5.0)))
	_check(absf(lift.car_y() - (2.0 + halfway)) < 0.01,
		"it carries on from the height it had reached, over the new spawn")
	for i in 520:
		lift.check(Vector3.ZERO, 1.0 / 60.0)
		if not lift.descending():
			break
	_check(is_equal_approx(lift.car_y(), 2.0), "and still lands on the new spawn")
	lift.free()


func _falls_away_once_you_step_off() -> void:
	var lift := _lift()
	lift.park(Transform3D(Basis.IDENTITY, Vector3.ZERO))
	lift.descend()
	for i in 520:
		lift.check(Vector3.ZERO, 1.0 / 60.0)
		if not lift.descending():
			break
	# Standing in the middle: it waits.
	for i in 60:
		lift.check(Vector3.ZERO, 1.0 / 60.0)
	_check(lift.has_car() and is_equal_approx(lift.car_y(), 0.0), "it waits while you stand on it")
	# Leaning over the edge is not getting off.
	for i in 60:
		lift.check(Vector3(Elevator.HALF + 0.2, 0.0, 0.0), 1.0 / 60.0)
	_check(is_equal_approx(lift.car_y(), 0.0), "leaning out of the doors is not stepping off")
	# Walking away is.
	for i in 120:
		lift.check(Vector3(9.0, 0.0, 0.0), 1.0 / 60.0)
	_check(not lift.has_car() or lift.car_y() < -1.0, "it drops once you are off it")
	lift.free()


## The panel keyboard. Driven by a thumbstick, so "which key is highlighted"
## is the whole of its state and worth pinning down.
func _types() -> void:
	var keys := WorldKeyboard.new()
	root.add_child(keys)
	var accepted := [""]
	var cancelled := [false]
	keys.accepted.connect(func(text: String) -> void: accepted[0] = text)
	keys.cancelled.connect(func() -> void: cancelled[0] = true)

	keys.open("Server", "ab")
	_check(keys.is_open(), "the keyboard opens")
	_check(keys.text() == "ab", "it starts from whatever the field already held")
	keys.backspace()
	keys.type("z")
	_check(keys.text() == "az", "a real keyboard types straight into it")

	keys.press() # Top left, which is "1".
	_check(keys.text() == "az1", "pressing the highlighted key types it")
	keys.move(Vector2i(0, 1))
	keys.press()
	_check(keys.text() == "az1q", "a row down is the next bank of keys")

	# Shift is the first key of the command row, three rows further down.
	keys.move(Vector2i(0, 3))
	keys.press()
	keys.move(Vector2i(0, -3))
	keys.press()
	_check(keys.text() == "az1qQ", "shift gives you one capital")
	keys.press()
	_check(keys.text() == "az1qQq", "and then turns itself off again")

	# The command row is shorter than the letter rows, and leaving the grid
	# must not lose the highlight.
	keys.move(Vector2i(0, -9))
	keys.press()
	_check(keys.text().ends_with("1"), "the highlight stops at the top row")
	keys.move(Vector2i(-9, 0))
	keys.press()
	_check(keys.text().ends_with("11"), "and at the left edge")

	keys.open("Server", "")
	for i in WorldKeyboard.MAX_LENGTH + 10:
		keys.type("x")
	_check(keys.text().length() == WorldKeyboard.MAX_LENGTH, "a field has a length limit")
	keys.accept()
	_check(accepted[0].length() == WorldKeyboard.MAX_LENGTH, "Done hands back what was typed")
	_check(not cancelled[0], "and is not a cancel")
	keys.free()


## The desktop form: one line, no keys, and not something you can point at.
func _types_on_the_desktop() -> void:
	var keys := WorldKeyboard.new()
	root.add_child(keys)
	var accepted := [""]
	keys.accepted.connect(func(text: String) -> void: accepted[0] = text)

	keys.open("Server", "10.0.0.", false)
	_check(keys.is_open(), "the line opens")
	_check(not keys.has_keys(), "with no key grid, because there is a real keyboard")
	_check(keys.hit(Vector3(0.0, 0.0, 1.0), Vector3(0, 0, -1)).is_empty(),
		"and it cannot be pointed at: it is a readout, not a control")

	keys.type("5")
	_check(keys.text() == "10.0.0.5", "typing on the real keyboard reaches it")
	keys.backspace()
	_check(keys.text() == "10.0.0.", "and so does backspace")

	# The stick and the trigger have nothing to drive here, and must not crash
	# trying: both arrive anyway, because main.gd feeds them to whatever is up.
	keys.move(Vector2i(1, 1))
	keys.hold(Vector2i(0, 1), 0.016)
	keys.press()
	_check(keys.text() == "10.0.0.", "a stick or a trigger changes nothing and breaks nothing")

	keys.accept()
	_check(accepted[0] == "10.0.0.", "Enter hands back what was typed")
	keys.close()
	_check(not keys.is_open() and not keys.has_keys(), "and it closes")
	keys.free()
