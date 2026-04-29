# Operations

## Install

Build a Debian package:

```sh
make package
```

Install the generated `.deb` on every Proxmox node:

```sh
dpkg -i build/proxmox-notify_0.1.0_$(dpkg --print-architecture).deb
```

The package installs:

- `/usr/local/bin/proxmox-notify`
- `/usr/local/lib/systemd/system/proxmox-notify-agent.service`
- `/etc/proxmox-notify/config.toml`

## Configure

Edit `/etc/proxmox-notify/config.toml` on each node:

```toml
reconcile_interval = "60s"

publishes = ["cluster-folder-git"]
subscribes = ["cluster-folder-git"]

[handlers]
cluster-folder-git = "/usr/local/bin/cluster-folder-git-handler"
```

Then enable the agent:

```sh
systemctl enable --now proxmox-notify-agent.service
```

Add a subscription from the CLI if you do not want to edit TOML directly:

```sh
proxmox-notify subscribe \
  --namespace cluster-folder-git \
  --handler /usr/local/bin/cluster-folder-git-handler
```

`subscribe` only updates local config. The already-running agent reads config on
each poll cycle and reconciles every namespace listed in `subscribes`.

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

1. Install the package on all nodes.
2. Confirm every node writes an `announcements.toml`.
3. Publish from node A and verify B/C observe the manifest.
4. Stop an agent, publish, restart it, and confirm reconcile catches up.
5. Test quorum loss: publish should fail cleanly on the minority side.
6. Trigger rapid publishes and confirm handlers run at most twice per burst.
7. Corrupt one manifest and confirm other manifests still list cleanly.
