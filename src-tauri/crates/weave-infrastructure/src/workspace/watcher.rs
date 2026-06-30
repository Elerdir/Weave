/// File system watcher — sleduje změny ve workspace složce a triggeruje re-index.
/// Produkčně: `notify` crate s debounce. Zde stub pro scaffold — rozšíříme v dalším PR.
pub struct WorkspaceWatcher {
    root: String,
}

impl WorkspaceWatcher {
    pub fn new(root: impl Into<String>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &str {
        &self.root
    }
}
