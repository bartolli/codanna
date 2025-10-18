//! Test suite for enhanced C# call tracking
//!
//! This test suite validates the improvements to C# method call tracking,
//! focusing on patterns commonly found in real-world C# codebases like Codere.Sports.

#[cfg(test)]
mod tests {
    use codanna::parsing::LanguageParser;
    use codanna::parsing::csharp::parser::CSharpParser;

    /// Test member access method calls like _logger.Information()
    #[test]
    fn test_member_access_calls() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class Service {
            private ILogger _logger;

            public void Execute() {
                _logger.Information("Starting");
                _logger.Warning("Warning message");
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find Execute -> Information
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "Execute" && *callee == "Information" }),
            "Should detect Execute -> Information. Found: {calls:?}"
        );

        // Should find Execute -> Warning
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "Execute" && *callee == "Warning" }),
            "Should detect Execute -> Warning. Found: {calls:?}"
        );
    }

    /// Test async/await method calls
    #[test]
    fn test_async_await_calls() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class ApiClient {
            private HttpClient _httpClient;

            public async Task<string> FetchData() {
                var result = await _httpClient.GetStringAsync("url");
                return result;
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find FetchData -> GetStringAsync (even with await)
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "FetchData" && *callee == "GetStringAsync"
            }),
            "Should detect async call through await. Found: {calls:?}"
        );
    }

    /// Test constructor invocations with 'new' keyword
    #[test]
    fn test_constructor_calls() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class Factory {
            public IService CreateService() {
                var service = new ServiceImplementation();
                return service;
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should track constructor calls
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "CreateService" && *callee == "ServiceImplementation"
            }),
            "Should detect constructor call. Found: {calls:?}"
        );
    }

    /// Test static method calls
    #[test]
    fn test_static_method_calls() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class PolicyBuilder {
            public void Configure() {
                var policy = Policy.Handle<Exception>();
                var retry = Policy.Timeout(TimeSpan.FromSeconds(5));
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find Configure -> Handle (with or without generic type parameters)
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "Configure" && callee.starts_with("Handle")
            }),
            "Should detect static method call Policy.Handle. Found: {calls:?}"
        );

        // Should find Configure -> Timeout
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "Configure" && *callee == "Timeout" }),
            "Should detect static method call Policy.Timeout. Found: {calls:?}"
        );
    }

    /// Test method chaining (LINQ-style)
    #[test]
    fn test_method_chaining() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class DataProcessor {
            public List<int> ProcessNumbers(List<int> numbers) {
                return numbers
                    .Where(x => x > 0)
                    .Select(x => x * 2)
                    .ToList();
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find ProcessNumbers -> Where
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "ProcessNumbers" && *callee == "Where" }),
            "Should detect LINQ Where call. Found: {calls:?}"
        );

        // Should find ProcessNumbers -> Select
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "ProcessNumbers" && *callee == "Select" }),
            "Should detect LINQ Select call. Found: {calls:?}"
        );

        // Should find ProcessNumbers -> ToList
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "ProcessNumbers" && *callee == "ToList" }),
            "Should detect LINQ ToList call. Found: {calls:?}"
        );
    }

    /// Test complex real-world pattern from Codere.Sports
    #[test]
    fn test_codere_sports_pattern() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class HGProxy : IHGProxy {
            private readonly IAppLogger _logger;
            private readonly HttpClient _httpClient;

            public async Task<HGSchedule> GetHGScheduleAsync(string channel, int? afterId = null) {
                var url = afterId == null ?
                    $"schedule/{channel}" :
                    $"schedule/{channel}?afterId={afterId}";

                return await _httpClient.GetFromJsonAsync<HGSchedule>(url, _logger);
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find GetHGScheduleAsync -> GetFromJsonAsync (with or without generic type parameters)
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "GetHGScheduleAsync" && callee.starts_with("GetFromJsonAsync")
            }),
            "Should detect async generic method call. Found: {calls:?}"
        );
    }

    /// Test Polly resilience policy pattern
    #[test]
    fn test_polly_policy_pattern() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class ResilienceService {
            public void ConfigurePolicy() {
                var retryPolicy = Policy
                    .Handle<Exception>()
                    .WaitAndRetryAsync(3);

                var timeoutPolicy = Policy.TimeoutAsync(TimeSpan.FromSeconds(10));

                var circuitBreaker = Policy
                    .Handle<Exception>()
                    .CircuitBreakerAsync(5, TimeSpan.FromSeconds(30));
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find ConfigurePolicy -> Handle (multiple times, with or without generic type parameters)
        let handle_calls: Vec<_> = calls
            .iter()
            .filter(|(caller, callee, _)| {
                *caller == "ConfigurePolicy" && callee.starts_with("Handle")
            })
            .collect();
        assert!(
            handle_calls.len() >= 2,
            "Should detect multiple Policy.Handle calls. Found: {} calls",
            handle_calls.len()
        );

        // Should find ConfigurePolicy -> WaitAndRetryAsync
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "ConfigurePolicy" && *callee == "WaitAndRetryAsync"
            }),
            "Should detect Policy.WaitAndRetryAsync. Found: {calls:?}"
        );
    }

    /// Test extension method calls
    #[test]
    fn test_extension_method_calls() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class ServiceRegistration {
            public void ConfigureServices(IServiceCollection services) {
                services.AddSingleton<ILogger, ConsoleLogger>();
                services.AddScoped<IRepository, SqlRepository>();
                services.AddTransient<IValidator, Validator>();
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find ConfigureServices -> AddSingleton (with or without generic type parameters)
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "ConfigureServices" && callee.starts_with("AddSingleton")
            }),
            "Should detect extension method AddSingleton. Found: {calls:?}"
        );

        // Should find ConfigureServices -> AddScoped (with or without generic type parameters)
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "ConfigureServices" && callee.starts_with("AddScoped")
            }),
            "Should detect extension method AddScoped. Found: {calls:?}"
        );
    }

    /// Test nested member access (chained properties)
    #[test]
    fn test_nested_member_access() {
        let mut parser = CSharpParser::new().unwrap();
        let code = r#"
        public class ConfigService {
            public void Initialize() {
                var value = this.Configuration.GetValue<string>("key");
                this.Logger.Context.LogInfo("message");
            }
        }
    "#;

        let calls = parser.find_calls(code);

        // Should find Initialize -> GetValue (with or without generic type parameters)
        assert!(
            calls.iter().any(|(caller, callee, _)| {
                *caller == "Initialize" && callee.starts_with("GetValue")
            }),
            "Should detect nested member access call. Found: {calls:?}"
        );

        // Should find Initialize -> LogInfo
        assert!(
            calls
                .iter()
                .any(|(caller, callee, _)| { *caller == "Initialize" && *callee == "LogInfo" }),
            "Should detect chained property method call. Found: {calls:?}"
        );
    }
}
