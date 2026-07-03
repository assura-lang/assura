# crates.io packaging and first release

This document is the operational guide for publishing Assura libraries to
[crates.io](https://crates.io) and cutting a versioned GitHub Release via
cargo-dist. It is not a product roadmap.

**Do not push a `v*` tag or run a real publish without an explicit go-ahead
from a maintainer.** Tagging starts an irreversible public release pipeline.

## What ships on crates.io

Library crates in dependency order (computed by `scripts/publish-crates.sh`):

`assura-ast` → `assura-config` → `assura-diagnostics` → `assura-macros` →
`assura-runtime` → `assura-parser` → `assura-fmt` → `assura-stdlib` →
`assura-resolve` → `assura-types` → `assura-codegen` → `assura-smt` →
**`assura-pipeline`** (preferred public embed API).

All of the above share the workspace version in root `Cargo.toml`
(`[workspace.package] version`). Today that is **0.1.0**.

## What does not (yet)

| Package | Reason |
|---------|--------|
| `assura` (CLI binary package) | Depends on unpublished frontends (`assura-lsp`, `assura-mcp`, `assura-llm`, …). Install via **GitHub Releases / cargo-dist**, not `cargo install assura`. |
| `assura-test-support` | Internal test helpers only (`publish = false`). |
| `assura-lsp` / `mcp` / `llm` / `server` / … | Product frontends; not required for the library stack. |

## How release automation works

Pushing a tag that cargo-dist recognizes as a version (for example `v0.1.0`)
triggers [`.github/workflows/release.yml`](../.github/workflows/release.yml):

1. **plan** – cargo-dist plans artifacts for the tag.
2. **build-local-artifacts / build-global-artifacts** – multi-platform CLI binaries
   (macOS arm64/x86_64, Linux x86_64) with Z3/protobuf system deps.
3. **host** – uploads artifacts, creates the GitHub Release, SBOM, Cosign
   signatures (as configured in the workflow).
4. **publish-crates** – runs `bash scripts/publish-crates.sh` with
   `secrets.CARGO_REGISTRY_TOKEN` (fail-closed; not `|| true`).
5. **announce** – final announcement step from cargo-dist.

The publish script:

- Includes only packages where `package.publish` is not `false`.
- Excludes any package that still path-depends on an unpublished workspace
  crate (so cargo packaging cannot resolve it from crates.io).
- Topo-sorts the remaining set by normal+build path dependencies.
- Treats "already uploaded" for this version as success (idempotent re-run).
- Sleeps 15s between crates on real publish so the crates.io index can catch up.

## Local dry-run

From a clean tree on the commit you intend to tag:

```bash
git status   # should be clean
bash scripts/publish-crates.sh --dry-run
```

Expected on a **first** monorepo release (nothing on crates.io yet):

- Leaves (no unpublished workspace deps) package successfully under dry-run.
- Dependents may print `no matching package named assura-*` and a note that
  this is expected; the graph preflight already passed.
- Exit code **0**. Exit non-zero means a real packaging bug; fix before tagging.

After the first successful real publish, a full dry-run should succeed for the
entire ordered set against the live index (for the same version, real publish
will report "already uploaded").

Fail-closed real publish (normally only in CI):

```bash
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```

Do not run real publish from a laptop unless the release workflow is broken
and a maintainer has asked for a manual recovery.

---

## First release checklist (`v0.1.0`)

Use this for the **first** public tag. Later releases reuse the same shape
with a bumped workspace version.

### 0. Preconditions (one-time / verify once)

- [ ] Repo is the intended public surface (`assura-lang/assura`).
- [ ] `CARGO_REGISTRY_TOKEN` is set as a repository secret (verified:
      `gh secret list` shows `CARGO_REGISTRY_TOKEN`). Token must allow publish
      for new crate names owned by the crates.io account that created it.
- [ ] Workspace version in root `Cargo.toml` is the version you want on
      crates.io (`0.1.0` for the first tag).
- [ ] Every publishable path dependency that points at another workspace crate
      includes `version = "0.1.0"` (the publish script fails preflight if not).
- [ ] No open release-blocking CI failures on `main` for the commit you will tag.
  Check at least: CI (test/clippy/guards), not only Link Check / Scorecard.
- [ ] Messaging is intentional: first release is **experimental**. Breaking
  API changes are allowed before 1.0. Prefer documenting that on the GitHub
  Release notes rather than implying production stability.

### 1. Preflight on the exact commit

```bash
git fetch origin
git checkout main
git pull --ff-only origin main
git rev-parse HEAD   # record this SHA

# Fast local gate (or full workspace test on a developer machine)
bash scripts/preflight.sh
bash scripts/publish-crates.sh --dry-run

# Confirm publish plan lists 13 libraries ending in assura-pipeline
# and does not list assura (CLI) or assura-test-support
```

- [ ] `preflight` / required checks pass for this SHA.
- [ ] Dry-run exits 0 with the expected 13-crate plan.
- [ ] Working tree is clean (`git status`).

### 2. Write release notes (before the tag)

Draft short notes for the GitHub Release (cargo-dist may seed notes; you can
edit after the release is created if needed):

Suggested themes for `v0.1.0`:

- First crates.io publication of the core library stack (`assura-pipeline` and
  dependencies).
- CLI and installers ship via **this GitHub Release** (cargo-dist), not
  `cargo install assura`.
- Experimental: contracts / SMT / API may change.

- [ ] Notes drafted (issue comment, PR, or local markdown ready to paste).

### 3. Tag and push (the irreversible step)

Tag the **exact** main tip you preflighted. Prefer an annotated tag:

```bash
# Confirm still on the intended SHA
git rev-parse HEAD

# Annotated tag matching workspace version
git tag -a v0.1.0 -m "assura v0.1.0

First public release of the core library stack on crates.io
and multi-platform CLI installers via cargo-dist."

# Push only the tag (does not force anything on main)
git push origin v0.1.0
```

Rules:

- Tag name must be a version cargo-dist accepts (for example `v0.1.0`).
- Do **not** retag or force-move `v0.1.0` after a successful publish.
- If you need a redo after a failed **pre-publish** pipeline (no crates
  uploaded), delete the remote tag only after confirming crates.io has no
  `0.1.0` packages, then re-tag a fixed commit. If any crate already
  uploaded `0.1.0`, you must bump the workspace version (yank is not a
  substitute for a new version).

- [ ] Tag pushed: `git ls-remote --tags origin 'v0.1.0'`.

### 4. Watch the release workflow

```bash
# Find the run for the tag
gh run list --workflow=release.yml --limit 5

# Stream / wait
gh run watch <RUN_ID>
# or:
bash ~/.grok/skills/github-workflow/scripts/gh-monitor.sh run-wait assura-lang/assura <RUN_ID>
```

Jobs to confirm green:

| Job | Meaning |
|-----|---------|
| `plan` | Tag recognized; dist manifest OK |
| `build-local-artifacts` / `build-global-artifacts` | CLI binaries built |
| `host` | GitHub Release + artifacts published |
| `publish-crates` | All 13 libraries on crates.io (or already uploaded) |
| `announce` | cargo-dist announce step |

If `publish-crates` fails mid-graph:

1. Read the job log; note the first crate that failed and why.
2. Fix on `main` if it is a packaging bug.
3. Re-run the failed job only if the failure was transient (index lag,
   network). The script is idempotent for already-uploaded crates.
4. If a crate partially published and a later one failed for a **content**
   bug, you cannot overwrite `0.1.0`. Bump to `0.1.1` (or next version),
   tag `v0.1.1`, and release again. Already-good crates at `0.1.0` stay.

- [ ] Full `release.yml` run succeeded for `v0.1.0`.

### 5. Post-release verification

**crates.io**

```bash
# Each should return 200 / show version 0.1.0
for c in assura-ast assura-config assura-diagnostics assura-macros \
  assura-runtime assura-parser assura-fmt assura-stdlib assura-resolve \
  assura-types assura-codegen assura-smt assura-pipeline; do
  echo -n "$c: "
  curl -fsS "https://crates.io/api/v1/crates/$c" | python3 -c \
    "import sys,json; d=json.load(sys.stdin); print(d['crate']['max_version'])"
done
```

Optional consumer smoke test in a throwaway project:

```bash
cargo new /tmp/assura-embed-smoke --lib && cd /tmp/assura-embed-smoke
# Prefer the pipeline facade
cargo add assura-pipeline@0.1.0
cargo check
```

Note: `assura-smt` / verification paths may need system Z3 (and optional CVC5)
matching the crate features; a pure `cargo check` of the graph is the minimum bar.

**GitHub Release**

```bash
gh release view v0.1.0
gh release view v0.1.0 --json assets --jq '.assets[].name'
```

- [ ] All 13 crates show `0.1.0` on crates.io.
- [ ] Release page has installers / archives for the configured targets.
- [ ] Release notes state experimental status and CLI install path.

**Messaging (optional same day)**

- [ ] README install section points at GitHub Releases for the CLI (and
      `assura-pipeline` for embedding), without claiming `cargo install assura`
      works from crates.io until that package is publishable.
- [ ] No announcement that implies production-ready formal verification guarantees.

### 6. Explicit non-goals for `v0.1.0`

Do **not** block the first tag on:

- Publishing `assura` CLI to crates.io (`cargo install assura`).
- Publishing LSP / MCP / server crates.
- Feature-complete SMT parity or 1.0 stability promises.

Those are follow-ups after the library graph and release machinery are proven.

---

## Subsequent releases (short form)

1. Bump `[workspace.package] version` (and any hard-coded `version =` on path
   deps if they are not inherited) in one PR; land on `main`.
2. Preflight + `bash scripts/publish-crates.sh --dry-run` on the release SHA.
3. Tag `vX.Y.Z` on that SHA; `git push origin vX.Y.Z`.
4. Watch `release.yml`; verify crates.io + GitHub Release.

Never reuse a version that already exists on crates.io.

## Manual recovery (break-glass)

Only if `publish-crates` cannot run in CI and a maintainer approves:

```bash
git checkout v0.1.0   # or the release commit
# ensure clean tree matching the tag
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```

Prefer re-running the GitHub Actions job over laptop publish so logs and
token handling stay in one place.

## Related files

| Path | Role |
|------|------|
| `scripts/publish-crates.sh` | Graph filter, topo order, fail-closed publish |
| `.github/workflows/release.yml` | cargo-dist release + `publish-crates` job |
| `dist-workspace.toml` | cargo-dist targets and installers |
| Root `Cargo.toml` | Workspace version and shared package metadata |
| Individual `crates/*/Cargo.toml` | `publish = false` where needed; path+version deps |
