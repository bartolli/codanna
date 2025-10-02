/// <summary>
/// Comprehensive C# test file for Codanna parser validation.
/// Tests all major C# language features and relationship tracking.
///
/// This file is designed to validate:
/// - Symbol extraction (classes, interfaces, methods, properties, fields, enums)
/// - Relationship tracking (implements, calls, callers)
/// - Documentation comment parsing
/// - Method call resolution
/// - Interface implementation detection
/// </summary>

using System;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace Codanna.Examples.CSharp
{
    // === TEST SCENARIO 1: Clear Interface Implementation ===

    /// <summary>
    /// Service interface for data processing.
    /// Tests: Interface symbol extraction, implementer detection
    /// </summary>
    public interface IDataProcessor
    {
        /// <summary>
        /// Process data with the given options.
        /// </summary>
        /// <param name="data">The data to process</param>
        /// <param name="options">Processing options</param>
        /// <returns>Processed result</returns>
        string ProcessData(string data, ProcessingOptions options);

        /// <summary>
        /// Validate data before processing.
        /// </summary>
        bool ValidateData(string data);
    }

    /// <summary>
    /// Concrete implementation of IDataProcessor.
    /// Tests: Class-to-interface relationship, method definitions
    /// </summary>
    public class DataProcessorService : IDataProcessor
    {
        private readonly ILogger _logger;
        private readonly IValidator _validator;

        /// <summary>
        /// Initializes a new instance of the DataProcessorService class.
        /// </summary>
        /// <param name="logger">Logger for diagnostic traces</param>
        /// <param name="validator">Validator for data validation</param>
        public DataProcessorService(ILogger logger, IValidator validator)
        {
            _logger = logger;
            _validator = validator;
        }

        /// <inheritdoc />
        public string ProcessData(string data, ProcessingOptions options)
        {
            // CALLS: ValidateData (should show in get_calls)
            if (!ValidateData(data))
            {
                // CALLS: ILogger.LogError
                _logger.LogError("Invalid data provided");
                throw new ArgumentException("Data validation failed", nameof(data));
            }

            // CALLS: Transform (should show internal call)
            var transformed = Transform(data, options);

            // CALLS: ILogger.LogInfo
            _logger.LogInfo("Data processed successfully");

            return transformed;
        }

        /// <inheritdoc />
        public bool ValidateData(string data)
        {
            // CALLS: IValidator.Validate
            return _validator.Validate(data);
        }

        /// <summary>
        /// Internal transformation method.
        /// Tests: Method call from ProcessData
        /// </summary>
        private string Transform(string data, ProcessingOptions options)
        {
            // CALLS: ProcessingOptions.Apply
            return options.Apply(data);
        }
    }

    // === TEST SCENARIO 2: Base Class and Inheritance ===

    /// <summary>
    /// Base service class providing common functionality.
    /// Tests: Base class extraction, inheritance relationships
    /// </summary>
    public abstract class BaseService
    {
        protected readonly Guid ActivityId;

        protected BaseService()
        {
            ActivityId = Guid.NewGuid();
        }

        /// <summary>
        /// Log a diagnostic message.
        /// Tests: Protected method visibility
        /// </summary>
        protected virtual void LogDiagnostic(string message)
        {
            Console.WriteLine($"[{ActivityId}] {message}");
        }

        /// <summary>
        /// Abstract method to be implemented by derived classes.
        /// </summary>
        public abstract void Execute();
    }

    /// <summary>
    /// Concrete service inheriting from BaseService.
    /// Tests: Inheritance, abstract method implementation
    /// </summary>
    public class ConcreteService : BaseService
    {
        /// <inheritdoc />
        public override void Execute()
        {
            // CALLS: BaseService.LogDiagnostic
            LogDiagnostic("Executing concrete service");
        }
    }

    // === TEST SCENARIO 3: Properties and Fields ===

    /// <summary>
    /// Configuration options class.
    /// Tests: Property extraction, auto-properties, fields
    /// </summary>
    public class ProcessingOptions
    {
        /// <summary>
        /// Gets or sets the timeout in seconds.
        /// </summary>
        public int TimeoutSeconds { get; set; }

        /// <summary>
        /// Gets or sets whether to enable compression.
        /// </summary>
        public bool EnableCompression { get; set; }

        /// <summary>
        /// Gets the configuration name (read-only).
        /// </summary>
        public string Name { get; }

        private readonly DateTime _createdAt;

        /// <summary>
        /// Initializes a new instance with default values.
        /// </summary>
        public ProcessingOptions()
        {
            TimeoutSeconds = 30;
            EnableCompression = false;
            Name = "Default";
            _createdAt = DateTime.UtcNow;
        }

        /// <summary>
        /// Apply options to transform data.
        /// Tests: Method called from other class
        /// </summary>
        public string Apply(string data)
        {
            return EnableCompression ? Compress(data) : data;
        }

        private string Compress(string data)
        {
            return $"compressed[{data}]";
        }
    }

    // === TEST SCENARIO 4: Enums and Constants ===

    /// <summary>
    /// Processing status enumeration.
    /// Tests: Enum extraction, enum member detection
    /// </summary>
    public enum ProcessingStatus
    {
        /// <summary>
        /// Processing is pending.
        /// </summary>
        Pending = 0,

        /// <summary>
        /// Processing is in progress.
        /// </summary>
        InProgress = 1,

        /// <summary>
        /// Processing completed successfully.
        /// </summary>
        Completed = 2,

        /// <summary>
        /// Processing failed with errors.
        /// </summary>
        Failed = 3
    }

    /// <summary>
    /// Constants for configuration values.
    /// Tests: Constant field extraction
    /// </summary>
    public static class Constants
    {
        /// <summary>
        /// Maximum allowed data size in bytes.
        /// </summary>
        public const int MaxDataSize = 1048576; // 1MB

        /// <summary>
        /// Default processing timeout.
        /// </summary>
        public const int DefaultTimeout = 30;

        /// <summary>
        /// Application name constant.
        /// </summary>
        public const string ApplicationName = "Codanna C# Test Suite";
    }

    // === TEST SCENARIO 5: Interface Dependencies (Multiple Interfaces) ===

    /// <summary>
    /// Logger interface for diagnostic tracing.
    /// Tests: Simple interface, method signatures
    /// </summary>
    public interface ILogger
    {
        /// <summary>
        /// Log an informational message.
        /// </summary>
        void LogInfo(string message);

        /// <summary>
        /// Log an error message.
        /// </summary>
        void LogError(string message);
    }

    /// <summary>
    /// Validator interface for data validation.
    /// Tests: Simple interface detection
    /// </summary>
    public interface IValidator
    {
        /// <summary>
        /// Validate the provided data.
        /// </summary>
        bool Validate(string data);
    }

    // === TEST SCENARIO 6: Generic Classes ===

    /// <summary>
    /// Generic result wrapper class.
    /// Tests: Generic type parameter extraction
    /// </summary>
    /// <typeparam name="T">The type of the result value</typeparam>
    public class Result<T>
    {
        /// <summary>
        /// Gets the result value.
        /// </summary>
        public T Value { get; }

        /// <summary>
        /// Gets whether the operation was successful.
        /// </summary>
        public bool IsSuccess { get; }

        /// <summary>
        /// Gets the error message if operation failed.
        /// </summary>
        public string ErrorMessage { get; }

        private Result(T value, bool isSuccess, string errorMessage)
        {
            Value = value;
            IsSuccess = isSuccess;
            ErrorMessage = errorMessage;
        }

        /// <summary>
        /// Create a successful result.
        /// </summary>
        public static Result<T> Success(T value)
        {
            return new Result<T>(value, true, null);
        }

        /// <summary>
        /// Create a failed result.
        /// </summary>
        public static Result<T> Failure(string errorMessage)
        {
            return new Result<T>(default(T), false, errorMessage);
        }
    }

    // === TEST SCENARIO 7: Async Methods ===

    /// <summary>
    /// Async operations handler.
    /// Tests: Async method detection, Task return types
    /// </summary>
    public class AsyncHandler
    {
        /// <summary>
        /// Process data asynchronously.
        /// Tests: Async keyword, Task return type
        /// </summary>
        public async Task<string> ProcessAsync(string data)
        {
            await Task.Delay(100);
            return data.ToUpper();
        }

        /// <summary>
        /// Process with void return (fire and forget).
        /// </summary>
        public async Task ProcessFireAndForgetAsync(string data)
        {
            await Task.Delay(50);
            Console.WriteLine(data);
        }
    }

    // === TEST SCENARIO 8: Nested Classes ===

    /// <summary>
    /// Container class with nested types.
    /// Tests: Nested class extraction
    /// </summary>
    public class Container
    {
        /// <summary>
        /// Nested configuration class.
        /// Tests: Nested class visibility
        /// </summary>
        public class NestedConfig
        {
            public string Setting { get; set; }
        }

        /// <summary>
        /// Private nested helper class.
        /// </summary>
        private class NestedHelper
        {
            public void Help() { }
        }
    }

    // === TEST SCENARIO 9: Extension Methods ===

    /// <summary>
    /// Extension methods for string operations.
    /// Tests: Static class, extension method detection
    /// </summary>
    public static class StringExtensions
    {
        /// <summary>
        /// Reverse the string.
        /// Tests: Extension method with 'this' parameter
        /// </summary>
        public static string Reverse(this string input)
        {
            if (string.IsNullOrEmpty(input))
                return input;

            char[] chars = input.ToCharArray();
            Array.Reverse(chars);
            return new string(chars);
        }

        /// <summary>
        /// Check if string is palindrome.
        /// </summary>
        public static bool IsPalindrome(this string input)
        {
            // CALLS: StringExtensions.Reverse
            return input == Reverse(input);
        }
    }

    // === TEST SCENARIO 10: Delegates and Events ===

    /// <summary>
    /// Data processing event arguments.
    /// </summary>
    public class DataProcessedEventArgs : EventArgs
    {
        public string Data { get; set; }
        public DateTime ProcessedAt { get; set; }
    }

    /// <summary>
    /// Class with events.
    /// Tests: Event declaration, delegate types
    /// </summary>
    public class EventPublisher
    {
        /// <summary>
        /// Event raised when data is processed.
        /// </summary>
        public event EventHandler<DataProcessedEventArgs> DataProcessed;

        /// <summary>
        /// Trigger the data processed event.
        /// </summary>
        protected virtual void OnDataProcessed(string data)
        {
            DataProcessed?.Invoke(this, new DataProcessedEventArgs
            {
                Data = data,
                ProcessedAt = DateTime.UtcNow
            });
        }
    }
}
