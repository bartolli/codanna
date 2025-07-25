use std::process::Command;
use tempfile::TempDir;

#[test]
fn test_init_command() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();
    
    // Change to temp directory
    std::env::set_current_dir(temp_path).unwrap();
    
    // Run init command
    let output = Command::new(env!("CARGO_BIN_EXE_codanna"))
        .arg("init")
        .output()
        .expect("Failed to run init command");
    
    assert!(output.status.success());
    
    // Check that config file was created
    let config_path = temp_path.join(".codanna/settings.toml");
    assert!(config_path.exists());
    
    // Verify config content
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("version = 1"));
    assert!(content.contains("[indexing]"));
    assert!(content.contains("[languages.rust]"));
}

#[test]
fn test_config_command() {
    let temp_dir = TempDir::new().unwrap();
    let temp_path = temp_dir.path();
    
    // Create a custom config
    let config_dir = temp_path.join(".codanna");
    std::fs::create_dir_all(&config_dir).unwrap();
    
    let config_content = r#"
version = 2
[indexing]
parallel_threads = 99
"#;
    
    std::fs::write(config_dir.join("settings.toml"), config_content).unwrap();
    
    // Change to temp directory
    std::env::set_current_dir(temp_path).unwrap();
    
    // Run config command
    let output = Command::new(env!("CARGO_BIN_EXE_codanna"))
        .arg("config")
        .output()
        .expect("Failed to run config command");
    
    assert!(output.status.success());
    
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("version = 2"));
    assert!(stdout.contains("parallel_threads = 99"));
}