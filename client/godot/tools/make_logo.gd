## Renders logo.svg into the files the engine and Windows need:
##   godot --headless --path client/godot --script res://tools/make_logo.gd
## splash.png is the boot splash; the rest become logo.ico (see client/build.ps1).
extends SceneTree
func _init() -> void:
	var src := FileAccess.get_file_as_string("res://logo.svg")
	for size in [512, 256, 128, 64, 48, 32, 16]:
		var img := Image.new()
		if img.load_svg_from_string(src, float(size) / 512.0) != OK:
			push_error("svg failed")
			quit(1)
			return
		img.save_png("res://.logo-%d.png" % size)
		print("wrote %d" % size)
	quit()
