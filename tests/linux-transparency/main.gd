extends Node2D

const COLORS := [Color8(24, 130, 184), Color8(229, 172, 32)]
var view: Control
var holder: Control
var output := OS.get_environment("WRY_PROBE_OUTPUT")
var phase := "initial"
var frame := 0
var ready_count := 0
var command_id := 0

func _ready() -> void:
    holder = Control.new()
    add_child(holder)
    _create_view()

func _create_view() -> void:
    view = ClassDB.instantiate("WebView")
    view.set("full_window_size", false)
    view.set("transparent", true)
    view.set("focused_when_created", false)
    view.set("url", "")
    view.set("html", FileAccess.get_file_as_string("res://overlay.html"))
    view.position = Vector2(160, 120)
    view.size = Vector2(640, 400)
    view.connect("ipc_message", _on_ipc)
    holder.add_child(view)

func _on_ipc(message: String) -> void:
    var data = JSON.parse_string(message)
    if data is Dictionary and data.get("probe") == "ready":
        ready_count += 1

func _draw() -> void:
    for index in range(8):
        draw_rect(Rect2(index * 120, 0, 120, 640), COLORS[index % 2])
    # Visible motion verifies that the compositor sees continued Godot presents.
    draw_rect(Rect2((frame * 3) % 900, 600, 50, 30), Color.WHITE)

func _process(_delta: float) -> void:
    frame += 1
    queue_redraw()
    if output.is_empty():
        return
    var path := output.path_join("command.json")
    if FileAccess.file_exists(path):
        var command = JSON.parse_string(FileAccess.get_file_as_string(path))
        if command is Dictionary and command.get("id", 0) > command_id:
            command_id = command.id
            phase = command.phase
            match phase:
                "resize":
                    view.position = Vector2(80, 80)
                    view.size = Vector2(720, 480)
                "hidden": view.call("set_visible", false)
                "shown": view.call("set_visible", true)
                "parent-hidden": holder.hide()
                "parent-shown": holder.show()
                "changed-html": view.call("eval", "document.getElementById('red').style.background='rgb(32,255,32)'")
                "load-html": view.call("load_html", FileAccess.get_file_as_string("res://overlay.html"))
                "full-window":
                    view.set("full_window_size", true)
                    view.call("resize")
                "recreate":
                    view.free()
                    _create_view()
                "removed": view.free()
                "quit": get_tree().quit()
    if frame % 6 == 0:
        var file := FileAccess.open(output.path_join("state.tmp"), FileAccess.WRITE)
        file.store_string(JSON.stringify({"phase": phase, "command_id": command_id,
            "frame": frame, "ready_count": ready_count}))
        file.close()
        DirAccess.rename_absolute(output.path_join("state.tmp"), output.path_join("state.json"))
