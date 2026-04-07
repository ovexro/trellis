#!/usr/bin/env bash
# release-library.sh — publish a Trellis Arduino library release.
#
# Builds a lean snapshot of just the Arduino library files on the
# `library-release` branch, tags it as the requested vX.Y.Z, and pushes
# both the branch and the tag. The Arduino Library Manager indexer
# clones at the tag commit and ships only library files (~50 KB),
# instead of the entire monorepo (~740 KB) as in v0.1.8 through v0.3.1.
#
# The matching desktop-app build is still fired by the same vX.Y.Z tag
# push: .github/workflows/release.yml extracts the main commit SHA from
# the tag's annotation and builds the desktop bundles from main's tree.
#
# Usage:
#   scripts/release-library.sh vX.Y.Z
#
# Pre-flight (run BEFORE this script):
#   1. Bump versions in the 5 version files (see reference_build.md)
#   2. Update CHANGELOG.md
#   3. cd app/src-tauri && cargo build --release   # refresh Cargo.lock
#   4. git add -A && git commit -m "release: vX.Y.Z — <summary>"
#   5. git push origin main
#   6. Run hardware test gate (feedback_hardware_test.md)
#
# Post-flight (run AFTER this script succeeds):
#   1. cd ~/trellis && pio pkg publish --no-interactive .
#   2. Verify GitHub Releases CI built .deb/.rpm/.AppImage
#   3. Verify Arduino LM indexer log picked up the new tag
#      (http://downloads.arduino.cc/libraries/logs/github.com/ovexro/trellis/)
#   4. Verify the published zip is lean:
#      curl -sIL https://downloads.arduino.cc/libraries/github.com/ovexro/Trellis-X.Y.Z.zip
#      (expect content-length ~50 KB, not ~740 KB)

set -euo pipefail

# ─── arg parsing ──────────────────────────────────────────────────────
if [ $# -ne 1 ]; then
  echo "usage: $0 vX.Y.Z" >&2
  exit 2
fi

TAG="$1"
case "$TAG" in
  v[0-9]*.[0-9]*.[0-9]*) ;;
  *)
    echo "ERROR: tag must look like vX.Y.Z (got: $TAG)" >&2
    exit 2
    ;;
esac

VERSION="${TAG#v}"

# ─── locate repo root ──────────────────────────────────────────────────
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

LIB_BRANCH="library-release"
WORKTREE_PATH="$(mktemp -d -t trellis-lib-release.XXXXXX)"
cleanup() {
  if [ -d "$WORKTREE_PATH" ]; then
    git worktree remove --force "$WORKTREE_PATH" 2>/dev/null || true
    rm -rf "$WORKTREE_PATH" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# ─── pre-flight checks ────────────────────────────────────────────────
echo "==> Pre-flight checks"

# Working tree must be clean
if ! git diff-index --quiet HEAD --; then
  echo "ERROR: working tree has uncommitted changes" >&2
  git status --short >&2
  exit 1
fi

# Must be on main
CURRENT_BRANCH="$(git symbolic-ref --short HEAD 2>/dev/null || echo "")"
if [ "$CURRENT_BRANCH" != "main" ]; then
  echo "ERROR: must run from main branch (currently on '$CURRENT_BRANCH')" >&2
  exit 1
fi

# Local main must be in sync with origin/main
git fetch origin main --quiet
LOCAL_SHA="$(git rev-parse main)"
ORIGIN_SHA="$(git rev-parse origin/main)"
if [ "$LOCAL_SHA" != "$ORIGIN_SHA" ]; then
  echo "ERROR: local main ($LOCAL_SHA) is not in sync with origin/main ($ORIGIN_SHA)" >&2
  echo "Run: git pull --ff-only origin main" >&2
  exit 1
fi

# library.properties version must match the tag
LIB_PROP_VERSION="$(grep -E '^version=' library.properties | cut -d= -f2 | tr -d '[:space:]')"
if [ "$LIB_PROP_VERSION" != "$VERSION" ]; then
  echo "ERROR: library.properties version=$LIB_PROP_VERSION does not match tag $TAG" >&2
  exit 1
fi

# library.json version must match
LIB_JSON_VERSION="$(python3 -c "import json; print(json.load(open('library.json'))['version'])")"
if [ "$LIB_JSON_VERSION" != "$VERSION" ]; then
  echo "ERROR: library.json version=$LIB_JSON_VERSION does not match tag $TAG" >&2
  exit 1
fi

# Tag must not already exist locally or remotely
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "ERROR: tag $TAG already exists locally" >&2
  exit 1
fi
git fetch origin --tags --quiet
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  echo "ERROR: tag $TAG already exists on origin" >&2
  exit 1
fi

MAIN_SHA="$LOCAL_SHA"
MAIN_SHORT="$(git rev-parse --short main)"
echo "    main sha:    $MAIN_SHA"
echo "    main short:  $MAIN_SHORT"
echo "    tag:         $TAG"
echo "    version:     $VERSION"

# ─── prepare worktree ─────────────────────────────────────────────────
echo "==> Preparing worktree at $WORKTREE_PATH"
git fetch origin "$LIB_BRANCH" --quiet 2>/dev/null || true

if git rev-parse -q --verify "refs/remotes/origin/$LIB_BRANCH" >/dev/null; then
  # Existing branch — check it out into the worktree
  git worktree add "$WORKTREE_PATH" "origin/$LIB_BRANCH" --quiet
  (cd "$WORKTREE_PATH" && git checkout -B "$LIB_BRANCH" "origin/$LIB_BRANCH" --quiet)
else
  # First-time bootstrap: create an orphan branch
  echo "    branch '$LIB_BRANCH' does not exist yet — bootstrapping orphan branch"
  git worktree add --detach "$WORKTREE_PATH" --quiet
  (
    cd "$WORKTREE_PATH"
    git checkout --orphan "$LIB_BRANCH"
    git rm -rf . >/dev/null 2>&1 || true
  )
fi

# ─── sync library files into worktree ─────────────────────────────────
echo "==> Syncing library files into worktree"
LIB_FILES=(
  src
  examples
  library.properties
  library.json
  LICENSE
  README.md
  CHANGELOG.md
)

# Wipe everything in the worktree (tracked + untracked, but not .git)
(
  cd "$WORKTREE_PATH"
  if git ls-files | grep -q .; then
    git rm -rf -- . >/dev/null
  fi
)

# Copy each library path from main into the worktree
for path in "${LIB_FILES[@]}"; do
  if [ ! -e "$REPO_ROOT/$path" ]; then
    echo "ERROR: $REPO_ROOT/$path missing — refusing to release a broken tree" >&2
    exit 1
  fi
  cp -a "$REPO_ROOT/$path" "$WORKTREE_PATH/"
done

# Add to staging
(cd "$WORKTREE_PATH" && git add -A)

# Sanity-check what we're about to commit
echo "==> Files in the lean release tree:"
(cd "$WORKTREE_PATH" && git ls-files | sed 's/^/      /')
FILE_COUNT="$(cd "$WORKTREE_PATH" && git ls-files | wc -l)"
TREE_SIZE_BYTES="$(cd "$WORKTREE_PATH" && git ls-files | xargs -I{} stat -c %s {} 2>/dev/null | awk '{s+=$1} END {print s+0}')"
echo "    files: $FILE_COUNT"
echo "    bytes: $TREE_SIZE_BYTES (~$((TREE_SIZE_BYTES/1024)) KB)"

if [ "$FILE_COUNT" -lt 20 ] || [ "$FILE_COUNT" -gt 60 ]; then
  echo "ERROR: file count $FILE_COUNT is outside expected range (20-60). Refusing to release." >&2
  exit 1
fi

# Sanity-check: library.properties at root, src/Trellis.h present, no app/, no docs/
for required in library.properties library.json LICENSE README.md src/Trellis.h examples/BasicSwitch/BasicSwitch.ino; do
  if [ ! -e "$WORKTREE_PATH/$required" ]; then
    echo "ERROR: required file $required missing from lean tree" >&2
    exit 1
  fi
done
for forbidden in app docs scripts screenshots install.sh ABOUT.md FEATURES.md CONTRIBUTING.md .github; do
  if [ -e "$WORKTREE_PATH/$forbidden" ]; then
    echo "ERROR: forbidden path $forbidden present in lean tree" >&2
    exit 1
  fi
done

# ─── commit and tag ───────────────────────────────────────────────────
COMMIT_MSG="release: $TAG (main: $MAIN_SHORT)"
TAG_MSG="$TAG — Trellis Arduino library release

Library files snapshotted from main at $MAIN_SHORT.

main-sha: $MAIN_SHA
"

(
  cd "$WORKTREE_PATH"
  if git diff --cached --quiet; then
    echo "ERROR: lean tree is identical to previous library-release commit; nothing to release" >&2
    exit 1
  fi
  echo "==> Committing lean release"
  git -c user.name="Trellis release script" -c user.email="release@trellis.local" \
    commit -m "$COMMIT_MSG" --quiet
  echo "==> Tagging $TAG (annotated, with main-sha)"
  git -c user.name="Trellis release script" -c user.email="release@trellis.local" \
    tag -a "$TAG" -m "$TAG_MSG"
)

# ─── push ─────────────────────────────────────────────────────────────
echo "==> Pushing $LIB_BRANCH and $TAG to origin"
echo "    (this is the only step that touches the remote — abort here with Ctrl-C if you need to)"
(
  cd "$WORKTREE_PATH"
  git push origin "$LIB_BRANCH"
  git push origin "$TAG"
)

# ─── done ─────────────────────────────────────────────────────────────
cat <<EOF

==> SUCCESS

Released $TAG from a lean library tree.

  branch: $LIB_BRANCH
  tag:    $TAG (annotated)
  main-sha embedded in tag annotation: $MAIN_SHORT

Next steps (manual):
  1. Watch GitHub Actions: https://github.com/ovexro/trellis/actions
     The release.yml workflow should fire on the $TAG push and build .deb/.rpm/.AppImage from main's tree.
  2. Publish to PlatformIO Registry from main:
       cd $REPO_ROOT && pio pkg publish --no-interactive .
  3. Verify the Arduino LM indexer picks up the new tag (~30 min delay):
       curl -sL https://downloads.arduino.cc/libraries/logs/github.com/ovexro/trellis/
       curl -sL https://downloads.arduino.cc/libraries/library_index.json \\
         | python3 -c "import sys,json; d=json.load(sys.stdin); print(sorted({l['version'] for l in d['libraries'] if l['name']=='Trellis'}))"
  4. Verify the published Arduino LM zip is lean (should be ~50 KB):
       curl -sILo /dev/null -w '%{size_download}\n' \\
         https://downloads.arduino.cc/libraries/github.com/ovexro/Trellis-$VERSION.zip
EOF
