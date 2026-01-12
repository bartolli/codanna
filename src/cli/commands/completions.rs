//! Shell completion installation command.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use clap::CommandFactory;
use clap_complete::{Shell as ClapShell, generate_to};

use crate::cli::args::{Cli, CompletionShell};

/// Install shell completions for the selected shell (or current shell if omitted).
pub fn run_install_completion(shell: Option<CompletionShell>) {
    let shell = shell.or_else(detect_shell).unwrap_or_else(|| {
        eprintln!(
            "Could not detect your shell. Specify one of: bash, zsh, fish, powershell, elvish."
        );
        std::process::exit(1);
    });

    let completion_dir = match completion_dir(shell) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!(
                "Failed to determine completion directory for {}: {e}",
                shell.as_str()
            );
            std::process::exit(1);
        }
    };

    let mut cmd = Cli::command();
    let bin_name = cmd.get_name().to_string();
    let out_path = match generate_to(ClapShell::from(shell), &mut cmd, bin_name, &completion_dir) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Failed to write completion script: {e}");
            std::process::exit(1);
        }
    };

    println!(
        "Installed {} completions to {}",
        shell.as_str(),
        out_path.display()
    );

    if let Some(hint) = completion_hint(shell, &out_path) {
        println!("{hint}");
    }
}

fn completion_dir(shell: CompletionShell) -> io::Result<PathBuf> {
    let candidates = match shell {
        CompletionShell::Bash => bash_candidates(),
        CompletionShell::Zsh => zsh_candidates(),
        CompletionShell::Fish => fish_candidates(),
        CompletionShell::PowerShell => powershell_candidates(),
        CompletionShell::Elvish => elvish_candidates(),
    };

    first_creatable_dir(candidates)
}

fn first_creatable_dir(candidates: Vec<PathBuf>) -> io::Result<PathBuf> {
    let mut last_err: Option<io::Error> = None;
    let mut seen = std::collections::HashSet::new();

    for dir in candidates {
        if dir.as_os_str().is_empty() || !seen.insert(dir.clone()) {
            continue;
        }

        match fs::create_dir_all(&dir) {
            Ok(()) => {
                let probe_path = dir.join(format!(".codanna-completion-{}", std::process::id()));
                match fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&probe_path)
                {
                    Ok(file) => {
                        drop(file);
                        let _ = fs::remove_file(&probe_path);
                        return Ok(dir);
                    }
                    Err(e) => last_err = Some(e),
                }
            }
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "No suitable completion directory candidates found",
        )
    }))
}

fn bash_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = env::var_os("BASH_COMPLETION_USER_DIR") {
        candidates.push(PathBuf::from(dir));
    }

    if let Some(dir) = dirs::data_dir() {
        candidates.push(dir.join("bash-completion").join("completions"));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".bash_completion.d"));
    }

    candidates
}

fn zsh_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(fpath) = env::var_os("FPATH") {
        candidates.extend(env::split_paths(&fpath));
    }

    if let Some(zdotdir) = env::var_os("ZDOTDIR") {
        candidates.push(PathBuf::from(zdotdir).join("completions"));
    }

    if let Some(config) = dirs::config_dir() {
        candidates.push(config.join("zsh").join("completions"));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".zsh").join("completions"));
    }

    if let Some(dir) = dirs::data_dir() {
        candidates.push(dir.join("zsh").join("site-functions"));
    }

    candidates
}

fn fish_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = dirs::config_dir() {
        candidates.push(dir.join("fish").join("completions"));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("fish").join("completions"));
    }

    candidates
}

fn powershell_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(base) = powershell_base_dir() {
        candidates.push(base.join("completions"));
        candidates.push(base.join("Completions"));
    }

    if let Some(dir) = dirs::config_dir() {
        candidates.push(dir.join("powershell").join("completions"));
    }

    candidates
}

fn powershell_base_dir() -> Option<PathBuf> {
    if let Some(ps_module_path) = env::var_os("PSModulePath") {
        for path in env::split_paths(&ps_module_path) {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case("Modules"))
                .unwrap_or(false)
            {
                if let Some(parent) = path.parent() {
                    return Some(parent.to_path_buf());
                }
            }
        }
    }

    if cfg!(windows) {
        if let Some(documents) = dirs::document_dir() {
            let powershell_dir = documents.join("PowerShell");
            if powershell_dir.exists() {
                return Some(powershell_dir);
            }
            let windows_powershell_dir = documents.join("WindowsPowerShell");
            if windows_powershell_dir.exists() {
                return Some(windows_powershell_dir);
            }
        }
    }

    dirs::config_dir().map(|dir| dir.join("powershell"))
}

fn elvish_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(dir) = dirs::config_dir() {
        candidates.push(dir.join("elvish").join("lib"));
    }

    if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".elvish").join("lib"));
    }

    candidates
}

fn completion_hint(shell: CompletionShell, out_path: &Path) -> Option<String> {
    let dir = out_path.parent()?;
    let restart_hint = "Restart your shell to enable completions.".to_string();

    match shell {
        CompletionShell::Zsh => {
            if !fpath_contains(dir) {
                return Some(format!(
                    "Add this to your zshrc to enable completions:\n  fpath=({} $fpath)\n  autoload -Uz compinit && compinit",
                    dir.display()
                ));
            }
            Some(restart_hint)
        }
        CompletionShell::PowerShell => Some(format!(
            "Add this line to your PowerShell profile:\n  . '{}'",
            out_path.display()
        )),
        CompletionShell::Elvish => Some(format!(
            "Load this in elvish with:\n  use {}",
            out_path.display()
        )),
        CompletionShell::Bash | CompletionShell::Fish => Some(restart_hint),
    }
}

fn fpath_contains(dir: &Path) -> bool {
    let Some(fpath) = env::var_os("FPATH") else {
        return false;
    };

    env::split_paths(&fpath).any(|path| path == dir)
}

fn detect_shell() -> Option<CompletionShell> {
    if env::var_os("ZSH_VERSION").is_some() {
        return Some(CompletionShell::Zsh);
    }
    if env::var_os("BASH_VERSION").is_some() {
        return Some(CompletionShell::Bash);
    }
    if env::var_os("FISH_VERSION").is_some() {
        return Some(CompletionShell::Fish);
    }
    if env::var_os("ELVISH_VERSION").is_some() {
        return Some(CompletionShell::Elvish);
    }
    if env::var_os("PSModulePath").is_some()
        || env::var_os("POWERSHELL_DISTRIBUTION_CHANNEL").is_some()
    {
        return Some(CompletionShell::PowerShell);
    }

    if let Some(shell) = env::var_os("SHELL") {
        if let Some(parsed) = shell_from_path(&shell) {
            return Some(parsed);
        }
    }

    if let Some(shell) = env::var_os("COMSPEC") {
        if let Some(parsed) = shell_from_path(&shell) {
            return Some(parsed);
        }
    }

    None
}

fn shell_from_path(shell: &std::ffi::OsStr) -> Option<CompletionShell> {
    let name = Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match name.as_str() {
        "bash" | "bash.exe" => Some(CompletionShell::Bash),
        "zsh" | "zsh.exe" => Some(CompletionShell::Zsh),
        "fish" | "fish.exe" => Some(CompletionShell::Fish),
        "pwsh" | "pwsh.exe" | "powershell" | "powershell.exe" => Some(CompletionShell::PowerShell),
        "elvish" | "elvish.exe" => Some(CompletionShell::Elvish),
        _ => None,
    }
}

impl CompletionShell {
    fn as_str(&self) -> &'static str {
        match self {
            CompletionShell::Bash => "bash",
            CompletionShell::Zsh => "zsh",
            CompletionShell::Fish => "fish",
            CompletionShell::PowerShell => "powershell",
            CompletionShell::Elvish => "elvish",
        }
    }
}

impl From<CompletionShell> for ClapShell {
    fn from(shell: CompletionShell) -> Self {
        match shell {
            CompletionShell::Bash => ClapShell::Bash,
            CompletionShell::Zsh => ClapShell::Zsh,
            CompletionShell::Fish => ClapShell::Fish,
            CompletionShell::PowerShell => ClapShell::PowerShell,
            CompletionShell::Elvish => ClapShell::Elvish,
        }
    }
}
