## Player controller used for GDScript parser relationship tests.
extends CharacterBody2D
class_name Player

## Emitted whenever the player's health value changes.
signal health_changed(new_value)

const EnemyScene := preload("res://enemies/enemy.gd")
const HealEffect := preload("res://effects/heal_effect.gd")

var health := 100

## Called by the scene tree once the node is ready.
func _ready():
    spawn_enemy()
    emit_signal("health_changed", health)

## Spawns a new enemy instance and attaches it to this node.
func spawn_enemy():
    var enemy_scene := EnemyScene.instantiate()
    enemy_scene.setup()
    add_child(enemy_scene)

## Applies incoming damage to the player and notifies listeners.
func apply_damage(amount):
    health -= amount
    emit_signal("health_changed", health)
    if health <= 0:
        _reset()

## Resets player state and plays a heal effect.
func _reset():
    health = 100
    var effect_scene := preload("res://effects/heal_effect.gd")
    var effect := effect_scene.instantiate()
    add_child(effect)
