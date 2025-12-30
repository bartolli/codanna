//! Init and Config commands.

use std::path::PathBuf;

use crate::config::Settings;

/// Run init command - create configuration file.
pub fn run_init(force: bool) {
    let config_path = PathBuf::from(".codanna/settings.toml");

    if config_path.exists() && !force {
        eprintln!(
            "Configuration file already exists at: {}",
            config_path.display()
        );
        eprintln!("Use --force to overwrite");
        std::process::exit(1);
    }

    match Settings::init_config_file(force) {
        Ok(path) => {
            println!("Created configuration file at: {}", path.display());
            println!("Edit this file to customize your settings.");
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

/// Run config command - display current configuration.
pub fn run_config(config: &Settings) {
    println!("Current Configuration:");
    println!("{}", "=".repeat(50));
    match toml::to_string_pretty(config) {
        Ok(toml_str) => println!("{toml_str}"),
        Err(e) => eprintln!("Error displaying config: {e}"),
    }
}
