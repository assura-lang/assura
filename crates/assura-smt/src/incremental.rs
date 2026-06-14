// ===========================================================================
// T115: Incremental compilation
// ===========================================================================

#[derive(Debug, Clone)]
pub struct IncrementalCompiler {
    modules: std::collections::HashMap<String, ModuleState>,
    dependencies: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ModuleState {
    pub name: String,
    pub hash: String,
    pub last_checked: u64,
    pub dirty: bool,
}

impl IncrementalCompiler {
    pub fn new() -> Self {
        Self {
            modules: std::collections::HashMap::new(),
            dependencies: Vec::new(),
        }
    }

    pub fn register_module(&mut self, name: String, hash: String) {
        self.modules.insert(
            name.clone(),
            ModuleState {
                name,
                hash,
                last_checked: 0,
                dirty: true,
            },
        );
    }

    pub fn add_dependency(&mut self, from: String, to: String) {
        self.dependencies.push((from, to));
    }

    pub fn mark_changed(&mut self, name: &str) {
        if let Some(m) = self.modules.get_mut(name) {
            m.dirty = true;
        }
        let dependents: Vec<_> = self
            .dependencies
            .iter()
            .filter(|(_, to)| to == name)
            .map(|(from, _)| from.clone())
            .collect();
        for dep in dependents {
            if let Some(m) = self.modules.get_mut(&dep) {
                m.dirty = true;
            }
        }
    }

    pub fn mark_checked(&mut self, name: &str, timestamp: u64) {
        if let Some(m) = self.modules.get_mut(name) {
            m.dirty = false;
            m.last_checked = timestamp;
        }
    }

    pub fn dirty_modules(&self) -> Vec<&str> {
        self.modules
            .values()
            .filter(|m| m.dirty)
            .map(|m| m.name.as_str())
            .collect()
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
}

impl Default for IncrementalCompiler {
    fn default() -> Self {
        Self::new()
    }
}
