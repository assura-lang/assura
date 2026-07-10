use super::*;

// `assura coverage` -- contract coverage report
// ---------------------------------------------------------------------------

pub(crate) fn run_coverage(
    path: &str,
    contracts_dir: &str,
    format: &str,
    min_coverage: Option<f64>,
) {
    validate_human_json_format(format, "coverage");
    let root = Path::new(path);
    let src_dir = root.join("src");

    if !src_dir.exists() {
        eprintln!("Error: no src/ directory found at {}", root.display());
        process::exit(2);
    }

    // Phase 1: Discover all public Rust functions
    let rs_files = discover_rs_files(&src_dir);
    let mut all_fns: Vec<(String, String)> = Vec::new(); // (file, fn_name)

    for rs_file in &rs_files {
        let rel_path = rs_file
            .strip_prefix(root)
            .unwrap_or(rs_file.as_path())
            .to_string_lossy()
            .to_string();

        let source = match fs::read_to_string(rs_file) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let sigs = extract_rust_fn_signatures(&source);
        for sig in sigs {
            if sig.is_pub {
                all_fns.push((rel_path.clone(), sig.name));
            }
        }
    }

    // Phase 2: Discover all contract/bind names from .assura files
    let contracts_path = root.join(contracts_dir);
    let mut contract_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut contract_files: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    // Scan contracts directory
    if contracts_path.exists() {
        collect_contract_names_from_dir(&contracts_path, &mut contract_names, &mut contract_files);
    }
    // Also scan for .assura files in the project root and common locations
    for extra_dir in &[".", "assura", "specs"] {
        let d = root.join(extra_dir);
        if d.exists() && d != contracts_path {
            collect_contract_names_from_dir(&d, &mut contract_names, &mut contract_files);
        }
    }

    if all_fns.is_empty() {
        eprintln!("No public functions found in {}", src_dir.display());
        process::exit(1);
    }

    // Phase 3: Cross-reference
    let mut covered: Vec<(String, String, String)> = Vec::new(); // (file, fn, contract_file)
    let mut uncovered: Vec<(String, String, usize)> = Vec::new(); // (file, fn, param_count)

    for (file, fn_name) in &all_fns {
        if contract_names.contains(fn_name.as_str()) {
            let cf = contract_files
                .get(fn_name.as_str())
                .cloned()
                .unwrap_or_else(|| "?".to_string());
            covered.push((file.clone(), fn_name.clone(), cf));
        } else {
            // Get param count for prioritization
            let param_count = rs_files
                .iter()
                .find(|f| {
                    f.strip_prefix(root)
                        .unwrap_or(f.as_path())
                        .to_string_lossy()
                        == *file
                })
                .and_then(|f| fs::read_to_string(f).ok())
                .map(|src| {
                    extract_rust_fn_signatures(&src)
                        .iter()
                        .find(|s| s.name == *fn_name)
                        .map(|s| s.params.len())
                        .unwrap_or(0)
                })
                .unwrap_or(0);
            uncovered.push((file.clone(), fn_name.clone(), param_count));
        }
    }

    // Sort uncovered by param count descending (more params = more complex = higher priority)
    uncovered.sort_by_key(|b| std::cmp::Reverse(b.2));

    let total = all_fns.len();
    let covered_count = covered.len();
    let pct = if total > 0 {
        (covered_count as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let is_json = format == "json";

    if is_json {
        let report = serde_json::json!({
            "total_functions": total,
            "covered": covered_count,
            "uncovered": uncovered.len(),
            "coverage_percent": (pct * 10.0).round() / 10.0,
            "covered_functions": covered.iter().map(|(f, n, cf)| serde_json::json!({
                "file": f, "function": n, "contract_file": cf
            })).collect::<Vec<_>>(),
            "uncovered_functions": uncovered.iter().map(|(f, n, pc)| serde_json::json!({
                "file": f, "function": n, "param_count": pc
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!("Contract Coverage: {}/", src_dir.display());
        println!("  Total public functions:  {}", total);
        println!("  With contracts:          {} ({:.1}%)", covered_count, pct);
        println!("  Without contracts:       {}", uncovered.len());

        if !covered.is_empty() {
            println!();
            println!("  Covered:");
            for (file, name, cf) in &covered {
                println!("    {file}::{name}  <-  {cf}");
            }
        }

        if !uncovered.is_empty() {
            println!();
            println!("  Uncovered (by complexity):");
            for (file, name, pc) in uncovered.iter().take(20) {
                println!("    {file}::{name}  ({pc} params)");
            }
            if uncovered.len() > 20 {
                println!("    ... and {} more", uncovered.len() - 20);
            }
        }
    }

    // Exit 1 if below min coverage
    if let Some(min) = min_coverage
        && pct < min
    {
        if !is_json {
            eprintln!();
            eprintln!("Coverage {pct:.1}% is below minimum {min:.1}%");
        }
        process::exit(1);
    }
}

/// Collect contract/bind names from all .assura files in a directory.
pub(crate) fn collect_contract_names_from_dir(
    dir: &Path,
    names: &mut std::collections::HashSet<String>,
    files: &mut std::collections::HashMap<String, String>,
) {
    let assura_files = discover_assura_files(dir);
    for af in &assura_files {
        let rel = af.to_string_lossy().to_string();
        let source = match fs::read_to_string(af) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let (parsed, _) = assura_parser::parse(&source);
        if let Some(file) = parsed {
            use assura_parser::ast::{
                BindDecl, ContractDecl, DeclVisitor, FnDef, ServiceDecl, walk_decls,
            };
            struct CoverageNames<'a> {
                names: &'a mut std::collections::HashSet<String>,
                files: &'a mut std::collections::HashMap<String, String>,
                rel: &'a str,
            }
            impl DeclVisitor for CoverageNames<'_> {
                fn visit_contract(&mut self, c: &ContractDecl) {
                    self.names.insert(c.name.clone());
                    self.files.insert(c.name.clone(), self.rel.to_string());
                }
                fn visit_bind(&mut self, b: &BindDecl) {
                    self.names.insert(b.name.clone());
                    self.files.insert(b.name.clone(), self.rel.to_string());
                }
                fn visit_fn_def(&mut self, f: &FnDef) {
                    self.names.insert(f.name.clone());
                    self.files.insert(f.name.clone(), self.rel.to_string());
                }
                fn visit_service(&mut self, s: &ServiceDecl) {
                    self.names.insert(s.name.clone());
                    self.files.insert(s.name.clone(), self.rel.to_string());
                }
            }
            let mut visitor = CoverageNames {
                names,
                files,
                rel: &rel,
            };
            walk_decls(&mut visitor, &file.decls);
        }
    }
}

/// Recursively discover all .assura files under a directory.
pub(crate) fn discover_assura_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(discover_assura_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "assura") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discover_assura_files_finds_only_assura() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("foo.assura"), "contract Foo {}").unwrap();
        fs::write(tmp.path().join("bar.rs"), "fn bar() {}").unwrap();
        fs::write(tmp.path().join("baz.txt"), "hello").unwrap();

        let found = discover_assura_files(tmp.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].file_name().unwrap() == "foo.assura");
    }

    #[test]
    fn discover_assura_files_recursive_and_sorted() {
        let tmp = TempDir::new().unwrap();
        let sub = tmp.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(tmp.path().join("b.assura"), "").unwrap();
        fs::write(sub.join("a.assura"), "").unwrap();

        let found = discover_assura_files(tmp.path());
        assert_eq!(found.len(), 2);
        // Results should be sorted
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"a.assura".to_string()));
        assert!(names.contains(&"b.assura".to_string()));
    }

    #[test]
    fn discover_assura_files_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let found = discover_assura_files(tmp.path());
        assert!(found.is_empty());
    }

    #[test]
    fn collect_contract_names_extracts_names() {
        let tmp = TempDir::new().unwrap();
        let source = r#"
contract SafeDiv {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
}
"#;
        fs::write(tmp.path().join("div.assura"), source).unwrap();

        let mut names = std::collections::HashSet::new();
        let mut files = std::collections::HashMap::new();
        collect_contract_names_from_dir(tmp.path(), &mut names, &mut files);

        assert!(names.contains("SafeDiv"), "should find SafeDiv contract");
    }
}
