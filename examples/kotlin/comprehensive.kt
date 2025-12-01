/**
 * Comprehensive Kotlin test file for parser maturity assessment
 * Tests all major Kotlin language features and constructs
 */

package com.example.comprehensive

import kotlin.collections.List
import kotlin.collections.Map
import java.util.*

// === TEST SCENARIO: Clear Relationship Testing ===

/**
 * Test service for demonstrating clear relationships
 */
class TestService(
    val name: String,
    private val config: Config
) {
    /**
     * Create a new test service with default config
     */
    constructor(name: String) : this(name, Config.default())

    /**
     * Process data using config - this should show calls
     */
    fun process(): String {
        val result = getConfigName()  // CALLS: TestService.getConfigName
        return "Processing: $result"
    }

    /**
     * Helper method that will be called by process
     */
    private fun getConfigName(): String {
        return config.getDisplayName()  // CALLS: Config.getDisplayName
    }

    /**
     * Nested class example
     */
    class NestedClass {
        fun doSomething() {}
    }

    /**
     * Inner class example
     */
    inner class InnerClass {
        fun accessOuter() = name
    }

    companion object {
        /**
         * Factory method in companion object
         */
        fun create(name: String): TestService {
            return TestService(name)
        }

        const val DEFAULT_NAME = "default"
    }
}

// === DATA CLASSES ===

/**
 * Data class for configuration
 */
data class Config(
    val displayName: String,
    val enabled: Boolean = true,
    var retries: Int = 3
) {
    fun getDisplayName(): String = displayName

    companion object {
        fun default(): Config = Config("Default Config")
    }
}

/**
 * User data class with multiple properties
 */
data class User(
    val id: Long,
    val email: String,
    val name: String?
)

// === INTERFACES ===

/**
 * Repository interface
 */
interface Repository<T> {
    fun save(item: T): Boolean
    fun findById(id: Long): T?
    fun findAll(): List<T>
}

/**
 * Named entity interface
 */
interface Named {
    val name: String
}

/**
 * Auditable interface with default implementation
 */
interface Auditable {
    fun audit(): String = "Audited"
}

// === INTERFACE IMPLEMENTATION ===

/**
 * User repository implementing Repository interface
 */
class UserRepository : Repository<User>, Auditable {
    override fun save(item: User): Boolean {
        // Implementation
        return true
    }

    override fun findById(id: Long): User? {
        return null
    }

    override fun findAll(): List<User> {
        return emptyList()
    }
}

// === ENUM CLASSES ===

/**
 * Status enum for tracking state
 */
enum class Status {
    ACTIVE,
    INACTIVE,
    PENDING,
    ARCHIVED;

    fun isActive(): Boolean = this == ACTIVE
}

/**
 * Priority enum with properties
 */
enum class Priority(val level: Int, val label: String) {
    LOW(1, "Low Priority"),
    MEDIUM(2, "Medium Priority"),
    HIGH(3, "High Priority"),
    CRITICAL(4, "Critical Priority");

    companion object {
        fun fromLevel(level: Int): Priority? {
            return values().find { it.level == level }
        }
    }
}

// === SEALED CLASSES ===

/**
 * Sealed class for result types
 */
sealed class Result<out T> {
    data class Success<T>(val data: T) : Result<T>()
    data class Error(val message: String, val code: Int) : Result<Nothing>()
    object Loading : Result<Nothing>()
}

/**
 * Sealed interface for events
 */
sealed interface Event {
    data class Click(val x: Int, val y: Int) : Event
    data class KeyPress(val key: String) : Event
    object Refresh : Event
}

// === OBJECT DECLARATIONS ===

/**
 * Singleton configuration manager
 */
object ConfigManager {
    private val configs = mutableMapOf<String, Config>()

    fun register(key: String, config: Config) {
        configs[key] = config
    }

    fun get(key: String): Config? = configs[key]
}

/**
 * Logger singleton
 */
object Logger {
    fun log(message: String) {
        println("[LOG] $message")
    }
}

// === EXTENSION FUNCTIONS ===

/**
 * Extension function for String
 */
fun String.toTitleCase(): String {
    return this.split(" ").joinToString(" ") {
        it.replaceFirstChar { c -> c.uppercase() }
    }
}

/**
 * Extension function for List
 */
fun <T> List<T>.secondOrNull(): T? {
    return if (this.size > 1) this[1] else null
}

// === TYPE ALIASES ===

typealias StringMap = Map<String, String>
typealias UserList = List<User>
typealias ResultCallback<T> = (Result<T>) -> Unit

// === GENERIC CLASSES ===

/**
 * Generic box class
 */
class Box<T>(val value: T) {
    fun unwrap(): T = value

    fun <R> map(transform: (T) -> R): Box<R> {
        return Box(transform(value))
    }
}

/**
 * Generic repository with constraints
 */
class CachingRepository<T : Any>(
    private val delegate: Repository<T>
) : Repository<T> {
    private val cache = mutableMapOf<Long, T>()

    override fun save(item: T): Boolean {
        return delegate.save(item)
    }

    override fun findById(id: Long): T? {
        return cache[id] ?: delegate.findById(id)?.also { cache[id] = it }
    }

    override fun findAll(): List<T> {
        return delegate.findAll()
    }
}

// === ANNOTATIONS ===

/**
 * Custom annotation
 */
@Target(AnnotationTarget.CLASS, AnnotationTarget.FUNCTION)
@Retention(AnnotationRetention.RUNTIME)
annotation class Documented(val author: String, val date: String)

/**
 * Deprecated function example
 */
@Deprecated("Use newFunction instead", ReplaceWith("newFunction()"))
fun oldFunction() {
    println("Old function")
}

@Documented(author = "John Doe", date = "2025-11-02")
fun newFunction() {
    println("New function")
}

// === LAMBDA AND HIGHER-ORDER FUNCTIONS ===

/**
 * Higher-order function example
 */
fun <T, R> List<T>.customMap(transform: (T) -> R): List<R> {
    val result = mutableListOf<R>()
    for (item in this) {
        result.add(transform(item))
    }
    return result
}

/**
 * Function taking lambda parameter
 */
fun processUsers(users: List<User>, handler: (User) -> Unit) {
    users.forEach(handler)
}

// === NULLABLE TYPES ===

/**
 * Service handling nullable types
 */
class NullableService {
    fun findUser(id: Long): User? {
        return null
    }

    fun getUserName(user: User?): String {
        return user?.name ?: "Unknown"
    }

    fun requireUser(user: User?): User {
        return user ?: throw IllegalArgumentException("User required")
    }
}

// === DELEGATION ===

/**
 * Interface for delegation
 */
interface Printer {
    fun print(message: String)
}

/**
 * Concrete printer implementation
 */
class ConsolePrinter : Printer {
    override fun print(message: String) {
        println(message)
    }
}

/**
 * Class using delegation
 */
class LoggingPrinter(printer: Printer) : Printer by printer {
    override fun print(message: String) {
        Logger.log("Printing: $message")
        // Delegate to the wrapped printer would happen here in real impl
    }
}

// === PROPERTY DELEGATES ===

/**
 * Class with lazy property
 */
class LazyService {
    val expensiveValue: String by lazy {
        computeExpensiveValue()
    }

    private fun computeExpensiveValue(): String {
        return "Computed value"
    }
}

// === OPERATOR OVERLOADING ===

/**
 * Point class with operator overloading
 */
data class Point(val x: Int, val y: Int) {
    operator fun plus(other: Point): Point {
        return Point(x + other.x, y + other.y)
    }

    operator fun minus(other: Point): Point {
        return Point(x - other.x, y - other.y)
    }

    operator fun unaryMinus(): Point {
        return Point(-x, -y)
    }
}

// === SUSPEND FUNCTIONS (COROUTINES) ===

/**
 * Suspend function for async operations
 */
suspend fun fetchUser(id: Long): User? {
    // Simulated async operation
    return null
}

/**
 * Repository with suspend functions
 */
class AsyncUserRepository {
    suspend fun save(user: User): Boolean {
        return true
    }

    suspend fun findAll(): List<User> {
        return emptyList()
    }
}

// === INLINE FUNCTIONS ===

/**
 * Inline function for performance
 */
inline fun <T> measureTime(block: () -> T): T {
    val start = System.currentTimeMillis()
    val result = block()
    val end = System.currentTimeMillis()
    println("Took ${end - start}ms")
    return result
}

/**
 * Inline function with reified type
 */
inline fun <reified T> isInstance(value: Any): Boolean {
    return value is T
}

// === VISIBILITY MODIFIERS ===

/**
 * Class demonstrating visibility modifiers
 */
class VisibilityDemo {
    public val publicField: String = "public"
    internal val internalField: String = "internal"
    protected val protectedField: String = "protected"
    private val privateField: String = "private"

    public fun publicMethod() {}
    internal fun internalMethod() {}
    protected fun protectedMethod() {}
    private fun privateMethod() {}
}

// === TOP-LEVEL FUNCTIONS ===

/**
 * Top-level function
 */
fun topLevelFunction(): String {
    return "Top level"
}

/**
 * Top-level function with parameters
 */
fun processData(data: String, config: Config): Result<String> {
    return Result.Success(data)
}

// === TOP-LEVEL PROPERTIES ===

val topLevelProperty: String = "Top level property"
const val CONSTANT_VALUE: Int = 42

// === VARARG AND DEFAULT PARAMETERS ===

/**
 * Function with vararg and default parameters
 */
fun format(separator: String = ", ", vararg items: String): String {
    return items.joinToString(separator)
}

/**
 * Function with multiple default parameters
 */
fun createUser(
    id: Long,
    name: String,
    email: String = "unknown@example.com",
    active: Boolean = true
): User {
    return User(id, email, name)
}

// === INFIX FUNCTIONS ===

/**
 * Infix function example
 */
infix fun Int.times(str: String): String {
    return str.repeat(this)
}

// === CONTEXT RECEIVERS ===

/**
 * Context receiver function (Kotlin 1.6.20+ experimental feature)
 * Tree-sitter parses this as infix_expression, not function_declaration
 */
context(Logger) fun logMessage(message: String) {
    log("Context receiver: $message")
}

// === TAILREC FUNCTIONS ===

/**
 * Tail recursive function
 */
tailrec fun factorial(n: Long, accumulator: Long = 1): Long {
    return if (n <= 1) accumulator else factorial(n - 1, n * accumulator)
}

// === MAIN FUNCTION ===

/**
 * Main entry point
 */
fun main(args: Array<String>) {
    val service = TestService.create("MyService")
    val result = service.process()
    Logger.log(result)

    val user = User(1, "user@example.com", "John")
    val repo = UserRepository()
    repo.save(user)

    val point1 = Point(1, 2)
    val point2 = Point(3, 4)
    val sum = point1 + point2

    println("Comprehensive Kotlin test completed")
}
