-- Lua module patterns

local M = {}

-- Module-level constant
M.VERSION = "2.0.0"
M.AUTHOR = "Test Author"

-- Public function
function M.process(data)
    return _privateHelper(data)
end

-- Another public function
function M.validate(input)
    if type(input) ~= "string" then
        return false, "Expected string"
    end
    return true, nil
end

-- Public utility
function M.formatOutput(result)
    return string.format("Result: %s", tostring(result))
end

-- Private helper (underscore prefix convention)
local function _privateHelper(data)
    return data
end

-- Private validator
local function _validateInput(input)
    return input ~= nil
end

-- Nested module structure
M.utils = {}

function M.utils.trim(s)
    return s:match("^%s*(.-)%s*$")
end

function M.utils.split(s, sep)
    local result = {}
    for match in (s .. sep):gmatch("(.-)" .. sep) do
        table.insert(result, match)
    end
    return result
end

-- Initialization function
function M.init(config)
    M._config = config or {}
    return M
end

return M
