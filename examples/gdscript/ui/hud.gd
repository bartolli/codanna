@tool
## Simple HUD overlay showing connected players.
class_name Hud
extends Control

var player_names: Array = []

func setup(names: Array) -> void:
	player_names = names.duplicate()
	_refresh_labels()

func add_player(name: String) -> void:
	if not player_names.has(name):
		player_names.append(name)
		_refresh_labels()

func _ready() -> void:
	_refresh_labels()

func _refresh_labels() -> void:
	for name in player_names:
		print("HUD: showing player", name)
