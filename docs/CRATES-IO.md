# crates.io packaging and release-please

Operational guide for publishing Assura libraries to
[crates.io](https://crates.io) and shipping CLI installers via cargo-dist.
Versioning and GitHub Releases are driven by **release-please**, not by
manually pushing `v*` tags.

**Do not merge a release-please PR without explicit maintainer approval.**
Merging it creates the tag, GitHub Release, cargo-dist artifacts, and
crates.io publish for the version in that PR.

## Current status (post-v0.2.0)

| Channel | What | Notes |
|---------|------|--------|
| crates.io | **13 library crates** at the workspace version | Public embed surface: **`assura-pipeline`** |
| GitHub Releases / cargo-dist | **`assura` CLI** installers | Package is `publish = false` with `[package.metadata.dist] dist = true` |
| crates.io CLI | **Not published** | Do not `cargo install assura` (placeholder only); use release installers or `cargo install --path crates/assura-cli` from a clone |

v0.2.0 shipped 2026-07-04 (after v0.1.0). Open release-please PRs after a cut
are **normal version bumps**, not a sign that the prior release failed. Merge
them only when you intentionally want a new release.

The temporary `release-as: 0.1.0` pin used for the first cut has been
**removed**. Subsequent versions follow conventional commits
(`feat:` / `fix:` / `chore:`) under `bump-minor-pre-major`.

## What ships on crates.io

Library crates in dependency order (computed by `scripts/publish-crates.sh`):

`assura-ast` → `assura-config` → `assura-diagnostics` → `assura-runtime` →
`assura-parser` → `assura-macros` → `assura-fmt` → `assura-stdlib` →
`assura-resolve` → `assura-types` → `assura-codegen` → `assura-smt` →
**`assura-pipeline`** (preferred public embed API).

Order is graph-derived (all path deps including **dev**). Example:
`assura-macros` has a path dev-dependency on `assura-runtime`, so runtime
publishes first.

All of the above share `[workspace.package] version` in the root
`Cargo.toml`. Path dependencies also pin `version = "…"` so packaging can
resolve them from crates.io; `scripts/sync-path-dep-versions.sh` keeps
those pins aligned on release PRs.

## What does not ship on crates.io

| Package | Reason |
|---------|--------|
| `assura` (CLI binary package) | `publish = false`; install via **GitHub Releases / cargo-dist** only. |
| `assura-test-support` | Internal test helpers only (`publish = false`). |
| `assura-lsp` / `mcp` / `llm` / `server` / … | Product frontends; not part of the library stack. |

## How the release pipeline works

```
push to main
  └─ release-please.yml
       ├─ opens/updates release PR (label: autorelease: pending)
       │    └─ sync-path-dep-versions (align path dep pins on that branch)
       └─ when release PR is merged (release_created=true):
            tag + GitHub Release
            └─ dispatches release.yml with tag=…
                 plan → build-local/global (cargo-dist) → host (upload assets)
                 → publish-crates (scripts/publish-crates.sh) → announce
```

Important details:

- **Auto-merge uses the `assura-auto-approve` GitHub App**
  (`vars.AUTO_APPROVE_CLIENT_ID` + `secrets.AUTO_APPROVE_PRIVATE_KEY`) so
  merge pushes are not suppressed. Historical note: **Auto-merge with
  `GITHUB_TOKEN` does not start push workflows** (including
  `release-please.yml`). Prefer a human merge for release-related PRs, use
  the hourly cron catch-up, or `gh workflow run "Release Please"`. See
  issue #785.
- Tags created with `GITHUB_TOKEN` **do not** start a separate `push: tags`
  workflow. `release-please.yml` therefore **workflow_dispatch**es
  `release.yml` with the **`tag` input** when a release is created.
- Auto-approve skips PRs labeled `autorelease: pending`. Release PRs must
  be merged by a human.
- **Virtual workspace + `release-type: simple`:** release-please only rewrites
  `workspace.package.version` (via `extra-files`). That does **not** update
  `Cargo.lock` the way `release-type: rust` does for a single root package
  (e.g. patchloom). The `sync-release-pr-versions` job runs
  `sync-path-dep-versions.sh` and `sync-cargo-lock-workspace-versions.sh` on
  the release PR branch so CI with `--locked` stays green. Do not omit the
  lock step when copying this layout to another monorepo.
- Use `chore:` for CI/docs/refactor. Reserve `fix:` / `feat:` for
  user-visible changes so version bumps stay intentional.
- Optional: commit `RELEASE_NOTES.md` on **main** (not on the release-please
  branch) before merging the release PR to override the GitHub Release body.
  Remove it after the release lands (see prior issue #813).

Config files:

| Path | Role |
|------|------|
| `release-please-config.json` | `release-type: simple` (workspace root has no `[package]`), updates `workspace.package.version`, `CHANGELOG.md`, `bump-minor-pre-major` |
| `.release-please-manifest.json` | Last released version per package (root `"."`) |
| `scripts/publish-crates.sh` | Fail-closed graph filter + topo publish (pre-check, 429 handling, skip already-uploaded) |
| `scripts/sync-path-dep-versions.sh` | Path-dep `version=` pins = workspace version |
| `scripts/sync-cargo-lock-workspace-versions.sh` | Align `Cargo.lock` workspace member versions after a version bump (required for CI `--locked`) |
| `scripts/check-publish-plan.sh` | Assert publish order matches the 13-crate library stack |
| `scripts/check-cargo-package.sh` | `cargo package` gate; full verify when version is on crates.io, `--list` on co-publish version-bump PRs (#814, co-publish skew) |
| `.github/workflows/release-please.yml` | Opens release PR on main push; syncs path-deps + lock on that PR; dispatches Release on create |
| `.github/workflows/release.yml` | cargo-dist installers + publish-crates (`tag` / dispatch) |
| `dist-workspace.toml` | cargo-dist targets / installers |

## Preflight (before merging a release PR)

```bash
git status   # clean working tree preferred
bash scripts/check-publish-plan.sh
bash scripts/check-cargo-package.sh   # full package+verify if version on crates.io;
                                      # auto --list on pre-publish version-bump PRs
# Optional dry-run of the publish script (no token required):
bash scripts/publish-crates.sh --dry-run
```

On a release-please PR that bumps to a version not yet on crates.io, full
`cargo package` cannot resolve path+version deps against the registry
(only the old version exists). The script falls back to `--list` so CI still
checks tarball membership; co-publish (`publish-crates.sh`) is the ordered
full verify at release time.

`check-cargo-package.sh` is the gate that would have caught the v0.1.0
`assura-smt` failure (monorepo `include_str!` paths outside the package
tarball). CI runs the same script on every rust-touching change (job
**Cargo package (publishable)**).

Fast listing only (no verify build):

```bash
bash scripts/check-cargo-package.sh --list-only
```

Fail-closed real publish normally runs only in CI after a release-please
merge:

```bash
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```

## Cutting a new release (normal path)

1. Land normal work on `main` with conventional commits (`feat:`, `fix:`,
   `chore:` as appropriate).
2. release-please updates the open release PR (or opens a new one). An open
   PR such as "release 0.1.1" after 0.1.0 is expected.
3. Review version bump, `CHANGELOG.md`, and path-dep pins
   (`sync-path-dep-versions` on the release workflow run).
4. Optionally land curated `RELEASE_NOTES.md` on **main**, wait for
   release-please to refresh the PR. The host job applies it to the GitHub
   Release body; `cleanup-release-notes` then opens an auto-merge PR to
   remove the file so the next release does not reuse stale notes.
5. Merge the release PR with an **explicit human decision** (never auto-merge
   `autorelease: pending`).
6. Watch the Release workflow; verify crates.io + GitHub Release assets.

```bash
gh run list --workflow=release.yml --limit 5
gh run watch <RUN_ID>
```

| Job | Meaning |
|-----|---------|
| `release-please` (in release-please workflow) | Tag + GitHub Release created; dispatches Release with `tag` |
| `plan` | cargo-dist plan for that tag |
| `build-local-artifacts` / `build-global-artifacts` | CLI installers |
| `host` | Upload assets (idempotent if the Release already exists); applies `RELEASE_NOTES.md` if present |
| `publish-crates` | Libraries via `scripts/publish-crates.sh` (skips versions already on crates.io) |
| `announce` | Final confirmation |
| `cleanup-release-notes` | PR to delete `RELEASE_NOTES.md` from main after apply (non-fatal) |

If `publish-crates` fails mid-graph, fix on `main` and re-dispatch the same
tag (idempotent for already-uploaded crates). You cannot overwrite a version
on crates.io.

```bash
# Re-dispatch Release for an existing tag after a script/CI fix
gh workflow run release.yml -f tag=v0.1.0
# or re-run failed jobs on an existing run:
gh run rerun <RUN_ID> --failed
```

## Post-release verification

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
gh release view vX.Y.Z
gh release view vX.Y.Z --json assets --jq '.assets[].name'
```

- [ ] All 13 library crates show the released version on crates.io.
- [ ] Release page has installers for configured targets.
- [ ] Notes state experimental status and CLI install path (GitHub Release,
      not `cargo install assura`).

## IR templates and packaging pitfalls

IR prompt markdown used by `include_str!` **must** live under
`crates/assura-smt/templates/ir/` (crate-local). Monorepo
`templates/ir/` is a pointer README only (#812). `scripts/guards.sh`
section 13 fails if pattern bodies reappear at the monorepo root.
`scripts/check-cargo-package.sh` fails if any publishable crate cannot
package/verify.

## Manual / break-glass

Only if CI cannot run and a maintainer approves a laptop publish:

```bash
git checkout vX.Y.Z
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```

Prefer re-running GitHub Actions so logs and token handling stay in one place.

## Historical: first release (v0.1.0)

The first cut used a temporary `release-as: 0.1.0` pin (otherwise historical
commits computed `1.0.0`), multiple Release re-dispatches, and script fixes
for graph order and "already exists" pre-checks. That pin is gone. Do not
re-apply `release-as` unless intentionally forcing a version for a future
cut. Details live in session notes / `assura-contrib` skill, not in day-to-day
procedure above.

## Related skill notes

Canonical patterns live in `ci-build-release` (release-please same-workflow,
cargo-dist host vs release-please body ownership, RELEASE_NOTES.md override,
`cargo package` preflight for publishable members) and `ci-branch-protection`
(never auto-merge `autorelease: pending`).
