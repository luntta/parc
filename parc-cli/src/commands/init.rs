use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

use anyhow::{bail, Result};
use parc_core::config::{render_types, TypesConfig};
use parc_core::schema::BUILTIN_TYPES;
use parc_core::vault;

/// What each built-in type is for, as `parc init` offers it.
fn describe(name: &str) -> &'static str {
    match name {
        "note" => "free-form thoughts and captures",
        "todo" => "tasks with a status, due date and priority",
        "decision" => "choices and the reasons for them",
        "risk" => "what could go wrong, and what is being done about it",
        "idea" => "possibilities to explore later",
        _ => "",
    }
}

pub fn run(global: bool, explicit_vault: Option<&Path>, yes: bool) -> Result<()> {
    let path = if let Some(p) = explicit_vault {
        if p.ends_with(".parc") {
            p.to_path_buf()
        } else {
            p.join(".parc")
        }
    } else if global {
        vault::global_vault_path()?
    } else {
        std::env::current_dir()?.join(".parc")
    };
    if vault::is_vault(&path) {
        return Err(parc_core::ParcError::VaultAlreadyExists(path).into());
    }

    // Like `npm init`: questions on a terminal, defaults with -y or when
    // nobody is there to answer (a script, a pipe, the desktop app).
    let interactive = !yes && io::stdin().is_terminal() && io::stdout().is_terminal();
    let types = if interactive {
        let stdin = io::stdin();
        match ask(&path, &mut stdin.lock(), &mut io::stdout())? {
            Some(types) => types,
            None => {
                println!("Aborted. No vault was created.");
                return Ok(());
            }
        }
    } else {
        vault::default_init_types()
    };

    vault::init_vault_with(&path, &types)?;

    let scope = if global {
        "global"
    } else if explicit_vault.is_some() {
        "new"
    } else {
        "local"
    };
    println!("Initialized {} vault at {}", scope, path.display());

    // Initialize the index
    parc_core::index::init_index(&path)?;

    Ok(())
}

/// Ask which built-in types to turn on and which one captures start as.
/// Returns `None` when the answer to "Is this OK?" is no.
fn ask(path: &Path, input: &mut impl BufRead, out: &mut impl Write) -> Result<Option<TypesConfig>> {
    writeln!(out, "This walks you through creating a parc vault.")?;
    writeln!(
        out,
        "Press Enter to keep the answer in parentheses, or ^C to quit."
    )?;
    writeln!(out)?;
    writeln!(out, "Vault: {}", path.display())?;
    writeln!(out)?;
    writeln!(
        out,
        "Which types should new fragments use? You can turn types on or off later."
    )?;

    let width = BUILTIN_TYPES.iter().map(|t| t.len()).max().unwrap_or(0);
    let enabled = loop {
        let mut enabled = Vec::new();
        for name in BUILTIN_TYPES {
            let question = format!("{:<width$}  {} (Y/n) ", name, describe(name));
            if yes_no(input, out, &question, true)? {
                enabled.push(*name);
            }
        }
        if !enabled.is_empty() {
            break enabled;
        }
        writeln!(out, "Keep at least one type on.")?;
    };

    let default = if enabled.len() == 1 {
        enabled[0]
    } else {
        let suggested = if enabled.contains(&"note") {
            "note"
        } else {
            enabled[0]
        };
        loop {
            let answer = prompt(
                input,
                out,
                &format!("Type new captures start as: ({}) ", suggested),
            )?;
            let answer = answer.trim().to_lowercase();
            if answer.is_empty() {
                break suggested;
            }
            if let Some(name) = enabled.iter().find(|t| **t == answer) {
                break *name;
            }
            writeln!(out, "Choose one of: {}", enabled.join(", "))?;
        }
    };

    let types = TypesConfig {
        default: Some(default.to_string()),
        disabled: BUILTIN_TYPES
            .iter()
            .filter(|t| !enabled.contains(t))
            .map(|t| t.to_string())
            .collect(),
    };

    writeln!(out)?;
    writeln!(
        out,
        "About to write to {}:",
        path.join("config.yml").display()
    )?;
    writeln!(out)?;
    write!(out, "{}", render_types(&types)?)?;
    writeln!(out)?;
    if yes_no(input, out, "Is this OK? (yes) ", true)? {
        Ok(Some(types))
    } else {
        Ok(None)
    }
}

fn prompt(input: &mut impl BufRead, out: &mut impl Write, question: &str) -> Result<String> {
    write!(out, "{}", question)?;
    out.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        writeln!(out)?;
        bail!("aborted: no answer");
    }
    Ok(line)
}

fn yes_no(
    input: &mut impl BufRead,
    out: &mut impl Write,
    question: &str,
    default: bool,
) -> Result<bool> {
    loop {
        match prompt(input, out, question)?.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(out, "Answer yes or no.")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answers(text: &str) -> (Result<Option<TypesConfig>>, String) {
        let mut out = Vec::new();
        let result = ask(
            Path::new("/tmp/x/.parc"),
            &mut io::Cursor::new(text.as_bytes()),
            &mut out,
        );
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn enter_everywhere_takes_the_defaults() {
        let (types, out) = answers("\n\n\n\n\n\n\n");
        assert_eq!(types.unwrap().unwrap(), vault::default_init_types());
        assert!(out.contains("note      free-form thoughts and captures (Y/n) "));
        assert!(out.contains("Type new captures start as: (note) "));
    }

    #[test]
    fn turned_off_types_and_a_chosen_default() {
        // note, todo, decision, risk, idea; default; confirm.
        let (types, out) = answers("n\n\nno\nN\nyes\nidea\ny\n");
        let types = types.unwrap().unwrap();
        assert_eq!(types.default.as_deref(), Some("idea"));
        assert_eq!(types.disabled, vec!["note", "decision", "risk"]);
        assert!(out.contains("Type new captures start as: (todo) "));
        assert!(out.contains("disabled:\n"));
    }

    #[test]
    fn unclear_answers_are_asked_again() {
        // A bad yes/no, then a default that is off, then a good one.
        let (types, out) = answers("maybe\n\n\n\nn\nn\nrisk\ntodo\n\n");
        let types = types.unwrap().unwrap();
        assert_eq!(types.default.as_deref(), Some("todo"));
        assert_eq!(types.disabled, vec!["risk", "idea"]);
        assert!(out.contains("Answer yes or no."));
        assert!(out.contains("Choose one of: note, todo, decision"));
    }

    #[test]
    fn every_type_off_starts_over_and_one_type_needs_no_default_question() {
        let (types, out) = answers("n\nn\nn\nn\nn\nn\ny\nn\nn\nn\n\n");
        let types = types.unwrap().unwrap();
        assert_eq!(types.default.as_deref(), Some("todo"));
        assert!(out.contains("Keep at least one type on."));
        assert!(!out.contains("start as"));
    }

    #[test]
    fn no_at_the_end_and_end_of_input_create_nothing() {
        let (types, _) = answers("\n\n\n\n\n\nno\n");
        assert!(types.unwrap().is_none());
        let (types, _) = answers("\n\n");
        assert!(types.is_err());
    }
}
