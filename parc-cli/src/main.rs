mod commands;
pub mod hooks;
mod render;

use std::io::{self, IsTerminal};
use std::path::PathBuf;

use clap::Parser;
use parc_core::schema::load_schemas;
use parc_core::vault::resolve_vault;

#[derive(Parser)]
#[command(name = "parc", about = "Personal Archive — structured fragments of thought")]
#[command(allow_external_subcommands = true)]
struct Cli {
    /// Path to vault (overrides PARC_VAULT and vault discovery)
    #[arg(global = true, long)]
    vault: Option<PathBuf>,

    /// Disable the TUI for bare `parc`
    #[arg(global = true, long)]
    no_tui: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Initialize a new vault
    Init {
        /// Create the global vault (~/.parc) instead of a local one
        #[arg(long)]
        global: bool,
    },
    /// Quickly capture a note
    #[command(name = "+")]
    Capture {
        /// Text to capture. Reads stdin when omitted.
        #[arg(value_name = "TEXT")]
        text: Vec<String>,
        /// Add tags
        #[arg(long)]
        tag: Vec<String>,
        /// Link to other fragments
        #[arg(long)]
        link: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new fragment
    New {
        /// Fragment type (or alias)
        type_name: String,
        /// Title (positional, takes precedence over --title)
        title: Option<String>,
        /// Title (flag alternative)
        #[arg(long = "title", name = "title_flag")]
        title_flag: Option<String>,
        /// Body text (skips $EDITOR)
        #[arg(long)]
        body: Option<String>,
        /// Add tags
        #[arg(long)]
        tag: Vec<String>,
        /// Link to other fragments
        #[arg(long)]
        link: Vec<String>,
        /// Due date (YYYY-MM-DD)
        #[arg(long)]
        due: Option<String>,
        /// Priority level
        #[arg(long)]
        priority: Option<String>,
        /// Status
        #[arg(long)]
        status: Option<String>,
        /// Assignee
        #[arg(long)]
        assignee: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Convert a fragment to another type
    Promote {
        /// Fragment ID or prefix
        id: String,
        /// New fragment type (or alias)
        new_type: String,
        /// Add tags
        #[arg(long)]
        tag: Vec<String>,
        /// Link to other fragments
        #[arg(long)]
        link: Vec<String>,
        /// Due date (YYYY-MM-DD)
        #[arg(long)]
        due: Option<String>,
        /// Priority level
        #[arg(long)]
        priority: Option<String>,
        /// Status
        #[arg(long)]
        status: Option<String>,
        /// Assignee
        #[arg(long)]
        assignee: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List fragments
    List {
        /// Filter by type
        type_name: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by tag (AND semantics)
        #[arg(long)]
        tag: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Limit results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show a fragment
    Show {
        /// Fragment ID or prefix
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Edit a fragment in $EDITOR
    Edit {
        /// Fragment ID or prefix
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Set a metadata field on a fragment
    Set {
        /// Fragment ID or prefix
        id: String,
        /// Field name
        field: String,
        /// New value
        value: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Fuzzy-search fragments (supports DSL: type:todo status:open #tag "phrase")
    Search {
        /// Search query: bare words are fuzzy-matched against title+body;
        /// "quoted" phrases must appear as substrings; structured filters
        /// (type:, status:, priority:, tag:/#, due:, created:, updated:,
        /// by:, has:, linked:, is:) compose with the fuzzy match.
        query: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Sort order (score [default], updated, created, updated-asc, created-asc, random)
        #[arg(long)]
        sort: Option<String>,
        /// Limit results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show today's resurfacing digest
    Today {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Open the terminal UI
    Tui,
    /// Show due and overdue todos
    Due {
        /// Bucket: today, this-week, or overdue
        bucket: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show open work that has gone quiet
    Stale {
        /// Days since last update
        #[arg(long)]
        days: Option<u64>,
        /// Fragment types to include, comma-separated
        #[arg(long = "types", value_delimiter = ',')]
        types: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Limit results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show random resurfaced fragments
    Random {
        /// Limit results
        #[arg(long)]
        limit: Option<usize>,
        /// Fragment type to include
        #[arg(long = "type")]
        type_name: Option<String>,
        /// Include completed or closed fragments when a type is specified
        #[arg(long)]
        include_done: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a multi-section review digest
    Review {
        /// Review window (default from config)
        #[arg(long)]
        since: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a fragment (move to trash)
    Delete {
        /// Fragment ID or prefix
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a bidirectional link between two fragments
    Link {
        /// First fragment ID or prefix
        id_a: String,
        /// Second fragment ID or prefix
        id_b: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a bidirectional link between two fragments
    Unlink {
        /// First fragment ID or prefix
        id_a: String,
        /// Second fragment ID or prefix
        id_b: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List all fragments linking to a given fragment
    Backlinks {
        /// Fragment ID or prefix
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check vault health (broken links, orphans, schema violations)
    Doctor {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show fragment version history
    History {
        /// Fragment ID or prefix
        id: String,
        /// Show a specific version
        #[arg(long)]
        show: Option<String>,
        /// Diff current vs. previous (or specific) version
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        diff: Option<String>,
        /// Restore a previous version
        #[arg(long)]
        restore: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Attach a file to a fragment
    Attach {
        /// Fragment ID or prefix
        id: String,
        /// Path to file to attach
        file: PathBuf,
        /// Move the file instead of copying
        #[arg(long = "mv")]
        mv: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove an attachment from a fragment
    Detach {
        /// Fragment ID or prefix
        id: String,
        /// Filename of the attachment to remove
        filename: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List attachments for a fragment
    Attachments {
        /// Fragment ID or prefix
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage schemas
    Schema {
        #[command(subcommand)]
        subcommand: SchemaCommands,
    },
    /// Generate shell completions
    Completions {
        /// Shell name (bash, zsh, fish, elvish)
        shell: String,
    },
    /// Rebuild the search index from fragment files
    Reindex {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List registered fragment types
    Types {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show active vault info, or manage vaults
    Vault {
        #[command(subcommand)]
        subcommand: Option<VaultCommands>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List all tags with usage counts
    Tags {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Archive a fragment (exclude from default listing)
    Archive {
        /// Fragment ID or prefix
        id: String,
        /// Unarchive instead
        #[arg(long)]
        undo: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// View and manage trashed fragments
    Trash {
        /// Permanently delete trashed fragment(s)
        #[arg(long)]
        purge: bool,
        /// Fragment ID for --purge or --restore
        id: Option<String>,
        /// Restore a trashed fragment
        #[arg(long)]
        restore: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Export fragments to JSON, CSV, or HTML
    Export {
        /// Output format (json, csv, html)
        #[arg(long, default_value = "json")]
        format: String,
        /// Output file or directory (default: stdout)
        #[arg(long)]
        output: Option<String>,
        /// Optional search query to filter fragments
        query: Vec<String>,
    },
    /// Import fragments from a JSON file
    Import {
        /// Path to JSON file
        file: String,
        /// Validate without writing
        #[arg(long)]
        dry_run: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage git hooks
    GitHooks {
        #[command(subcommand)]
        subcommand: GitHooksCommands,
    },
    /// Start JSON-RPC server
    Server {
        /// Use Unix domain socket instead of stdio
        #[arg(long)]
        socket: bool,
        /// Custom socket path (implies --socket)
        #[arg(long)]
        socket_path: Option<String>,
    },
    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        subcommand: PluginCommands,
    },
    /// External subcommand (dispatched to plugins)
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(clap::Subcommand)]
enum SchemaCommands {
    /// Register a user-defined fragment type from a YAML file
    Add {
        /// Path to schema YAML file
        path: String,
    },
}

#[derive(clap::Subcommand)]
enum VaultCommands {
    /// List all known vaults
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(clap::Subcommand)]
enum GitHooksCommands {
    /// Install post-merge hook for automatic reindex
    Install,
}

#[derive(clap::Subcommand)]
enum PluginCommands {
    /// List installed plugins
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show details about a plugin
    Info {
        /// Plugin name
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Install a plugin from a .wasm file
    Install {
        /// Path to .wasm file
        path: String,
        /// Path to manifest .toml file
        #[arg(long)]
        manifest: Option<String>,
    },
    /// Remove an installed plugin
    Remove {
        /// Plugin name
        name: String,
        /// Skip confirmation
        #[arg(long)]
        force: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let command = cli.command;

    match command {
        None => {
            let vault = resolve_vault(cli.vault.as_deref())?;
            if !cli.no_tui && io::stdout().is_terminal() {
                commands::tui::run(&vault)
            } else {
                commands::today::run(&vault, false)
            }
        }
        Some(Commands::Init { global }) => {
            if global && cli.vault.is_some() {
                anyhow::bail!("--vault and --global are mutually exclusive");
            }
            commands::init::run(global, cli.vault.as_deref())
        }
        Some(Commands::Completions { shell }) => commands::completions::run(&shell),
        Some(Commands::Vault { subcommand, json }) => {
            let vault = resolve_vault(cli.vault.as_deref())?;
            commands::vault::run(&vault, subcommand.map(|s| match s {
                VaultCommands::List { json } => commands::vault::VaultSubcommand::List { json },
            }), json)
        }
        Some(command) => {
            // All other commands: resolve vault once, pass to command
            let vault = resolve_vault(cli.vault.as_deref())?;
            match command {
                Commands::Capture {
                    text,
                    tag,
                    link,
                    json,
                } => commands::capture::run(&vault, text, tag, link, json),
                Commands::New {
                    type_name,
                    title,
                    title_flag,
                    body,
                    tag,
                    link,
                    due,
                    priority,
                    status,
                    assignee,
                    json,
                } => commands::new::run(
                    &vault,
                    &type_name,
                    title.or(title_flag),
                    body,
                    tag,
                    link,
                    due,
                    priority,
                    status,
                    assignee,
                    json,
                ),
                Commands::Promote {
                    id,
                    new_type,
                    tag,
                    link,
                    due,
                    priority,
                    status,
                    assignee,
                    json,
                } => commands::promote::run(
                    &vault,
                    &id,
                    &new_type,
                    tag,
                    link,
                    due,
                    priority,
                    status,
                    assignee,
                    json,
                ),
                Commands::List {
                    type_name,
                    status,
                    tag,
                    json,
                    limit,
                } => commands::list::run(&vault, type_name, status, tag, json, limit),
                Commands::Show { id, json } => commands::show::run(&vault, &id, json),
                Commands::Edit { id, json } => commands::edit::run(&vault, &id, json),
                Commands::Set { id, field, value, json } => commands::set::run(&vault, &id, &field, &value, json),
                Commands::Search {
                    query,
                    json,
                    sort,
                    limit,
                } => commands::search::run(&vault, query, json, sort, limit),
                Commands::Today { json } => commands::today::run(&vault, json),
                Commands::Tui => commands::tui::run(&vault),
                Commands::Due { bucket, json } => commands::due::run(&vault, bucket, json),
                Commands::Stale {
                    days,
                    types,
                    json,
                    limit,
                } => commands::stale::run(&vault, days, types, json, limit),
                Commands::Random {
                    limit,
                    type_name,
                    include_done,
                    json,
                } => commands::random::run(&vault, limit, type_name, include_done, json),
                Commands::Review { since, json } => commands::review::run(&vault, since, json),
                Commands::Delete { id, json } => commands::delete::run(&vault, &id, json),
                Commands::Link { id_a, id_b, json } => commands::link::run(&vault, &id_a, &id_b, json),
                Commands::Unlink { id_a, id_b, json } => commands::unlink::run(&vault, &id_a, &id_b, json),
                Commands::Backlinks { id, json } => commands::backlinks::run(&vault, &id, json),
                Commands::Doctor { json } => commands::doctor::run(&vault, json),
                Commands::History {
                    id,
                    show,
                    diff,
                    restore,
                    json,
                } => {
                    let is_diff = diff.is_some();
                    let diff_ts = diff.filter(|s| !s.is_empty());
                    commands::history::run(&vault, &id, show, is_diff, diff_ts, restore, json)
                }
                Commands::Attach { id, file, mv, json } => {
                    commands::attach::run_attach(&vault, &id, &file, mv, json)
                }
                Commands::Detach { id, filename, json } => {
                    commands::attach::run_detach(&vault, &id, &filename, json)
                }
                Commands::Attachments { id, json } => commands::attach::run_attachments(&vault, &id, json),
                Commands::Schema { subcommand } => match subcommand {
                    SchemaCommands::Add { path } => commands::schema::run_add(&vault, &path),
                },
                Commands::Reindex { json } => commands::reindex::run(&vault, json),
                Commands::Types { json } => commands::types::run(&vault, json),
                Commands::Tags { json } => commands::tags::run(&vault, json),
                Commands::Archive { id, undo, json } => commands::archive::run(&vault, &id, undo, json),
                Commands::Trash { purge, id, restore, json } => {
                    commands::trash::run(&vault, purge, id, restore, json)
                }
                Commands::Export { format, output, query } => {
                    commands::export::run(&vault, &format, output.as_deref(), query)
                }
                Commands::Import { file, dry_run, json } => {
                    commands::import::run(&vault, &file, dry_run, json)
                }
                Commands::GitHooks { subcommand } => match subcommand {
                    GitHooksCommands::Install => commands::git_hooks::run_install(&vault),
                },
                Commands::Server { socket, socket_path } => {
                    commands::server::run(&vault, socket, socket_path)
                }
                Commands::Plugin { subcommand } => match subcommand {
                    PluginCommands::List { json } => commands::plugin::run_list(&vault, json),
                    PluginCommands::Info { name, json } => {
                        commands::plugin::run_info(&vault, &name, json)
                    }
                    PluginCommands::Install { path, manifest } => {
                        commands::plugin::run_install(&vault, &path, manifest.as_deref())
                    }
                    PluginCommands::Remove { name, force } => {
                        commands::plugin::run_remove(&vault, &name, force)
                    }
                },
                Commands::External(args) => {
                    run_external_command(&vault, args)
                }
                Commands::Init { .. } | Commands::Vault { .. } | Commands::Completions { .. } => unreachable!(),
            }
        }
    }
}

/// Clap struct for parsing alias arguments (mirrors `New` variant fields minus `type_name`)
#[derive(Parser, Debug)]
#[command(no_binary_name = true)]
struct AliasNewArgs {
    /// Title (positional)
    title: Option<String>,
    /// Title (flag alternative)
    #[arg(long = "title", name = "title_flag")]
    title_flag: Option<String>,
    /// Body text (skips $EDITOR)
    #[arg(long)]
    body: Option<String>,
    /// Add tags
    #[arg(long)]
    tag: Vec<String>,
    /// Link to other fragments
    #[arg(long)]
    link: Vec<String>,
    /// Due date
    #[arg(long)]
    due: Option<String>,
    /// Priority level
    #[arg(long)]
    priority: Option<String>,
    /// Status
    #[arg(long)]
    status: Option<String>,
    /// Assignee
    #[arg(long)]
    assignee: Option<String>,
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

fn run_external_command(vault: &std::path::Path, args: Vec<String>) -> anyhow::Result<()> {
    if args.is_empty() {
        anyhow::bail!("unknown command");
    }

    let cmd_name = &args[0];

    // Check if the command name is a type alias
    if let Ok(schemas) = load_schemas(vault) {
        if schemas.resolve(cmd_name).is_some() {
            let remaining = &args[1..];
            let alias_args = AliasNewArgs::try_parse_from(remaining)
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            return commands::new::run(
                vault,
                cmd_name,
                alias_args.title.or(alias_args.title_flag),
                alias_args.body,
                alias_args.tag,
                alias_args.link,
                alias_args.due,
                alias_args.priority,
                alias_args.status,
                alias_args.assignee,
                alias_args.json,
            );
        }
    }

    // Try to dispatch to a plugin
    #[cfg(feature = "wasm-plugins")]
    {
        let cmd_args: Vec<String> = args[1..].to_vec();
        let config = parc_core::config::load_config(vault)?;
        let mut manager = parc_core::plugin::manager::PluginManager::load_all(vault, &config)?;
        let commands = manager.list_commands();

        for pc in &commands {
            if pc.command == *cmd_name {
                let output = manager.execute_command(&pc.plugin_name, cmd_name, &cmd_args)?;
                if !output.is_empty() {
                    print!("{}", output);
                }
                return Ok(());
            }
        }
    }

    #[cfg(not(feature = "wasm-plugins"))]
    {
        let _ = vault;
    }

    anyhow::bail!("unknown command: {}", cmd_name)
}
