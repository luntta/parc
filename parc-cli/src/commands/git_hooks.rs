use std::path::Path;

use anyhow::{bail, Result};

pub fn run_install(vault: &Path) -> Result<()> {
    // Walk up from vault to find .git directory
    let mut dir = vault.to_path_buf();
    loop {
        let git_dir = dir.join(".git");
        if git_dir.is_dir() {
            let hooks_dir = git_dir.join("hooks");
            std::fs::create_dir_all(&hooks_dir)?;

            let hook_path = hooks_dir.join("post-merge");
            let parc_line = "parc reindex";

            if hook_path.exists() {
                let content = std::fs::read_to_string(&hook_path)?;
                if content.contains(parc_line) {
                    println!("post-merge hook already contains 'parc reindex'.");
                    return Ok(());
                }
                // Append to existing hook
                let mut new_content = content;
                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                new_content.push_str(parc_line);
                new_content.push('\n');
                std::fs::write(&hook_path, new_content)?;
            } else {
                // Write new hook
                let content = format!("#!/bin/sh\n{}\n", parc_line);
                std::fs::write(&hook_path, content)?;
            }

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&hook_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&hook_path, perms)?;
            }

            println!("Installed post-merge hook at {}", hook_path.display());
            return Ok(());
        }

        if !dir.pop() {
            break;
        }
    }

    bail!("no .git directory found (walk up from vault)");
}
