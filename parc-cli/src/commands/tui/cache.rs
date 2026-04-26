use std::collections::{HashMap, VecDeque};
use std::path::Path;

use anyhow::Result;
use parc_core::fragment::{read_fragment, Fragment};

pub(super) struct FragmentCache {
    entries: HashMap<String, Fragment>,
    order: VecDeque<String>,
    cap: usize,
}

impl FragmentCache {
    pub(super) fn new(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            cap,
        }
    }

    pub(super) fn get_or_load(&mut self, vault: &Path, id: &str) -> Result<&Fragment> {
        if !self.entries.contains_key(id) {
            let frag = read_fragment(vault, id)?;
            if self.entries.len() >= self.cap {
                if let Some(oldest) = self.order.pop_front() {
                    self.entries.remove(&oldest);
                }
            }
            self.order.push_back(id.to_string());
            self.entries.insert(id.to_string(), frag);
        }
        Ok(self.entries.get(id).unwrap())
    }

    pub(super) fn invalidate(&mut self, id: &str) {
        if self.entries.remove(id).is_some() {
            self.order.retain(|x| x != id);
        }
    }
}
