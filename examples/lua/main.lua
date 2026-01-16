--- Main entry point demonstrating module usage
--- @module main

local comprehensive = require("comprehensive")
local helper = require("utils.helper")

-- Use Config class
local config = comprehensive.Config:new("main-app", 3000)
print(config:toString())

-- Use inheritance
local extConfig = comprehensive.ExtendedConfig:new("extended", 4000, "extra-data")
print(extConfig:toString())
print("Extra:", extConfig:getExtra())

-- Use Container
local container = comprehensive.Container:new()
container:add("first")
container:add("second")
container:add("third")

print("Container size:", container:size())
for item in container:iter() do
    print("  Item:", item)
end

-- Use Vector with operator overloading
local v1 = comprehensive.Vector:new(3, 4)
local v2 = comprehensive.Vector:new(1, 2)
local v3 = v1 + v2
print("Vector sum:", tostring(v3))
print("Magnitude:", v1:magnitude())

-- Use helper utilities
local data = { name = "test", nested = { value = 1 } }
local copied = helper.deepCopy(data)
copied.nested.value = 2
print("Original nested value:", data.nested.value)
print("Copied nested value:", copied.nested.value)

-- Use Result pattern
local result = comprehensive.calculate("add", 10, 5)
if result.ok then
    print("Calculation result:", result.value)
else
    print("Error:", result.error)
end

-- Use counter closure
local inc, dec = comprehensive.createCounter(10)
print("Increment:", inc())
print("Increment:", inc())
print("Decrement:", dec())

-- Use variadic function
print("Sum:", comprehensive.sum(1, 2, 3, 4, 5))

-- Use multiple returns
local min, max, sum = comprehensive.minMaxSum(3, 7)
print(string.format("Min: %d, Max: %d, Sum: %d", min, max, sum))

-- Use higher-order function
local loggedSum = comprehensive.withLogging(function(a, b)
    return a + b
end)
loggedSum(2, 3)

-- Use factory
local obj = comprehensive.createObject("vector")
print("Factory created:", tostring(obj))

-- Use string utilities
local trimmed = helper.trim("  hello world  ")
print("Trimmed:", "'" .. trimmed .. "'")

local parts = helper.split("a,b,c", ",")
print("Split parts:", table.concat(parts, " | "))

-- Use merge
local base = { a = 1, b = { x = 10 } }
local override = { b = { y = 20 }, c = 3 }
local merged = helper.merge(base, override)
print("Merged b.x:", merged.b.x)
print("Merged b.y:", merged.b.y)
print("Merged c:", merged.c)

print("\nAll examples completed successfully!")
