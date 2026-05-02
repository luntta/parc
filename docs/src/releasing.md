---
layout: layouts/doc.njk
title: Releasing
eyebrow: Reference · §10
---

parc releases are tag-driven. GitHub Actions builds and publishes artifacts with `dist` when a SemVer tag is pushed.

## Before tagging

Update the package versions, then run the local checks:

```bash
cargo fmt --check
cargo test --workspace --no-default-features
dist plan
```

`dist plan` should list both release apps:

- `parc-cli`, which installs the `parc` binary
- `parc-server`, which installs the standalone `parc-server` binary

## Publish

Push a SemVer tag for the version you want to release:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The `Release` workflow creates the GitHub release and uploads platform archives, shell installers, PowerShell installers, and SHA-256 checksum files. Pull requests run the workflow in plan-only mode; only tag pushes publish.

Protect `v*` tags in the GitHub repository settings so release tags cannot be created or moved casually. The workflow uses the repository `GITHUB_TOKEN`; do not add a personal access token unless a future release target actually requires one.

## Installers

The latest CLI installer URLs are:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/luntta/parc/releases/latest/download/parc-cli-installer.sh | sh
```

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/luntta/parc/releases/latest/download/parc-cli-installer.ps1 | iex"
```

The standalone server installers use `parc-server-installer.sh` and `parc-server-installer.ps1`.

## Update Checks

`parc update check` queries the latest GitHub release for `luntta/parc`. That means the newest published GitHub release tag is the source of truth for update availability.
