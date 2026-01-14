--- Comprehensive Lua test file for parser maturity assessment
--- Tests all major Lua language features and constructs
---
--- @module comprehensive
--- @author Codanna

local M = {}

-- Module-level constants (convention: SCREAMING_CASE)
local MAX_SIZE = 1024
local DEFAULT_NAME = "default"
M.PUBLIC_CONSTANT = 42

-- Module-level variables
local counter = 0
local instance = nil

---
--- Configuration class using metatables
--- @class Config
--- @field name string The config name
--- @field port number The port number
--- @field enabled boolean Whether config is enabled
---
local Config = {}
Config.__index = Config

--- Create a new Config instance
--- @param name string The configuration name
--- @param port number? Optional port (defaults to 8080)
--- @return Config
function Config:new(name, port)
    local self = setmetatable({}, Config)
    self.name = name or DEFAULT_NAME
    self.port = port or 8080
    self.enabled = true
    self._private_field = "internal"
    return self
end

--- Get the port number
--- @return number
function Config:getPort()
    return self.port
end

--- Set the port number
--- @param port number The new port
function Config:setPort(port)
    self.port = port
end

--- Convert config to string representation
--- @return string
function Config:toString()
    return string.format("Config{name=%s, port=%d}", self.name, self.port)
end

--- Static method to create default config
--- @return Config
function Config.createDefault()
    return Config:new(DEFAULT_NAME, 8080)
end

M.Config = Config

---
--- Generic container class
--- @class Container
--- @field items table Array of items
---
local Container = {}
Container.__index = Container

function Container:new()
    local self = setmetatable({}, Container)
    self.items = {}
    self._count = 0
    return self
end

function Container:add(item)
    table.insert(self.items, item)
    self._count = self._count + 1
end

function Container:get(index)
    return self.items[index]
end

function Container:size()
    return self._count
end

--- Iterator for container items
--- @return function
function Container:iter()
    local i = 0
    return function()
        i = i + 1
        return self.items[i]
    end
end

M.Container = Container

---
--- Inheritance example: ExtendedConfig inherits from Config
--- @class ExtendedConfig : Config
--- @field extra string Extra configuration data
---
local ExtendedConfig = setmetatable({}, { __index = Config })
ExtendedConfig.__index = ExtendedConfig

function ExtendedConfig:new(name, port, extra)
    local self = setmetatable(Config:new(name, port), ExtendedConfig)
    self.extra = extra or ""
    return self
end

function ExtendedConfig:getExtra()
    return self.extra
end

--- Override parent method
function ExtendedConfig:toString()
    return string.format("ExtendedConfig{name=%s, port=%d, extra=%s}",
        self.name, self.port, self.extra)
end

M.ExtendedConfig = ExtendedConfig

-- Enum-like pattern using tables
M.Status = {
    ACTIVE = "active",
    INACTIVE = "inactive",
    PENDING = "pending",
}

-- Result type pattern
local function Ok(value)
    return { ok = true, value = value }
end

local function Err(message)
    return { ok = false, error = message }
end

M.Ok = Ok
M.Err = Err

---
--- Complex function with multiple parameters
--- @param reference string A reference string
--- @param items table A table of items
--- @param callback function A callback function
--- @return string, table
---
function M.complexFunction(reference, items, callback)
    local results = {}
    for i, item in ipairs(items) do
        results[i] = callback(item)
    end
    return reference, results
end

--- Async-like pattern using coroutines
--- @param url string The URL to fetch
--- @return thread
function M.asyncOperation(url)
    return coroutine.create(function()
        -- Simulate async work
        coroutine.yield("connecting")
        coroutine.yield("fetching")
        return Ok(url)
    end)
end

--- Higher-order function
--- @param f function The function to wrap
--- @return function
function M.withLogging(f)
    return function(...)
        print("Calling function with args:", ...)
        local result = f(...)
        print("Function returned:", result)
        return result
    end
end

--- Closure example
--- @param initial number Initial counter value
--- @return function, function
function M.createCounter(initial)
    local count = initial or 0

    local function increment()
        count = count + 1
        return count
    end

    local function decrement()
        count = count - 1
        return count
    end

    return increment, decrement
end

--- Variadic function
--- @vararg any
--- @return number
function M.sum(...)
    local total = 0
    for _, v in ipairs({...}) do
        total = total + v
    end
    return total
end

--- Multiple return values
--- @param x number
--- @param y number
--- @return number, number, number
function M.minMaxSum(x, y)
    local min = math.min(x, y)
    local max = math.max(x, y)
    local sum = x + y
    return min, max, sum
end

--- Pattern matching equivalent using table lookup
local handlers = {
    add = function(a, b) return a + b end,
    sub = function(a, b) return a - b end,
    mul = function(a, b) return a * b end,
    div = function(a, b) return a / b end,
}

function M.calculate(op, a, b)
    local handler = handlers[op]
    if handler then
        return Ok(handler(a, b))
    else
        return Err("Unknown operation: " .. op)
    end
end

--- Metatable-based operator overloading
local Vector = {}
Vector.__index = Vector

function Vector:new(x, y)
    return setmetatable({ x = x or 0, y = y or 0 }, Vector)
end

function Vector.__add(a, b)
    return Vector:new(a.x + b.x, a.y + b.y)
end

function Vector.__sub(a, b)
    return Vector:new(a.x - b.x, a.y - b.y)
end

function Vector.__mul(a, scalar)
    return Vector:new(a.x * scalar, a.y * scalar)
end

function Vector.__tostring(v)
    return string.format("Vector(%d, %d)", v.x, v.y)
end

function Vector:magnitude()
    return math.sqrt(self.x * self.x + self.y * self.y)
end

M.Vector = Vector

--- Mixin pattern
local Loggable = {}

function Loggable:log(message)
    print(string.format("[%s] %s", self.name or "unknown", message))
end

function Loggable:debug(message)
    print(string.format("[DEBUG][%s] %s", self.name or "unknown", message))
end

--- Apply mixin to a class
--- @param class table The class to extend
function M.makeLoggable(class)
    for k, v in pairs(Loggable) do
        if class[k] == nil then
            class[k] = v
        end
    end
end

-- Apply mixin to Config
M.makeLoggable(Config)

--- Factory function pattern
--- @param type string The type of object to create
--- @return table|nil
function M.createObject(type)
    if type == "config" then
        return Config:new("factory-created")
    elseif type == "container" then
        return Container:new()
    elseif type == "vector" then
        return Vector:new(0, 0)
    else
        return nil
    end
end

--- Lazy initialization pattern
local _lazyValue = nil

function M.getLazyValue()
    if _lazyValue == nil then
        _lazyValue = {
            initialized = true,
            timestamp = os.time(),
        }
    end
    return _lazyValue
end

--- Error handling pattern
--- @param fn function The function to call safely
--- @return boolean, any
function M.pcallWrapper(fn, ...)
    local ok, result = pcall(fn, ...)
    if ok then
        return Ok(result)
    else
        return Err(result)
    end
end

--- Control flow examples for parser coverage
--- @param items table Items to process
--- @param limit number Maximum iterations
--- @return table Processed results
function M.controlFlowExamples(items, limit)
    local results = {}
    local i = 1

    while i <= limit and i <= #items do
        table.insert(results, items[i])
        i = i + 1
    end

    local j = 1
    repeat
        if results[j] then
            results[j] = results[j] * 2
        end
        j = j + 1
    until j > #results

    do
        local temp = {}
        for idx = #results, 1, -1 do
            table.insert(temp, results[idx])
        end
        results = temp
    end

    return results
end

--- Module initialization
local function _init()
    counter = 0
    instance = nil
end

_init()

return M
