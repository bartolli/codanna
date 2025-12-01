//! Debug helper to dump relationship extraction results
//! Run with: cargo test debug_dump_swift --test parsers_tests -- --nocapture --ignored

use codanna::parsing::LanguageParser;
use codanna::parsing::swift::SwiftParser;
use std::fs;
use std::path::Path;

#[test]
#[ignore]
fn debug_dump_swift_extends() {
    let path = Path::new("examples/swift/comprehensive.swift");
    let code = fs::read_to_string(path).expect("Failed to read file");
    let mut parser = SwiftParser::new().expect("Failed to create parser");

    let extends = parser.find_extends(&code);

    println!("\n=== EXTENDS ({} total) ===", extends.len());
    for (derived, base, range) in &extends {
        println!("  {} : {} (line {})", derived, base, range.start_line + 1);
    }
}

#[test]
#[ignore]
fn debug_dump_swift_defines() {
    let path = Path::new("examples/swift/comprehensive.swift");
    let code = fs::read_to_string(path).expect("Failed to read file");
    let mut parser = SwiftParser::new().expect("Failed to create parser");

    let defines = parser.find_defines(&code);

    println!("\n=== DEFINES ({} total) ===", defines.len());
    for (type_name, method, range) in &defines {
        println!("  {}.{} (line {})", type_name, method, range.start_line + 1);
    }
}

#[test]
#[ignore]
fn debug_dump_swift_uses() {
    let path = Path::new("examples/swift/comprehensive.swift");
    let code = fs::read_to_string(path).expect("Failed to read file");
    let mut parser = SwiftParser::new().expect("Failed to create parser");

    let uses = parser.find_uses(&code);

    println!("\n=== USES ({} total) ===", uses.len());
    for (context, type_name, range) in &uses {
        println!(
            "  {} uses {} (line {})",
            context,
            type_name,
            range.start_line + 1
        );
    }
}

#[test]
#[ignore]
fn debug_dump_swift_calls() {
    let path = Path::new("examples/swift/comprehensive.swift");
    let code = fs::read_to_string(path).expect("Failed to read file");
    let mut parser = SwiftParser::new().expect("Failed to create parser");

    let calls = parser.find_calls(&code);

    println!("\n=== CALLS ({} total) ===", calls.len());
    for (caller, callee, range) in &calls {
        println!("  {} -> {} (line {})", caller, callee, range.start_line + 1);
    }
}
