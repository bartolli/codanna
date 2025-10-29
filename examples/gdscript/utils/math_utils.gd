## Utility helpers shared by demo scripts.
class_name MathUtils
extends RefCounted

static func clamp01(value: float) -> float:
	return clamp(value, 0.0, 1.0)

static func clamp_speed(value: float) -> float:
	return clamp(value, 50.0, 400.0)

static func lerp_value(from_value: float, to_value: float, weight: float) -> float:
	return lerp(from_value, to_value, clamp01(weight))
