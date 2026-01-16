-- Object-Oriented Programming patterns in Lua

-- Class definition using metatables
local Animal = {}
Animal.__index = Animal

--- Create a new Animal instance
function Animal.new(name)
    local self = setmetatable({}, Animal)
    self.name = name
    return self
end

--- Get the animal's name
function Animal:getName()
    return self.name
end

--- Make the animal speak
function Animal:speak()
    return "..."
end

-- Inheritance: Dog extends Animal
local Dog = setmetatable({}, { __index = Animal })
Dog.__index = Dog

function Dog.new(name, breed)
    local self = setmetatable(Animal.new(name), Dog)
    self.breed = breed
    return self
end

function Dog:speak()
    return "Woof!"
end

function Dog:getBreed()
    return self.breed
end

-- Another subclass: Cat
local Cat = setmetatable({}, { __index = Animal })
Cat.__index = Cat

function Cat.new(name, indoor)
    local self = setmetatable(Animal.new(name), Cat)
    self.indoor = indoor
    return self
end

function Cat:speak()
    return "Meow!"
end

function Cat:isIndoor()
    return self.indoor
end

-- Singleton pattern
local Logger = {
    _instance = nil
}

function Logger:getInstance()
    if not self._instance then
        self._instance = {
            level = "INFO",
            log = function(self, msg)
                print("[" .. self.level .. "] " .. msg)
            end
        }
    end
    return self._instance
end

return {
    Animal = Animal,
    Dog = Dog,
    Cat = Cat,
    Logger = Logger
}
