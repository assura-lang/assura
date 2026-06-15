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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_compiler_has_no_modules() {
        let ic = IncrementalCompiler::new();
        assert_eq!(ic.module_count(), 0);
        assert!(ic.dirty_modules().is_empty());
    }

    #[test]
    fn register_module_increases_count() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("math".into(), "abc123".into());
        assert_eq!(ic.module_count(), 1);
    }

    #[test]
    fn registered_module_starts_dirty() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("math".into(), "abc".into());
        let dirty = ic.dirty_modules();
        assert!(dirty.contains(&"math"), "new module should be dirty");
    }

    #[test]
    fn mark_checked_clears_dirty() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("math".into(), "abc".into());
        ic.mark_checked("math", 100);
        let dirty = ic.dirty_modules();
        assert!(
            !dirty.contains(&"math"),
            "checked module should not be dirty"
        );
    }

    #[test]
    fn mark_changed_sets_dirty() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("math".into(), "abc".into());
        ic.mark_checked("math", 100);
        ic.mark_changed("math");
        let dirty = ic.dirty_modules();
        assert!(dirty.contains(&"math"), "changed module should be dirty");
    }

    #[test]
    fn mark_changed_propagates_to_dependents() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("core".into(), "a".into());
        ic.register_module("math".into(), "b".into());
        ic.register_module("app".into(), "c".into());
        ic.add_dependency("math".into(), "core".into());
        ic.add_dependency("app".into(), "math".into());
        // Clear all dirty flags
        ic.mark_checked("core", 1);
        ic.mark_checked("math", 2);
        ic.mark_checked("app", 3);
        assert!(ic.dirty_modules().is_empty());
        // Change core; math depends on core, so math should become dirty
        ic.mark_changed("core");
        let dirty = ic.dirty_modules();
        assert!(dirty.contains(&"core"));
        assert!(dirty.contains(&"math"), "dependent of core should be dirty");
        // app depends on math, but mark_changed only propagates one level
        // (the current implementation propagates to direct dependents)
    }

    #[test]
    fn mark_changed_unknown_module_is_noop() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("math".into(), "a".into());
        ic.mark_checked("math", 1);
        ic.mark_changed("nonexistent");
        // Should not panic and math should remain clean
        assert!(ic.dirty_modules().is_empty());
    }

    #[test]
    fn multiple_modules_tracked_independently() {
        let mut ic = IncrementalCompiler::new();
        ic.register_module("a".into(), "1".into());
        ic.register_module("b".into(), "2".into());
        ic.register_module("c".into(), "3".into());
        assert_eq!(ic.module_count(), 3);
        ic.mark_checked("a", 1);
        ic.mark_checked("b", 2);
        let dirty = ic.dirty_modules();
        assert_eq!(dirty.len(), 1);
        assert!(dirty.contains(&"c"));
    }

    #[test]
    fn default_creates_empty_compiler() {
        let ic = IncrementalCompiler::default();
        assert_eq!(ic.module_count(), 0);
    }
}
