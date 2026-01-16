-- Method definitions in Lua

-- Object with colon-style methods
local Counter = {}
Counter.__index = Counter

function Counter.new(initial)
    local self = setmetatable({}, Counter)
    self.value = initial or 0
    return self
end

-- Colon syntax method (implicit self)
function Counter:increment()
    self.value = self.value + 1
end

function Counter:decrement()
    self.value = self.value - 1
end

function Counter:getValue()
    return self.value
end

function Counter:reset()
    self.value = 0
end

-- Dot syntax method (explicit self parameter)
function Counter.add(self, amount)
    self.value = self.value + amount
end

-- Static method (no self)
function Counter.create()
    return Counter.new(0)
end

-- String buffer with method chaining
local StringBuilder = {}
StringBuilder.__index = StringBuilder

function StringBuilder.new()
    local self = setmetatable({}, StringBuilder)
    self.parts = {}
    return self
end

function StringBuilder:append(str)
    table.insert(self.parts, str)
    return self  -- Enable chaining
end

function StringBuilder:appendLine(str)
    table.insert(self.parts, str)
    table.insert(self.parts, "\n")
    return self
end

function StringBuilder:toString()
    return table.concat(self.parts)
end

function StringBuilder:clear()
    self.parts = {}
    return self
end

-- Table with metamethods
local Vector = {}
Vector.__index = Vector

function Vector.new(x, y)
    local self = setmetatable({}, Vector)
    self.x = x or 0
    self.y = y or 0
    return self
end

function Vector:length()
    return math.sqrt(self.x * self.x + self.y * self.y)
end

function Vector:normalize()
    local len = self:length()
    if len > 0 then
        self.x = self.x / len
        self.y = self.y / len
    end
    return self
end

-- Metamethod for addition
function Vector.__add(a, b)
    return Vector.new(a.x + b.x, a.y + b.y)
end

-- Metamethod for string representation
function Vector.__tostring(v)
    return string.format("Vector(%g, %g)", v.x, v.y)
end

return {
    Counter = Counter,
    StringBuilder = StringBuilder,
    Vector = Vector
}
