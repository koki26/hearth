extends SceneTree

## Generates ui_theme.tres from the tokens in ui.gd:
##   godot --headless --path client/godot --script res://tools/make_theme.gd
##
## project.godot applies that theme to every Control in the project, so the
## launcher and the world builder are styled without a line of styling code in
## either of them. Run this after changing a colour in ui.gd.

const OUT := "res://ui_theme.tres"


func _initialize() -> void:
	var theme := UiStyle.build_theme()
	var problem := ResourceSaver.save(theme, OUT)
	if problem != OK:
		printerr("could not write %s: error %d" % [OUT, problem])
		quit(1)
		return
	print("wrote %s" % OUT)
	quit(0)
