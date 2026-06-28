use super::*;

// `assura lsp` -- start the LSP server
// ---------------------------------------------------------------------------

pub(crate) fn run_lsp() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let (service, socket) = tower_lsp::LspService::new(assura_lsp::AssuraLanguageServer::new);
        tower_lsp::Server::new(stdin, stdout, socket)
            .serve(service)
            .await;
    });
}

// ---------------------------------------------------------------------------
// `assura doctor` -- check installation health
// ---------------------------------------------------------------------------

pub(crate) fn run_doctor() {
    let mut all_ok = true;

    // assura version
    let version = env!("CARGO_PKG_VERSION");
    println!("Assura Doctor");
    println!("  assura:       v{version}");

    // rustc
    match process::Command::new("rustc").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim().strip_prefix("rustc ").unwrap_or(ver.trim());
            println!("  rustc:        {ver} ... OK");
        }
        _ => {
            println!("  rustc:        not found ... MISSING");
            println!("                Install: https://rustup.rs/");
            all_ok = false;
        }
    }

    // cargo
    match process::Command::new("cargo").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim().strip_prefix("cargo ").unwrap_or(ver.trim());
            println!("  cargo:        {ver} ... OK");
        }
        _ => {
            println!("  cargo:        not found ... MISSING");
            all_ok = false;
        }
    }

    // z3
    match process::Command::new("z3").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            // z3 --version outputs "Z3 version 4.13.0 - ..."
            let short = ver
                .strip_prefix("Z3 version ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or(ver);
            println!("  z3:           {short} ... OK");
        }
        _ => {
            println!("  z3:           not found ... MISSING (required for verification)");
            println!("                Install: brew install z3  (macOS)");
            println!("                         sudo apt-get install -y libz3-dev  (Ubuntu)");
            all_ok = false;
        }
    }

    // cvc5 (optional)
    match process::Command::new("cvc5").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            let short = ver.lines().next().unwrap_or(ver);
            println!("  cvc5:         {short} ... OK");
        }
        _ => {
            println!("  cvc5:         not found ... OPTIONAL (enables portfolio mode)");
            println!("                Install: bash scripts/setup-cvc5.sh");
        }
    }

    // wasm target (optional)
    match process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let installed = String::from_utf8_lossy(&out.stdout);
            if installed.contains("wasm32") {
                println!("  wasm target:  installed ... OK");
            } else {
                println!("  wasm target:  not installed ... OPTIONAL");
                println!("                Install: rustup target add wasm32-wasip1");
            }
        }
        _ => {
            println!("  wasm target:  unknown (rustup not found) ... OPTIONAL");
        }
    }

    println!();
    if all_ok {
        println!("All required dependencies are installed.");
    } else {
        println!(
            "Some required dependencies are missing. Install them and re-run `assura doctor`."
        );
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn z3_version_prefix_strip() {
        // run_doctor extracts the Z3 version with this logic:
        let ver = "Z3 version 4.13.0 - 64 bit";
        let short = ver
            .strip_prefix("Z3 version ")
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or(ver);
        assert_eq!(short, "4.13.0");
    }

    #[test]
    fn z3_version_unexpected_format_falls_back() {
        let ver = "z3 unknown format";
        let short = ver
            .strip_prefix("Z3 version ")
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or(ver);
        // Falls back to the full string when prefix doesn't match.
        assert_eq!(short, "z3 unknown format");
    }

    #[test]
    fn rustc_version_prefix_strip() {
        let ver = "rustc 1.85.0 (4d91de4e4 2025-02-17)";
        let short = ver.strip_prefix("rustc ").unwrap_or(ver);
        assert_eq!(short, "1.85.0 (4d91de4e4 2025-02-17)");
    }

    #[test]
    fn cargo_version_prefix_strip() {
        let ver = "cargo 1.85.0 (d73d2caf9 2024-12-31)";
        let short = ver.strip_prefix("cargo ").unwrap_or(ver);
        assert_eq!(short, "1.85.0 (d73d2caf9 2024-12-31)");
    }
}
