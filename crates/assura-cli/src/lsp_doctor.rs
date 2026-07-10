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

pub(crate) fn run_doctor(output_mode: OutputMode) {
    let mut checks: Vec<serde_json::Value> = Vec::new();
    let mut all_ok = true;

    let version = env!("CARGO_PKG_VERSION");

    // rustc
    let (rustc_status, rustc_detail) =
        match process::Command::new("rustc").arg("--version").output() {
            Ok(out) if out.status.success() => {
                let ver = String::from_utf8_lossy(&out.stdout);
                let ver = ver.trim().strip_prefix("rustc ").unwrap_or(ver.trim());
                ("ok", ver.to_string())
            }
            _ => {
                all_ok = false;
                ("missing", "not found".into())
            }
        };
    checks.push(serde_json::json!({
        "name": "rustc", "status": rustc_status, "detail": rustc_detail,
        "required": true,
    }));

    // cargo
    let (cargo_status, cargo_detail) =
        match process::Command::new("cargo").arg("--version").output() {
            Ok(out) if out.status.success() => {
                let ver = String::from_utf8_lossy(&out.stdout);
                let ver = ver.trim().strip_prefix("cargo ").unwrap_or(ver.trim());
                ("ok", ver.to_string())
            }
            _ => {
                all_ok = false;
                ("missing", "not found".into())
            }
        };
    checks.push(serde_json::json!({
        "name": "cargo", "status": cargo_status, "detail": cargo_detail,
        "required": true,
    }));

    // z3
    let (z3_status, z3_detail) = match process::Command::new("z3").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            let short = ver
                .strip_prefix("Z3 version ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or(ver);
            ("ok", short.to_string())
        }
        _ => {
            all_ok = false;
            ("missing", "not found".into())
        }
    };
    checks.push(serde_json::json!({
        "name": "z3", "status": z3_status, "detail": z3_detail,
        "required": true,
    }));

    // cvc5 (optional)
    let (cvc5_status, cvc5_detail) = match process::Command::new("cvc5").arg("--version").output() {
        Ok(out) if out.status.success() => {
            let ver = String::from_utf8_lossy(&out.stdout);
            let ver = ver.trim();
            let short = ver.lines().next().unwrap_or(ver);
            ("ok", short.to_string())
        }
        _ => ("optional", "not found".into()),
    };
    checks.push(serde_json::json!({
        "name": "cvc5", "status": cvc5_status, "detail": cvc5_detail,
        "required": false,
    }));

    // wasm target (optional)
    let (wasm_status, wasm_detail): (&str, String) = match process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let installed = String::from_utf8_lossy(&out.stdout);
            if installed.contains("wasm32") {
                ("ok", "installed".into())
            } else {
                ("optional", "not installed".into())
            }
        }
        _ => ("optional", "unknown (rustup not found)".into()),
    };
    checks.push(serde_json::json!({
        "name": "wasm_target", "status": wasm_status, "detail": wasm_detail,
        "required": false,
    }));

    if output_mode == OutputMode::Json {
        let json = serde_json::json!({
            "assura": version,
            "ok": all_ok,
            "checks": checks,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json).unwrap_or_default()
        );
    } else {
        println!("Assura Doctor");
        println!("  assura:       v{version}");
        for c in &checks {
            let name = c["name"].as_str().unwrap_or("?");
            let status = c["status"].as_str().unwrap_or("?");
            let detail = c["detail"].as_str().unwrap_or("");
            let (label, pad) = match name {
                "rustc" => ("rustc:", 14usize),
                "cargo" => ("cargo:", 14),
                "z3" => ("z3:", 14),
                "cvc5" => ("cvc5:", 14),
                "wasm_target" => ("wasm target:", 14),
                other => (other, 14),
            };
            let status_label = match status {
                "ok" => "OK",
                "missing" => "MISSING",
                _ => "OPTIONAL",
            };
            // Match historical layout: "  rustc:        1.97.0 ... OK"
            println!("  {label:<pad$} {detail} ... {status_label}");
            if status == "missing" && name == "rustc" {
                println!("                Install: https://rustup.rs/");
            }
            if status == "missing" && name == "z3" {
                println!("                Install: brew install z3  (macOS)");
                println!("                         sudo apt-get install -y libz3-dev  (Ubuntu)");
            }
            if name == "cvc5" && status != "ok" {
                println!("                Install: bash scripts/setup-cvc5.sh");
            }
            if name == "wasm_target" && detail.contains("not installed") {
                println!("                Install: rustup target add wasm32-wasip1");
            }
        }
        println!();
        if all_ok {
            println!("All required dependencies are installed.");
        } else {
            println!(
                "Some required dependencies are missing. Install them and re-run `assura doctor`."
            );
        }
    }

    if !all_ok {
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
