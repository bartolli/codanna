-- Basic Lua constructs

-- Global function
function greet(name)
    return "Hello, " .. name
end

-- Local function
local function helper(x)
    return x * 2
end

-- Local variables
local counter = 0
local name = "test"

-- Global variable (module-level constant convention)
VERSION = "1.0.0"

-- Screaming case constant
local MAX_RETRIES = 5

-- Table as data structure
local config = {
    host = "localhost",
    port = 8080,
    debug = true
}

-- Function with multiple parameters
function calculate(a, b, operation)
    if operation == "add" then
        return a + b
    elseif operation == "sub" then
        return a - b
    else
        return 0
    end
end

-- Nested function
function outer()
    local function inner()
        return "inner"
    end
    return inner()
end
