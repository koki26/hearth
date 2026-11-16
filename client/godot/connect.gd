class_name ConnectDetails
extends RefCounted

## The server address and the name you join under, and the parsing and saving
## that go with them.
##
## This used to be a 2D screen. It is plain data now, because the details are
## typed on a panel in the lift, where a player wearing a headset can reach
## them: see main.gd and keyboard.gd.

const SETTINGS_PATH := Session.SETTINGS
const DEFAULT_PORT := 7777

## The Fireplace: the one server that knows who people are (docs/TRUST.md).
## A base URL, not a host and port, because it is reached over HTTPS through
## whatever terminator stands in front of it. It is deliberately one named
## constant so that pointing the game at another one is exactly one line.
const DEFAULT_FIREPLACE := "https://fireplace.kokiland-services.eu"

var address := "127.0.0.1:7777"
var player_name := ""
## Where accounts come from. Changing it hands whoever runs that host your
## sign-in, which is why it is behind a warning in the interface.
var account_server := DEFAULT_FIREPLACE
## Join with nothing but a name: no account, and nothing checking who anybody
## is. The only way there is today, because nothing issues tokens yet -- the
## setting exists so the default can flip when something does.
var direct := true
## Signed in as, and the two tokens that say so: one for asking things, one
## for getting another of those without typing a password on a keyboard that
## floats in mid-air. Saved in plain text on this machine, which is the same
## bargain every game launcher makes.
var account_handle := ""
var account_token := ""
var account_refresh := ""
## Whether friends may see which Flame and which ember you are in.
var share_location := true


func _init() -> void:
	var settings := ConfigFile.new()
	settings.load(SETTINGS_PATH)
	address = str(settings.get_value("last", "address", "127.0.0.1:7777"))
	player_name = str(settings.get_value("last", "name", "Player%d" % (randi() % 1000)))
	account_server = str(settings.get_value("advanced", "account_server", DEFAULT_FIREPLACE))
	# The default moved; settings saved before that still hold the old one.
	if account_server.strip_edges() == "https://accounts.kokiland-services.eu":
		account_server = DEFAULT_FIREPLACE
	direct = bool(settings.get_value("advanced", "direct", true))
	account_handle = str(settings.get_value("account", "handle", ""))
	account_token = str(settings.get_value("account", "token", ""))
	account_refresh = str(settings.get_value("account", "refresh", ""))
	share_location = bool(settings.get_value("account", "share_location", true))


## Splits `address` into a host and a port. Returns an empty host when it is
## not usable, with the reason in "problem".
func parsed() -> Dictionary:
	var text := address.strip_edges()
	var host := text
	var port := DEFAULT_PORT
	# rfind, so an IPv6 address without a port is not cut at its first colon.
	var colon := text.rfind(":")
	if colon > 0:
		host = text.substr(0, colon)
		port = int(text.substr(colon + 1))
	if host.is_empty() or port <= 0 or port > 65535:
		return {"host": "", "port": 0, "problem": "Use host or host:port, like 192.168.1.20:7777"}
	if player_name.strip_edges().is_empty():
		return {"host": "", "port": 0, "problem": "Pick a name."}
	return {"host": host, "port": port, "problem": ""}


func save() -> void:
	var settings := ConfigFile.new()
	settings.load(SETTINGS_PATH)
	settings.set_value("last", "address", address.strip_edges())
	settings.set_value("last", "name", player_name.strip_edges())
	settings.set_value("advanced", "account_server", account_server.strip_edges())
	settings.set_value("advanced", "direct", direct)
	settings.set_value("account", "handle", account_handle)
	settings.set_value("account", "token", account_token)
	settings.set_value("account", "refresh", account_refresh)
	settings.set_value("account", "share_location", share_location)
	settings.save(SETTINGS_PATH)


func summary() -> String:
	return "%s at %s" % [player_name.strip_edges(), address.strip_edges()]
