//! Swift relationship extraction tests
//!
//! Tests find_extends, find_uses, find_defines, find_calls using examples/swift/comprehensive.swift

use codanna::parsing::LanguageParser;
use codanna::parsing::swift::SwiftParser;
use std::fs;
use std::path::Path;

/// Load the comprehensive Swift example file
fn load_comprehensive_swift() -> String {
    let path = Path::new("examples/swift/comprehensive.swift");
    fs::read_to_string(path).expect("Failed to read examples/swift/comprehensive.swift")
}

#[test]
fn test_find_extends_class_inheritance() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let extends = parser.find_extends(&code);

    // Dog extends Animal
    let dog_extends = extends
        .iter()
        .find(|(derived, base, _)| *derived == "Dog" && *base == "Animal");
    assert!(
        dog_extends.is_some(),
        "Should find Dog extends Animal, got: {extends:?}"
    );

    // Cat extends Animal
    let cat_extends = extends
        .iter()
        .find(|(derived, base, _)| *derived == "Cat" && *base == "Animal");
    assert!(cat_extends.is_some(), "Should find Cat extends Animal");
}

#[test]
fn test_find_extends_protocol_conformance() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let extends = parser.find_extends(&code);

    // Animal conforms to Named
    let animal_named = extends
        .iter()
        .find(|(derived, base, _)| *derived == "Animal" && *base == "Named");
    assert!(animal_named.is_some(), "Should find Animal: Named");

    // Rectangle conforms to Drawable
    let rect_drawable = extends
        .iter()
        .find(|(derived, base, _)| *derived == "Rectangle" && *base == "Drawable");
    assert!(rect_drawable.is_some(), "Should find Rectangle: Drawable");
}

#[test]
fn test_find_extends_enum_raw_type() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let extends = parser.find_extends(&code);

    // NetworkError: Error
    let network_error = extends
        .iter()
        .find(|(derived, base, _)| *derived == "NetworkError" && *base == "Error");
    assert!(
        network_error.is_some(),
        "Should find NetworkError: Error, got: {:?}",
        extends
            .iter()
            .filter(|(d, _, _)| *d == "NetworkError")
            .collect::<Vec<_>>()
    );

    // Planet: Int
    let planet_int = extends
        .iter()
        .find(|(derived, base, _)| *derived == "Planet" && *base == "Int");
    assert!(planet_int.is_some(), "Should find Planet: Int");
}

#[test]
fn test_find_defines_class_methods() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let defines = parser.find_defines(&code);

    // Animal.makeSound
    let animal_make_sound = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Animal" && *method == "makeSound");
    assert!(
        animal_make_sound.is_some(),
        "Should find Animal.makeSound, got: {:?}",
        defines
            .iter()
            .filter(|(t, _, _)| *t == "Animal")
            .collect::<Vec<_>>()
    );

    // Dog.fetch
    let dog_fetch = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Dog" && *method == "fetch");
    assert!(dog_fetch.is_some(), "Should find Dog.fetch");

    // Point.distance
    let point_distance = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Point" && *method == "distance");
    assert!(point_distance.is_some(), "Should find Point.distance");
}

#[test]
fn test_find_defines_init_deinit() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let defines = parser.find_defines(&code);

    // Animal.init
    let animal_init = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Animal" && *method == "init");
    assert!(animal_init.is_some(), "Should find Animal.init");

    // Animal.deinit
    let animal_deinit = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Animal" && *method == "deinit");
    assert!(animal_deinit.is_some(), "Should find Animal.deinit");
}

#[test]
fn test_find_defines_protocol_methods() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let defines = parser.find_defines(&code);

    // Drawable.draw (protocol method)
    let drawable_draw = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Drawable" && *method == "draw");
    assert!(
        drawable_draw.is_some(),
        "Should find Drawable.draw protocol method"
    );
}

#[test]
fn test_find_defines_subscript() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let defines = parser.find_defines(&code);

    // Matrix.subscript
    let matrix_subscript = defines
        .iter()
        .find(|(type_name, method, _)| *type_name == "Matrix" && *method == "subscript");
    assert!(matrix_subscript.is_some(), "Should find Matrix.subscript");
}

#[test]
fn test_find_uses_filters_builtins() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let uses = parser.find_uses(&code);

    // Should NOT find Int, String, Bool, Double as they are builtins
    let has_int = uses.iter().any(|(_, type_name, _)| *type_name == "Int");
    let has_string = uses.iter().any(|(_, type_name, _)| *type_name == "String");
    let has_bool = uses.iter().any(|(_, type_name, _)| *type_name == "Bool");

    assert!(!has_int, "Should filter out Int builtin type");
    assert!(!has_string, "Should filter out String builtin type");
    assert!(!has_bool, "Should filter out Bool builtin type");
}

#[test]
fn test_find_uses_custom_types() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let uses = parser.find_uses(&code);

    // Rectangle uses Point
    let rect_uses_point = uses
        .iter()
        .find(|(context, type_name, _)| *context == "Rectangle" && *type_name == "Point");
    assert!(
        rect_uses_point.is_some(),
        "Should find Rectangle uses Point, got uses in Rectangle: {:?}",
        uses.iter()
            .filter(|(c, _, _)| *c == "Rectangle")
            .collect::<Vec<_>>()
    );

    // Rectangle uses Size
    let rect_uses_size = uses
        .iter()
        .find(|(context, type_name, _)| *context == "Rectangle" && *type_name == "Size");
    assert!(rect_uses_size.is_some(), "Should find Rectangle uses Size");
}

#[test]
fn test_find_calls() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let calls = parser.find_calls(&code);

    // Should find some calls
    assert!(!calls.is_empty(), "Should find some function calls");

    // processData calls fetchData
    let process_calls_fetch = calls
        .iter()
        .find(|(caller, callee, _)| *caller == "processData" && *callee == "fetchData");
    assert!(
        process_calls_fetch.is_some(),
        "Should find processData calls fetchData, got calls from processData: {:?}",
        calls
            .iter()
            .filter(|(c, _, _)| *c == "processData")
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_relationship_counts() {
    let code = load_comprehensive_swift();
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");

    let extends = parser.find_extends(&code);
    let defines = parser.find_defines(&code);
    let uses = parser.find_uses(&code);
    let calls = parser.find_calls(&code);

    println!("Relationship counts:");
    println!("  extends: {}", extends.len());
    println!("  defines: {}", defines.len());
    println!("  uses: {}", uses.len());
    println!("  calls: {}", calls.len());

    // Sanity checks - should find reasonable numbers
    assert!(
        extends.len() >= 5,
        "Should find at least 5 extends relationships"
    );
    assert!(
        defines.len() >= 20,
        "Should find at least 20 method definitions"
    );
}

#[test]
fn test_typealias_does_not_emit_extends() {
    // Test that typealias doesn't incorrectly emit extends relationships
    // This mirrors the Kingfisher ImageResource pattern that caused SKIP-INCOMPATIBLE
    let code = r#"
@available(*, deprecated)
public typealias ImageResource = KF.ImageResource

enum KF {
    public struct ImageResource: Resource {
        public var cacheKey: String
    }
}

protocol Resource {
    var cacheKey: String { get }
}
"#;
    let mut parser = SwiftParser::new().expect("Failed to create Swift parser");
    let extends = parser.find_extends(code);

    // Should find KF.ImageResource extends Resource (from the struct)
    // Should NOT find ImageResource (typealias) extends anything
    println!("Extends found: {extends:?}");

    // The struct ImageResource should extend Resource
    let struct_extends = extends
        .iter()
        .find(|(derived, base, _)| *derived == "ImageResource" && *base == "Resource");
    assert!(
        struct_extends.is_some(),
        "Should find struct ImageResource extends Resource"
    );

    // There should be exactly one extends for ImageResource (the struct, not typealias)
    let image_resource_extends: Vec<_> = extends
        .iter()
        .filter(|(derived, _, _)| *derived == "ImageResource")
        .collect();
    assert_eq!(
        image_resource_extends.len(),
        1,
        "Should have exactly one extends for ImageResource (struct only, not typealias), got: {image_resource_extends:?}"
    );
}
