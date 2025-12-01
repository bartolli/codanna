#!/usr/bin/env python3
"""
Comprehensive Python test file for parser maturity assessment.
Tests all major Python language features and constructs.
"""

import os
import sys
from typing import List, Dict, Optional, Union, TypeVar, Generic, Protocol, Final, Literal
from dataclasses import dataclass, field
from enum import Enum, auto
from abc import ABC, abstractmethod
import asyncio
from functools import wraps, lru_cache
from contextlib import contextmanager
import warnings

# Module-level constants
MAX_SIZE: Final[int] = 1024
DEFAULT_NAME: str = "default"
_PRIVATE_CONSTANT = "private"
__MANGLED_CONSTANT = "mangled"

# Module-level variables
global_counter = 0
_module_cache: Dict[str, any] = {}

# Aliased imports (for audit coverage)
import json as json_module
from pathlib import Path as FilePath

# Augmented assignment
counter = 0
counter += 1
counter -= 1
counter *= 2

# Global and nonlocal statements
def outer_function():
    outer_var = 10
    def inner_function():
        nonlocal outer_var
        global global_counter
        outer_var += 1
        global_counter += 1
    inner_function()

# While statement
def countdown(n: int):
    while n > 0:
        n -= 1

# With statement
def read_file(path: str):
    with open(path) as f:
        return f.read()

# Type variables and aliases
T = TypeVar('T')
K = TypeVar('K')
V = TypeVar('V')

Number = Union[int, float]
JsonValue = Union[None, bool, int, float, str, List['JsonValue'], Dict[str, 'JsonValue']]

# Simple function
def simple_function(x: int, y: int) -> int:
    """Add two numbers."""
    return x + y

# Function with default arguments
def function_with_defaults(
    name: str,
    age: int = 0,
    active: bool = True,
    tags: Optional[List[str]] = None
) -> Dict[str, any]:
    """Function with various default arguments."""
    if tags is None:
        tags = []
    return {"name": name, "age": age, "active": active, "tags": tags}

# Function with *args and **kwargs
def variadic_function(*args: int, **kwargs: str) -> tuple:
    """Function with variable arguments."""
    return args, kwargs

# Nested function
def outer_function(x: int) -> callable:
    """Function that returns a closure."""
    def inner_function(y: int) -> int:
        """Inner function accessing outer scope."""
        return x + y
    return inner_function

# Lambda expressions
square = lambda x: x ** 2
filtered_map = lambda items, pred: list(filter(pred, map(str.upper, items)))

# Generator function
def fibonacci_generator(n: int):
    """Generate Fibonacci sequence."""
    a, b = 0, 1
    for _ in range(n):
        yield a
        a, b = b, a + b

# Async function
async def async_fetch(url: str) -> str:
    """Async function for fetching data."""
    await asyncio.sleep(0.1)
    return f"Data from {url}"

# Async generator
async def async_counter(start: int, end: int):
    """Async generator function."""
    for i in range(start, end):
        await asyncio.sleep(0.01)
        yield i

# Decorator definition
def timer_decorator(func):
    """Decorator to time function execution."""
    @wraps(func)
    def wrapper(*args, **kwargs):
        import time
        start = time.time()
        result = func(*args, **kwargs)
        end = time.time()
        print(f"{func.__name__} took {end - start}s")
        return result
    return wrapper

# Decorator with parameters
def retry(max_attempts: int = 3):
    """Decorator factory with parameters."""
    def decorator(func):
        @wraps(func)
        def wrapper(*args, **kwargs):
            for attempt in range(max_attempts):
                try:
                    return func(*args, **kwargs)
                except Exception as e:
                    if attempt == max_attempts - 1:
                        raise
            return None
        return wrapper
    return decorator

# Class decorator
def singleton(cls):
    """Class decorator to implement singleton pattern."""
    instances = {}
    def get_instance(*args, **kwargs):
        if cls not in instances:
            instances[cls] = cls(*args, **kwargs)
        return instances[cls]
    return get_instance

# Context manager using contextlib
@contextmanager
def managed_resource(name: str):
    """Context manager for resource management."""
    print(f"Acquiring {name}")
    try:
        yield name
    finally:
        print(f"Releasing {name}")

# Simple class
class SimpleClass:
    """A simple class with basic features."""
    
    class_variable = "shared"
    
    def __init__(self, name: str):
        self.name = name
        self._protected = "protected"
        self.__private = "private"
    
    def method(self) -> str:
        """Instance method."""
        return self.name
    
    @classmethod
    def class_method(cls) -> str:
        """Class method."""
        return cls.class_variable
    
    @staticmethod
    def static_method(x: int) -> int:
        """Static method."""
        return x * 2
    
    @property
    def protected(self) -> str:
        """Property getter."""
        return self._protected
    
    @protected.setter
    def protected(self, value: str):
        """Property setter."""
        self._protected = value
    
    def __str__(self) -> str:
        """String representation."""
        return f"SimpleClass({self.name})"
    
    def __repr__(self) -> str:
        """Debug representation."""
        return f"SimpleClass(name={self.name!r})"
    
    def _protected_method(self):
        """Protected method (by convention)."""
        pass
    
    def __private_method(self):
        """Private method (name mangling)."""
        pass

# Class with inheritance
class BaseClass(ABC):
    """Abstract base class."""
    
    def __init__(self, id: int):
        self.id = id
    
    @abstractmethod
    def process(self, data: any) -> any:
        """Abstract method that must be implemented."""
        pass
    
    def common_method(self) -> int:
        """Concrete method available to all subclasses."""
        return self.id

class DerivedClass(BaseClass):
    """Derived class implementing abstract methods."""
    
    def __init__(self, id: int, name: str):
        super().__init__(id)
        self.name = name
    
    def process(self, data: any) -> any:
        """Implementation of abstract method."""
        return f"Processing {data} with {self.name}"

# Multiple inheritance
class Mixin1:
    """First mixin class."""
    def mixin1_method(self):
        return "mixin1"

class Mixin2:
    """Second mixin class."""
    def mixin2_method(self):
        return "mixin2"

class MultipleInheritance(DerivedClass, Mixin1, Mixin2):
    """Class with multiple inheritance."""
    pass

# Generic class
class GenericContainer(Generic[T]):
    """Generic container class."""
    
    def __init__(self):
        self._items: List[T] = []
    
    def add(self, item: T) -> None:
        """Add an item to the container."""
        self._items.append(item)
    
    def get(self, index: int) -> Optional[T]:
        """Get an item by index."""
        if 0 <= index < len(self._items):
            return self._items[index]
        return None
    
    def __iter__(self):
        """Make the container iterable."""
        return iter(self._items)

# Protocol (structural subtyping)
class Drawable(Protocol):
    """Protocol defining drawable interface."""
    
    def draw(self) -> None:
        """Draw the object."""
        ...

# Dataclass
@dataclass
class Person:
    """Dataclass with various field types."""
    name: str
    age: int
    email: Optional[str] = None
    tags: List[str] = field(default_factory=list)
    metadata: Dict[str, any] = field(default_factory=dict)
    
    def __post_init__(self):
        """Post-initialization processing."""
        if self.age < 0:
            raise ValueError("Age cannot be negative")

# Frozen dataclass (immutable)
@dataclass(frozen=True)
class Point3D:
    """Immutable 3D point."""
    x: float
    y: float
    z: float
    
    def distance_from_origin(self) -> float:
        """Calculate distance from origin."""
        return (self.x**2 + self.y**2 + self.z**2) ** 0.5

# Enum
class Color(Enum):
    """Enumeration with explicit values."""
    RED = "#FF0000"
    GREEN = "#00FF00"
    BLUE = "#0000FF"

class Status(Enum):
    """Enumeration with auto values."""
    PENDING = auto()
    PROCESSING = auto()
    COMPLETED = auto()
    FAILED = auto()

# NamedTuple
from typing import NamedTuple

class Coordinate(NamedTuple):
    """Named tuple for coordinates."""
    lat: float
    lon: float
    altitude: Optional[float] = None

# TypedDict
from typing import TypedDict

class UserDict(TypedDict):
    """TypedDict for user data."""
    id: int
    name: str
    email: str
    active: bool
    tags: List[str]

# Custom exception
class CustomError(Exception):
    """Custom exception class."""
    
    def __init__(self, message: str, code: int = 0):
        super().__init__(message)
        self.code = code

# Exception hierarchy
class ValidationError(CustomError):
    """Validation error."""
    pass

class NetworkError(CustomError):
    """Network-related error."""
    pass

# Generator expression
squares = (x**2 for x in range(10))

# List comprehension
even_squares = [x**2 for x in range(20) if x % 2 == 0]

# Dict comprehension
word_lengths = {word: len(word) for word in ["hello", "world", "python"]}

# Set comprehension
unique_lengths = {len(word) for word in ["hello", "world", "python", "code"]}

# Complex nested comprehension
matrix = [[i*j for j in range(5)] for i in range(5)]

# Walrus operator (3.8+)
def process_data(data: List[int]) -> Optional[int]:
    """Process data with walrus operator."""
    if (n := len(data)) > 10:
        return sum(data) // n
    return None

# Pattern matching (3.10+)
def handle_command(command: Union[str, List[str], Dict[str, any]]) -> str:
    """Handle command using pattern matching."""
    match command:
        case str(s) if s.startswith("help"):
            return "Showing help"
        case ["list", *items]:
            return f"Listing {len(items)} items"
        case {"action": action, "target": target}:
            return f"Performing {action} on {target}"
        case _:
            return "Unknown command"

# Type guards
def is_string_list(val: List[any]) -> bool:
    """Type guard for list of strings."""
    return all(isinstance(item, str) for item in val)

# Metaclass
class SingletonMeta(type):
    """Metaclass for singleton pattern."""
    _instances = {}
    
    def __call__(cls, *args, **kwargs):
        if cls not in cls._instances:
            cls._instances[cls] = super().__call__(*args, **kwargs)
        return cls._instances[cls]

class SingletonClass(metaclass=SingletonMeta):
    """Class using singleton metaclass."""
    pass

# __slots__ for memory optimization
class OptimizedClass:
    """Class with __slots__ for memory efficiency."""
    __slots__ = ['x', 'y', 'z']
    
    def __init__(self, x, y, z):
        self.x = x
        self.y = y
        self.z = z

# Module-level special variables
__all__ = ['SimpleClass', 'BaseClass', 'DerivedClass', 'Person']
__version__ = "1.0.0"
__author__ = "Test Author"

# Conditional imports
if sys.platform == "win32":
    import msvcrt
else:
    import termios

# Type checking block
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from some_module import SomeType

# Main guard
if __name__ == "__main__":
    # Test code
    obj = SimpleClass("test")
    print(obj)
    
    # Async code
    async def main():
        result = await async_fetch("http://example.com")
        print(result)
    
    # Run async main
    asyncio.run(main())