# Operations

## Install

Preferred install path: place one release binary on the node, then let the
binary install itself and the long-running agent unit.

To build that binary locally, run `make package`; it prints the artifact path
under `build/`.

```sh
install -m 0755 proxmox-notify /usr/local/bin/proxmox-notify
/usr/local/bin/proxmox-notify install --enable-now
```

When installing from a freshly built or downloaded binary, running it directly is
enough; `install` copies the current executable into `/usr/local/bin`:

```sh
sudo ./proxmox-notify install --enable-now
```

The install command manages:

- `/usr/local/bin/proxmox-notify`
- `/usr/local/lib/systemd/system/proxmox-notify-agent.service`
- `/etc/proxmox-notify/config.toml`

It preserves an existing config file. Omit `--enable-now` if the unit should be
installed but not started.

To remove only the managed systemd unit and stop the service:

```sh
sudo /usr/local/bin/proxmox-notify uninstall
```

Add `--remove-binary --purge-config` when intentionally deleting the binary and
local config as well.

A Debian package can still be built with `make deb`, but direct binary install
is the canonical Proxmox deployment path.

## Configure

Edit `/etc/proxmox-notify/config.toml` on each node:

```toml
reconcile_interval = "60s"

publishes = ["cluster-folder-git"]
subscribes = ["cluster-folder-git"]

[handlers]
cluster-folder-git = "/usr/local/bin/cluster-folder-git-handler"
```

If the install command was run without `--enable-now`, enable the agent after
configuration:

```sh
systemctl enable --now proxmox-notify-agent.service
```

The already-running agent rereads `/etc/proxmox-notify/config.toml` on every
poll cycle and reconciles every namespace listed in `subscribes`. Add or remove
subscriptions by editing that file directly; no restart is required.

Stale per-node directories under `/etc/pve/proxmox-notify/<nodename>/` are an
admin-managed concern. When a node is permanently decommissioned, remove its
directory by hand from any cluster member.

## Publish

Payload files are TOML tables. Their content becomes the manifest `[payload]`
table.

```sh
proxmox-notify publish \
  --namespace cluster-folder-git \
  --payload-file /path/to/payload.toml
```

Publishing fails if the namespace is not listed in local `publishes`.

## Inspect

```sh
proxmox-notify list-nodes
proxmox-notify list-manifests --namespace cluster-folder-git
proxmox-notify get --namespace cluster-folder-git --node pve-01
```

List commands print JSON. Stored files remain TOML.

## Validate On A Cluster

Before relying on it, test on a real three-node Proxmox cluster:

1. Install the binary on all nodes.
2. Confirm every node writes an `announcements.toml`.
3. Publish from node A and verify B/C observe the manifest.
4. Stop an agent, publish, restart it, and confirm reconcile catches up.
5. Test quorum loss: publish should fail cleanly on the minority side.
6. Trigger rapid publishes and confirm handlers run at most twice per burst.
7. Corrupt one manifest and confirm other manifests still list cleanly.
