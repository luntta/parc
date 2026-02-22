mod commands;
mod render;

use clap::Parser;

#[derive(Parser)]
#[command(name = "parc", about = "Personal Archive — structured fragments of thought")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Initialize a new vault
    Init {
        /// Create the global vault (~/.parc) instead of a local one
        #[arg(long)]
        global: bool,
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
    },
    /// Set a metadata field on a fragment
    Set {
        /// Fragment ID or prefix
        id: String,
        /// Field name
        field: String,
        /// New value
        value: String,
    },
    /// Search fragments
    Search {
        /// Search query (full-text)
        query: Vec<String>,
        /// Filter by type
        #[arg(long = "type")]
        type_filter: Option<String>,
        /// Filter by status
        #[arg(long)]
        status: Option<String>,
        /// Filter by tag
        #[arg(long)]
        tag: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Sort order (updated, created, updated-asc, created-asc)
        #[arg(long)]
        sort: Option<String>,
        /// Limit results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Delete a fragment (move to trash)
    Delete {
        /// Fragment ID or prefix
        id: String,
    },
    /// Create a bidirectional link between two fragments
    Link {
        /// First fragment ID or prefix
        id_a: String,
        /// Second fragment ID or prefix
        id_b: String,
    },
    /// Remove a bidirectional link between two fragments
    Unlink {
        /// First fragment ID or prefix
        id_a: String,
        /// Second fragment ID or prefix
        id_b: String,
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
    /// Rebuild the search index from fragment files
    Reindex,
    /// List registered fragment types
    Types,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { global } => commands::init::run(global),
        Commands::New {
            type_name,
            title,
            title_flag,
            tag,
            link,
            due,
            priority,
            status,
            assignee,
        } => commands::new::run(
            &type_name,
            title.or(title_flag),
            tag,
            link,
            due,
            priority,
            status,
            assignee,
        ),
        Commands::List {
            type_name,
            status,
            tag,
            json,
            limit,
        } => commands::list::run(type_name, status, tag, json, limit),
        Commands::Show { id, json } => commands::show::run(&id, json),
        Commands::Edit { id } => commands::edit::run(&id),
        Commands::Set { id, field, value } => commands::set::run(&id, &field, &value),
        Commands::Search {
            query,
            type_filter,
            status,
            tag,
            json,
            sort,
            limit,
        } => commands::search::run(query, type_filter, status, tag, json, sort, limit),
        Commands::Delete { id } => commands::delete::run(&id),
        Commands::Link { id_a, id_b } => commands::link::run(&id_a, &id_b),
        Commands::Unlink { id_a, id_b } => commands::unlink::run(&id_a, &id_b),
        Commands::Backlinks { id, json } => commands::backlinks::run(&id, json),
        Commands::Doctor { json } => commands::doctor::run(json),
        Commands::Reindex => commands::reindex::run(),
        Commands::Types => commands::types::run(),
    }
}
