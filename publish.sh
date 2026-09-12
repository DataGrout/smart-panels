#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

# ─── Usage ────────────────────────────────────────────────────────────────────

usage() {
  cat <<EOF
Usage: ./publish.sh [--dry-run] [--skip-checks] [--allow-dirty]

Publishes every workspace crate whose version is not yet on crates.io.

The version is not bumped here. All crates share the workspace version in
rust/Cargo.toml, and a release commit sets it together with a dated CHANGELOG
entry. This script then reads that version, works out which crates crates.io
does not have at it, and publishes exactly those, in dependency order. Running
it again after a partial failure publishes only what is still missing.

  --dry-run       package and verify the pending crates without uploading
  --skip-checks   skip fmt / clippy / test / doc (CI runs the same set)
  --allow-dirty   publish from a working tree with uncommitted changes
EOF
  exit 1
}

DRY_RUN=false
SKIP_CHECKS=false
ALLOW_DIRTY=false
for ARG in "$@"; do
  case "$ARG" in
    --dry-run) DRY_RUN=true ;;
    --skip-checks) SKIP_CHECKS=true ;;
    --allow-dirty) ALLOW_DIRTY=true ;;
    -h|--help) usage ;;
    *) echo "Unknown argument: $ARG"; usage ;;
  esac
done

# ─── Workspace version ───────────────────────────────────────────────────────

VERSION=$(awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p && /^version/ {gsub(/[" ]/,"",$3); print $3; exit}' rust/Cargo.toml)
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]]; then
  echo "Error: could not read [workspace.package].version from rust/Cargo.toml (got '$VERSION')"
  exit 1
fi

# Member crates, in manifest order. cargo orders the actual upload by
# dependency, so this order only affects the report below.
CRATES=()
while IFS= read -r NAME; do
  CRATES+=("$NAME")
done < <(awk '/^members = \[/{p=1;next} p && /\]/{exit} p {gsub(/[",[:space:]]/,""); if ($0!="") print}' rust/Cargo.toml)

if [[ ${#CRATES[@]} -eq 0 ]]; then
  echo "Error: no workspace members found in rust/Cargo.toml"
  exit 1
fi

echo "Workspace version: $VERSION"
echo ""

# Every crate must take the workspace version, or "one version for all" is
# not true and the crates.io check below would be looking at the wrong number.
for CRATE in "${CRATES[@]}"; do
  if ! grep -qE '^version(\.workspace)?[[:space:]]*=[[:space:]]*(\{[[:space:]]*)?(workspace[[:space:]]*=[[:space:]]*)?true' "rust/$CRATE/Cargo.toml"; then
    echo "Error: rust/$CRATE/Cargo.toml does not use 'version.workspace = true'"
    exit 1
  fi
done

# ─── Release hygiene ─────────────────────────────────────────────────────────

if ! $ALLOW_DIRTY && [[ -n "$(git status --porcelain)" ]]; then
  echo "Error: working tree has uncommitted changes. Commit them or pass --allow-dirty."
  exit 1
fi

if ! grep -qE "^## $VERSION — [0-9]{4}-[0-9]{2}-[0-9]{2}" CHANGELOG.md; then
  echo "Error: CHANGELOG.md has no dated entry '## $VERSION — YYYY-MM-DD'."
  echo "       A release commit adds that entry alongside the version bump."
  exit 1
fi

LOCK_VERSION=$(awk '/^name = "'"${CRATES[0]}"'"$/{getline; gsub(/[" ]/,"",$3); print $3; exit}' rust/Cargo.lock)
if [[ "$LOCK_VERSION" != "$VERSION" ]]; then
  echo "Error: rust/Cargo.lock records ${CRATES[0]} $LOCK_VERSION, manifest says $VERSION."
  echo "       Run 'cargo check' in rust/ and commit the lockfile."
  exit 1
fi

# ─── What crates.io already has ──────────────────────────────────────────────

# crates.io refuses requests without a User-Agent that identifies the caller.
UA="datagrout-panels-publish (https://github.com/DataGrout/smart-panels)"

published() {
  local CODE
  CODE=$(curl -s -o /dev/null -w '%{http_code}' -A "$UA" \
    "https://crates.io/api/v1/crates/$1/$2")
  case "$CODE" in
    200) return 0 ;;
    404) return 1 ;;
    *) echo "Error: crates.io answered HTTP $CODE for $1 $2" >&2; exit 1 ;;
  esac
}

PENDING=()
for CRATE in "${CRATES[@]}"; do
  if published "$CRATE" "$VERSION"; then
    printf "  %-26s %s already on crates.io\n" "$CRATE" "$VERSION"
  else
    printf "  %-26s %s not published\n" "$CRATE" "$VERSION"
    PENDING+=("$CRATE")
  fi
done
echo ""

if [[ ${#PENDING[@]} -eq 0 ]]; then
  echo "Nothing to publish: every crate is on crates.io at $VERSION."
  if ! git tag -l "v$VERSION" | grep -q .; then
    git tag "v$VERSION"
    echo "Created missing local tag v$VERSION. Push it with 'git push --tags'."
  fi
  exit 0
fi

# ─── The same checks CI runs ─────────────────────────────────────────────────

if ! $SKIP_CHECKS; then
  echo "Running the CI checks..."
  (
    cd rust
    cargo fmt --all --check
    echo "  cargo fmt: OK"
    cargo clippy --workspace --all-targets --quiet -- -D warnings
    echo "  cargo clippy: OK"
    cargo test --workspace --quiet 2>&1 | grep -E "^test result"
    echo "  cargo test: OK"
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --quiet
    echo "  cargo doc: OK"
  )
  echo ""
fi

# ─── Publish (or dry-run) ────────────────────────────────────────────────────

PKG_ARGS=()
for CRATE in "${PENDING[@]}"; do
  PKG_ARGS+=(-p "$CRATE")
done

# One invocation for all pending crates: cargo publishes them in dependency
# order and waits for each to land in the index before uploading the next,
# so a renderer never fails on a model version that is still propagating.
if $DRY_RUN; then
  echo "─── DRY RUN: ${PENDING[*]} ───"
  (cd rust && cargo publish --dry-run "${PKG_ARGS[@]}")
  echo ""
  echo "Dry run complete. Run without --dry-run to upload."
  exit 0
fi

if [[ -t 0 ]]; then
  read -r -p "Publish ${PENDING[*]} at $VERSION to crates.io? [y/N] " CONFIRM
  case "${CONFIRM:-}" in
    y|Y|yes|YES) ;;
    *) echo "Cancelled."; exit 0 ;;
  esac
  echo ""
fi

echo "─── PUBLISHING $VERSION: ${PENDING[*]} ───"
if ! (cd rust && cargo publish "${PKG_ARGS[@]}"); then
  echo ""
  echo "Publish stopped early. Re-run ./publish.sh: crates that made it are"
  echo "detected as already published and only the rest are uploaded."
  exit 1
fi
echo ""

if git tag -l "v$VERSION" | grep -q .; then
  echo "Local tag v$VERSION already exists; left as is."
else
  git tag "v$VERSION"
  echo "Created local tag v$VERSION."
fi

echo "Published $VERSION: ${PENDING[*]}"
echo "Run 'git push && git push --tags' to publish the tag."
