use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub vault_path: Mutex<PathBuf>,
}

impl AppState {
    pub fn new(vault_path: PathBuf) -> Self {
        Self {
            vault_path: Mutex::new(vault_path),
        }
    }

    pub fn vault_path(&self) -> PathBuf {
        self.vault_path.lock().unwrap().clone()
    }

    pub fn set_vault_path(&self, path: PathBuf) {
        *self.vault_path.lock().unwrap() = path;
    }
}
