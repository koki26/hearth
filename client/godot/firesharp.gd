class_name FireSharp
extends RefCounted

## FireSharp (Fire#) — The Hearth world scripting language.
## C#-inspired, lightweight, safe component scripting for interactive objects.
##
## Example FireSharp script (Spin.fs):
## ```csharp
## public float speed = 45.0;
## public Vector3 axis = new Vector3(0, 1, 0);
## public bool active = true;
##
## void Start() {
##     Print("Spin script initialized on " + gameObject.name);
## }
##
## void Update(float delta) {
##     if (active) {
##         transform.Rotate(axis * speed * delta);
##     }
## }
##
## void OnInteract() {
##     active = !active;
## }
## ```

const KEYWORDS := [
	"class", "public", "private", "protected", "void", "float", "int", "string",
	"bool", "var", "let", "if", "else", "while", "for", "return", "new",
	"true", "false", "null", "using", "function", "func"
]

## Token kinds
enum TokenKind {
	EOF,
	IDENTIFIER,
	KEYWORD,
	NUMBER,
	STRING,
	OP,
	PUNCT
}

class Token:
	var kind: TokenKind
	var value: Variant
	var line: int
	var col: int

	func _init(k: TokenKind, val: Variant, l: int, c: int) -> void:
		kind = k
		value = val
		line = l
		col = c

	func _to_string() -> String:
		return "Token(%s, %s, L%d:%d)" % [kind, str(value), line, col]


# --- Lexer ------------------------------------------------------------------

static func tokenize(source: String) -> Dictionary:
	var tokens: Array[Token] = []
	var i := 0
	var length := source.length()
	var line := 1
	var col := 1

	while i < length:
		var c := source[i]

		# Newlines and whitespace
		if c == "\n":
			line += 1
			col = 1
			i += 1
			continue
		elif c == "\r" or c == "\t" or c == " ":
			col += 1
			i += 1
			continue

		# Comments
		if c == "/" and i + 1 < length:
			if source[i + 1] == "/":
				# Single line comment
				i += 2
				while i < length and source[i] != "\n":
					i += 1
				continue
			elif source[i + 1] == "*":
				# Multi line comment
				i += 2
				while i + 1 < length and not (source[i] == "*" and source[i + 1] == "/"):
					if source[i] == "\n":
						line += 1
						col = 1
					else:
						col += 1
					i += 1
				i += 2
				continue

		# Numbers
		if (c >= "0" and c <= "9") or (c == "." and i + 1 < length and source[i + 1] >= "0" and source[i + 1] <= "9"):
			var start_col := col
			var num_str := ""
			var is_float := false
			while i < length:
				var ch := source[i]
				if (ch >= "0" and ch <= "9"):
					num_str += ch
					i += 1
					col += 1
				elif ch == "." and not is_float:
					is_float = true
					num_str += ch
					i += 1
					col += 1
				elif (ch == "f" or ch == "F") and is_float:
					i += 1
					col += 1
					break
				else:
					break
			var val: Variant = float(num_str) if is_float else int(num_str)
			tokens.append(Token.new(TokenKind.NUMBER, val, line, start_col))
			continue

		# Strings
		if c == "\"" or c == "'":
			var quote := c
			var start_col := col
			var s := ""
			i += 1
			col += 1
			var escaped := false
			while i < length:
				var ch := source[i]
				if escaped:
					match ch:
						"n": s += "\n"
						"t": s += "\t"
						"r": s += "\r"
						"\\": s += "\\"
						"\"": s += "\""
						"'": s += "'"
						_: s += ch
					escaped = false
				elif ch == "\\":
					escaped = true
				elif ch == quote:
					i += 1
					col += 1
					break
				else:
					s += ch
				i += 1
				col += 1
			tokens.append(Token.new(TokenKind.STRING, s, line, start_col))
			continue

		# Two-character operators
		if i + 1 < length:
			var op2 := source.substr(i, 2)
			if op2 in ["==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/="]:
				tokens.append(Token.new(TokenKind.OP, op2, line, col))
				i += 2
				col += 2
				continue

		# Single-character operators & punctuation
		if c in ["+", "-", "*", "/", "%", "=", "!", "<", ">"]:
			tokens.append(Token.new(TokenKind.OP, c, line, col))
			i += 1
			col += 1
			continue

		if c in [";", ",", ".", "(", ")", "{", "}", "[", "]"]:
			tokens.append(Token.new(TokenKind.PUNCT, c, line, col))
			i += 1
			col += 1
			continue

		# Identifiers and keywords
		if (c >= "a" and c <= "z") or (c >= "A" and c <= "Z") or c == "_":
			var start_col := col
			var ident := ""
			while i < length:
				var ch := source[i]
				if (ch >= "a" and ch <= "z") or (ch >= "A" and ch <= "Z") or (ch >= "0" and ch <= "9") or ch == "_":
					ident += ch
					i += 1
					col += 1
				else:
					break
			if ident in KEYWORDS:
				tokens.append(Token.new(TokenKind.KEYWORD, ident, line, start_col))
			else:
				tokens.append(Token.new(TokenKind.IDENTIFIER, ident, line, start_col))
			continue

		# Unknown character
		i += 1
		col += 1

	tokens.append(Token.new(TokenKind.EOF, "", line, col))
	return {"tokens": tokens, "error": ""}


# --- Static Analysis / Reflection ------------------------------------------

## Extracts public exposed fields from source code for the Unity-style inspector.
## Looks for: public <type> <name> = <val>;
static func get_exposed_fields(source: String) -> Array[Dictionary]:
	var res := tokenize(source)
	var tokens: Array[Token] = res.get("tokens", [])
	var fields: Array[Dictionary] = []
	var i := 0
	var n := tokens.size()

	while i < n:
		var tok: Token = tokens[i]
		if tok.kind == TokenKind.KEYWORD and tok.value == "public":
			# Expected: public [type] name [= default_val] ;
			i += 1
			if i >= n: break
			var type_name := "var"
			var tok_next: Token = tokens[i]
			if tok_next.kind == TokenKind.KEYWORD or tok_next.kind == TokenKind.IDENTIFIER:
				type_name = str(tok_next.value)
				i += 1
			if i >= n: break
			var name_tok: Token = tokens[i]
			if name_tok.kind != TokenKind.IDENTIFIER:
				i += 1
				continue
			var field_name := str(name_tok.value)
			var default_val: Variant = null

			i += 1
			if i < n and tokens[i].kind == TokenKind.OP and tokens[i].value == "=":
				i += 1
				# Parse default value
				var expr_tokens: Array[Token] = []
				while i < n and not (tokens[i].kind == TokenKind.PUNCT and tokens[i].value == ";"):
					expr_tokens.append(tokens[i])
					i += 1
				default_val = _eval_literal(expr_tokens)
			else:
				# Default based on type
				match type_name:
					"float": default_val = 0.0
					"int": default_val = 0
					"string": default_val = ""
					"bool": default_val = false
					"Vector3": default_val = Vector3.ZERO
					"Color": default_val = Color.WHITE

			fields.append({
				"name": field_name,
				"type": type_name,
				"default": default_val,
				"value": default_val,
			})
		i += 1
	return fields


static func _eval_literal(tokens: Array[Token]) -> Variant:
	if tokens.is_empty():
		return null
	if tokens.size() == 1:
		var t := tokens[0]
		if t.kind == TokenKind.NUMBER: return t.value
		if t.kind == TokenKind.STRING: return t.value
		if t.kind == TokenKind.KEYWORD:
			if t.value == "true": return true
			if t.value == "false": return false
			if t.value == "null": return null

	# Check for Vector3(x, y, z) or new Vector3(x, y, z)
	var offset := 0
	if tokens[0].kind == TokenKind.KEYWORD and tokens[0].value == "new":
		offset = 1
	if tokens.size() > offset and tokens[offset].value == "Vector3":
		# Read numbers in paren
		var nums: Array[float] = []
		for idx in range(offset + 1, tokens.size()):
			if tokens[idx].kind == TokenKind.NUMBER:
				nums.append(float(tokens[idx].value))
			elif tokens[idx].kind == TokenKind.OP and tokens[idx].value == "-" and idx + 1 < tokens.size() and tokens[idx+1].kind == TokenKind.NUMBER:
				nums.append(-float(tokens[idx+1].value))
		if nums.size() >= 3:
			return Vector3(nums[0], nums[1], nums[2])
		return Vector3.ZERO

	# Check for Color or string hex
	if tokens.size() > offset and tokens[offset].value == "Color":
		var nums: Array[float] = []
		for idx in range(offset + 1, tokens.size()):
			if tokens[idx].kind == TokenKind.NUMBER:
				nums.append(float(tokens[idx].value))
			elif tokens[idx].kind == TokenKind.STRING:
				return Color.html(str(tokens[idx].value))
		if nums.size() >= 3:
			return Color(nums[0], nums[1], nums[2], nums[3] if nums.size() > 3 else 1.0)
		return Color.WHITE

	# Check for Vector3.UP, etc.
	if tokens.size() == 3 and tokens[0].value == "Vector3" and tokens[1].value == ".":
		match str(tokens[2].value).to_upper():
			"UP": return Vector3.UP
			"DOWN": return Vector3.DOWN
			"LEFT": return Vector3.LEFT
			"RIGHT": return Vector3.RIGHT
			"FORWARD": return Vector3.FORWARD
			"BACK": return Vector3.BACK
			"ZERO": return Vector3.ZERO
			"ONE": return Vector3.ONE

	if tokens[0].kind == TokenKind.NUMBER:
		return tokens[0].value
	if tokens[0].kind == TokenKind.STRING:
		return tokens[0].value

	return null


# --- Syntax Validation -----------------------------------------------------

static func validate(source: String) -> Dictionary:
	var res := tokenize(source)
	var tokens: Array[Token] = res.get("tokens", [])
	# Basic bracket matching and syntax check
	var stack: Array[String] = []
	for tok in tokens:
		if tok.kind == TokenKind.PUNCT:
			if tok.value in ["{", "(", "["]:
				stack.append(str(tok.value))
			elif tok.value in ["}", ")", "]"]:
				if stack.is_empty():
					return {"ok": false, "error": "Unmatched closing '%s'" % tok.value, "line": tok.line, "col": tok.col}
				var open_b := stack.pop_back()
				if (tok.value == "}" and open_b != "{") or (tok.value == ")" and open_b != "(") or (tok.value == "]" and open_b != "["):
					return {"ok": false, "error": "Mismatched bracket '%s' with '%s'" % [open_b, tok.value], "line": tok.line, "col": tok.col}
	if not stack.is_empty():
		return {"ok": false, "error": "Unclosed '%s'" % stack[-1], "line": tokens[-1].line if not tokens.is_empty() else 1, "col": 1}
	return {"ok": true, "error": "", "line": 0, "col": 0}


# --- FireSharp Instance Runtime --------------------------------------------

## Represents an executable instance of a FireSharp script bound to a Node3D.
class Instance:
	var target_node: Node3D
	var script_name := ""
	var source_code := ""
	var variables: Dictionary = {}
	var functions: Dictionary = {} # name -> function AST node
	var parsed_ok: bool = false
	var error_message: String = ""
	var running_time: float = 0.0

	func _init(node: Node3D, script: String, name: String = "script.fs") -> void:
		target_node = node
		source_code = script
		script_name = name
		_parse_and_bind()

	func set_variable(var_name: String, value: Variant) -> void:
		variables[var_name] = value

	func get_variable(var_name: String) -> Variant:
		return variables.get(var_name, null)

	func call_method(method_name: String, args: Array = []) -> Variant:
		if not functions.has(method_name):
			return null
		return _execute_func(functions[method_name], args)

	func start() -> void:
		if functions.has("Start"):
			call_method("Start", [])
		elif functions.has("OnStart"):
			call_method("OnStart", [])

	func update(delta: float) -> void:
		running_time += delta
		if functions.has("Update"):
			call_method("Update", [delta])
		elif functions.has("OnUpdate"):
			call_method("OnUpdate", [delta])

	func on_interact(player: Variant = null) -> void:
		if functions.has("OnInteract"):
			call_method("OnInteract", [player] if player != null else [])

	func on_grab() -> void:
		if functions.has("OnGrab"):
			call_method("OnGrab", [])

	func on_drop() -> void:
		if functions.has("OnDrop"):
			call_method("OnDrop", [])

	func _parse_and_bind() -> void:
		var lex := FireSharp.tokenize(source_code)
		var tokens: Array[FireSharp.Token] = lex.get("tokens", [])
		var parser := _Parser.new(tokens)
		var ast := parser.parse_script()
		if parser.has_error:
			parsed_ok = false
			error_message = parser.error_str
			return

		parsed_ok = true
		# Extract global/class fields and functions
		for stmt in ast.get("body", []):
			match stmt.get("type"):
				"var_decl":
					var val := _eval(stmt.get("value"), {})
					variables[stmt["name"]] = val
				"func_decl":
					functions[stmt["name"]] = stmt
				"class_decl":
					for member in stmt.get("members", []):
						if member.get("type") == "var_decl":
							variables[member["name"]] = _eval(member.get("value"), {})
						elif member.get("type") == "func_decl":
							functions[member["name"]] = member

	func _execute_func(func_node: Dictionary, args: Array) -> Variant:
		var scope := {}
		var params: Array = func_node.get("params", [])
		for i in params.size():
			var p_name: String = params[i]
			scope[p_name] = args[i] if i < args.size() else null

		for stmt in func_node.get("body", []):
			var ret := _exec_stmt(stmt, scope)
			if ret != null and typeof(ret) == TYPE_DICTIONARY and ret.get("__return__", false):
				return ret.get("value")
		return null

	func _exec_stmt(stmt: Dictionary, scope: Dictionary) -> Variant:
		match stmt.get("type"):
			"var_decl":
				var val := _eval(stmt.get("value"), scope)
				scope[stmt["name"]] = val
				return null
			"assign":
				var val := _eval(stmt.get("value"), scope)
				var target = stmt.get("target")
				if typeof(target) == TYPE_STRING:
					var target_name: String = target
					if scope.has(target_name):
						scope[target_name] = _apply_assign_op(stmt.get("op", "="), scope[target_name], val)
					elif variables.has(target_name):
						variables[target_name] = _apply_assign_op(stmt.get("op", "="), variables[target_name], val)
					else:
						scope[target_name] = val
				elif typeof(target) == TYPE_DICTIONARY and target.get("type") == "member":
					_set_member(target["obj"], target["member"], val, scope, stmt.get("op", "="))
				return null
			"if":
				var cond := bool(_eval(stmt.get("condition"), scope))
				if cond:
					for s in stmt.get("then", []):
						var ret := _exec_stmt(s, scope)
						if ret != null and typeof(ret) == TYPE_DICTIONARY and ret.get("__return__", false):
							return ret
				elif stmt.has("else"):
					for s in stmt.get("else", []):
						var ret := _exec_stmt(s, scope)
						if ret != null and typeof(ret) == TYPE_DICTIONARY and ret.get("__return__", false):
							return ret
				return null
			"while":
				var iterations := 0
				while bool(_eval(stmt.get("condition"), scope)) and iterations < 10000:
					iterations += 1
					for s in stmt.get("body", []):
						var ret := _exec_stmt(s, scope)
						if ret != null and typeof(ret) == TYPE_DICTIONARY and ret.get("__return__", false):
							return ret
				return null
			"expr_stmt":
				_eval(stmt.get("expr"), scope)
				return null
			"return":
				var val := _eval(stmt.get("value"), scope)
				return {"__return__": true, "value": val}
		return null

	func _apply_assign_op(op: String, current: Variant, incoming: Variant) -> Variant:
		match op:
			"+=": return current + incoming
			"-=": return current - incoming
			"*=": return current * incoming
			"/=": return current / incoming if incoming != 0 else current
			_: return incoming

	func _eval(expr: Variant, scope: Dictionary) -> Variant:
		if typeof(expr) != TYPE_DICTIONARY:
			return expr

		match expr.get("type"):
			"literal":
				return expr.get("value")
			"ident":
				var name: String = expr.get("name")
				if scope.has(name): return scope[name]
				if variables.has(name): return variables[name]
				return _resolve_global(name)
			"unary":
				var val := _eval(expr.get("expr"), scope)
				match expr.get("op"):
					"-": return -val
					"!": return not bool(val)
				return val
			"binary":
				var left := _eval(expr.get("left"), scope)
				var right := _eval(expr.get("right"), scope)
				match expr.get("op"):
					"+": return left + right
					"-": return left - right
					"*": return left * right
					"/": return (left / right) if right != 0 else 0
					"%": return (int(left) % int(right)) if right != 0 else 0
					"==": return left == right
					"!=": return left != right
					"<": return left < right
					"<=": return left <= right
					">": return left > right
					">=": return left >= right
					"&&": return bool(left) and bool(right)
					"||": return bool(left) or bool(right)
				return null
			"call":
				var callee = expr.get("callee")
				var args: Array = []
				for a in expr.get("args", []):
					args.append(_eval(a, scope))
				return _call_func(callee, args, scope)
			"member":
				var obj := _eval(expr.get("obj"), scope)
				return _get_member(obj, expr.get("member"))
			"new_vec":
				var x: float = float(_eval(expr.get("x", 0.0), scope))
				var y: float = float(_eval(expr.get("y", 0.0), scope))
				var z: float = float(_eval(expr.get("z", 0.0), scope))
				return Vector3(x, y, z)
		return null

	func _resolve_global(name: String) -> Variant:
		match name:
			"transform": return {"__type__": "transform", "node": target_node}
			"gameObject", "node": return {"__type__": "gameObject", "node": target_node}
			"light": return {"__type__": "light", "node": target_node}
			"rigidbody", "physics": return {"__type__": "physics", "node": target_node}
			"Mathf": return {"__type__": "Mathf"}
			"Time": return {"time": running_time}
			"Vector3": return {"__type__": "Vector3Static"}
			"Color": return {"__type__": "ColorStatic"}
			"PI": return PI
		return null

	func _get_member(obj: Variant, member: String) -> Variant:
		if typeof(obj) == TYPE_VECTOR3:
			match member:
				"x": return obj.x
				"y": return obj.y
				"z": return obj.z
				"length": return obj.length()
				"normalized": return obj.normalized()
			return null
		elif typeof(obj) == TYPE_COLOR:
			match member:
				"r": return obj.r
				"g": return obj.g
				"b": return obj.b
				"a": return obj.a
			return null
		elif typeof(obj) == TYPE_DICTIONARY:
			var t := str(obj.get("__type__", ""))
			if t == "transform" and target_node != null:
				match member:
					"position": return target_node.position
					"rotation": return Vector3(rad_to_deg(target_node.rotation.x), rad_to_deg(target_node.rotation.y), rad_to_deg(target_node.rotation.z))
					"scale": return target_node.scale
			elif t == "gameObject" and target_node != null:
				match member:
					"name": return target_node.name
					"visible": return target_node.visible
			elif t == "light" and target_node != null:
				var l := _find_light(target_node)
				if l != null:
					match member:
						"energy": return l.light_energy
						"range": return l.omni_range if l is OmniLight3D else 10.0
						"color": return l.light_color
			elif t == "Vector3Static":
				match member.to_upper():
					"UP": return Vector3.UP
					"DOWN": return Vector3.DOWN
					"LEFT": return Vector3.LEFT
					"RIGHT": return Vector3.RIGHT
					"FORWARD": return Vector3.FORWARD
					"BACK": return Vector3.BACK
					"ZERO": return Vector3.ZERO
					"ONE": return Vector3.ONE
			elif t == "ColorStatic":
				match member.to_upper():
					"WHITE": return Color.WHITE
					"BLACK": return Color.BLACK
					"RED": return Color.RED
					"GREEN": return Color.GREEN
					"BLUE": return Color.BLUE
			return obj.get(member, null)
		return null

	func _set_member(obj_expr: Variant, member: String, val: Variant, scope: Dictionary, op: String = "=") -> void:
		var obj := _eval(obj_expr, scope)
		if typeof(obj) == TYPE_DICTIONARY:
			var t := str(obj.get("__type__", ""))
			if t == "transform" and target_node != null:
				match member:
					"position":
						var cur: Vector3 = target_node.position
						target_node.position = _apply_assign_op(op, cur, val)
					"rotation":
						var cur_deg := Vector3(rad_to_deg(target_node.rotation.x), rad_to_deg(target_node.rotation.y), rad_to_deg(target_node.rotation.z))
						var new_deg: Vector3 = _apply_assign_op(op, cur_deg, val)
						target_node.rotation = Vector3(deg_to_rad(new_deg.x), deg_to_rad(new_deg.y), deg_to_rad(new_deg.z))
					"scale":
						var cur_s: Vector3 = target_node.scale
						target_node.scale = _apply_assign_op(op, cur_s, val)
			elif t == "gameObject" and target_node != null:
				match member:
					"name": target_node.name = StringName(str(val))
					"visible": target_node.visible = bool(val)
			elif t == "light" and target_node != null:
				var l := _find_light(target_node)
				if l != null:
					match member:
						"energy": l.light_energy = float(val)
						"range":
							if l is OmniLight3D: l.omni_range = float(val)
						"color": l.light_color = val if val is Color else Color.html(str(val))

	func _call_func(callee: Variant, args: Array, scope: Dictionary) -> Variant:
		if typeof(callee) == TYPE_STRING:
			var func_name: String = callee
			match func_name:
				"Print", "print":
					var msg := ""
					for a in args: msg += str(a) + " "
					print("[FireSharp: %s] %s" % [script_name, msg.strip_edges()])
					return null
				"Vector3":
					if args.size() >= 3:
						return Vector3(float(args[0]), float(args[1]), float(args[2]))
					return Vector3.ZERO
				"Color":
					if args.size() == 1 and typeof(args[0]) == TYPE_STRING:
						return Color.html(str(args[0]))
					elif args.size() >= 3:
						return Color(float(args[0]), float(args[1]), float(args[2]), float(args[3]) if args.size() > 3 else 1.0)
					return Color.WHITE
				"RandomRange":
					var mn := float(args[0]) if args.size() > 0 else 0.0
					var mx := float(args[1]) if args.size() > 1 else 1.0
					return randf_range(mn, mx)
				"SetColor":
					if args.size() > 0 and target_node != null:
						_set_node_color(target_node, args[0])
					return null
				_:
					if functions.has(func_name):
						return _execute_func(functions[func_name], args)
		elif typeof(callee) == TYPE_DICTIONARY and callee.get("type") == "member":
			var obj := _eval(callee.get("obj"), scope)
			var method_name: String = callee.get("member")
			if typeof(obj) == TYPE_DICTIONARY:
				var t := str(obj.get("__type__", ""))
				if t == "transform" and target_node != null:
					match method_name:
						"Rotate":
							if args.size() == 1 and args[0] is Vector3:
								var deg: Vector3 = args[0]
								target_node.rotate_x(deg_to_rad(deg.x))
								target_node.rotate_y(deg_to_rad(deg.y))
								target_node.rotate_z(deg_to_rad(deg.z))
							elif args.size() == 2:
								var axis: Vector3 = args[0]
								var angle_deg: float = float(args[1])
								target_node.rotate(axis.normalized(), deg_to_rad(angle_deg))
							return null
						"Translate":
							if args.size() == 1 and args[0] is Vector3:
								target_node.translate(args[0])
							return null
						"LookAt":
							if args.size() >= 1 and args[0] is Vector3:
								var up: Vector3 = args[1] if args.size() > 1 and args[1] is Vector3 else Vector3.UP
								target_node.look_at(args[0], up)
							return null
				elif t == "Mathf":
					match method_name:
						"Sin": return sin(float(args[0])) if not args.is_empty() else 0.0
						"Cos": return cos(float(args[0])) if not args.is_empty() else 0.0
						"Tan": return tan(float(args[0])) if not args.is_empty() else 0.0
						"Abs": return absf(float(args[0])) if not args.is_empty() else 0.0
						"Clamp": return clampf(float(args[0]), float(args[1]), float(args[2])) if args.size() >= 3 else 0.0
						"Lerp": return lerpf(float(args[0]), float(args[1]), float(args[2])) if args.size() >= 3 else 0.0
						"Round": return roundf(float(args[0])) if not args.is_empty() else 0.0
						"Floor": return floorf(float(args[0])) if not args.is_empty() else 0.0
						"Ceil": return ceilf(float(args[0])) if not args.is_empty() else 0.0
						"Sqrt": return sqrt(float(args[0])) if not args.is_empty() else 0.0
		return null

	func _find_light(n: Node) -> Light3D:
		if n is Light3D: return n
		for c in n.get_children():
			var l := _find_light(c)
			if l != null: return l
		return null

	func _set_node_color(n: Node, c_val: Variant) -> void:
		var color := c_val if c_val is Color else Color.html(str(c_val))
		for mesh_inst in _find_meshes(n):
			var mat: Material = mesh_inst.material_override
			if mat == null:
				mat = mesh_inst.get_active_material(0)
			if mat is StandardMaterial3D:
				var m := (mat as StandardMaterial3D).duplicate()
				m.albedo_color = color
				mesh_inst.material_override = m

	func _find_meshes(n: Node) -> Array[MeshInstance3D]:
		var res: Array[MeshInstance3D] = []
		if n is MeshInstance3D: res.append(n)
		for c in n.get_children():
			res.append_array(_find_meshes(c))
		return res


# --- Parser AST -------------------------------------------------------------

class _Parser:
	var tokens: Array[Token]
	var pos := 0
	var has_error := false
	var error_str := ""

	func _init(toks: Array[Token]) -> void:
		tokens = toks

	func cur() -> Token:
		return tokens[pos] if pos < tokens.size() else tokens[-1]

	func peek() -> Token:
		return tokens[pos + 1] if pos + 1 < tokens.size() else tokens[-1]

	func advance() -> Token:
		var t := cur()
		pos += 1
		return t

	func match_punct(p: String) -> bool:
		if cur().kind == TokenKind.PUNCT and cur().value == p:
			advance()
			return true
		return false

	func match_op(o: String) -> bool:
		if cur().kind == TokenKind.OP and cur().value == o:
			advance()
			return true
		return false

	func match_keyword(k: String) -> bool:
		if cur().kind == TokenKind.KEYWORD and cur().value == k:
			advance()
			return true
		return false

	func parse_script() -> Dictionary:
		var body: Array[Dictionary] = []
		while cur().kind != TokenKind.EOF:
			if match_keyword("using"):
				# Skip namespace
				while cur().kind != TokenKind.EOF and not match_punct(";"):
					advance()
				continue
			var stmt := parse_top_level()
			if not stmt.is_empty():
				body.append(stmt)
			else:
				advance()
		return {"type": "script", "body": body}

	func parse_top_level() -> Dictionary:
		if match_keyword("class"):
			var class_name_str := str(advance().value)
			# optional : BaseClass
			if match_op(":"):
				advance()
			match_punct("{")
			var members: Array[Dictionary] = []
			while not match_punct("}") and cur().kind != TokenKind.EOF:
				var member := parse_member()
				if not member.is_empty():
					members.append(member)
				else:
					advance()
			return {"type": "class_decl", "name": class_name_str, "members": members}
		return parse_member()

	func parse_member() -> Dictionary:
		# Could be public/private or direct type/void
		var is_public := false
		if match_keyword("public"):
			is_public = true
		elif match_keyword("private") or match_keyword("protected"):
			is_public = false

		var type_or_ident := str(advance().value)
		if cur().kind == TokenKind.IDENTIFIER and peek().kind == TokenKind.PUNCT and peek().value == "(":
			# Function: type name ( params ) { body }
			var func_name := str(advance().value)
			match_punct("(")
			var params: Array[String] = []
			while not match_punct(")") and cur().kind != TokenKind.EOF:
				# type param or just param
				var p := str(advance().value)
				if cur().kind == TokenKind.IDENTIFIER:
					p = str(advance().value)
				params.append(p)
				match_punct(",")
			var body := parse_block()
			return {"type": "func_decl", "name": func_name, "params": params, "body": body, "return_type": type_or_ident, "public": is_public}
		elif cur().kind == TokenKind.IDENTIFIER:
			# Field: type name = expr;
			var field_name := str(advance().value)
			var val_expr: Variant = null
			if match_op("="):
				val_expr = parse_expr()
			match_punct(";")
			return {"type": "var_decl", "name": field_name, "value": val_expr, "data_type": type_or_ident, "public": is_public}
		return {}

	func parse_block() -> Array[Dictionary]:
		var stmts: Array[Dictionary] = []
		match_punct("{")
		while not match_punct("}") and cur().kind != TokenKind.EOF:
			var s := parse_stmt()
			if not s.is_empty():
				stmts.append(s)
		return stmts

	func parse_stmt() -> Dictionary:
		if match_keyword("if"):
			match_punct("(")
			var cond := parse_expr()
			match_punct(")")
			var then_block: Array[Dictionary] = []
			if cur().kind == TokenKind.PUNCT and cur().value == "{":
				then_block = parse_block()
			else:
				then_block = [parse_stmt()]
			var else_block: Array[Dictionary] = []
			if match_keyword("else"):
				if cur().kind == TokenKind.PUNCT and cur().value == "{":
					else_block = parse_block()
				else:
					else_block = [parse_stmt()]
			return {"type": "if", "condition": cond, "then": then_block, "else": else_block}
		elif match_keyword("while"):
			match_punct("(")
			var cond := parse_expr()
			match_punct(")")
			var body := parse_block()
			return {"type": "while", "condition": cond, "body": body}
		elif match_keyword("return"):
			var val: Variant = null
			if not (cur().kind == TokenKind.PUNCT and cur().value == ";"):
				val = parse_expr()
			match_punct(";")
			return {"type": "return", "value": val}
		elif cur().kind == TokenKind.KEYWORD and cur().value in ["var", "let", "float", "int", "string", "bool", "Vector3", "Color"]:
			var data_type := str(advance().value)
			var var_name := str(advance().value)
			var val_expr: Variant = null
			if match_op("="):
				val_expr = parse_expr()
			match_punct(";")
			return {"type": "var_decl", "name": var_name, "value": val_expr, "data_type": data_type}
		else:
			# Expression or assignment
			var expr := parse_expr()
			if cur().kind == TokenKind.OP and cur().value in ["=", "+=", "-=", "*=", "/="]:
				var op := str(advance().value)
				var rhs := parse_expr()
				match_punct(";")
				return {"type": "assign", "target": expr.get("name") if expr.get("type") == "ident" else expr, "op": op, "value": rhs}
			match_punct(";")
			return {"type": "expr_stmt", "expr": expr}

	func parse_expr() -> Dictionary:
		return parse_logic_or()

	func parse_logic_or() -> Dictionary:
		var left := parse_logic_and()
		while cur().kind == TokenKind.OP and cur().value == "||":
			var op := str(advance().value)
			var right := parse_logic_and()
			left = {"type": "binary", "op": op, "left": left, "right": right}
		return left

	func parse_logic_and() -> Dictionary:
		var left := parse_equality()
		while cur().kind == TokenKind.OP and cur().value == "&&":
			var op := str(advance().value)
			var right := parse_equality()
			left = {"type": "binary", "op": op, "left": left, "right": right}
		return left

	func parse_equality() -> Dictionary:
		var left := parse_comparison()
		while cur().kind == TokenKind.OP and cur().value in ["==", "!="]:
			var op := str(advance().value)
			var right := parse_comparison()
			left = {"type": "binary", "op": op, "left": left, "right": right}
		return left

	func parse_comparison() -> Dictionary:
		var left := parse_term()
		while cur().kind == TokenKind.OP and cur().value in ["<", "<=", ">", ">="]:
			var op := str(advance().value)
			var right := parse_term()
			left = {"type": "binary", "op": op, "left": left, "right": right}
		return left

	func parse_term() -> Dictionary:
		var left := parse_factor()
		while cur().kind == TokenKind.OP and cur().value in ["+", "-"]:
			var op := str(advance().value)
			var right := parse_factor()
			left = {"type": "binary", "op": op, "left": left, "right": right}
		return left

	func parse_factor() -> Dictionary:
		var left := parse_unary()
		while cur().kind == TokenKind.OP and cur().value in ["*", "/", "%"]:
			var op := str(advance().value)
			var right := parse_unary()
			left = {"type": "binary", "op": op, "left": left, "right": right}
		return left

	func parse_unary() -> Dictionary:
		if cur().kind == TokenKind.OP and cur().value in ["-", "!"]:
			var op := str(advance().value)
			var expr := parse_unary()
			return {"type": "unary", "op": op, "expr": expr}
		return parse_postfix()

	func parse_postfix() -> Dictionary:
		var expr := parse_primary()
		while true:
			if match_punct("."):
				var member := str(advance().value)
				if match_punct("("):
					# Member call: obj.method(args)
					var args: Array[Dictionary] = []
					while not match_punct(")") and cur().kind != TokenKind.EOF:
						args.append(parse_expr())
						match_punct(",")
					expr = {"type": "call", "callee": {"type": "member", "obj": expr, "member": member}, "args": args}
				else:
					expr = {"type": "member", "obj": expr, "member": member}
			elif match_punct("("):
				# Direct call: func(args)
				var args: Array[Dictionary] = []
				while not match_punct(")") and cur().kind != TokenKind.EOF:
					args.append(parse_expr())
					match_punct(",")
				expr = {"type": "call", "callee": expr.get("name") if expr.get("type") == "ident" else expr, "args": args}
			else:
				break
		return expr

	func parse_primary() -> Dictionary:
		var tok := cur()
		if tok.kind == TokenKind.NUMBER:
			advance()
			return {"type": "literal", "value": tok.value}
		elif tok.kind == TokenKind.STRING:
			advance()
			return {"type": "literal", "value": tok.value}
		elif tok.kind == TokenKind.KEYWORD:
			if tok.value == "true":
				advance()
				return {"type": "literal", "value": true}
			elif tok.value == "false":
				advance()
				return {"type": "literal", "value": false}
			elif tok.value == "null":
				advance()
				return {"type": "literal", "value": null}
			elif tok.value == "new":
				advance()
				var cls_name := str(advance().value)
				match_punct("(")
				var args: Array[Dictionary] = []
				while not match_punct(")") and cur().kind != TokenKind.EOF:
					args.append(parse_expr())
					match_punct(",")
				if cls_name == "Vector3":
					return {
						"type": "new_vec",
						"x": args[0] if args.size() > 0 else 0.0,
						"y": args[1] if args.size() > 1 else 0.0,
						"z": args[2] if args.size() > 2 else 0.0
					}
				return {"type": "call", "callee": cls_name, "args": args}
		elif tok.kind == TokenKind.IDENTIFIER:
			advance()
			return {"type": "ident", "name": str(tok.value)}
		elif match_punct("("):
			var inner := parse_expr()
			match_punct(")")
			return inner
		advance()
		return {"type": "literal", "value": null}
