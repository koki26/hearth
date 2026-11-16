extends SceneTree

## Checks the Fireplace client without a Fireplace:
##   godot --headless --path client/godot --script res://tests/fireplace_test.gd
##
## Nothing here touches the network. What is worth checking is the three
## places this client can quietly go wrong: which URL it decides to talk to,
## what it does with an answer it did not expect, and the rows it hands the
## menu.

var _failed := 0
var _checks := 0


func _initialize() -> void:
	_run()


func _check(ok: bool, what: String) -> void:
	_checks += 1
	if not ok:
		_failed += 1
		printerr("FAILED: %s" % what)


func _run() -> void:
	await process_frame
	_urls()
	_rows()
	await _answers()
	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)


## A password must never cross the internet in the clear, so anything that is
## not plainly a local test server gets https. This is also the one place the
## old `host:port` form from an older settings file is dealt with.
func _urls() -> void:
	var details := ConnectDetails.new()
	var fireplace := FireplaceClient.new(details)
	for pair in [
		["https://accounts.example.com", "https://accounts.example.com"],
		["http://127.0.0.1:7800", "http://127.0.0.1:7800"],
		["accounts.example.com", "https://accounts.example.com"],
		["accounts.example.com/", "https://accounts.example.com"],
		# A port says "the one I am running on my own machine".
		["127.0.0.1:7800", "http://127.0.0.1:7800"],
		["  accounts.example.com  ", "https://accounts.example.com"],
	]:
		details.account_server = str(pair[0])
		_check(fireplace.base_url() == str(pair[1]),
			"%s -> %s (got %s)" % [pair[0], pair[1], fireplace.base_url()])
	_check(ConnectDetails.DEFAULT_FIREPLACE.begins_with("https://"),
		"the Fireplace everybody is pointed at by default is an https one")
	fireplace.free()


## The rows the People page draws. A pure function, so it can be checked
## without a menu, a scene or a server.
func _rows() -> void:
	var rows := FireplaceClient.friend_rows(
		[
			{"handle": "ada", "online": true, "flame": "1.2.3.4:7777", "ember": 3, "ember_name": "the loft"},
			{"handle": "bob", "online": false},
		],
		[{"handle": "cat"}],
		"")
	var ids: Array = rows.map(func(row): return str(row.get("id", "")))
	_check(ids.has("friend:ada"), "a friend who is somewhere can be followed")
	_check(not ids.has("friend:bob"), "and one who is not, cannot")
	_check(ids.has("accept:cat"), "somebody asking to be friends can be accepted")
	_check(ids.has("addfriend"), "and there is a way to add one")

	var ada: Dictionary = rows.filter(func(row): return str(row.get("id", "")) == "friend:ada")[0]
	_check(str(ada["sub"]).contains("the loft"), "a friend's row says where they are")
	var typed := FireplaceClient.friend_rows([], [], "half-typed")
	_check(str(typed[-1].get("text", "")) == "half-typed", "what is half typed stays on the row")
	_check(str(typed[1].get("label", "")).contains("nobody"), "an empty list says so")


## What the client does with answers it did not ask for. A server can say
## anything, including nothing at all.
func _answers() -> void:
	var details := ConnectDetails.new()
	var fireplace := FireplaceClient.new(details)
	root.add_child(fireplace)
	await process_frame
	# Unplugged: answers are fed in by hand below, and a test must never
	# reach for the real Fireplace -- the handler sends a follow-up request of
	# its own, which would be a live call to whatever host is configured.
	fireplace._http.queue_free()
	fireplace._http = null
	var complaints := []
	var out := []
	fireplace.failed.connect(func(what: String, message: String) -> void: complaints.append([what, message]))
	fireplace.signed_out.connect(func(why: String) -> void: out.append(why))

	# A sign-in that worked.
	fireplace._tag = "signin"
	fireplace._on_answer(HTTPRequest.RESULT_SUCCESS, 200,
		PackedStringArray(), '{"access":"v1.x","refresh":"v1.y","handle":"ada","id":4}'.to_utf8_buffer())
	_check(fireplace.is_signed_in() and fireplace.handle == "ada", "a good answer signs you in")

	# A token the Fireplace no longer believes: sign out rather than keep asking.
	fireplace._tag = "friends"
	fireplace._on_answer(HTTPRequest.RESULT_SUCCESS, 401, PackedStringArray(), "{}".to_utf8_buffer())
	_check(not fireplace.is_signed_in(), "a refused token signs you out")
	_check(out.size() == 1, "and says so once")

	# Rubbish, and not reaching it at all. Neither may crash the client.
	fireplace._tag = "friends"
	fireplace._on_answer(HTTPRequest.RESULT_SUCCESS, 200, PackedStringArray(), "not json at all".to_utf8_buffer())
	fireplace._tag = "friends"
	fireplace._on_answer(HTTPRequest.RESULT_CANT_CONNECT, 0, PackedStringArray(), PackedByteArray())
	_check(complaints.size() >= 1, "a Fireplace that cannot be reached is one message, not a crash")
	_check(str(complaints[-1][1]).contains("reach"), "and the message says so: %s" % str(complaints[-1][1]))

	# A password is never written to the settings file.
	details.account_token = "v1.x"
	details.save()
	var reloaded := ConnectDetails.new()
	_check(reloaded.account_token == "v1.x", "a token survives a restart, so you stay signed in")
	var text := ConfigFile.new()
	text.load(Session.SETTINGS)
	_check(not text.has_section_key("account", "password"), "and a password is never saved")
	# Put the settings file back as it was found.
	details.account_token = ""
	details.account_refresh = ""
	details.account_handle = ""
	details.save()
	fireplace.queue_free()
