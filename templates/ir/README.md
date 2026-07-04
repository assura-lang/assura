# IR prompt templates (pointer only)

**Canonical source of truth:**
[`crates/assura-smt/templates/ir/`](../../crates/assura-smt/templates/ir/)

Runtime strings are loaded via `include_str!` in
`crates/assura-smt/src/ir_modules/ir_templates.rs` from that crate-local
tree so `cargo package` / crates.io verify include the files.

Do **not** re-introduce full markdown bodies under this monorepo path.
They duplicate the crate tree and can drift silently (see issue #812).

| Path | Role |
|------|------|
| `crates/assura-smt/templates/ir/base.md` | Base IR generation prompt |
| `crates/assura-smt/templates/ir/patterns/*.md` | Pattern overlays (identity, arithmetic, …) |

Edit the crate-local files only. After changes, run:

```bash
cargo test -p assura-smt --locked ir_templates
bash scripts/check-cargo-package.sh   # includes assura-smt packaging
```
