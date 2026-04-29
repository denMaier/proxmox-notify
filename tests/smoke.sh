#!/usr/bin/env bash
set -euo pipefail

repo_root="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf -- "$tmp_dir"' EXIT

export PROXMOX_NOTIFY_CLUSTER_ROOT="${tmp_dir}/pve/proxmox-notify"
export PROXMOX_NOTIFY_CONFIG="${tmp_dir}/etc/proxmox-notify/config.toml"
export PROXMOX_NOTIFY_RUN_DIR="${tmp_dir}/run/proxmox-notify"
export PROXMOX_NOTIFY_NODE_NAME="pve-01"
export PROXMOX_NOTIFY_LIB_DIR="${repo_root}/lib/proxmox-notify"
export PROXMOX_NOTIFY_HELPER="${repo_root}/lib/proxmox-notify/helpers/toml.py"

mkdir -p -- "$(dirname -- "$PROXMOX_NOTIFY_CONFIG")" "$tmp_dir/bin"

cat >"$PROXMOX_NOTIFY_CONFIG" <<'TOML'
reconcile_interval = "60s"
publishes = ["demo"]
subscribes = ["demo"]

[handlers]
demo = "__HANDLER__"
TOML

handler="${tmp_dir}/bin/handler"
sed -i.bak "s#__HANDLER__#${handler}#" "$PROXMOX_NOTIFY_CONFIG"
rm -f -- "${PROXMOX_NOTIFY_CONFIG}.bak"

cat >"$handler" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >>"${PROXMOX_NOTIFY_TEST_HANDLER_LOG}"
SH
chmod +x "$handler"
export PROXMOX_NOTIFY_TEST_HANDLER_LOG="${tmp_dir}/handler.log"

payload="${tmp_dir}/payload.toml"
cat >"$payload" <<'TOML'
revision = "abc123"
enabled = true
peers = ["pve-02", "pve-03"]
TOML

"${repo_root}/bin/proxmox-notify" announce
test -f "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/announcements.toml"
announcement_before="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/announcements.toml")"
sleep 1
"${repo_root}/bin/proxmox-notify" announce
announcement_after="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/announcements.toml")"
test "$announcement_before" = "$announcement_after"

"${repo_root}/bin/proxmox-notify" publish --namespace demo --payload-file "$payload"
test -f "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml"
manifest_before="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml")"
sleep 1
"${repo_root}/bin/proxmox-notify" publish --namespace demo --payload-file "$payload"
manifest_after="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml")"
test "$manifest_before" = "$manifest_after"

"${repo_root}/bin/proxmox-notify" get --namespace demo --node pve-01 >/dev/null
"${repo_root}/bin/proxmox-notify" list-manifests --namespace demo | python3 -m json.tool >/dev/null
"${repo_root}/bin/proxmox-notify" list-nodes | python3 -m json.tool >/dev/null

"${repo_root}/bin/proxmox-notify" reconcile --namespace demo
grep -qx demo "$PROXMOX_NOTIFY_TEST_HANDLER_LOG"

if "${repo_root}/bin/proxmox-notify" publish --namespace ../bad --payload-file "$payload" 2>/dev/null; then
  printf 'invalid namespace was accepted\n' >&2
  exit 1
fi

"${repo_root}/bin/proxmox-notify" delete --namespace demo
test ! -e "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml"
