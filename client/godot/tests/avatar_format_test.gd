extends SceneTree

## Headless test for AvatarFormat:
##   godot --headless --path client/godot --script res://tests/avatar_format_test.gd

var _checks := 0
var _failed := 0


func _initialize() -> void:
	_test_validation()
	_test_presets()
	_test_persistence()
	_test_node_building()
	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)


func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)


func _test_validation() -> void:
	var def := AvatarFormat.default_avatar("Testy")
	_check(AvatarFormat.validate(def) == "", "default avatar is valid")

	var bad_kind := def.duplicate(true)
	bad_kind["kind"] = "world"
	_check(AvatarFormat.validate(bad_kind) != "", "wrong kind is refused")

	var bad_name := def.duplicate(true)
	bad_name["name"] = "   "
	_check(AvatarFormat.validate(bad_name) != "", "empty name is refused")

	var bad_height := def.duplicate(true)
	bad_height["height"] = 99.0
	_check(AvatarFormat.validate(bad_height) != "", "out-of-range height is refused")


func _test_presets() -> void:
	var presets := AvatarFormat.get_presets()
	_check(presets.size() >= 5, "at least 5 presets are defined")
	for p_name: String in presets:
		var p: Dictionary = presets[p_name]
		_check(AvatarFormat.validate(p) == "", "preset '%s' is valid" % p_name)


func _test_persistence() -> void:
	var av := AvatarFormat.default_avatar("UnitTestBot")
	av["colors"]["skin"] = "#ff00aa"
	var err := AvatarFormat.save(av, "UnitTestBot")
	_check(err == "", "saving avatar succeeds")

	var loaded := AvatarFormat.load_avatar("UnitTestBot")
	_check(not loaded.has("error"), "loading avatar succeeds")
	_check(str(loaded.get("name", "")) == "UnitTestBot", "avatar name matches")
	_check(str(loaded.get("colors", {}).get("skin", "")) == "#ff00aa", "custom color preserved")

	var active_ok := AvatarFormat.set_active_avatar_name("UnitTestBot")
	_check(active_ok, "setting active avatar succeeds")
	var active := AvatarFormat.get_active_avatar()
	_check(str(active.get("name", "")) == "UnitTestBot", "active avatar matches")

	# Clean up
	_check(AvatarFormat.delete_avatar("UnitTestBot"), "deleting avatar succeeds")


func _test_node_building() -> void:
	var presets := AvatarFormat.get_presets()
	for p_name: String in presets:
		var node := AvatarFormat.build_preview_avatar(presets[p_name])
		_check(node != null, "build preview node for '%s' succeeds" % p_name)
		_check(node.has_node("Head"), "node has Head")
		_check(node.has_node("Body"), "node has Body")
		_check(node.has_node("Left") and node.has_node("Right"), "node has hands")
		node.free()
