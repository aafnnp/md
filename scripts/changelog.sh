#!/usr/bin/env bash
#
# Lift one version's section out of CHANGELOG.md.
#
#     scripts/changelog.sh 0.1.0     # the body of `## [0.1.0]`, or exit 1
#     scripts/changelog.sh --current # the section for the version in Cargo.toml
#     scripts/changelog.sh --list    # every version the file knows about
#
# This exists because the changelog is the release notes rather than a
# summary kept alongside them, and the two ways of breaking that are both
# silent without a check: tagging a version whose entry was never written,
# and bumping the version without writing the entry. The first is caught in
# `release.yml`, the second in `ci.yml`, and both go through here so they
# cannot disagree about what a section is.
#
# A section runs from its own `## [version]` heading to the next `## ` heading.
# The heading itself is not printed: the release page already shows the tag,
# and repeating it as the first line of the body reads as a stutter.

set -euo pipefail

# Resolve through symlinks so the script finds the changelog whether it was
# invoked by path, through a relative link, or from another directory.
self=${BASH_SOURCE[0]}
while [ -L "$self" ]; do
  target=$(readlink "$self")
  case $target in
    /*) self=$target ;;
    *) self=$(dirname "$self")/$target ;;
  esac
done
root=$(cd "$(dirname "$self")/.." && pwd)
changelog=$root/CHANGELOG.md

if [ ! -f "$changelog" ]; then
  echo "changelog.sh: no CHANGELOG.md at $changelog" >&2
  exit 1
fi

# The version of the workspace, read from `[workspace.package]` rather than
# from a crate that inherits it — there is one place the number lives.
workspace_version() {
  awk '
    /^\[workspace\.package\]/ { section = 1; next }
    /^\[/ { section = 0 }
    section && /^version[[:space:]]*=/ {
      # `version = "0.1.0"` — the third field is the quoted value.
      gsub(/["[:space:]]/, "", $3)
      print $3
      exit
    }
  ' "$root/Cargo.toml"
}

# `index(...) == 1` rather than a regex: a version like `0.1.0` is all regex
# metacharacters, and matching it as a pattern would accept `0x1y0` too.
section() {
  awk -v ver="$1" '
    index($0, "## [" ver "]") == 1 { inside = 1; next }
    inside && substr($0, 1, 3) == "## " { exit }
    inside { print }
  ' "$changelog"
}

# Every `## [x]` heading, in file order.
versions() {
  awk 'substr($0, 1, 4) == "## [" {
         rest = substr($0, 5)
         bracket = index(rest, "]")
         if (bracket > 0) print substr(rest, 1, bracket - 1)
       }' "$changelog"
}

# Drop blank lines from both ends. A heading followed by nothing but whitespace
# is then an empty string, which is what makes "no section" and "an empty
# section" the same failure rather than two.
trim() {
  awk '
    { line[NR] = $0 }
    END {
      last = NR
      while (last > 0 && line[last] ~ /^[[:space:]]*$/) last--
      first = 1
      while (first <= last && line[first] ~ /^[[:space:]]*$/) first++
      for (i = first; i <= last; i++) print line[i]
    }'
}

usage() {
  cat <<'USAGE'
Lift one version's section out of CHANGELOG.md.

    scripts/changelog.sh 0.1.0      the body of `## [0.1.0]`, or exit 1
    scripts/changelog.sh --current  the section for the version in Cargo.toml
    scripts/changelog.sh --list     every version the file knows about
USAGE
}

case ${1:-} in
  -h|--help|'')
    usage
    exit 0
    ;;
  --list)
    versions
    exit 0
    ;;
  --current)
    version=$(workspace_version)
    if [ -z "$version" ]; then
      echo "changelog.sh: could not read a version from $root/Cargo.toml" >&2
      exit 1
    fi
    ;;
  -*)
    echo "changelog.sh: unknown option '$1'" >&2
    exit 2
    ;;
  *)
    version=$1
    ;;
esac

body=$(section "$version" | trim)

if [ -z "$body" ]; then
  {
    echo "changelog.sh: CHANGELOG.md has no section for $version."
    echo
    echo "Add one before releasing:"
    echo "    ## [$version] - $(date +%Y-%m-%d)"
    echo
    echo "Versions the file does know about:"
    versions | sed 's/^/    /'
  } >&2
  exit 1
fi

printf '%s\n' "$body"
