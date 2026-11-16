extends SceneTree

## Checks a real Fireplace with the real client, end to end:
##
##   godot --headless --path client/godot --script res://tools/check_fireplace.gd
##   godot --headless --path client/godot --script res://tools/check_fireplace.gd \
##       -- --fireplace=https://fireplace.example.org
##
## It makes two throwaway accounts, befriends them, puts one of them somewhere
## and checks the other can see where. Not part of `tests/` on purpose: those
## must pass with nothing running. This one is how you know a box you just
## deployed actually works (docs/ORACLE.md).
##
## It leaves two accounts behind. There is no deletion yet, so point it at a
## real Fireplace only when you mean to.

const DEFAULT := "http://127.0.0.1:7800"

var _failed := 0
var _checks := 0


func _initialize() -> void:
	_run()


func _check(ok: bool, what: String) -> void:
	_checks += 1
	print(("  ok    " if ok else "  FAIL  ") + what)
	if not ok:
		_failed += 1


## Lets real time pass while the tree keeps ticking, because an HTTP request
## finishes when the network says so and not when the frame counter does.
func _settle(seconds := 2.0) -> void:
	await create_timer(seconds).timeout


func _client(where: String) -> FireplaceClient:
	var details := ConnectDetails.new()
	details.account_server = where
	details.account_refresh = ""
	details.account_token = ""
	var fireplace := FireplaceClient.new(details)
	root.add_child(fireplace)
	# Anything it could not do is printed: a check that fails silently is
	# worse than no check.
	fireplace.failed.connect(func(what: String, message: String) -> void:
		print("  ...  %s: %s" % [what, message]))
	return fireplace


func _run() -> void:
	await process_frame
	var where := DEFAULT
	for argument in OS.get_cmdline_user_args():
		if argument.begins_with("--fireplace="):
			where = argument.split("=", true, 1)[1]
	print("Checking %s" % where)

	# Handles have to be new every run: there is no deletion.
	var stamp := str(Time.get_unix_time_from_system()).md5_text().substr(0, 8)
	var ada_handle := "ada" + stamp
	var bob_handle := "bob" + stamp
	var ada := _client(where)
	var bob := _client(where)

	ada.create_account(ada_handle, "a long enough one", "1990-05-04", true, false)
	bob.create_account(bob_handle, "a long enough one", "1990-05-04", true, false)
	await _settle()
	_check(ada.is_signed_in() and bob.is_signed_in(), "two accounts made and signed in")
	if not ada.is_signed_in():
		print("%d checks, %d failed" % [_checks, _failed])
		quit(1)
		return

	ada.add_friend(bob_handle)
	await _settle()
	bob.refresh_friends()
	await _settle()
	_check(bob.incoming.size() == 1, "the request arrived")

	bob.accept_friend(ada_handle)
	await _settle()
	ada.refresh_friends()
	await _settle()
	_check(ada.friends.size() == 1 and bob.friends.size() == 1, "and friendship is mutual")

	ada.report("127.0.0.1:7777", 3, "the loft", "forest")
	await _settle()
	bob.refresh_friends()
	await _settle()
	var seen: Dictionary = bob.friends[0] if not bob.friends.is_empty() else {}
	_check(bool(seen.get("online", false)), "Bob can see Ada is somewhere")
	_check(str(seen.get("flame", "")) == "127.0.0.1:7777", "and on which Flame")
	_check(int(seen.get("ember", -1)) == 3, "and in which ember")
	var ids: Array = FireplaceClient.friend_rows(bob.friends, bob.incoming, "") \
		.map(func(row): return str(row.get("id", "")))
	_check(ids.has("friend:" + ada_handle), "so the menu draws a row that follows her")

	ada.set_sharing(false)
	await _settle()
	bob.refresh_friends()
	await _settle()
	_check(not bool(bob.friends[0].get("online", true)), "sharing off hides her at once")

	# The token saved on this machine is enough to come back signed in.
	var again := FireplaceClient.new(ada._details)
	root.add_child(again)
	await _settle()
	_check(again.is_signed_in(), "a saved token signs you back in with no password")

	print("%d checks, %d failed" % [_checks, _failed])
	quit(1 if _failed > 0 else 0)
