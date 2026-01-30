--- Utility helper module
--- @module utils.helper

local M = {}

--- Check if a value is nil or empty
--- @param value any The value to check
--- @return boolean
function M.isEmpty(value)
    if value == nil then
        return true
    end
    if type(value) == "string" and value == "" then
        return true
    end
    if type(value) == "table" and next(value) == nil then
        return true
    end
    return false
end

--- Deep copy a table
--- @param original table The table to copy
--- @param seen table|nil Optional memoization table to handle cycles
--- @return table
function M.deepCopy(original, seen)
    if type(original) ~= "table" then
        return original
    end

    -- Initialize seen table for cycle detection
    seen = seen or {}
    
    -- Return memoized copy if we've already seen this table
    if seen[original] then
        return seen[original]
    end

    local copy = {}
    -- Memoize before recursing to handle self-references
    seen[original] = copy
    
    for key, value in pairs(original) do
        copy[M.deepCopy(key, seen)] = M.deepCopy(value, seen)
    end
    return setmetatable(copy, getmetatable(original))
end

--- Merge two tables
--- @param base table The base table
--- @param override table The override table
--- @return table
function M.merge(base, override)
    local result = M.deepCopy(base)
    for key, value in pairs(override) do
        if type(value) == "table" and type(result[key]) == "table" then
            result[key] = M.merge(result[key], value)
        else
            result[key] = value
        end
    end
    return result
end

--- String trim
--- @param s string The string to trim
--- @return string
function M.trim(s)
    return s:match("^%s*(.-)%s*$")
end

--- Split string by delimiter
--- @param s string The string to split
--- @param delimiter string The delimiter
--- @return table
function M.split(s, delimiter)
    -- Guard against nil or empty delimiter
    if not delimiter or delimiter == "" then
        return {s}
    end
    
    -- Escape pattern-magic characters in delimiter
    -- Escapes: . * + ? ^ $ ( ) [ ] % -
    local escaped_delimiter = delimiter:gsub("([%.%*%+%?%^%$%(%)%[%]%%%-])", "%%%1")
    
    local result = {}
    for match in (s .. delimiter):gmatch("(.-)" .. escaped_delimiter) do
        table.insert(result, match)
    end
    return result
end

return M
