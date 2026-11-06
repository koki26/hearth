extends CanvasLayer

## Desktop connect screen. XR only starts once we are in, so you set up on the
## monitor and then put the headset on.

signal connect_requested(host: String, port: int, player_name: String, use_vr: bool)

const SETTINGS_PATH := "user://settings.cfg"

var _vr_available := false


func _ready() -> void:
	var settings := ConfigFile.new()
	settings.load(SETTINGS_PATH)
	%Address.text = settings.get_value("last", "address", "127.0.0.1:7777")
	%PlayerName.text = settings.get_value("last", "name", "Player%d" % (randi() % 1000))
	%JoinVR.pressed.connect(_on_join.bind(true))
	%JoinDesktop.pressed.connect(_on_join.bind(false))
	%Address.text_submitted.connect(func(_text: String) -> void: _on_join(_vr_available))
	%PlayerName.text_submitted.connect(func(_text: String) -> void: _on_join(_vr_available))


func fill(host: String, port: int, player_name: String) -> void:
	%Address.text = "%s:%d" % [host, port]
	%PlayerName.text = player_name


func set_vr_available(available: bool) -> void:
	_vr_available = available
	%JoinVR.disabled = not available
	%Message.text = "" if available else "No headset found - desktop only."
	(%JoinVR if available else %JoinDesktop).grab_focus()


func busy(text: String) -> void:
	%JoinVR.disabled = true
	%JoinDesktop.disabled = true
	%Message.text = text


func failed(reason: String) -> void:
	%JoinVR.disabled = not _vr_available
	%JoinDesktop.disabled = false
	%Message.text = reason


func _on_join(use_vr: bool) -> void:
	var address: String = %Address.text.strip_edges()
	var player_name: String = %PlayerName.text.strip_edges()
	if player_name.is_empty():
		failed("Pick a name.")
		return
	var host := address
	var port := 7777
	var colon := address.rfind(":")
	if colon > 0:
		host = address.substr(0, colon)
		port = int(address.substr(colon + 1))
	if host.is_empty() or port <= 0 or port > 65535:
		failed("Use host or host:port, like 192.168.1.20:7777")
		return

	var settings := ConfigFile.new()
	settings.set_value("last", "address", address)
	settings.set_value("last", "name", player_name)
	settings.save(SETTINGS_PATH)

	busy("Connecting to %s:%d..." % [host, port])
	connect_requested.emit(host, port, player_name, use_vr)
