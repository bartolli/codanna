//! Profile management command.

use crate::profiles;
use crate::profiles::commands::{ProfileAction, ProviderAction};

/// Run profile management command.
pub fn run(action: ProfileAction) {
    let result = match action {
        ProfileAction::Init {
            profile_name,
            source,
            force,
        } => profiles::init_profile(&profile_name, source.as_deref(), force),
        ProfileAction::Install {
            profile_name,
            source,
            r#ref,
            force,
        } => {
            // Check if --source or --ref flags are provided
            if source.is_some() || r#ref.is_some() {
                // Legacy direct installation from git source (not yet implemented)
                eprintln!("Direct git source installation not yet implemented");
                eprintln!("Use provider-based installation instead:");
                eprintln!("  1. codanna profile provider add <source>");
                eprintln!("  2. codanna profile install {profile_name}");
                Err(profiles::error::ProfileError::InvalidManifest {
                    reason: "Git source installation not yet implemented. Use provider registry."
                        .to_string(),
                })
            } else {
                // Use registry-based installation (supports profile@provider syntax)
                profiles::install_profile_from_registry(&profile_name, force)
            }
        }
        ProfileAction::List { verbose, json } => profiles::list_profiles(verbose, json),
        ProfileAction::Status { verbose } => profiles::show_status(verbose),
        ProfileAction::Sync { force } => profiles::sync_team_config(force),
        ProfileAction::Update {
            profile_name,
            force,
        } => profiles::update_profile(&profile_name, force),
        ProfileAction::Provider { action } => match action {
            ProviderAction::Add { source, id } => profiles::add_provider(&source, id.as_deref()),
            ProviderAction::Remove { provider_id } => profiles::remove_provider(&provider_id),
            ProviderAction::List { verbose } => profiles::list_providers(verbose),
        },
        ProfileAction::Remove {
            profile_name,
            verbose,
        } => profiles::remove_profile(&profile_name, verbose),
        ProfileAction::Verify {
            profile_name,
            all,
            verbose,
        } => {
            if all {
                profiles::verify_all_profiles(verbose)
            } else if let Some(name) = profile_name {
                profiles::verify_profile(&name, verbose)
            } else {
                eprintln!("Error: Must provide profile name or use --all");
                Err(profiles::error::ProfileError::InvalidManifest {
                    reason: "Must provide profile name or use --all".to_string(),
                })
            }
        }
    };

    if let Err(e) = result {
        use crate::io::exit_code::ExitCode;
        let code: ExitCode = e.exit_code();
        eprintln!("Profile operation failed: {e}");
        std::process::exit(i32::from(code));
    }
}
