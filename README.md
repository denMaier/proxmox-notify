# proxmox-notify

`proxmox-notify` is a small Rust CLI for node-to-node state announcements on a
Proxmox cluster. It uses pmxcfs at `/etc/pve/proxmox-notify` as the only
transport.

The model is intentionally narrow:

- each node writes only files below `/etc/pve/proxmox-notify/<nodename>/`
- writes are atomic `*.tmp` writes followed by rename
- no-op announce/publish calls avoid touching pmxcfs
- handlers reconcile current state, not individual events
- handlers receive environment that exposes local degraded/minority state
- per-namespace reconciles are single-flight with one coalesced rerun
- one long-running agent polls subscriptions for correctness

## Documentation

- [DOCS.md](DOCS.md): documentation index
- [Architecture](docs/architecture.md): storage model, write behavior, and locks
- [Operations](docs/operations.md): install, configure, publish, and validate
- [Development](docs/development.md): tests, Nix, release binaries, and CI

## Files

- CLI: `/usr/local/bin/proxmox-notify`
- Local config: `/etc/proxmox-notify/config.toml`
- Cluster state: `/etc/pve/proxmox-notify/`
- Runtime locks: `/run/proxmox-notify/`

## Configuration

```toml
reconcile_interval = "60s"

publishes = ["cluster-folder-git"]
subscribes = ["cluster-folder-git"]

[handlers]
cluster-folder-git = "/usr/local/bin/cluster-folder-git-handler"
```

`reconcile_interval` controls the polling interval used by
`proxmox-notify agent`. Polling is the correctness path; filesystem watches are
not required.

## Commands

```sh
proxmox-notify announce
proxmox-notify publish --namespace cluster-folder-git --payload-file payload.toml
proxmox-notify get --namespace cluster-folder-git --node pve-01
proxmox-notify list-manifests --namespace cluster-folder-git
proxmox-notify list-nodes
proxmox-notify delete --namespace cluster-folder-git
proxmox-notify reconcile --namespace cluster-folder-git
proxmox-notify agent
proxmox-notify install
proxmox-notify uninstall
```

`list-manifests` and `list-nodes` print JSON. Manifest and announcement files
stored in pmxcfs are TOML.

## Install

Build a release binary:

```sh
make package
```

Install onto a host and insert the long-running agent unit:

```sh
artifact="$(make package | tail -n 1)"
sudo "$artifact" install --enable-now
```

The install command copies the running binary to `/usr/local/bin/proxmox-notify`,
creates `/etc/proxmox-notify/config.toml` if missing, writes the systemd unit,
and reloads systemd. For deployment automation that already placed the binary:

```sh
install -m 0755 proxmox-notify /usr/local/bin/proxmox-notify
/usr/local/bin/proxmox-notify install --enable-now
```

Build an optional Debian package:

```sh
make deb
```

Use Nix:

```sh
nix develop
nix build
```

## Verification

Local smoke test:

```sh
make test
```

Installed-binary e2e test, intended for a disposable Debian/Proxmox-like VM
after installing the binary:

```sh
tests/installed-e2e.sh
```

The real acceptance test is still a Proxmox cluster test: install on all nodes,
publish and delete manifests, stop/restart the agent, test a quorum loss, and
confirm polling reconcile converges.
