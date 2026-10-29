# Wave Hub as a Flatpak

`io.github.postmanpat_303.WaveHub.yml` builds Wave Hub from its git tag with no network,
using the Rust SDK and `cargo-sources.json` (the crates from `Cargo.lock`, vendored).

## Build a bundle

```sh
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y flathub org.freedesktop.Sdk//24.08 org.freedesktop.Sdk.Extension.rust-stable//24.08

flatpak-builder --user --force-clean --repo=repo build-dir io.github.postmanpat_303.WaveHub.yml
flatpak build-bundle repo WaveHub-0.2.8-linux-x86_64.flatpak io.github.postmanpat_303.WaveHub
flatpak install --user WaveHub-0.2.8-linux-x86_64.flatpak
```

The GitHub workflow `.github/workflows/flatpak.yml` does the same and, given a release tag,
attaches the bundle to that release.

## For a new version

1. Change `tag`/`commit` in the manifest to the new `vX.Y.Z` tag and its commit.
2. If `Cargo.lock` changed, regenerate the sources:
   `python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json`
   (from <https://github.com/flatpak/flatpak-builder-tools>).

## Flathub

Flathub has no Wave Hub listing yet. A listing is a separate submission: it needs Flathub's
build to pass its own checks and a human review, and it is created on the Flathub side (not
from this repo). The manifest here is the one to submit; the app id
`io.github.postmanpat_303.WaveHub` has to stay the same forever once a listing exists.

One thing to know before submitting: a Flatpak runs in a sandbox. Wave Hub's job is to
download and launch other programs, and programs that need something the sandbox doesn't
provide (FUSE for AppImages, their own system libraries) may not start from inside it.
