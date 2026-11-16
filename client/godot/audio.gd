class_name GameAudio
extends Node

## Music and sound effects.
##
## Everything is looked up by name in res://audio: `.ogg` first, then `.wav`,
## so dropping a real recording next to a generated one replaces it without a
## code change. A name with no file is silently ignored, which is what keeps
## this safe to call from anywhere.
##
## The generated files come from tools/make_audio.py.

const DIR := "res://audio"
## Two players, so changing track is a crossfade and not a cut.
const CROSSFADE := 1.5
## Sound effects that may overlap: a click while a bell is still ringing.
const VOICES := 6

var music_level := 0.6: set = set_music_level
var sfx_level := 0.8: set = set_sfx_level

var _players: Array[AudioStreamPlayer] = []
var _front := 0
var _playing := ""
var _fade := 0.0
var _sfx: Array[AudioStreamPlayer] = []
var _next_sfx := 0
var _cache := {}


func _ready() -> void:
	for i in 2:
		var player := AudioStreamPlayer.new()
		player.volume_db = -80.0
		add_child(player)
		_players.append(player)
	for i in VOICES:
		var player := AudioStreamPlayer.new()
		add_child(player)
		_sfx.append(player)
	var settings := ConfigFile.new()
	settings.load(Session.SETTINGS)
	music_level = float(settings.get_value("audio", "music", 0.6))
	sfx_level = float(settings.get_value("audio", "sfx", 0.8))


## Crossfades to a track, or to silence when `name` is empty. Asking for the
## track already playing does nothing, so calling this on every world change
## is fine.
func music(track: String) -> void:
	if track == _playing:
		return
	_playing = track
	_fade = 0.0
	if track == "":
		# There is nothing to fade up to, so the player that was in front
		# becomes the one fading out. Without the swap, silence would cut.
		_front = 1 - _front
		return
	var stream := _stream(track)
	if stream == null:
		return
	_front = 1 - _front
	var player := _players[_front]
	player.stream = stream
	player.volume_db = -80.0
	player.play()


func play(effect: String) -> void:
	if sfx_level <= 0.0:
		return
	var stream := _stream(effect)
	if stream == null:
		return
	var player := _sfx[_next_sfx]
	_next_sfx = (_next_sfx + 1) % VOICES
	player.stream = stream
	player.volume_db = linear_to_db(sfx_level)
	player.play()


func set_music_level(level: float) -> void:
	music_level = clampf(level, 0.0, 1.0)
	_save()


func set_sfx_level(level: float) -> void:
	sfx_level = clampf(level, 0.0, 1.0)
	_save()


func _save() -> void:
	# Written on every nudge of the slider: a few bytes, and it means the
	# setting survives a crash as well as a clean quit.
	var settings := ConfigFile.new()
	settings.load(Session.SETTINGS)
	settings.set_value("audio", "music", music_level)
	settings.set_value("audio", "sfx", sfx_level)
	settings.save(Session.SETTINGS)


func _process(delta: float) -> void:
	_fade = minf(_fade + delta / CROSSFADE, 1.0)
	for i in _players.size():
		var player := _players[i]
		var share := _fade if i == _front else 1.0 - _fade
		var level := music_level * share
		if level <= 0.001:
			player.volume_db = -80.0
			if player.playing and share <= 0.001:
				player.stop()
		else:
			player.volume_db = linear_to_db(level)


func _exit_tree() -> void:
	# Otherwise Godot reports the streams still in use at exit, the same way
	# it does for a voice playback left in a freed avatar.
	for player in _players + _sfx:
		player.stop()
		player.stream = null
	_cache.clear()


## Looks a name up once and remembers the answer, including the answer "there
## is no such file", so a missing effect is not re-searched every click.
func _stream(name: String) -> AudioStream:
	if _cache.has(name):
		return _cache[name]
	var found: AudioStream = null
	for extension in [".ogg", ".wav"]:
		var path := "%s/%s%s" % [DIR, name, extension]
		if ResourceLoader.exists(path):
			found = load(path)
			break
	if found == null:
		push_warning("audio: nothing called %s in %s" % [name, DIR])
	_loop(found)
	_cache[name] = found
	return found


## Music must loop and effects must not, and the two tracks are the only long
## files, so looping anything longer than five seconds is the whole rule.
func _loop(stream: AudioStream) -> void:
	if stream == null or stream.get_length() < 5.0:
		return
	if stream is AudioStreamWAV:
		stream.loop_mode = AudioStreamWAV.LOOP_FORWARD
		stream.loop_end = stream.data.size() / (4 if stream.stereo else 2)
	elif stream is AudioStreamOggVorbis or stream is AudioStreamMP3:
		stream.loop = true
