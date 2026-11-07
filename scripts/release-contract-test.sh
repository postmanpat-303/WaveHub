#!/usr/bin/env bash
# Static checks for the release workflow, plus the matrix and publish gate.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
bash scripts/release-matrix-test.sh
bash scripts/release-gate-test.sh

wf=.github/workflows/release.yml
test -f "$wf"
# The only way to start a release is a person running this workflow.
# It must not dispatch the platform workflows, and it must not run on push or tag.
grep -q 'workflow_dispatch:' "$wf"
if grep -E 'gh workflow run|actions/workflows/.*/dispatches|workflow_run:' "$wf"; then
  echo "release.yml dispatches another workflow" >&2
  exit 1
fi
if grep -E '^on:|push:|release:' "$wf" | grep -E 'push:|release:'; then
  echo "release.yml has a push or release trigger" >&2
  exit 1
fi
grep -q 'default: validate' "$wf"
grep -q 'upload-publish' "$wf" || grep -q 'mode.*publish' "$wf"
# Publishing is a separate step from uploading, and it is not the default path.
awk '
  /^  assemble:/ { job=1 }
  job && /contents: write/ { write=1 }
  job && /draft=false/ { publish=1 }
  END { if (!write || !publish) exit 1 }
' "$wf"
# Build jobs must not be the writers.
python3 - <<'PY'
from pathlib import Path
text = Path(".github/workflows/release.yml").read_text()
# Crude: contents: write may appear once, on the assemble job.
if text.count("contents: write") != 1:
    raise SystemExit(f"expected exactly one contents: write, found {text.count('contents: write')}")
if "uses: ./.github/workflows/linux.yml" not in text:
    raise SystemExit("linux build is not a job of release.yml")
for name in ("windows.yml", "macos.yml", "flatpak.yml"):
    if f"uses: ./.github/workflows/{name}" not in text:
        raise SystemExit(f"missing reusable job {name}")
PY

for f in linux.yml windows.yml macos.yml flatpak.yml; do
  path=".github/workflows/$f"
  grep -q 'workflow_call:' "$path" || { echo "$f is not reusable" >&2; exit 1; }
  if grep -n 'SHA256SUMS' "$path" | grep -q 'release upload'; then
    echo "$f still uploads SHA256SUMS on its own" >&2
    exit 1
  fi
done
echo "release contract ok"
