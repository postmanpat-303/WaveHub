#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
gate() { bash scripts/release-gate.sh "$@"; }
expect() {
  local want="$1"
  shift
  local got
  got=$(gate "$@")
  test "$got" = "$want" || { echo "wanted $want from [$*], got $got" >&2; exit 1; }
}
refuse() {
  if gate "$@" >/dev/null; then
    echo "should have refused [$*]" >&2
    exit 1
  fi
}

expect none validate "" v0.2.9 true false
expect none validate anything v0.2.9 false true
expect upload update-draft v0.2.9 v0.2.9 true false
expect upload-publish publish v0.2.9 v0.2.9 true false
expect upload-publish publish v0.2.9 v0.2.9 false true

refuse update-draft v0.2.9 v0.2.9 false false
refuse update-draft v0.2.9 v0.2.9 true true
refuse update-draft wrong v0.2.9 true false
refuse publish wrong v0.2.9 true false
refuse publish v0.2.8 v0.2.8 false false
refuse publish "" v0.2.9 true false
refuse validate "" not-a-tag true false
refuse dance "" v0.2.9 true false

echo "release gate ok"
