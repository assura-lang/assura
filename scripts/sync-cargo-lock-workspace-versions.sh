#!/usr/bin/env bash
# After release-please (or any tool) bumps [workspace.package].version in
# Cargo.toml, Cargo.lock still lists each workspace member at the old
# version until cargo rewrites those entries. CI with --locked then fails
# on every job with "cannot update the lock file".
#
# release-type "rust" updates Cargo.lock for a single root package (e.g.
# patchloom). Virtual workspaces that use release-type "simple" +
# extra-files on workspace.package.version do NOT get that for free.
#
# This script only refreshes workspace package versions in the lock (same
# effect as `cargo check` without --locked after a version bump). It does
# NOT run `cargo generate-lockfile` (would re-resolve the whole graph).
#
# Usage (repo root, after version / path-dep sync):
#   bash scripts/sync-cargo-lock-workspace-versions.sh
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

# Any always-present workspace member is enough to force a lock rewrite
# of path/workspace package version entries.
cargo check -p assura-ast

cargo metadata --locked --format-version 1 >/dev/null
echo "Cargo.lock aligned with workspace package versions (cargo metadata --locked ok)"
