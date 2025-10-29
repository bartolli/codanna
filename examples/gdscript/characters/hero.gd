## Playable hero character used in the demo project.
class_name Hero
extends CharacterBody2D

const DEFAULT_SPEED := 220.0
const DEFAULT_HEALTH := 100

@export var starting_level: int = 1
@onready var sprite: Sprite2D = $Sprite2D if has_node("Sprite2D") else null

var display_name := "Hero"
var speed := DEFAULT_SPEED
var health := DEFAULT_HEALTH

const MathUtils := preload("../utils/math_utils.gd")

func setup(name: String) -> void:
	display_name = name
	health = DEFAULT_HEALTH

func take_damage(amount: int) -> void:
	health = max(0, health - amount)

func heal(amount: int) -> void:
	health = min(DEFAULT_HEALTH, health + amount)

func _physics_process(delta: float) -> void:
	var input_vector := Vector2(
		Input.get_action_strength("ui_right") - Input.get_action_strength("ui_left"),
		Input.get_action_strength("ui_down") - Input.get_action_strength("ui_up")
	)

	if input_vector.length_squared() > 0.0:
		velocity = input_vector.normalized() * speed
		move_and_slide()

func sprint(multiplier: float) -> void:
	speed = MathUtils.clamp_speed(DEFAULT_SPEED * multiplier)
