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
## A public profile came back: `key` is the per-Flame id or "@handle" it was
## asked for under.
signal profile_seen(key: String, profile: Dictionary)
## A picture finished downloading (see `picture`).
signal picture_ready(id: int, texture: Texture2D)
## The list of public Flames changed.
signal flames_changed
## Staff: open reports list changed.
signal reports_changed
## A report was accepted by the Fireplace.
signal report_filed

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
## Everything `GET /me` said: display_name, pronouns, bio, avatar, badge,
## perms. Empty while signed out.
var me := {}
## Public Flames, as `GET /flames` last listed them.
var flames: Array = []
## Open reports on this Fireplace, when `can("reports")` is true.
var admin_reports: Array = []

const DEFAULT_REPORT_REASONS: Array[Dictionary] = [
	{"id": "picture", "label": "Inappropriate picture"},
	{"id": "name", "label": "Offensive name"},
	{"id": "bio", "label": "Offensive bio or pronouns"},
	{"id": "harassment", "label": "Harassment or bullying"},
	{"id": "hate", "label": "Hate speech"},
	{"id": "cheating", "label": "Cheating"},
	{"id": "spam", "label": "Spam or scams"},
	{"id": "underage", "label": "May be under the minimum age"},
	{"id": "other", "label": "Something else"},
]

## Pictures by "id:version", null while one is on its way.
var _pictures := {}

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
	fetch_flames()
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
	me = {}
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


# --- profiles and public Flames ---------------------------------------------

## The two things on a profile besides the name and picture.
func set_profile(pronouns: String, bio: String) -> void:
	_send("POST", "/me/profile", {"pronouns": pronouns, "bio": bio}, "profile")


func set_display_name(display_name: String) -> void:
	_send("POST", "/me/name", {"display_name": display_name}, "profile")


## Who somebody on this Flame is: `account` is the id their ticket gave them
## here. Answered through `profile_seen`.
func look_up(certificate: String, account: String) -> void:
	if certificate != "" and account != "":
		_send("GET", "/profiles/at/%s/%s" % [certificate, account], {}, "seen:" + account)


func look_up_handle(whose: String) -> void:
	_send("GET", "/profiles/%s" % whose.uri_encode(), {}, "seen:@" + whose)


func fetch_flames() -> void:
	_send("GET", "/flames", {}, "flames")


## Whether this player has a permission (e.g. "reports", "ban", "moderate").
func can(perm: String) -> bool:
	if not is_signed_in() or me.is_empty():
		return false
	if bool(me.get("owner", false)):
		return true
	var perms = me.get("perms", [])
	return perms is Array and perms.has(perm)


func get_report_reasons() -> Array:
	if features.has("report_reasons") and features["report_reasons"] is Array and not features["report_reasons"].is_empty():
		return features["report_reasons"]
	return DEFAULT_REPORT_REASONS


## Reports another player. Sent to this Fireplace's moderators.
func file_report(whose_handle: String, reason: String, details: String = "") -> void:
	if not is_signed_in():
		failed.emit("file_report", "Sign in to report players")
		return
	_send("POST", "/reports", {"handle": whose_handle, "reason": reason, "details": details}, "file_report")


## For staff with "reports" permission: lists open reports.
func fetch_reports() -> void:
	if can("reports"):
		_send("GET", "/admin/reports", {}, "admin_reports")


## For staff with "reports" permission: resolves/dismisses a report.
func resolve_report(report_id: int) -> void:
	if can("reports"):
		_send("POST", "/admin/reports/resolve", {"id": report_id}, "admin_resolve")


## For staff with "moderate" permission: clears profile fields.
func moderate(whose_handle: String, clear: Array) -> void:
	if can("moderate"):
		_send("POST", "/admin/moderate", {"handle": whose_handle, "clear": clear}, "moderate")


## Somebody's picture, if it is here already; otherwise null, and it is
## fetched and announced through `picture_ready`. `version` is the profile's
## `avatar`: 0 means they have none.
func picture(id: int, version: int) -> Texture2D:
	if id <= 0 or version <= 0:
		return null
	var key := "%d:%d" % [id, version]
	if _pictures.has(key):
		return _pictures[key]
	_pictures[key] = null
	# A request of its own: the queue's one HTTPRequest answers in JSON.
	var http := HTTPRequest.new()
	http.timeout = TIMEOUT
	add_child(http)
	http.request_completed.connect(func(result: int, code: int, _h: PackedStringArray, body: PackedByteArray) -> void:
		http.queue_free()
		var image := Image.new()
		if result != HTTPRequest.RESULT_SUCCESS or code != 200 or image.load_jpg_from_buffer(body) != OK:
			return
		circle_mask(image)
		var texture := ImageTexture.create_from_image(image)
		_pictures[key] = texture
		picture_ready.emit(id, texture))
	http.request("%s/avatars/%d.jpg?v=%d" % [base_url(), id, version])
	return null


## Masks an image to a smooth anti-aliased circle.
static func circle_mask(image: Image) -> void:
	image.convert(Image.FORMAT_RGBA8)
	var w := image.get_width()
	var h := image.get_height()
	var r := float(mini(w, h)) * 0.5
	var cx := float(w) * 0.5
	var cy := float(h) * 0.5
	for y in h:
		for x in w:
			var dx := float(x) + 0.5 - cx
			var dy := float(y) + 0.5 - cy
			var dist := sqrt(dx * dx + dy * dy)
			if dist > r:
				image.set_pixel(x, y, Color(0, 0, 0, 0))
			elif dist > r - 1.0:
				var pixel := image.get_pixel(x, y)
				pixel.a *= clampf(r - dist, 0.0, 1.0)
				image.set_pixel(x, y, pixel)


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
	var answer: Dictionary = parsed if parsed is Dictionary else ({"_list": parsed} if parsed is Array else {})
	# A wrong password is a 401 as well, and must not sign anybody out.
	if code == 401 and tag != "signin" and tag != "email":
		# The token is no longer believed. Sign out rather than keep asking.
		var was := handle
		sign_out()
		failed.emit(tag, "%s was signed out" % was if was != "" else "signed out")
		_next()
		return
	if code >= 400:
		# Somebody without a profile here is a guest or from another
		# Fireplace: nothing to tell the player.
		if tag.begins_with("seen:"):
			_next()
			return
		if tag == "signin" and bool(answer.get("code_needed", false)):
			code_ticket = str(answer.get("ticket", ""))
		failed.emit(tag, str(answer.get("error", "that did not work (%d)" % code)))
		_next()
		return
	if tag.begins_with("seen:"):
		profile_seen.emit(tag.substr(5), answer)
	else:
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
			me = answer
			email = str(answer.get("email", "")) if answer.get("email") != null else ""
			email_verified = bool(answer.get("email_verified", false))
			account_changed.emit()
			if can("reports"):
				fetch_reports()
		"file_report":
			said.emit("Report sent. The moderators will look at it.")
			report_filed.emit()
		"admin_reports":
			admin_reports = answer.get("_list", [])
			reports_changed.emit()
		"admin_resolve":
			said.emit("Report resolved.")
			fetch_reports()
		"moderate":
			said.emit("Profile cleared.")
		"profile":
			_send("GET", "/me", {}, "me")
			said.emit("Profile saved.")
		"flames":
			flames = answer.get("flames", [])
			flames_changed.emit()
		"email":
			_send("GET", "/me", {}, "me")
			said.emit("Saved. Open the link we emailed to confirm it." if answer.get("email") != null
				else "Email removed.")
