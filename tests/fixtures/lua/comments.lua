-- Documentation comments in Lua

--- Calculate the factorial of a number
--- @param n number The input number
--- @return number The factorial result
function factorial(n)
    if n <= 1 then
        return 1
    end
    return n * factorial(n - 1)
end

--- Check if a number is prime
--- @param n number The number to check
--- @return boolean True if prime, false otherwise
local function isPrime(n)
    if n < 2 then
        return false
    end
    for i = 2, math.sqrt(n) do
        if n % i == 0 then
            return false
        end
    end
    return true
end

--[[
    Multi-line block comment
    This describes a complex data structure
]]
local ComplexData = {
    values = {},
    metadata = {}
}

--- Add a value to the data structure
--- @param value any The value to add
--- @param meta table Optional metadata
function ComplexData:add(value, meta)
    table.insert(self.values, value)
    if meta then
        self.metadata[#self.values] = meta
    end
end

--- Get all values
--- @return table Array of values
function ComplexData:getValues()
    return self.values
end

-- Regular single-line comment (not doc comment)
local helper = function() end

--- Module for string utilities
local StringUtils = {}

--- Capitalize the first letter of a string
--- @param s string Input string
--- @return string Capitalized string
function StringUtils.capitalize(s)
    return s:sub(1, 1):upper() .. s:sub(2)
end

--- Reverse a string
--- @param s string Input string
--- @return string Reversed string
function StringUtils.reverse(s)
    return s:reverse()
end

return {
    factorial = factorial,
    isPrime = isPrime,
    ComplexData = ComplexData,
    StringUtils = StringUtils
}
