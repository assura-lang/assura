//! Project-wide contract database for cross-function propagation.

use std::collections::HashMap;
use std::path::PathBuf;

use assura_rust_analyzer::{AnnotatedItem, AnnotatedItemKind};

use crate::types::CalledFunctionContract;

/// Index of all annotated functions in a project.
#[derive(Debug, Default)]
pub struct ContractDatabase {
    /// Free functions keyed by name.
    functions: HashMap<String, FunctionEntry>,
    /// Methods keyed by (type_name, method_name).
    methods: HashMap<(String, String), FunctionEntry>,
}

#[derive(Debug, Clone)]
struct FunctionEntry {
    name: String,
    signature: String,
    requires: Vec<String>,
    ensures: Vec<String>,
    file: PathBuf,
}

impl ContractDatabase {
    /// Build a contract database from scan results.
    pub fn from_scan(scan: &[(PathBuf, Vec<AnnotatedItem>)]) -> Self {
        let mut db = Self::default();
        for (file, items) in scan {
            let mut current_impl_type: Option<String> = None;
            for item in items {
                match &item.kind {
                    AnnotatedItemKind::ImplBlock { self_type, .. } => {
                        current_impl_type = Some(self_type.clone());
                    }
                    AnnotatedItemKind::Function {
                        name,
                        params,
                        return_type,
                        ..
                    } => {
                        let requires: Vec<String> = item
                            .contract
                            .requires
                            .iter()
                            .map(|c| c.body.clone())
                            .collect();
                        let ensures: Vec<String> = item
                            .contract
                            .ensures
                            .iter()
                            .map(|c| c.body.clone())
                            .collect();

                        // Skip functions with no contracts
                        if requires.is_empty() && ensures.is_empty() {
                            continue;
                        }

                        let param_str = params
                            .iter()
                            .map(|p| format!("{}: {}", p.name, p.ty))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let ret = return_type
                            .as_deref()
                            .map(|r| format!(" -> {r}"))
                            .unwrap_or_default();
                        let signature = format!("fn {name}({param_str}){ret}");

                        let entry = FunctionEntry {
                            name: name.clone(),
                            signature,
                            requires,
                            ensures,
                            file: file.clone(),
                        };

                        // If inside an impl block, index as method
                        if let Some(ty) = &current_impl_type {
                            db.methods.insert((ty.clone(), name.clone()), entry.clone());
                        }
                        // Always index as free function too (for name-based lookup)
                        db.functions.insert(name.clone(), entry);
                    }
                    AnnotatedItemKind::Struct { .. } => {}
                }
            }
        }

        db
    }

    /// Look up contracts for a called function by name.
    pub fn lookup_function(&self, name: &str) -> Option<CalledFunctionContract> {
        self.functions.get(name).map(|e| CalledFunctionContract {
            name: e.name.clone(),
            signature: e.signature.clone(),
            requires: e.requires.clone(),
            ensures: e.ensures.clone(),
            source_file: e.file.display().to_string(),
        })
    }

    /// Look up contracts for a method call by type and method name.
    pub fn lookup_method(&self, type_name: &str, method: &str) -> Option<CalledFunctionContract> {
        self.methods
            .get(&(type_name.to_string(), method.to_string()))
            .map(|e| CalledFunctionContract {
                name: e.name.clone(),
                signature: e.signature.clone(),
                requires: e.requires.clone(),
                ensures: e.ensures.clone(),
                source_file: e.file.display().to_string(),
            })
    }

    /// Look up by method name only (any type).
    pub fn lookup_method_by_name(&self, method: &str) -> Option<CalledFunctionContract> {
        self.methods
            .iter()
            .find(|((_, m), _)| m == method)
            .map(|(_, e)| CalledFunctionContract {
                name: e.name.clone(),
                signature: e.signature.clone(),
                requires: e.requires.clone(),
                ensures: e.ensures.clone(),
                source_file: e.file.display().to_string(),
            })
    }

    /// Get all annotated function/method contracts (for sibling context).
    pub fn all_contracts(&self) -> Vec<CalledFunctionContract> {
        self.functions
            .values()
            .map(|e| CalledFunctionContract {
                name: e.name.clone(),
                signature: e.signature.clone(),
                requires: e.requires.clone(),
                ensures: e.ensures.clone(),
                source_file: e.file.display().to_string(),
            })
            .collect()
    }

    /// Number of indexed functions.
    pub fn len(&self) -> usize {
        self.functions.len()
    }

    /// Whether the database is empty.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_rust_analyzer::*;

    fn make_item(name: &str, requires: &[&str], ensures: &[&str]) -> AnnotatedItem {
        let contract = InlineContract {
            requires: requires
                .iter()
                .map(|s| ContractClause {
                    kind: InlineClauseKind::Requires,
                    body: s.to_string(),
                    offset: 0,
                })
                .collect(),
            ensures: ensures
                .iter()
                .map(|s| ContractClause {
                    kind: InlineClauseKind::Ensures,
                    body: s.to_string(),
                    offset: 0,
                })
                .collect(),
            ..Default::default()
        };

        AnnotatedItem {
            contract,
            kind: AnnotatedItemKind::Function {
                name: name.to_string(),
                params: vec![ParamInfo {
                    name: "x".to_string(),
                    ty: "i32".to_string(),
                }],
                return_type: Some("i32".to_string()),
                is_unsafe: false,
                is_async: false,
            },
            line: 1,
            offset: 0,
        }
    }

    #[test]
    fn lookup_function() {
        let items = vec![make_item("validate", &["x > 0"], &["result >= 0"])];
        let scan = vec![(PathBuf::from("src/lib.rs"), items)];
        let db = ContractDatabase::from_scan(&scan);

        assert_eq!(db.len(), 1);
        let found = db.lookup_function("validate").unwrap();
        assert_eq!(found.name, "validate");
        assert_eq!(found.requires, vec!["x > 0"]);
        assert_eq!(found.ensures, vec!["result >= 0"]);
    }

    #[test]
    fn lookup_missing_returns_none() {
        let db = ContractDatabase::default();
        assert!(db.lookup_function("nonexistent").is_none());
    }

    #[test]
    fn all_contracts_returns_all() {
        let items = vec![
            make_item("foo", &["a > 0"], &[]),
            make_item("bar", &[], &["result > 0"]),
        ];
        let scan = vec![(PathBuf::from("lib.rs"), items)];
        let db = ContractDatabase::from_scan(&scan);
        assert_eq!(db.all_contracts().len(), 2);
    }

    #[test]
    fn functions_without_contracts_are_skipped() {
        let items = vec![make_item("no_contracts", &[], &[])];
        let scan = vec![(PathBuf::from("lib.rs"), items)];
        let db = ContractDatabase::from_scan(&scan);
        assert!(db.is_empty());
    }

    #[test]
    fn lookup_method_by_name_found() {
        let impl_item = AnnotatedItem {
            contract: InlineContract::default(),
            kind: AnnotatedItemKind::ImplBlock {
                self_type: "MyStruct".to_string(),
                trait_name: None,
            },
            line: 1,
            offset: 0,
        };
        let method_item = make_item("do_thing", &["x > 0"], &["result >= 0"]);
        let scan = vec![(PathBuf::from("src/lib.rs"), vec![impl_item, method_item])];
        let db = ContractDatabase::from_scan(&scan);

        let found = db.lookup_method_by_name("do_thing");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "do_thing");
    }

    #[test]
    fn lookup_method_with_self_type() {
        let impl_item = AnnotatedItem {
            contract: InlineContract::default(),
            kind: AnnotatedItemKind::ImplBlock {
                self_type: "Counter".to_string(),
                trait_name: None,
            },
            line: 1,
            offset: 0,
        };
        let method_item = make_item("increment", &["self.count < max"], &[]);
        let scan = vec![(PathBuf::from("src/lib.rs"), vec![impl_item, method_item])];
        let db = ContractDatabase::from_scan(&scan);

        let found = db.lookup_method("Counter", "increment");
        assert!(found.is_some());

        let not_found = db.lookup_method("WrongType", "increment");
        assert!(not_found.is_none());
    }
}
