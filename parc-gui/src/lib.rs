pub mod commands;
pub mod dto;
pub mod error;
pub mod state;

use state::AppState;

pub fn run() {
    let vault_path = parc_core::vault::resolve_vault(None)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            std::path::PathBuf::from(home).join(".parc")
        });

    tauri::Builder::default()
        .manage(AppState::new(vault_path))
        .invoke_handler(tauri::generate_handler![
            commands::fragment::list_fragments,
            commands::fragment::get_fragment,
            commands::fragment::create_fragment,
            commands::fragment::update_fragment,
            commands::fragment::delete_fragment,
            commands::fragment::archive_fragment,
            commands::search::search_fragments,
            commands::vault::vault_info,
            commands::vault::reindex,
            commands::vault::doctor,
            commands::vault::switch_vault,
            commands::vault::list_vaults,
            commands::vault::init_vault,
            commands::schema::list_schemas,
            commands::schema::get_schema,
            commands::tag::list_tags,
            commands::link::link_fragments,
            commands::link::unlink_fragments,
            commands::link::get_backlinks,
            commands::attachment::attach_file,
            commands::attachment::detach_file,
            commands::attachment::list_attachments,
            commands::attachment::get_attachment_path,
            commands::history::list_versions,
            commands::history::get_version,
            commands::history::restore_version,
            commands::history::diff_versions,
            commands::markdown::render_markdown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
