#!/usr/bin/env bash
# Decides whether a release run may change GitHub, before any upload.
#
#   release-gate.sh MODE CONFIRM TAG IS_DRAFT IS_LATEST
#
# Prints one word on success:
#   none            validate: do not touch the release
#   upload          update-draft: replace files on that draft only
#   upload-publish  publish: replace files, then make it Latest
#
# Anything else exits 1. Latest is left alone unless the word is upload-publish.
set -euo pipefail

[[ "$#" == 5 ]] || { echo "usage: release-gate.sh MODE CONFIRM TAG IS_DRAFT IS_LATEST" >&2; exit 2; }
mode="$1"
confirm="$2"
tag="$3"
is_draft="$4"
is_latest="$5"

[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "tag must look like v1.2.3" >&2; exit 1; }
[[ "$is_draft" == true || "$is_draft" == false ]] || { echo "is_draft must be true or false" >&2; exit 1; }
[[ "$is_latest" == true || "$is_latest" == false ]] || { echo "is_latest must be true or false" >&2; exit 1; }

case "$mode" in
  validate)
    echo none
    ;;
  update-draft)
    [[ "$confirm" == "$tag" ]] || { echo "update-draft requires confirm to be $tag" >&2; exit 1; }
    [[ "$is_draft" == true ]] || { echo "update-draft only changes a draft, and $tag is published" >&2; exit 1; }
    [[ "$is_latest" == false ]] || { echo "refusing to modify the current Latest release" >&2; exit 1; }
    echo upload
    ;;
  publish)
    [[ "$confirm" == "$tag" ]] || { echo "publish requires confirm to be $tag" >&2; exit 1; }
    if [[ "$is_draft" != true && "$is_latest" != true ]]; then
      echo "refusing to make an older published release Latest" >&2
      exit 1
    fi
    echo upload-publish
    ;;
  *)
    echo "unknown mode $mode" >&2
    exit 1
    ;;
esac
