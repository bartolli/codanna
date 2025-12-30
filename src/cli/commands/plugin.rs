//! Plugin management command.

use crate::cli::PluginAction;
use crate::config::Settings;
use crate::plugins;

/// Run plugin management command.
pub fn run(action: PluginAction, config: &Settings) {
    let result = match action {
        PluginAction::Add {
            marketplace,
            plugin_name,
            r#ref,
            force,
            dry_run,
        } => plugins::add_plugin(
            config,
            &marketplace,
            &plugin_name,
            r#ref.as_deref(),
            force,
            dry_run,
        ),
        PluginAction::Remove {
            plugin_name,
            force,
            dry_run,
        } => plugins::remove_plugin(config, &plugin_name, force, dry_run),
        PluginAction::Update {
            plugin_name,
            r#ref,
            force,
            dry_run,
        } => plugins::update_plugin(config, &plugin_name, r#ref.as_deref(), force, dry_run),
        PluginAction::List { verbose, json } => plugins::list_plugins(config, verbose, json),
        PluginAction::Verify {
            plugin_name,
            all,
            verbose,
        } => {
            if all {
                plugins::verify_all_plugins(config, verbose)
            } else {
                match plugin_name {
                    Some(name) => plugins::verify_plugin(config, &name, verbose),
                    None => {
                        eprintln!("Error: plugin_name is required when --all is not specified");
                        std::process::exit(1);
                    }
                }
            }
        }
    };

    if let Err(e) = result {
        use crate::io::exit_code::ExitCode;
        let code: ExitCode = e.exit_code();
        eprintln!("Plugin operation failed: {e}");
        std::process::exit(i32::from(code));
    }
}
