# Architecture

`proxmox-notify` uses the Proxmox cluster filesystem as a small replicated
state store. It is not an event stream. Events are wake-ups; files are truth.

## Storage

Cluster state lives under `/etc/pve/proxmox-notify`:

```text
/etc/pve/proxmox-notify/
└── <nodename>/
    ├── announcements.toml
    └── manifests/
        └── <namespace>.toml
```

Every file has one writer: the owning node. Other nodes only read it.

## Write behavior

The implementation keeps pmxcfs writes narrow:

- `announce` writes only when `node`, `publishes`, or `subscribes` changed.
- `publish` writes only when `namespace`, `node`, or `payload` changed.
- `delete` does nothing when the manifest is already absent.
- `agent`, `reconcile`, `get`, `list-nodes`, and `list-manifests` do not write
  pmxcfs except for `agent` calling the no-op-aware announce path.

Timestamps are metadata for changed writes; they do not force no-op updates.

All normal pmxcfs writes are atomic: write a temporary file next to the final
path, then rename it into place.

## Agent

Systemd supervises one long-running agent:

```text
proxmox-notify agent
```

The agent announces the local node, reads local config on every cycle, and
reconciles every namespace listed in `subscribes`. It sleeps for
`reconcile_interval` between cycles. Polling is the correctness path; filesystem
watches are intentionally not required because pmxcfs changes may originate on
another node.

## Locks

Reconcile locks are local runtime state under `/run/proxmox-notify`, not under
`/etc/pve`.

For a namespace:

- the active lock is `/run/proxmox-notify/<namespace>.lock`
- the coalesced rerun marker is `/run/proxmox-notify/<namespace>.rerun`

If a reconcile is already running, another trigger sets the rerun marker and
exits successfully. The active reconcile runs the handler once more before
releasing the lock. That gives one active run plus at most one queued rerun per
burst. The agent normally reconciles namespaces sequentially, but the lock still
protects against manual `reconcile` invocations or overlapping service restarts.

## Handler contract

A handler is any executable. It receives exactly one argument:

```text
argv[1] = namespace
```

Handlers should read current cluster state with:

```sh
proxmox-notify list-manifests --namespace <namespace>
```

Handlers must be idempotent. They should no-op quickly when local state already
matches peer manifests.
