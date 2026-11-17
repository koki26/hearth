extends SceneTree

## Unit test suite for FireSharp (Fire#) scripting engine
## Runs headless via: godot --headless --path client/godot --script res://tests/firesharp_test.gd

var _failed := 0
var _checks := 0

func _initialize() -> void:
	print("--- Running FireSharp Tests ---")
	_test_lexer()
	_test_reflection()
	_test_validation()
	_test_runtime_execution()
	_test_vector_and_math()
	print("FireSharp tests: %d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)

func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)

func _test_lexer() -> void:
	var code := "public float speed = 45.0; // comment\nVector3 axis = new Vector3(0, 1, 0);"
	var res := FireSharp.tokenize(code)
	var tokens: Array[FireSharp.Token] = res.get("tokens", [])
	_check(tokens.size() > 5, "Lexer produced tokens")
	_check(tokens[0].value == "public", "First token is public")
	_check(tokens[1].value == "float", "Second token is float")
	_check(tokens[2].value == "speed", "Third token is speed")
	_check(tokens[3].value == "=", "Fourth token is =")
	_check(is_equal_approx(float(tokens[4].value), 45.0), "Fifth token is 45.0")

func _test_reflection() -> void:
	var code := """
	public float speed = 60.0;
	public int count = 5;
	public string label = "Door";
	public bool active = true;
	public Vector3 axis = new Vector3(0, 1, 0);
	public Color tint = "#ffaa00";
	"""
	var fields := FireSharp.get_exposed_fields(code)
	_check(fields.size() == 6, "Found all 6 exposed fields: %d" % fields.size())
	_check(fields[0]["name"] == "speed" and is_equal_approx(fields[0]["default"], 60.0), "Field 0 speed float")
	_check(fields[1]["name"] == "count" and fields[1]["default"] == 5, "Field 1 count int")
	_check(fields[2]["name"] == "label" and fields[2]["default"] == "Door", "Field 2 label string")
	_check(fields[3]["name"] == "active" and fields[3]["default"] == true, "Field 3 active bool")
	_check(fields[4]["name"] == "axis" and fields[4]["default"] == Vector3(0, 1, 0), "Field 4 axis Vector3")

func _test_validation() -> void:
	var good := "void Start() { if (true) { print(\"ok\"); } }"
	_check(FireSharp.validate(good)["ok"], "Good syntax passes validation")
	var bad := "void Start() { if (true) { print(\"unclosed\"); }"
	_check(not FireSharp.validate(bad)["ok"], "Unclosed brace fails validation")

func _test_runtime_execution() -> void:
	var node := Node3D.new()
	var code := """
	public float speed = 10.0;
	var counter = 0;

	void Start() {
		counter = 100;
	}

	void Update(float dt) {
		counter += 1;
		transform.position += new Vector3(1, 0, 0) * dt;
	}

	void OnInteract() {
		counter = 999;
	}
	"""
	var inst := FireSharp.Instance.new(node, code, "Test.fs")
	_check(inst.parsed_ok, "Instance parsed successfully: %s" % inst.error_message)

	inst.start()
	_check(inst.get_variable("counter") == 100, "Start() sets counter to 100")

	inst.update(0.5)
	_check(inst.get_variable("counter") == 101, "Update() increments counter to 101")
	_check(is_equal_approx(node.position.x, 0.5), "Update() moved transform.position.x to 0.5")

	inst.on_interact()
	_check(inst.get_variable("counter") == 999, "OnInteract() sets counter to 999")
	node.free()

func _test_vector_and_math() -> void:
	var node := Node3D.new()
	var code := """
	var val = 0.0;
	void Start() {
		var s = Mathf.Sin(0.0);
		var l = Mathf.Lerp(10.0, 20.0, 0.5);
		val = l + s;
	}
	"""
	var inst := FireSharp.Instance.new(node, code, "Math.fs")
	_check(inst.parsed_ok, "Math script parsed ok")
	inst.start()
	_check(is_equal_approx(inst.get_variable("val"), 15.0), "Mathf.Lerp and Sin computed correctly: 15.0")
	node.free()
