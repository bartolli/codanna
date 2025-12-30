//! Benchmark command - parser performance testing.

use std::path::PathBuf;
use std::time::Instant;

use crate::display::tables::create_benchmark_table;
use crate::display::theme::Theme;
use crate::parsing::{
    CSharpParser, GoParser, LanguageParser, PhpParser, PythonParser, RustParser, TypeScriptParser,
};
use crate::types::{FileId, SymbolCounter};
use console::style;

/// Run parser performance benchmarks
pub fn run(language: &str, custom_file: Option<PathBuf>) {
    // Print styled header
    if Theme::should_disable_colors() {
        println!("\n=== Codanna Parser Benchmarks ===\n");
    } else {
        println!(
            "\n{}\n",
            style("=== Codanna Parser Benchmarks ===").cyan().bold()
        );
    }

    match language.to_lowercase().as_str() {
        "rust" => benchmark_rust_parser(custom_file),
        "python" => benchmark_python_parser(custom_file),
        "php" => benchmark_php_parser(custom_file),
        "typescript" | "ts" => benchmark_typescript_parser(custom_file),
        "go" => benchmark_go_parser(custom_file),
        "csharp" | "c#" | "cs" => benchmark_csharp_parser(custom_file),
        "all" => {
            benchmark_rust_parser(None);
            println!();
            benchmark_python_parser(None);
            println!();
            benchmark_php_parser(None);
            println!();
            benchmark_typescript_parser(None);
            println!();
            benchmark_go_parser(None);
            println!();
            benchmark_csharp_parser(None);
        }
        _ => {
            eprintln!("Unknown language: {language}");
            eprintln!("Available languages: rust, python, php, typescript, go, csharp, all");
            std::process::exit(1);
        }
    }

    // Print target info with styling
    if Theme::should_disable_colors() {
        println!("\nTarget: >10,000 symbols/second");
    } else {
        println!(
            "\n{}: {}",
            style("Target").dim(),
            style(">10,000 symbols/second").dim()
        );
    }
}

fn benchmark_rust_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        (generate_rust_benchmark_code(), None)
    };

    let mut parser = RustParser::new().expect("Failed to create Rust parser");
    benchmark_parser("Rust", &mut parser, &code, file_path);
}

fn benchmark_python_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else if std::path::Path::new("tests/python_comprehensive_features.py").exists() {
        match std::fs::read_to_string("tests/python_comprehensive_features.py") {
            Ok(content) => (content, None),
            Err(e) => {
                eprintln!("Warning: Failed to read test file: {e}");
                eprintln!("Generating benchmark code instead...");
                (generate_python_benchmark_code(), None)
            }
        }
    } else {
        (generate_python_benchmark_code(), None)
    };

    let mut parser = PythonParser::new().expect("Failed to create Python parser");
    benchmark_parser("Python", &mut parser, &code, file_path);
}

fn benchmark_php_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        (generate_php_benchmark_code(), None)
    };

    let mut parser = PhpParser::new().expect("Failed to create PHP parser");
    benchmark_parser("PHP", &mut parser, &code, file_path);
}

fn benchmark_typescript_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        (generate_typescript_benchmark_code(), None)
    };

    let mut parser = TypeScriptParser::new().expect("Failed to create TypeScript parser");
    benchmark_parser("TypeScript", &mut parser, &code, file_path);
}

fn benchmark_go_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        (generate_go_benchmark_code(), None)
    };

    let mut parser = GoParser::new().expect("Failed to create Go parser");
    benchmark_parser("Go", &mut parser, &code, file_path);
}

fn benchmark_csharp_parser(custom_file: Option<PathBuf>) {
    let (code, file_path) = if let Some(path) = custom_file {
        let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Failed to read {}: {e}", path.display());
            std::process::exit(1);
        });
        (content, Some(path))
    } else {
        (generate_csharp_benchmark_code(), None)
    };

    let mut parser = CSharpParser::new().expect("Failed to create C# parser");
    benchmark_parser("C#", &mut parser, &code, file_path);
}

fn benchmark_parser(
    language: &str,
    parser: &mut dyn LanguageParser,
    code: &str,
    file_path: Option<PathBuf>,
) {
    let file_id = FileId::new(1).expect("Failed to create file ID");
    let mut counter = SymbolCounter::new();

    // Warm up
    let _ = parser.parse(code, file_id, &mut counter);

    // Measure parsing performance (average of 3 runs)
    let mut total_duration = std::time::Duration::ZERO;
    let mut symbols_count = 0;

    for _ in 0..3 {
        counter = SymbolCounter::new();
        let start = Instant::now();
        let symbols = parser.parse(code, file_id, &mut counter);
        total_duration += start.elapsed();
        symbols_count = symbols.len();
    }

    let avg_duration = total_duration / 3;
    let rate = symbols_count as f64 / avg_duration.as_secs_f64();

    let table = create_benchmark_table(
        language,
        file_path
            .as_ref()
            .map(|p| p.to_str().unwrap_or("<invalid path>")),
        symbols_count,
        avg_duration,
        rate,
    );

    println!("\n{table}");

    // Verify zero-cost abstractions (silently)
    let calls = parser.find_calls(code);
    if !calls.is_empty() {
        let (caller, _callee, _) = &calls[0];
        let caller_ptr = caller.as_ptr();
        let code_ptr = code.as_ptr();
        let within_bounds =
            caller_ptr >= code_ptr && caller_ptr < unsafe { code_ptr.add(code.len()) };

        if !within_bounds {
            println!("\nWarning: String allocation detected!");
        }
    }
}

fn generate_rust_benchmark_code() -> String {
    let mut code = String::from("//! Rust benchmark file\n\n");

    // Generate 500 functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"/// Function {i} documentation
fn function_{i}(param1: i32, param2: &str) -> bool {{
    let result = param1 * 2;
    result > 0
}}

"#
        ));
    }

    // Generate 50 structs with methods
    for i in 0..50 {
        code.push_str(&format!(
            r#"/// Struct {i} documentation
struct Struct{i} {{
    value: i32,
}}

impl Struct{i} {{
    fn new(value: i32) -> Self {{
        Self {{ value }}
    }}

    fn method_a(&self) -> i32 {{
        self.value * 2
    }}
}}

"#
        ));
    }

    code
}

fn generate_python_benchmark_code() -> String {
    let mut code = String::from("\"\"\"Python benchmark file\"\"\"\n\n");

    // Generate 500 functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"def function_{i}(param1: int, param2: str = 'default') -> bool:
    """Function {i} documentation."""
    result = param1 * 2
    return result > 0

"#
        ));
    }

    // Generate 50 classes
    for i in 0..50 {
        code.push_str(&format!(
            r#"class Class_{i}:
    """Class {i} documentation."""

    def __init__(self, value: int):
        self.value = value

    def method_a(self) -> int:
        return self.value * 2

"#
        ));
    }

    code
}

fn generate_php_benchmark_code() -> String {
    let mut code = String::from("<?php\n/**\n * PHP benchmark file\n */\n\n");

    // Generate 500 functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"/**
 * Function {i} documentation
 */
function function_{i}(int $param1, string $param2 = 'default'): bool {{
    $result = $param1 * 2;
    return $result > 0;
}}

"#
        ));
    }

    // Generate 50 classes with methods
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Class {i} documentation
 */
class Class_{i} {{
    private int $value;

    public function __construct(int $value) {{
        $this->value = $value;
    }}

    public function methodA(): int {{
        return $this->value * 2;
    }}

    public function methodB(string $param): string {{
        return strtoupper($param);
    }}
}}

"#
        ));
    }

    // Generate 25 interfaces
    for i in 0..25 {
        code.push_str(&format!(
            r#"interface Interface_{i} {{
    public function method_{i}(): void;
}}

"#
        ));
    }

    // Generate 25 traits
    for i in 0..25 {
        code.push_str(&format!(
            r#"trait Trait_{i} {{
    public function traitMethod_{i}(): string {{
        return 'trait_{i}';
    }}
}}

"#
        ));
    }

    code.push_str("?>");
    code
}

fn generate_typescript_benchmark_code() -> String {
    let mut code = String::from("// TypeScript benchmark file\n\n");

    // Generate 500 functions with various TypeScript features
    for i in 0..500 {
        code.push_str(&format!(
            r#"/**
 * Function {i} documentation
 * @param param1 The first parameter
 * @param param2 The second parameter
 * @returns A boolean result
 */
export function function_{i}(param1: number, param2: string = 'default'): boolean {{
    const result = param1 > 0 && param2.length > 0;
    return result;
}}

"#
        ));
    }

    // Generate 50 interfaces
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Interface {i} for data structure
 */
export interface Interface_{i} {{
    id: number;
    name: string;
    optional?: boolean;
    readonly immutable: string;
    method(param: string): void;
}}

"#
        ));
    }

    // Generate 50 classes with methods
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Class {i} implementation
 */
export class Class_{i} implements Interface_{i} {{
    public id: number;
    public name: string;
    public optional?: boolean;
    public readonly immutable: string;
    private _internal: number;
    protected _protected: string;

    constructor(id: number, name: string) {{
        this.id = id;
        this.name = name;
        this.immutable = 'fixed';
        this._internal = 0;
        this._protected = 'protected';
    }}

    public method(param: string): void {{
        console.log(param);
    }}

    private privateMethod(): number {{
        return this._internal;
    }}

    protected protectedMethod(): string {{
        return this._protected;
    }}

    static staticMethod(): void {{
        console.log('static');
    }}
}}

"#
        ));
    }

    // Generate 50 type aliases
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Type alias {i}
 */
export type TypeAlias_{i} = string | number | boolean;

type ComplexType_{i} = {{
    field1: TypeAlias_{i};
    field2: Interface_{i};
    field3: (param: string) => void;
}};

"#
        ));
    }

    // Generate 50 enums
    for i in 0..50 {
        code.push_str(&format!(
            r#"/**
 * Enum {i} definition
 */
export enum Enum_{i} {{
    First = 0,
    Second = 1,
    Third = 'three',
    Fourth = 'four'
}}

"#
        ));
    }

    // Add some arrow functions and const declarations
    for i in 0..50 {
        code.push_str(&format!(
            r#"export const arrowFunction_{i} = (x: number, y: number): number => x + y;

export const constant_{i}: string = 'constant value';

let variable_{i}: number = {i};

"#
        ));
    }

    code
}

fn generate_go_benchmark_code() -> String {
    let mut code =
        String::from("// Go benchmark file\n\npackage bench\n\nimport (\n\t\"fmt\"\n)\n\n");

    // Generate 500 free functions
    for i in 0..500 {
        code.push_str(&format!(
            r#"// Function {i} documentation
func Function_{i}(param1 int, param2 string) bool {{
    result := param1 * 2
    return result > 0 && len(param2) > 0
}}

"#
        ));
    }

    // Generate 50 structs with methods and interface satisfaction
    for i in 0..50 {
        code.push_str(&format!(
            r#"// Struct {i} documentation
type Struct{i} struct {{
    Value int
}}

func NewStruct{i}(v int) *Struct{i} {{
    return &Struct{i}{{Value: v}}
}}

func (s *Struct{i}) MethodA() int {{
    return s.Value * 2
}}

func (s *Struct{i}) Do(param string) int {{
    fmt.Println(param)
    return len(param) + s.Value
}}

"#
        ));
    }

    // Generate 25 interfaces
    for i in 0..25 {
        code.push_str(&format!(
            r#"// Interface {i} documentation
type Interface_{i} interface {{
    Do(param string) int
}}

"#
        ));
    }

    // A small main-like entry to keep parser busy with calls/selectors
    code.push_str(
        r#"// Entry point (not used, just for call patterns)
func main() {
    s := NewStruct0(42)
    _ = s.MethodA()
    _ = s.Do("hello")
    ok := Function_0(1, "x")
    if ok {
        fmt.Println("ok")
    }
}
"#,
    );

    code
}

fn generate_csharp_benchmark_code() -> String {
    let mut code = String::from(
        "// C# benchmark file\n\nusing System;\nusing System.Collections.Generic;\nusing System.Linq;\n\nnamespace BenchmarkNamespace\n{\n",
    );

    // Generate 500 static classes with methods
    for i in 0..500 {
        code.push_str(&format!(
            r#"    /// <summary>
    /// Static class {i} documentation
    /// </summary>
    public static class StaticClass{i}
    {{
        public static int Method{i}(int param)
        {{
            return param * 2;
        }}

        public static string Process{i}(string input)
        {{
            return input.ToUpper();
        }}
    }}

"#
        ));
    }

    // Generate 50 classes with properties/fields/constructors
    for i in 0..50 {
        code.push_str(&format!(
            r#"    /// <summary>
    /// Class {i} documentation
    /// </summary>
    public class Class{i}
    {{
        private int _value;

        public int Value {{ get; set; }}
        public string Name {{ get; set; }}

        public Class{i}(int value, string name)
        {{
            _value = value;
            Value = value;
            Name = name;
        }}

        public int Calculate()
        {{
            return _value * 2;
        }}

        public void Process(string input)
        {{
            Console.WriteLine(input);
        }}
    }}

"#
        ));
    }

    // Generate 25 interfaces
    for i in 0..25 {
        code.push_str(&format!(
            r#"    /// <summary>
    /// Interface {i} documentation
    /// </summary>
    public interface IInterface{i}
    {{
        void Process(string input);
        int Calculate();
    }}

"#
        ));
    }

    // Program class with Main method
    code.push_str(
        r#"    /// <summary>
    /// Entry point class
    /// </summary>
    class Program
    {
        static void Main(string[] args)
        {
            var obj = new Class0(42, "test");
            var result = obj.Calculate();
            obj.Process("hello");

            var staticResult = StaticClass0.Method0(10);
            var processed = StaticClass0.Process0("world");

            Console.WriteLine($"Result: {result}, Static: {staticResult}, Processed: {processed}");
        }
    }
}
"#,
    );

    code
}
