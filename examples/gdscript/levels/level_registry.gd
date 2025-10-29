## Maintains level metadata and raises lifecycle events.
class_name LevelRegistry
extends Node

signal stage_started(stage_name)
signal stage_completed(stage_name, time_taken)

const LEVELS := {
	"intro": {"title": "Intro Stage", "difficulty": 1},
	"forest": {"title": "Whispering Forest", "difficulty": 2},
	"castle": {"title": "Frostkeep Citadel", "difficulty": 3},
}

var active_stage := ""

func load(stage_name: String) -> void:
	active_stage = stage_name
	emit_signal("stage_started", stage_name)

func complete_active_stage(time_taken: float) -> void:
	if active_stage == "":
		push_error("No active stage to complete")
		return
	emit_signal("stage_completed", active_stage, time_taken)

func get_stage_info(stage_name: String) -> Dictionary:
	return LEVELS.get(stage_name, {})

func available_stages() -> Array:
	return LEVELS.keys()
