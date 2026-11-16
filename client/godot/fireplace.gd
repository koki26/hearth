class_name FireplaceClient
extends Node

## Talking to the Fireplace: signing in, friends, and saying where you are.
##
## HTTP in GDScript rather than in the extension, because the engine already
## does HTTPS with a certificate bundle it ships, and the extension would need
## a whole HTTP stack and a second TLS story for a handful of small requests.
## The extension is for the things Godot cannot do -- QUIC and Opus.
##
## One `HTTPRequest` serves one request at a time, so calls queue here. Every
## answer comes back as a signal, the same shape as the rest of the client.
##
## Nothing here knows about the world. It holds the token, the friends list,
## and what the menu needs to draw them (`friend_rows`); main.gd does the
## joining.

## Signed in, with the handle it signed in as.
signal signed_in(handle: String)
## Signed out, deliberately or because the Fireplace stopped believing us.
signal signed_out(reason: String)
## The friends list changed: redraw whatever is showing it.
signal friends_changed
## Something did not work, with a line a player can read.
signal failed(what: String, message: String)
## Something worked that the player should hear about.
signal said(message: String)
## What the account page shows changed: features, or the email on file.
signal account_changed

## How often we say where we are, and ask where everybody else is. The server
## forgets a player it has not heard from in two minutes.
const HEARTBEAT := 45.0
## Give up on a request that has not finished in this long.
const TIMEOUT := 10.0
## An access token lasts an hour; trade the refresh one for another well
## before then, or a heartbeat gets a 401 and signs the player out.
const RENEW := 50.0 * 60.0

## The token for asking things, the one that renews it, and who we are.
var token := ""
var handle := ""
var account_id := 0
## Friends, as the Fireplace last described them: id, handle, online, flame,
## ember, ember_name, world.
var friends: Array = []
## Requests waiting: people who asked us, and people we asked.
var incoming: Array = []
var outgoing: Array = []
var share_location := true
## What this Fireplace offers (`GET /features`): whether it sends email, and
## where its reset, privacy and terms pages are. Empty until it answers, and
## then nothing email-shaped is shown, which is the safe way round.
var features := {}
## Shown once, straight after an account is made, and then forgotten. They
## are never written to the settings file.
var recovery_codes: Array = []
## The address on the account, if any, and whether its link was opened.
var email := ""
var email_verified := false

var _details: ConnectDetails
var _http: HTTPRequest
## Waiting to be sent: each is [method, path, body, tag].
var _queue: Array = []
var _sending := false
## What the request in flight was for, so the answer can be understood.
var _tag := ""
var _beat := 0.0
var _renew := RENEW
## Where we last said we were, re-sent on every heartbeat.
var _where := {}


func _init(details: ConnectDetails) -> void:
	_details = details
	handle = details.account_handle
	token = details.account_token
	share_location = details.share_location


func _ready() -> void:
	_http = HTTPRequest.new()
	_http.timeout = TIMEOUT
	_http.request_completed.connect(_on_answer)
	add_child(_http)
	fetch_features()
	# A token saved from last time is worth a try: if it is still good, the
	# player never sees a sign-in page at all.
	if _details.account_refresh != "":
		_send("POST", "/sessions/refresh", {"refresh": _details.account_refresh}, "refresh")


func is_signed_in() -> bool:
	return token != ""


func _process(delta: float) -> void:
	if not is_signed_in():
		return
	_renew -= delta
	if _renew <= 0.0 and _details.account_refresh != "":
		_renew = RENEW
		_send("POST", "/sessions/refresh", {"refresh": _details.account_refresh}, "renew")
	_beat -= delta
	if _beat > 0.0:
		return
	_beat = HEARTBEAT
	if not _where.is_empty():
		_send("POST", "/presence", _where, "presence")
	refresh_friends()


## Where the Fireplace is, as something `HTTPRequest` can use.
##
## The stored value may be a bare host from an older settings file, a
## `host:port`, or a URL. A host with a port is plainly a test server, so it
## gets `http://`; anything else gets `https://`, because a password must not
## cross the internet in the clear.
func base_url() -> String:
	var text := _details.account_server.strip_edges()
	if text.ends_with("/"):
		text = text.substr(0, text.length() - 1)
	if text.contains("://"):
		return text
	var colon := text.rfind(":")
	var port := text.substr(colon + 1) if colon > 0 else ""
	if port != "" and port.is_valid_int():
		return "http://" + text
	return "https://" + text


# --- signing in -------------------------------------------------------------

## The birth date is checked against the Fireplace's minimum age and not kept.
## Where the Fireplace sends email, `address` is required and gets a link to
## confirm it; until then the account can sign in but not join a server.
func create_account(new_handle: String, password: String, address: String, birthdate: String,
		accept_terms: bool, policy_emails: bool) -> void:
	_send("POST", "/accounts", {"handle": new_handle, "password": password, "email": address,
		"birthdate": birthdate, "accept_terms": accept_terms, "policy_emails": policy_emails}, "signin")


## The second step, when signing in needed a code from the email (a new IP
## address, or two-step sign-in): the ticket the Fireplace handed back.
var code_ticket := ""


func sign_in_with_code(code: String) -> void:
	_send("POST", "/sessions/code", {"ticket": code_ticket, "code": code.strip_edges()}, "signin")


func sign_in(as_handle: String, password: String) -> void:
	_send("POST", "/sessions", {"handle": as_handle, "password": password}, "signin")


## Asks what this Fireplace offers. Again whenever the address changes.
func fetch_features() -> void:
	features = {}
	_send("GET", "/features", {}, "features")


## Whether "Forgot password?" means anything here.
func can_reset() -> bool:
	return bool(features.get("email", false)) or bool(features.get("recovery_codes", false))


## A page the Fireplace serves, for the system browser: the reset form, the
## privacy policy, the terms.
func page_url(which: String) -> String:
	return base_url() + str(features.get(which, "/" + which))


## Add or change the address (empty removes it). The password as well, as the
## server asks: the address is how a password gets reset.
func set_email(password: String, address: String) -> void:
	_send("POST", "/me/email", {"password": password, "email": address}, "email")


## Forgets the tokens on this machine. The one on the server runs out on its
## own; there is nothing to call back.
func sign_out() -> void:
	token = ""
	handle = ""
	account_id = 0
	friends = []
	incoming = []
	outgoing = []
	_where = {}
	_details.account_handle = ""
	_details.account_token = ""
	_details.account_refresh = ""
	_details.save()
	recovery_codes = []
	email = ""
	email_verified = false
	signed_out.emit("signed out")


func refresh_friends() -> void:
	if is_signed_in():
		_send("GET", "/friends", {}, "friends")


func add_friend(whose_handle: String) -> void:
	_send("POST", "/friends/add", {"handle": whose_handle}, "friends_changed")


func accept_friend(whose_handle: String) -> void:
	_send("POST", "/friends/accept", {"handle": whose_handle}, "friends_changed")


func remove_friend(whose_handle: String) -> void:
	_send("POST", "/friends/remove", {"handle": whose_handle}, "friends_changed")


func set_sharing(on: bool) -> void:
	share_location = on
	_details.share_location = on
	_details.save()
	if is_signed_in():
		_send("POST", "/settings", {"share_location": on}, "settings")


## Says where we are. Kept, and re-sent on every heartbeat, until it changes
## or we leave.
func report(flame: String, ember: int, ember_name: String, world: String) -> void:
	_where = {"flame": flame, "ember": ember, "ember_name": ember_name, "world": world}
	if is_signed_in():
		_send("POST", "/presence", _where, "presence")


## Left the Flame. Best effort: a client that is killed says nothing, which is
## why the Fireplace forgets anybody it has not heard from for two minutes.
func clear_presence() -> void:
	_where = {}
	if is_signed_in():
		_send("POST", "/presence", {}, "presence")


# --- the rows the menu draws --------------------------------------------------

## Friends, requests and the way to add one, as menu rows (see menu.gd).
##
## A pure function of what it is handed, so it can be checked without a
## Fireplace, a network or a scene.
static func friend_rows(friends_now: Array, incoming_now: Array, typed: String) -> Array:
	var online := 0
	for friend: Dictionary in friends_now:
		online += 1 if bool(friend.get("online", false)) else 0
	var rows: Array = [{"heading": "Friends  %d / %d online" % [online, friends_now.size()]
		if not friends_now.is_empty() else "Friends"}]
	if friends_now.is_empty():
		rows.append({"label": "nobody yet -- add somebody below"})
	# Whoever is about first: they are the ones you can go and see.
	var sorted := friends_now.duplicate()
	sorted.sort_custom(func(a: Dictionary, b: Dictionary) -> bool:
		return bool(a.get("online", false)) and not bool(b.get("online", false)))
	for friend: Dictionary in sorted:
		var name := str(friend.get("handle", "?"))
		if not bool(friend.get("online", false)):
			rows.append({"label": name, "avatar": name, "status": "offline", "sub": "offline"})
			continue
		# Online and somewhere we could go: pickable, and it says where.
		var flame := str(friend.get("flame", ""))
		var ember_name := str(friend.get("ember_name", ""))
		rows.append({
			"id": "friend:%s" % name,
			"label": name,
			"avatar": name,
			"status": "online",
			"sub": "in " + ember_name if ember_name != "" else "on " + flame,
			"action": "Join",
		})
	if not incoming_now.is_empty():
		rows.append({"heading": "Requests"})
	for asking: Dictionary in incoming_now:
		var who := str(asking.get("handle", "?"))
		rows.append({
			"id": "accept:%s" % who,
			"label": who,
			"avatar": who,
			"sub": "wants to be friends",
			"action": "Accept",
		})
	rows.append({"id": "addfriend", "label": "", "text": typed, "placeholder": "Add a friend by name",
		"icon": "res://icons/plus.svg"})
	return rows


# --- the plumbing -------------------------------------------------------------

func _send(method: String, path: String, body: Dictionary, tag: String) -> void:
	_queue.append([method, path, body, tag])
	_next()


func _next() -> void:
	if _sending or _queue.is_empty() or _http == null:
		return
	var next: Array = _queue.pop_front()
	var headers := PackedStringArray(["Content-Type: application/json"])
	if token != "":
		headers.append("Authorization: Bearer " + token)
	var method: int = HTTPClient.METHOD_POST if next[0] == "POST" else HTTPClient.METHOD_GET
	var body := JSON.stringify(next[2]) if next[0] == "POST" else ""
	var error := _http.request(base_url() + str(next[1]), headers, method, body)
	if error != OK:
		failed.emit(str(next[3]), "cannot reach %s" % base_url())
		return
	_sending = true
	_tag = str(next[3])


func _on_answer(result: int, code: int, _headers: PackedStringArray, body: PackedByteArray) -> void:
	_sending = false
	var tag := _tag
	_tag = ""
	if result != HTTPRequest.RESULT_SUCCESS:
		# A refusal by the Fireplace is one thing; not reaching it at all is
		# another, and the player can only do something about the second.
		failed.emit(tag, "cannot reach the Fireplace")
		_next()
		return
	# Anything at all may come back from anywhere: never assume it parses.
	var parsed = JSON.parse_string(body.get_string_from_utf8())
	var answer: Dictionary = parsed if parsed is Dictionary else {}
	# A wrong password is a 401 as well, and must not sign anybody out.
	if code == 401 and tag != "signin" and tag != "email":
		# The token is no longer believed. Sign out rather than keep asking.
		var was := handle
		sign_out()
		failed.emit(tag, "%s was signed out" % was if was != "" else "signed out")
		_next()
		return
	if code >= 400:
		if tag == "signin" and bool(answer.get("code_needed", false)):
			code_ticket = str(answer.get("ticket", ""))
		failed.emit(tag, str(answer.get("error", "that did not work (%d)" % code)))
		_next()
		return
	_took(tag, answer)
	_next()


func _took(tag: String, answer: Dictionary) -> void:
	match tag:
		"signin", "refresh":
			code_ticket = ""
			token = str(answer.get("access", ""))
			handle = str(answer.get("handle", ""))
			account_id = int(answer.get("id", 0))
			_details.account_handle = handle
			_details.account_token = token
			_details.account_refresh = str(answer.get("refresh", ""))
			_details.save()
			_beat = 0.0
			_renew = RENEW
			recovery_codes = answer.get("recovery_codes", [])
			signed_in.emit(handle)
			refresh_friends()
			_send("GET", "/me", {}, "me")
		"renew":
			# Quietly: the player is still signed in, nothing to announce.
			token = str(answer.get("access", token))
			_details.account_token = token
			_details.account_refresh = str(answer.get("refresh", _details.account_refresh))
			_details.save()
		"friends":
			friends = answer.get("friends", [])
			incoming = answer.get("incoming", [])
			outgoing = answer.get("outgoing", [])
			friends_changed.emit()
		"friends_changed":
			refresh_friends()
		"settings":
			share_location = bool(answer.get("share_location", share_location))
			friends_changed.emit()
		"features":
			features = answer
			account_changed.emit()
		"me":
			email = str(answer.get("email", "")) if answer.get("email") != null else ""
			email_verified = bool(answer.get("email_verified", false))
			account_changed.emit()
		"email":
			_send("GET", "/me", {}, "me")
			said.emit("Saved. Open the link we emailed to confirm it." if answer.get("email") != null
				else "Email removed.")
