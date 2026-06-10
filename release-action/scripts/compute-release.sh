#!/usr/bin/env bash
# Reads commits since the latest stable semver tag, parses `semver: major|minor|patch`
# trailers, and computes the next version + grouped changelog.
#
# Env in:
#   RELEASE_COMMIT_PREFIX  Subject prefix marking a release-bump commit (skip guard).
#   TAG_PREFIX             Prefix on git tags, e.g. "v".
#
# Outputs (GITHUB_OUTPUT):
#   skip            "true" if HEAD is itself a release commit.
#   should_release  "true" if a bump was determined.
#   version         e.g. "1.4.0" (no prefix).
#   bump            "major" | "minor" | "patch".
#   changelog       Multi-line markdown.

set -euo pipefail

: "${GITHUB_OUTPUT:?GITHUB_OUTPUT must be set}"
: "${RELEASE_COMMIT_PREFIX:?RELEASE_COMMIT_PREFIX must be set}"
: "${TAG_PREFIX:?TAG_PREFIX must be set}"

emit() { echo "$1=$2" >> "$GITHUB_OUTPUT"; }

emit_multiline() {
  local key=$1 value=$2
  local eof
  eof=$(dd if=/dev/urandom bs=15 count=1 status=none | base64)
  {
    echo "${key}<<${eof}"
    printf '%s\n' "$value"
    echo "${eof}"
  } >> "$GITHUB_OUTPUT"
}

LAST_MSG=$(git log -1 --pretty=format:"%s")
if [[ "$LAST_MSG" == "${RELEASE_COMMIT_PREFIX}"* ]]; then
  echo "HEAD is a release commit, skipping."
  emit skip true
  emit should_release false
  exit 0
fi
emit skip false

STABLE_RE="^${TAG_PREFIX}[0-9]+\.[0-9]+\.[0-9]+$"
LATEST_TAG=$(git tag --sort=-v:refname | grep -E "$STABLE_RE" | head -n1 || true)

if [ -z "$LATEST_TAG" ]; then
  MAJOR=0; MINOR=0; PATCH=0
  REF=$(git rev-list --max-parents=0 HEAD | head -n1)
  echo "No prior stable tag; walking from root commit."
else
  VERSION="${LATEST_TAG#"$TAG_PREFIX"}"
  MAJOR=${VERSION%%.*}
  REST=${VERSION#*.}
  MINOR=${REST%%.*}
  PATCH=${REST#*.}
  REF="$LATEST_TAG"
  echo "Latest stable tag: $LATEST_TAG"
fi

COMMITS=$(git log "${REF}..HEAD" --pretty=format:"%H %s" 2>/dev/null || true)
if [ -z "$COMMITS" ]; then
  echo "No new commits since $REF."
  emit should_release false
  exit 0
fi

BUMP=""
BREAKING=""
FEATURES=""
FIXES=""

# Precedence: major > minor > patch.
rank() { case "$1" in major) echo 3 ;; minor) echo 2 ;; patch) echo 1 ;; *) echo 0 ;; esac; }

while IFS= read -r line; do
  HASH=${line%% *}
  SUBJECT=${line#* }
  BODY=$(git log -1 --pretty=format:"%b" "$HASH")

  THIS=""
  if grep -qiE '^semver:[[:space:]]*major\b' <<<"$BODY"; then THIS=major
  elif grep -qiE '^semver:[[:space:]]*minor\b' <<<"$BODY"; then THIS=minor
  elif grep -qiE '^semver:[[:space:]]*patch\b' <<<"$BODY"; then THIS=patch
  fi
  [ -z "$THIS" ] && continue

  case "$THIS" in
    major) BREAKING+="- ${SUBJECT}"$'\n' ;;
    minor) FEATURES+="- ${SUBJECT}"$'\n' ;;
    patch) FIXES+="- ${SUBJECT}"$'\n' ;;
  esac

  if [ "$(rank "$THIS")" -gt "$(rank "$BUMP")" ]; then
    BUMP="$THIS"
  fi
done <<< "$COMMITS"

if [ -z "$BUMP" ]; then
  echo "No semver trailers found in commits since $REF."
  emit should_release false
  exit 0
fi

case "$BUMP" in
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  patch) PATCH=$((PATCH + 1)) ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
echo "Next version: $NEW_VERSION ($BUMP bump)"

CHANGELOG=""
[ -n "$BREAKING" ] && CHANGELOG+="### Breaking changes"$'\n'"${BREAKING}"$'\n'
[ -n "$FEATURES" ] && CHANGELOG+="### New features"$'\n'"${FEATURES}"$'\n'
[ -n "$FIXES" ]    && CHANGELOG+="### Fixes"$'\n'"${FIXES}"$'\n'

emit should_release true
emit version "$NEW_VERSION"
emit bump "$BUMP"
emit_multiline changelog "$CHANGELOG"
