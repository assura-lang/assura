# crates.io packaging and release-please

Operational guide for publishing Assura libraries to
[crates.io](https://crates.io) and shipping CLI installers via cargo-dist.
Versioning and GitHub Releases are driven by **release-please**, not by
manually pushing `v*` tags.

**Do not merge a release-please PR without explicit maintainer approval.**
Merging it creates the tag, GitHub Release, cargo-dist artifacts, and
crates.io publish for the version in that PR.

## What ships on crates.io

Library crates in dependency order (computed by `scripts/publish-crates.sh`):

`assura-ast` → `assura-config` → `assura-diagnostics` → `assura-macros` →
`assura-runtime` → `assura-parser` → `assura-fmt` → `assura-stdlib` →
`assura-resolve` → `assura-types` → `assura-codegen` → `assura-smt` →
**`assura-pipeline`** (preferred public embed API).

All of the above share `[workspace.package] version` in the root
`Cargo.toml`. Path dependencies also pin `version = "…"` so packaging can
resolve them from crates.io; `scripts/sync-path-dep-versions.sh` keeps
those pins aligned on release PRs.

## What does not (yet)

| Package | Reason |
|---------|--------|
| `assura` (CLI binary package) | Depends on unpublished frontends (`assura-lsp`, `assura-mcp`, `assura-llm`, …). Install via **GitHub Releases / cargo-dist**, not `cargo install assura`. |
| `assura-test-support` | Internal test helpers only (`publish = false`). |
| `assura-lsp` / `mcp` / `llm` / `server` / … | Product frontends; not required for the library stack. |

## How the release pipeline works

```
push to main
  └─ release-please
       ├─ opens/updates release PR (label: autorelease: pending)
       │    └─ sync-path-dep-versions (align path dep pins on that branch)
       └─ when release PR is merged:
            tag + GitHub Release (release_created=true)
            └─ same workflow run:
                 plan → build-local/global (cargo-dist) → host (upload assets)
                 → publish-crates (scripts/publish-crates.sh) → announce
```

Important details (from project skills / past incidents):

- Tags created with `GITHUB_TOKEN` **do not** start a separate `push: tags`
  workflow. Builds and publish therefore run in the **same** workflow when
  `release_created` is true (not “merge then hope a tag push fires”).
- Auto-approve skips PRs labeled `autorelease: pending`. Release PRs must
  be merged by a human.
- Use `chore:` for CI/docs/refactor. Reserve `fix:` / `feat:` for
  user-visible changes so version bumps stay intentional.
- Optional: commit `RELEASE_NOTES.md` on **main** (not on the release-please
  branch) before merging the release PR to override the GitHub Release body.

Config files:

| Path | Role |
|------|------|
| `release-please-config.json` | `release-type: rust`, `CHANGELOG.md`, `bump-minor-pre-major` |
| `.release-please-manifest.json` | Last released version per package (root `"."`) |
| `scripts/publish-crates.sh` | Fail-closed graph filter + topo publish |
| `scripts/sync-path-dep-versions.sh` | Path-dep `version=` pins = workspace version |
| `.github/workflows/release.yml` | release-please + cargo-dist + publish-crates |
| `dist-workspace.toml` | cargo-dist targets / installers |

## Local dry-run (packaging only)

```bash
git status   # should be clean for a release candidate
bash scripts/publish-crates.sh --dry-run
```

Expected on a **first** monorepo publish (nothing on crates.io yet):

- Leaves package successfully under dry-run.
- Dependents may note unpublished workspace deps; graph preflight still
  passed; exit code **0**.

Fail-closed real publish normally runs only in CI after a release-please
merge:

```bash
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```

## First release checklist (via release-please)

### 0. Preconditions

- [ ] `CARGO_REGISTRY_TOKEN` is set on the repo (publish-new + publish-update).
- [ ] Org/repo allow Actions to open PRs (Settings → Actions → General →
      “Allow GitHub Actions to create and approve pull requests”).
- [ ] `release-please-config.json` and `.release-please-manifest.json` are
      on `main` (this doc’s companion PR).
- [ ] Auto-approve excludes `autorelease: pending` (already true in
      `.github/workflows/auto-approve.yml`).
- [ ] Messaging is intentional: first release is **experimental**.

### 1. Land release-please wiring on main

Merge the CI PR that adds release-please. The next push to `main` runs
`release-please`. With manifest last-version `0.0.0` and current workspace
`0.1.0` history, it should open a release PR for the first version
(typically `v0.1.0` when the highest conventional-commit signal is a feat
or the computed next minor/patch under pre-1.0 rules).

If no release PR appears, push a small user-visible commit with a proper
prefix (`feat:` or `fix:`) or inspect the Release workflow logs for the
`release-please` job.

### 2. Review the release PR (do not merge yet)

- [ ] Version bump in root `Cargo.toml` / workspace package is correct.
- [ ] `CHANGELOG.md` section looks right (edit only via conventional commits
      or a normal main PR that release-please will re-absorb; **never**
      rewrite the release-please PR body in CI).
- [ ] `sync-path-dep-versions` job on the Release workflow run succeeded, or
      path-dep pins already match the workspace version.
- [ ] Optional: land `RELEASE_NOTES.md` on **main**, wait for release-please
      to refresh the PR, then continue.

Preflight packaging from the release PR tip (optional):

```bash
gh pr checkout <release-pr-number>
bash scripts/publish-crates.sh --dry-run
```

### 3. Merge the release PR (the irreversible step)

```bash
# Explicit human merge only. Do not use --admin auto-merge for this PR.
gh pr merge <release-pr-number> --squash   # or merge commit, per preference
```

Do **not** push a manual `v0.1.0` tag for the first release if release-please
owns versioning. Manual tags are break-glass only (see below).

### 4. Watch the same Release workflow run

After merge, the push to `main` should show `release_created=true` and run
plan → builds → host → publish-crates:

```bash
gh run list --workflow=release.yml --branch main --limit 5
gh run watch <RUN_ID>
```

| Job | Meaning |
|-----|---------|
| `release-please` | Tag + GitHub Release created |
| `plan` | cargo-dist plan for that tag |
| `build-local-artifacts` / `build-global-artifacts` | CLI installers |
| `host` | Upload assets (idempotent if release-please already created the Release) |
| `publish-crates` | 13 libraries on crates.io (or already uploaded) |
| `announce` | Final confirmation |

If `publish-crates` fails mid-graph, fix on `main` and either re-run failed
jobs (idempotent for already-uploaded crates) or cut a **new** version via
another release-please cycle. You cannot overwrite a version on crates.io.

### 5. Post-release verification

**crates.io**

```bash
for c in assura-ast assura-config assura-diagnostics assura-macros \
  assura-runtime assura-parser assura-fmt assura-stdlib assura-resolve \
  assura-types assura-codegen assura-smt assura-pipeline; do
  echo -n "$c: "
  curl -fsS "https://crates.io/api/v1/crates/$c" | python3 -c \
    "import sys,json; d=json.load(sys.stdin); print(d['crate']['max_version'])"
done
```

**GitHub Release**

```bash
gh release view v0.1.0   # use the actual tag from the release PR
gh release view v0.1.0 --json assets --jq '.assets[].name'
```

- [ ] All intended crates show the released version on crates.io.
- [ ] Release page has installers for configured targets.
- [ ] Notes state experimental status and CLI install path (GitHub Release,
      not `cargo install assura`).

### 6. Explicit non-goals for the first cut

Do not block the first release on:

- Publishing the `assura` CLI to crates.io.
- Publishing LSP / MCP / server crates.
- 1.0 stability promises.

## Subsequent releases

1. Land normal work on `main` with conventional commits (`feat:`, `fix:`,
   `chore:` as appropriate).
2. release-please updates the open release PR (or opens a new one).
3. Review, optionally add `RELEASE_NOTES.md` on main, merge when ready.
4. Watch the Release workflow; verify crates.io + GitHub Release.

Never reuse a version that already exists on crates.io.

## Manual / break-glass

If the same-workflow publish half failed after the tag exists:

```bash
gh workflow run release.yml -f tag=v0.1.0
# or re-run failed jobs on the existing run:
gh run rerun <RUN_ID> --failed
```

Only if CI cannot run and a maintainer approves a laptop publish:

```bash
git checkout v0.1.0
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```

Prefer re-running GitHub Actions so logs and token handling stay in one place.

## Related skill notes

Canonical patterns live in `ci-build-release` (release-please same-workflow,
cargo-dist host vs release-please body ownership, RELEASE_NOTES.md override)
and `ci-branch-protection` (never auto-merge `autorelease: pending`).
