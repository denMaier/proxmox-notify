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

After installing the binary into a disposable Debian or Proxmox-like machine:

```sh
tests/installed-e2e.sh
```

This test redirects cluster paths into `/tmp`; it verifies the installed
`/usr/local/bin/proxmox-notify` binary without touching `/etc/pve`.

The smoke test also exercises `proxmox-notify install` and `uninstall` against a
temporary `--destdir`, including systemd unit rendering and config preservation.

## Nix

Enter a development shell:

```sh
nix develop
```

Build the Nix package:

```sh
nix build
```

Run flake checks:

```sh
nix flake check
```

## Release Flow

CI builds a Linux release binary inside a Debian Bookworm container on every
push, pull request, and `v*` tag. It installs that binary into a temporary root
with `proxmox-notify install --destdir ...`, runs the installed end-to-end test,
and uploads the binary artifact. CI also checks the Nix flake and builds the Nix
package.

The optional Debian package remains available through `make deb`. It stages the
binary and config, then uses maintainer scripts to call the CLI-managed install
and uninstall paths for the agent unit.

For a release, tag the commit:

```sh
git tag v0.1.1
git push origin main v0.1.1
```

The tag build uses the tag name as the binary artifact version and publishes the
binary to the matching GitHub Release with `gh`.
