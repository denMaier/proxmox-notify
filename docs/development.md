# Development

## Local Checks

Run the default smoke test:

```sh
make test
```

Run the full local CI target:

```sh
make ci
```

The smoke test uses temporary paths for cluster state, config, and runtime locks.
It does not touch `/etc/pve`.

## Installed E2E

After installing the package into a disposable Debian or Proxmox-like machine:

```sh
tests/installed-e2e.sh
```

This test still redirects cluster paths into `/tmp`; it verifies the installed
`/usr/local/bin/proxmox-notify` and library paths.

## Nix

Enter a development shell:

```sh
nix develop
```

Build the Nix package:

```sh
nix build
```

Build the Debian package through Nix:

```sh
nix build .#deb
```

Run flake checks:

```sh
nix flake check
```

## Release Flow

CI builds a `.deb` on every push, pull request, and `v*` tag. It also checks
the Nix flake, builds the Nix package, and builds the `.deb` through Nix.

For a release, tag the commit:

```sh
git tag v0.1.0
git push origin main v0.1.0
```

The tag build uses the tag name as the Debian package version.
