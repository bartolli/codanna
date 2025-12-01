// Swift Comprehensive Test File
// This file covers major Swift language constructs for parser testing

import Foundation
import UIKit

// MARK: - Global Constants and Variables

let globalConstant = "Global Constant"
var globalVariable = 42

// MARK: - Enumerations

enum NetworkError: Error {
    case timeout
    case unauthorized(message: String)
    case serverError(code: Int)
}

enum Direction {
    case north, south, east, west
}

enum Planet: Int {
    case mercury = 1, venus, earth, mars
}

// MARK: - Protocols

protocol Drawable {
    func draw()
    var lineWidth: Double { get set }
}

protocol Named {
    var name: String { get }
}

// MARK: - Structures

struct Point {
    var x: Double
    var y: Double

    init(x: Double, y: Double) {
        self.x = x
        self.y = y
    }

    func distance(to other: Point) -> Double {
        let dx = x - other.x
        let dy = y - other.y
        return sqrt(dx * dx + dy * dy)
    }

    mutating func moveBy(dx: Double, dy: Double) {
        x += dx
        y += dy
    }
}

struct Rectangle: Drawable {
    var origin: Point
    var size: Size
    var lineWidth: Double

    var area: Double {
        return size.width * size.height
    }

    func draw() {
        print("Drawing rectangle at \(origin)")
    }
}

struct Size {
    var width: Double
    var height: Double
}

// MARK: - Classes

class Animal: Named {
    var name: String
    var age: Int

    init(name: String, age: Int) {
        self.name = name
        self.age = age
    }

    func makeSound() {
        print("Some generic animal sound")
    }

    deinit {
        print("\(name) is being deinitialized")
    }
}

class Dog: Animal {
    var breed: String

    init(name: String, age: Int, breed: String) {
        self.breed = breed
        super.init(name: name, age: age)
    }

    override func makeSound() {
        print("Woof!")
    }

    func fetch() {
        print("\(name) is fetching")
    }
}

final class Cat: Animal {
    var indoor: Bool

    init(name: String, age: Int, indoor: Bool) {
        self.indoor = indoor
        super.init(name: name, age: age)
    }

    override func makeSound() {
        print("Meow!")
    }
}

// MARK: - Generics

struct Stack<Element> {
    private var items: [Element] = []

    mutating func push(_ item: Element) {
        items.append(item)
    }

    mutating func pop() -> Element? {
        return items.popLast()
    }

    func peek() -> Element? {
        return items.last
    }

    var isEmpty: Bool {
        return items.isEmpty
    }
}

func swapValues<T>(_ a: inout T, _ b: inout T) {
    let temp = a
    a = b
    b = temp
}

// MARK: - Extensions

extension Int {
    var squared: Int {
        return self * self
    }

    func times(_ operation: () -> Void) {
        for _ in 0..<self {
            operation()
        }
    }
}

extension String {
    func reversed() -> String {
        return String(self.reversed())
    }
}

// MARK: - Closures and Higher-Order Functions

func performOperation(_ a: Int, _ b: Int, operation: (Int, Int) -> Int) -> Int {
    return operation(a, b)
}

let numbers = [1, 2, 3, 4, 5]
let doubled = numbers.map { $0 * 2 }
let evens = numbers.filter { $0 % 2 == 0 }
let sum = numbers.reduce(0, +)

// MARK: - Error Handling

func fetchData(from url: String) throws -> Data {
    guard !url.isEmpty else {
        throw NetworkError.timeout
    }

    if url.contains("unauthorized") {
        throw NetworkError.unauthorized(message: "Invalid credentials")
    }

    return Data()
}

func processData() {
    do {
        let data = try fetchData(from: "https://example.com")
        print("Data received: \(data.count) bytes")
    } catch NetworkError.timeout {
        print("Request timed out")
    } catch NetworkError.unauthorized(let message) {
        print("Unauthorized: \(message)")
    } catch {
        print("Unknown error: \(error)")
    }
}

// MARK: - Optionals and Optional Chaining

class Person {
    var name: String
    var address: Address?

    init(name: String) {
        self.name = name
    }
}

class Address {
    var street: String
    var city: String

    init(street: String, city: String) {
        self.street = street
        self.city = city
    }
}

func printAddress(for person: Person?) {
    if let city = person?.address?.city {
        print("City: \(city)")
    } else {
        print("No address available")
    }
}

// MARK: - Property Wrappers

@propertyWrapper
struct Capitalized {
    private var value: String = ""

    var wrappedValue: String {
        get { value }
        set { value = newValue.capitalized }
    }
}

struct User {
    @Capitalized var firstName: String
    @Capitalized var lastName: String
}

// MARK: - Async/Await

func fetchUserData(id: Int) async throws -> String {
    try await Task.sleep(nanoseconds: 1_000_000_000)
    return "User \(id)"
}

func processUsers() async {
    do {
        let user = try await fetchUserData(id: 1)
        print("Fetched: \(user)")
    } catch {
        print("Error fetching user: \(error)")
    }
}

// MARK: - Actor for Concurrency

actor Counter {
    private var value = 0

    func increment() {
        value += 1
    }

    func getValue() -> Int {
        return value
    }
}

// MARK: - Type Aliases

typealias Coordinate = (x: Double, y: Double)
typealias CompletionHandler = (Result<String, Error>) -> Void

// MARK: - Subscripts

struct Matrix {
    let rows: Int
    let columns: Int
    private var grid: [Double]

    init(rows: Int, columns: Int) {
        self.rows = rows
        self.columns = columns
        self.grid = Array(repeating: 0.0, count: rows * columns)
    }

    subscript(row: Int, column: Int) -> Double {
        get {
            return grid[row * columns + column]
        }
        set {
            grid[row * columns + column] = newValue
        }
    }
}

// MARK: - Static and Class Methods

class MathUtilities {
    static let pi = 3.14159

    static func add(_ a: Int, _ b: Int) -> Int {
        return a + b
    }

    class func multiply(_ a: Int, _ b: Int) -> Int {
        return a * b
    }
}

// MARK: - Nested Types

class Vehicle {
    enum VehicleType {
        case car, truck, motorcycle
    }

    struct Specification {
        var engineSize: Double
        var horsepower: Int
    }

    var type: VehicleType
    var spec: Specification

    init(type: VehicleType, spec: Specification) {
        self.type = type
        self.spec = spec
    }
}

// MARK: - Main Function

@main
struct App {
    static func main() {
        print("Swift Comprehensive Example")

        let dog = Dog(name: "Rex", age: 3, breed: "Labrador")
        dog.makeSound()

        var stack = Stack<Int>()
        stack.push(1)
        stack.push(2)
        print("Stack top: \(stack.peek() ?? 0)")

        processData()
    }
}

// MARK: - Visibility Test Cases (added for parsing validation)

public class PublicClass {
    private var privateVar: Int = 0
    internal func internalMethod() {}
    fileprivate let fileprivateProperty = "test"
    open func openMethod() {}
}

// MARK: - Switch Statements

func describeNumber(_ num: Int) -> String {
    switch num {
    case 0:
        return "zero"
    case 1...9:
        return "single digit"
    case 10..<100:
        return "double digit"
    default:
        return "large number"
    }
}

func describeValue(_ value: Any) -> String {
    switch value {
    case let x as Int:
        return "Integer: \(x)"
    case let s as String:
        return "String: \(s)"
    case is Double:
        return "It's a Double"
    default:
        return "Unknown type"
    }
}

// MARK: - While Loops

func countDown(from start: Int) {
    var count = start
    while count > 0 {
        print(count)
        count -= 1
    }
}

// MARK: - Type Casting

func processMixedArray(_ items: [Any]) {
    for item in items {
        if let number = item as? Int {
            print("Number: \(number)")
        } else if let text = item as? String {
            print("Text: \(text)")
        }
    }

    let forcedString = "hello" as String
    let optionalInt = 42 as? Int
    let definiteInt = 42 as! Int
    _ = forcedString
    _ = optionalInt
    _ = definiteInt
}

// MARK: - Ternary Expressions

func absoluteValue(_ x: Int) -> Int {
    return x >= 0 ? x : -x
}

let isEven = 4 % 2 == 0 ? true : false

// MARK: - Dictionary Operations

let emptyDict: [String: Int] = [:]
let ages: [String: Int] = ["Alice": 30, "Bob": 25]
var mutableDict = ["key1": "value1", "key2": "value2"]

func processDict(_ dict: [String: Any]) {
    for (key, value) in dict {
        print("\(key): \(value)")
    }
}

// MARK: - Boolean Literals

let isActive = true
let isDisabled = false
let condition: Bool = true && false || true

// MARK: - Property Observers (willSet/didSet)

class ObservableValue {
    var value: Int = 0 {
        willSet {
            print("About to change from \(value) to \(newValue)")
        }
        didSet {
            print("Changed from \(oldValue) to \(value)")
        }
    }

    var name: String = "" {
        didSet {
            print("Name changed to \(name)")
        }
    }
}

// MARK: - Generic Constraints with Where Clause

func findIndex<T: Equatable>(of valueToFind: T, in array: [T]) -> Int? {
    for (index, value) in array.enumerated() {
        if value == valueToFind {
            return index
        }
    }
    return nil
}

func allItemsMatch<C1: Collection, C2: Collection>(_ c1: C1, _ c2: C2) -> Bool
    where C1.Element == C2.Element, C1.Element: Equatable {
    if c1.count != c2.count {
        return false
    }
    for (item1, item2) in zip(c1, c2) {
        if item1 != item2 {
            return false
        }
    }
    return true
}

// MARK: - Associated Types in Protocols

protocol Container {
    associatedtype Item
    mutating func append(_ item: Item)
    var count: Int { get }
    subscript(i: Int) -> Item { get }
}

protocol SuffixableContainer: Container {
    associatedtype Suffix: SuffixableContainer where Suffix.Item == Item
    func suffix(_ size: Int) -> Suffix
}

struct IntStack: Container {
    typealias Item = Int
    var items: [Int] = []

    mutating func append(_ item: Int) {
        items.append(item)
    }

    var count: Int {
        return items.count
    }

    subscript(i: Int) -> Int {
        return items[i]
    }
}

// MARK: - Opaque Types (some)

protocol Shape {
    func draw() -> String
}

struct Triangle: Shape {
    var size: Int
    func draw() -> String {
        var result: [String] = []
        for length in 1...size {
            result.append(String(repeating: "*", count: length))
        }
        return result.joined(separator: "\n")
    }
}

func makeOpaqueShape() -> some Shape {
    return Triangle(size: 3)
}
