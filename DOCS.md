# proxmox-notify docs

This repository builds a small Proxmox cluster notification primitive. The
runtime contract is intentionally simple: node-owned TOML files in pmxcfs,
local locks in `/run`, and project-owned handlers that reconcile current state.

## Start here

- [Architecture](docs/architecture.md): design constraints, data model, and write behavior.
- [Operations](docs/operations.md): installation, configuration, commands, and cluster validation.
- [Development](docs/development.md): local tests, Nix workflow, package builds, and CI.

## Quick links

- Build a Debian package: `make package`
- Run local checks: `make ci`
- Enter a Nix dev shell: `nix develop`
- Build with Nix: `nix build`
- Build the `.deb` with Nix: `nix build .#deb`
