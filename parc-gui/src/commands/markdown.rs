use comrak::{markdown_to_html, Options};
use tauri::State;

use crate::error::GuiError;
use crate::state::AppState;

#[tauri::command]
pub fn render_markdown(
    _state: State<'_, AppState>,
    content: String,
) -> Result<String, GuiError> {
    let mut options = Options::default();
    options.extension.strikethrough = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.unsafe_ = true;

    let html = markdown_to_html(&content, &options);
    Ok(html)
}
