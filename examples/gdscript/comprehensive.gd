@tool
## Entry point for the GDScript demo project.
class_name GameManager
extends Node

const LevelRegistry := preload("./levels/level_registry.gd")
const PlayerScene := preload("./characters/hero.gd")
const HudScene := preload("./ui/hud.gd")
const MathUtils := preload("./utils/math_utils.gd")

signal stage_loaded(stage_name)
signal player_joined(player_name)

enum StageDifficulty { EASY, NORMAL, HARD }

const MAX_PLAYERS := 4
@export var starting_stage := "intro"
var players: Array = []
var current_stage := ""
var difficulty := StageDifficulty.EASY
var difficulty_scale := 1.0

func _ready() -> void:
	initialize()
	_connect_stage_events()

func initialize() -> void:
	current_stage = "intro"
	players.clear()
	difficulty = StageDifficulty.EASY
	difficulty_scale = calculate_difficulty(0.25)
	_load_stage(current_stage)

func _connect_stage_events() -> void:
	var level_registry = LevelRegistry.new()
	level_registry.stage_started.connect(_on_stage_started)
	level_registry.stage_completed.connect(_on_stage_completed)

func _load_stage(stage_name: String) -> void:
	var level_registry = LevelRegistry.new()
	level_registry.load(stage_name)
	emit_signal("stage_loaded", stage_name)

func register_player(player_name: String) -> void:
	if players.size() >= MAX_PLAYERS:
		push_warning("Maximum player count reached")
		return
	players.append(player_name)
	emit_signal("player_joined", player_name)

func spawn_player(player_name: String) -> Node:
	var hero: Node = PlayerScene.new()
	hero.setup(player_name)
	var normalized := float(players.size()) / float(MAX_PLAYERS)
	hero.sprint(1.0 + MathUtils.clamp01(normalized))
	return hero

func build_hud() -> Control:
	var hud = HudScene.new()
	hud.setup(players)
	return hud

func describe_stage(stage_name: String) -> String:
	match stage_name:
		"intro":
			return "Tutorial encounter"
		"forest":
			return "Whispering Forest"
		"castle":
			return "Frozen Keep"
		_:
			return "Unknown"

class StageStats:
	var name := ""
	var enemies_defeated := 0
	var completion_time := 0.0

	func _init(new_name := "unknown"):
		name = new_name
		completion_time = 0.0

	func summary() -> String:
		return "%s | Enemies: %d | Time: %.2f" % [name, enemies_defeated, completion_time]

func _on_stage_started(stage_name: String) -> void:
	var stats := StageStats.new()
	stats.name = stage_name
	stats.enemies_defeated = 0
	stats.completion_time = 0.0
	var weight := 0.0
	match stage_name:
		"intro":
			weight = 0.1
		"forest":
			weight = 0.5
		"castle":
			weight = 0.9
		_:
			weight = 0.3

	difficulty_scale = calculate_difficulty(weight)
	print("Stage started:", stats.summary())


func _on_stage_completed(stage_name: String, elapsed: float) -> void:
	print("Stage completed:", stage_name, "in", elapsed)

func debug_print_players() -> void:
	for name in players:
		print("Player:", name)

func calculate_difficulty(weight: float) -> float:
	var adjusted := weight
	var counter := 0
	while counter < 3:
		adjusted = MathUtils.clamp01(adjusted + 0.05)
		counter += 1

	return MathUtils.lerp_value(0.75, 1.5, adjusted)
