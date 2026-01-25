-- Lua module patterns

local M = {}

-- Module-level constant
M.VERSION = "2.0.0"
M.AUTHOR = "Test Author"

-- Private helper (underscore prefix convention)
local function _privateHelper(data)
    return data
end

-- Public function
function M.process(data)
    return _privateHelper(data)
end

-- Another public function
function M.validate(input)
    if input == nil then
        return false, "Input cannot be nil"
    end
    if type(input) ~= "string" then
        return false, "Expected string"
    end
    return true, nil
end

-- Public utility
function M.formatOutput(result)
    return string.format("Result: %s", tostring(result))
end

-- Nested module structure
M.utils = {}

function M.utils.trim(s)
    return s:match("^%s*(.-)%s*$")
end

function M.utils.split(s, sep)
    if sep == "" then
        local result = {}
        for i = 1, #s do
            result[i] = s:sub(i, i)
        end
        return result
    end

    local escaped_sep = sep:gsub("([%.%+%-%*%?%[%]%^%$%(%)%%])", "%%%1")
    local result = {}
    for match in (s .. sep):gmatch("(.-)" .. escaped_sep) do
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
