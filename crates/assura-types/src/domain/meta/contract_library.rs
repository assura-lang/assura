use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr};

use crate::TypeError;

// ===========================================================================
// T111: Contract libraries as publishable packages
// ===========================================================================

/// Tracks contract library packaging metadata.
#[derive(Debug, Clone)]
pub(crate) struct ContractLibraryChecker {
    libraries: Vec<ContractLibrary>,
}

#[derive(Debug, Clone)]
pub(crate) struct ContractLibrary {
    pub name: String,
    pub version: String,
    pub exported_contracts: Vec<String>,
    pub dependencies: Vec<LibraryDep>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryDep {
    pub name: String,
    pub version_req: String,
}

impl ContractLibraryChecker {
    pub fn new() -> Self {
        Self {
            libraries: Vec::new(),
        }
    }

    pub fn declare_library(&mut self, name: String, version: String) {
        self.libraries.push(ContractLibrary {
            name,
            version,
            exported_contracts: Vec::new(),
            dependencies: Vec::new(),
        });
    }

    pub fn add_export(&mut self, lib_name: &str, contract: String) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.exported_contracts.push(contract);
        }
    }

    pub fn add_dependency(&mut self, lib_name: &str, dep: LibraryDep) {
        if let Some(lib) = self.libraries.iter_mut().find(|l| l.name == lib_name) {
            lib.dependencies.push(dep);
        }
    }

    /// Check for libraries with no exports.
    pub fn check_empty_exports(&self) -> Vec<TypeError> {
        self.libraries
            .iter()
            .filter(|l| l.exported_contracts.is_empty())
            .map(|l| TypeError {
                code: "A55001".into(),
                message: format!("library `{}` has no exported contracts", l.name),
                span: 0..1,
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    /// Check for circular dependencies.
    pub fn check_circular_deps(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for lib in &self.libraries {
            for dep in &lib.dependencies {
                if dep.name == lib.name {
                    errors.push(TypeError {
                        code: "A55002".into(),
                        message: format!("library `{}` depends on itself", lib.name),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    /// Check for duplicate library names.
    pub fn check_duplicates(&self) -> Vec<TypeError> {
        let mut seen = std::collections::HashSet::new();
        let mut errors = Vec::new();
        for lib in &self.libraries {
            if !seen.insert(lib.name.clone()) {
                errors.push(TypeError {
                    code: "A55003".into(),
                    message: format!("duplicate library name `{}`", lib.name),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    /// Check for version constraint compatibility between libraries and deps.
    pub fn check_version_compat(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for lib in &self.libraries {
            for dep in &lib.dependencies {
                if dep.version_req != "*" && dep.version_req != lib.version {
                    // Check if any declared library matches the dep
                    let dep_lib = self.libraries.iter().find(|l| l.name == dep.name);
                    if let Some(found) = dep_lib
                        && dep.version_req != found.version
                    {
                        errors.push(TypeError {
                            code: "A55003".into(),
                            message: format!(
                                "library `{}` v{} depends on `{}` v{} but found v{}",
                                lib.name, lib.version, dep.name, dep.version_req, found.version
                            ),
                            span: 0..1,
                            secondary: None,
                            suggestion: None,
                        });
                    }
                }
            }
        }
        errors
    }
}

impl Default for ContractLibraryChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Contract library source walking
// ===========================================================================

impl ContractLibraryChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = ContractLibraryChecker::new();
        let mut found = false;
        for decl in &source.decls {
            if let Decl::Block {
                kind, name, body, ..
            } = &decl.node
                && *kind == BlockKind::Library
            {
                found = true;
                checker.declare_library(name.clone(), "0.1.0".into());
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind {
                        if (k == "export" || k == "exports")
                            && let Expr::Ident(contract_name) = &clause.body.node
                        {
                            checker.add_export(name, contract_name.clone());
                        }
                        if (k == "depends" || k == "dependency")
                            && let Expr::Ident(dep_name) = &clause.body.node
                        {
                            checker.add_dependency(
                                name,
                                LibraryDep {
                                    name: dep_name.clone(),
                                    version_req: "*".into(),
                                },
                            );
                        }
                    }
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = checker.check_empty_exports();
        errors.extend(checker.check_circular_deps());
        errors.extend(checker.check_duplicates());
        errors.extend(checker.check_version_compat());
        errors
    }
}
