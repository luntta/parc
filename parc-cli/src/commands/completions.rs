use anyhow::Result;
use clap::CommandFactory;
use clap_complete::{generate, Shell};

pub fn run(shell_name: &str) -> Result<()> {
    let shell: Shell = shell_name.parse().map_err(|_| {
        anyhow::anyhow!(
            "unsupported shell '{}': use bash, zsh, fish, or elvish",
            shell_name
        )
    })?;

    let mut cmd = crate::Cli::command();
    generate(shell, &mut cmd, "parc", &mut std::io::stdout());
    Ok(())
}
