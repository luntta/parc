use super::{QuickField, Tab};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandAction {
    Edit,
    ToggleStatus,
    Archive,
    Delete,
    Promote,
    YankId,
    SetField(QuickField),
    Capture,
    Reload,
    Help,
    SwitchTab(Tab),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LauncherKind {
    Fragments,
    Commands,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CommandEntry {
    pub label: &'static str,
    pub description: &'static str,
    pub key: &'static str,
    pub aliases: &'static [&'static str],
    pub requires_selection: bool,
    pub action: CommandAction,
}

impl CommandEntry {
    fn matches(self, query: &str) -> bool {
        let terms = query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        if terms.is_empty() {
            return true;
        }

        let haystack = format!(
            "{} {} {} {}",
            self.label,
            self.description,
            self.key,
            self.aliases.join(" ")
        )
        .to_lowercase();

        terms.iter().all(|term| haystack.contains(term))
    }
}

pub(crate) fn launcher_kind(input: &str) -> LauncherKind {
    if input.starts_with('>') {
        LauncherKind::Commands
    } else {
        LauncherKind::Fragments
    }
}

pub(crate) fn command_query(input: &str) -> &str {
    input.strip_prefix('>').unwrap_or(input).trim()
}

pub(crate) fn matching_commands(input: &str, has_selection: bool) -> Vec<CommandEntry> {
    let query = command_query(input);
    available_commands(has_selection)
        .into_iter()
        .filter(|command| command.matches(query))
        .collect()
}

pub(crate) fn available_commands(has_selection: bool) -> Vec<CommandEntry> {
    all_commands()
        .into_iter()
        .filter(|command| !command.requires_selection || has_selection)
        .collect()
}

fn all_commands() -> Vec<CommandEntry> {
    vec![
        CommandEntry {
            label: "Edit Fragment",
            description: "Open the selected fragment in $EDITOR.",
            key: "e",
            aliases: &["modify", "open editor"],
            requires_selection: true,
            action: CommandAction::Edit,
        },
        CommandEntry {
            label: "Toggle Status",
            description: "Toggle the selected todo between open and done.",
            key: "t",
            aliases: &["todo", "done", "open", "complete"],
            requires_selection: true,
            action: CommandAction::ToggleStatus,
        },
        CommandEntry {
            label: "Archive Fragment",
            description: "Toggle archived state for the selected fragment.",
            key: "a",
            aliases: &["hide", "unarchive"],
            requires_selection: true,
            action: CommandAction::Archive,
        },
        CommandEntry {
            label: "Delete Fragment",
            description: "Open a confirmation prompt before moving the selected fragment to trash.",
            key: "d",
            aliases: &["remove", "trash"],
            requires_selection: true,
            action: CommandAction::Delete,
        },
        CommandEntry {
            label: "Promote Fragment",
            description: "Prompt for a target type and promote the selected fragment.",
            key: "p",
            aliases: &["type", "convert"],
            requires_selection: true,
            action: CommandAction::Promote,
        },
        CommandEntry {
            label: "Yank ID",
            description: "Copy the selected fragment ID to the system clipboard.",
            key: "y",
            aliases: &["copy id", "clipboard"],
            requires_selection: true,
            action: CommandAction::YankId,
        },
        CommandEntry {
            label: "Set Status",
            description: "Prompt for the selected fragment status field.",
            key: "s",
            aliases: &["field", "todo state"],
            requires_selection: true,
            action: CommandAction::SetField(QuickField::Status),
        },
        CommandEntry {
            label: "Set Due Date",
            description: "Prompt for the selected fragment due date.",
            key: "D",
            aliases: &["deadline", "date", "field"],
            requires_selection: true,
            action: CommandAction::SetField(QuickField::Due),
        },
        CommandEntry {
            label: "Set Priority",
            description: "Prompt for the selected fragment priority.",
            key: "P",
            aliases: &["importance", "field"],
            requires_selection: true,
            action: CommandAction::SetField(QuickField::Priority),
        },
        CommandEntry {
            label: "Set Assignee",
            description: "Prompt for the selected fragment assignee.",
            key: "@",
            aliases: &["owner", "person", "field"],
            requires_selection: true,
            action: CommandAction::SetField(QuickField::Assignee),
        },
        CommandEntry {
            label: "Set Tags",
            description: "Prompt for the selected fragment tag list.",
            key: "#",
            aliases: &["labels", "metadata", "field"],
            requires_selection: true,
            action: CommandAction::SetField(QuickField::Tags),
        },
        CommandEntry {
            label: "Capture Fragment",
            description: "Open the capture form for a new fragment.",
            key: "c",
            aliases: &["new", "create", "add"],
            requires_selection: false,
            action: CommandAction::Capture,
        },
        CommandEntry {
            label: "Reload Current View",
            description: "Reload the active tab from disk.",
            key: "r",
            aliases: &["refresh"],
            requires_selection: false,
            action: CommandAction::Reload,
        },
        CommandEntry {
            label: "Help",
            description: "Open the TUI help overlay.",
            key: "?",
            aliases: &["keybindings", "shortcuts"],
            requires_selection: false,
            action: CommandAction::Help,
        },
        CommandEntry {
            label: "Switch to Today",
            description: "Show today's touched, due, and high priority fragments.",
            key: "1",
            aliases: &["tab", "home"],
            requires_selection: false,
            action: CommandAction::SwitchTab(Tab::Today),
        },
        CommandEntry {
            label: "Switch to List",
            description: "Show recent fragments, newest first.",
            key: "2",
            aliases: &["tab", "recent"],
            requires_selection: false,
            action: CommandAction::SwitchTab(Tab::List),
        },
        CommandEntry {
            label: "Switch to Stale",
            description: "Show stale fragments that may need attention.",
            key: "3",
            aliases: &["tab", "review"],
            requires_selection: false,
            action: CommandAction::SwitchTab(Tab::Stale),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_fragment_commands_are_hidden_without_selection() {
        let labels = available_commands(false)
            .into_iter()
            .map(|command| command.label)
            .collect::<Vec<_>>();

        assert!(!labels.contains(&"Edit Fragment"));
        assert!(!labels.contains(&"Delete Fragment"));
        assert!(labels.contains(&"Capture Fragment"));
        assert!(labels.contains(&"Reload Current View"));
    }

    #[test]
    fn prefixed_input_filters_commands() {
        let labels = matching_commands("> deadline", true)
            .into_iter()
            .map(|command| command.label)
            .collect::<Vec<_>>();

        assert_eq!(labels, vec!["Set Due Date"]);
    }

    #[test]
    fn plain_input_is_fragment_search() {
        assert_eq!(launcher_kind("due"), LauncherKind::Fragments);
        assert_eq!(launcher_kind(">due"), LauncherKind::Commands);
    }
}
