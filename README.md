# proxmox-notify

`proxmox-notify` is a small Rust CLI for node-to-node state announcements on a
Proxmox cluster. It uses pmxcfs at `/etc/pve/proxmox-notify` as the only
transport.

The model is intentionally narrow:

- each node writes only files below `/etc/pve/proxmox-notify/<nodename>/`
- writes are atomic `*.tmp` writes followed by rename
- no-op announce/publish calls avoid touching pmxcfs
- handlers reconcile current state, not individual events
- per-namespace reconciles are single-flight with one coalesced rerun
- periodic timers provide correctness when watch events are missed

## Documentation

- [DOCS.md](DOCS.md): documentation index
- [Architecture](docs/architecture.md): storage model, write behavior, and locks
- [Operations](docs/operations.md): install, configure, publish, and validate
- [Development](docs/development.md): tests, Nix, packaging, and CI

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

`reconcile_interval` is used by `proxmox-notify subscribe` when it writes the
systemd timer drop-in for a namespace. The packaged timer default is also 60s.

## Commands

```sh
proxmox-notify announce
proxmox-notify publish --namespace cluster-folder-git --payload-file payload.toml
proxmox-notify get --namespace cluster-folder-git --node pve-01
proxmox-notify list-manifests --namespace cluster-folder-git
proxmox-notify list-nodes
proxmox-notify delete --namespace cluster-folder-git
proxmox-notify reconcile --namespace cluster-folder-git
proxmox-notify subscribe --namespace cluster-folder-git --handler /usr/local/bin/handler
proxmox-notify prune-nodes --older-than 30d
```

`list-manifests` and `list-nodes` print JSON. Manifest and announcement files
stored in pmxcfs are TOML.

## Packaging

Build a simple Debian package:

```sh
make package
```

Install directly into a staging root:

```sh
make install DESTDIR=/tmp/proxmox-notify-root
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

Installed-package e2e test, intended for a disposable Debian/Proxmox-like VM
after installing the package:

```sh
tests/installed-e2e.sh
```

The real acceptance test is still a Proxmox cluster test: install on all nodes,
publish and delete manifests, interrupt watch units, test a quorum loss, and
confirm periodic reconcile converges.
