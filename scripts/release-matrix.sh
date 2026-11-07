#!/usr/bin/env bash
# The files a Wave Hub release must contain before it can become Latest.
#
# Platform jobs only build. This script is the one place that decides the file
# list and writes SHA256SUMS.txt, so those jobs never race to rewrite it.
#
#   expected VERSION
#   stage VERSION DEST SRC...   copy the files (and make versionless copies)
#   sums VERSION DIR            write SHA256SUMS.txt for exactly that list
#   check VERSION DIR           fail if anything is missing, extra, or mismatched
set -euo pipefail

version_ok() {
  [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]
}

expected_names() {
  local v="$1"
  cat <<EOF
WaveHub-${v}-setup-x64.exe
WaveHub-${v}-windows-x64.zip
WaveHub.exe
WaveHub-setup-x64.exe
WaveHub-${v}-linux-x86_64.AppImage
WaveHub-${v}-linux-x86_64.deb
WaveHub-${v}-linux-x86_64.rpm
WaveHub-${v}-linux-x86_64.pkg.tar.zst
WaveHub-${v}-linux-x86_64.tar.gz
WaveHub-${v}-linux-aarch64.AppImage
WaveHub-${v}-linux-aarch64.deb
WaveHub-${v}-linux-aarch64.rpm
WaveHub-${v}-linux-aarch64.pkg.tar.zst
WaveHub-${v}-linux-aarch64.tar.gz
WaveHub-linux-x86_64
WaveHub-linux-x86_64.AppImage
WaveHub-linux-aarch64.AppImage
WaveHub-${v}-linux-x86_64.flatpak
WaveHub-linux-x86_64.flatpak
WaveHub-${v}-macos-universal.dmg
WaveHub-${v}-macos-universal.zip
WaveHub-macos-universal.dmg
WaveHub-macos-universal.zip
EOF
}

# versionless name, then the versioned file it must match
aliases() {
  local v="$1"
  cat <<EOF
WaveHub-setup-x64.exe WaveHub-${v}-setup-x64.exe
WaveHub-linux-x86_64.AppImage WaveHub-${v}-linux-x86_64.AppImage
WaveHub-linux-aarch64.AppImage WaveHub-${v}-linux-aarch64.AppImage
WaveHub-linux-x86_64.flatpak WaveHub-${v}-linux-x86_64.flatpak
WaveHub-macos-universal.dmg WaveHub-${v}-macos-universal.dmg
WaveHub-macos-universal.zip WaveHub-${v}-macos-universal.zip
EOF
}

is_expected() {
  local v="$1" name="$2"
  expected_names "$v" | grep -qx "$name"
}

cmd_expected() {
  version_ok "$1"
  expected_names "$1"
}

cmd_stage() {
  local v="$1" dest="$2"
  shift 2
  version_ok "$v"
  [[ "$#" -ge 1 ]] || { echo "stage needs at least one source directory" >&2; exit 1; }
  mkdir -p "$dest"
  declare -A found=()
  local path base
  while IFS= read -r path; do
    base=$(basename "$path")
    [[ "$base" == SHA256SUMS.txt ]] && continue
    [[ "$base" == WaveHub* ]] || continue
    if [[ -n "${found[$base]:-}" ]]; then
      cmp -s "$path" "${found[$base]}" || { echo "two different copies of $base" >&2; exit 1; }
    else
      found[$base]="$path"
    fi
  done < <(find "$@" -type f -print)

  local name
  while IFS= read -r name; do
    if [[ -n "${found[$name]:-}" ]]; then
      cp -f "${found[$name]}" "$dest/$name"
    fi
  done < <(expected_names "$v")

  local alias src
  while read -r alias src; do
    [[ -n "$alias" ]] || continue
    if [[ ! -f "$dest/$alias" && -f "$dest/$src" ]]; then
      cp -f "$dest/$src" "$dest/$alias"
    fi
    if [[ -f "$dest/$alias" && -f "$dest/$src" ]] && ! cmp -s "$dest/$alias" "$dest/$src"; then
      echo "$alias is not the same file as $src" >&2
      exit 1
    fi
  done < <(aliases "$v")
}

cmd_sums() {
  local v="$1" dir="$2"
  version_ok "$v"
  [[ -d "$dir" ]] || { echo "no directory $dir" >&2; exit 1; }
  local name tmp
  tmp=$(mktemp)
  while IFS= read -r name; do
    [[ -s "$dir/$name" ]] || { echo "cannot checksum missing $name" >&2; rm -f "$tmp"; exit 1; }
    (cd "$dir" && sha256sum "$name") >> "$tmp"
  done < <(expected_names "$v")
  sort -k2 "$tmp" > "$dir/SHA256SUMS.txt"
  rm -f "$tmp"
}

cmd_check() {
  local v="$1" dir="$2"
  version_ok "$v"
  [[ -d "$dir" ]] || { echo "no directory $dir" >&2; exit 1; }
  local name path base bad=0
  while IFS= read -r name; do
    if [[ ! -s "$dir/$name" ]]; then
      echo "missing $name" >&2
      bad=1
    fi
  done < <(expected_names "$v")
  while IFS= read -r path; do
    base=$(basename "$path")
    [[ "$base" == SHA256SUMS.txt ]] && continue
    if ! is_expected "$v" "$base"; then
      echo "unexpected $base" >&2
      bad=1
    fi
  done < <(find "$dir" -type f -print)
  local alias src
  while read -r alias src; do
    [[ -n "$alias" ]] || continue
    if [[ -f "$dir/$alias" && -f "$dir/$src" ]] && ! cmp -s "$dir/$alias" "$dir/$src"; then
      echo "$alias is not the same file as $src" >&2
      bad=1
    fi
  done < <(aliases "$v")
  if [[ -f "$dir/SHA256SUMS.txt" ]]; then
    (cd "$dir" && sha256sum -c SHA256SUMS.txt) || bad=1
    local lines names
    lines=$(grep -cve '^$' "$dir/SHA256SUMS.txt" || true)
    names=$(expected_names "$v" | grep -cve '^$' || true)
    if [[ "$lines" != "$names" ]]; then
      echo "checksum list has $lines lines, the matrix has $names" >&2
      bad=1
    fi
  else
    echo "missing SHA256SUMS.txt" >&2
    bad=1
  fi
  [[ "$bad" == 0 ]]
}

usage() {
  echo "usage: release-matrix.sh expected|stage|sums|check VERSION ..." >&2
  exit 2
}

[[ "$#" -ge 1 ]] || usage
cmd="$1"
shift
case "$cmd" in
  expected) [[ "$#" == 1 ]] || usage; cmd_expected "$1" ;;
  stage) [[ "$#" -ge 3 ]] || usage; cmd_stage "$@" ;;
  sums) [[ "$#" == 2 ]] || usage; cmd_sums "$1" "$2" ;;
  check) [[ "$#" == 2 ]] || usage; cmd_check "$1" "$2" ;;
  *) usage ;;
esac
