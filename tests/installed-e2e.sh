#!/usr/bin/env bash
set -euo pipefail

root="${PROXMOX_NOTIFY_E2E_ROOT:-/tmp/proxmox-notify-installed-e2e}"
bin="${PROXMOX_NOTIFY_BIN:-/usr/local/bin/proxmox-notify}"

rm -rf -- "$root"
mkdir -p -- "$root"

export PROXMOX_NOTIFY_CLUSTER_ROOT="${root}/pve/proxmox-notify"
export PROXMOX_NOTIFY_CONFIG="${root}/etc/proxmox-notify/config.toml"
export PROXMOX_NOTIFY_RUN_DIR="${root}/run/proxmox-notify"
export PROXMOX_NOTIFY_NODE_NAME="pve-01"

mkdir -p -- "$(dirname -- "$PROXMOX_NOTIFY_CONFIG")"

handler="${root}/handler.sh"
handler_log="${root}/handler.log"
payload="${root}/payload.toml"

{
  printf '#!%s\n' "$(command -v bash)"
  cat <<'SH'
set -euo pipefail
printf '%s\n' "$1" >>"${PROXMOX_NOTIFY_E2E_HANDLER_LOG}"
if [[ -n "${PROXMOX_NOTIFY_E2E_HANDLER_SLEEP:-}" ]]; then
  sleep "$PROXMOX_NOTIFY_E2E_HANDLER_SLEEP"
fi
SH
} >"$handler"
chmod +x "$handler"

cat >"$PROXMOX_NOTIFY_CONFIG" <<TOML
reconcile_interval = "60s"
publishes = ["demo"]
subscribes = ["demo"]

[handlers]
demo = "${handler}"
TOML

cat >"$payload" <<'TOML'
revision = "abc123"
enabled = true

[source]
node = "pve-01"
path = "/srv/demo"
TOML

export PROXMOX_NOTIFY_E2E_HANDLER_LOG="$handler_log"

"$bin" announce
test -f "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/announcements.toml"
announcement_before="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/announcements.toml")"
sleep 1
"$bin" announce
announcement_after="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/announcements.toml")"
test "$announcement_before" = "$announcement_after"

"$bin" publish --namespace demo --payload-file "$payload"
test -f "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml"
manifest_before="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml")"
sleep 1
"$bin" publish --namespace demo --payload-file "$payload"
manifest_after="$(cat "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml")"
test "$manifest_before" = "$manifest_after"

"$bin" get --namespace demo --node pve-01 >/dev/null
"$bin" list-nodes | python3 -m json.tool >/dev/null
"$bin" list-manifests --namespace demo | python3 -m json.tool >/dev/null

"$bin" reconcile --namespace demo
grep -qx demo "$handler_log"

export PROXMOX_NOTIFY_NODE_NAME="pve-02"
"$bin" announce
"$bin" publish --namespace demo --payload-file "$payload"
manifest_count="$("$bin" list-manifests --namespace demo | python3 -c 'import json, sys; print(len(json.load(sys.stdin)))')"
test "$manifest_count" = "2"

>"$handler_log"
export PROXMOX_NOTIFY_NODE_NAME="pve-01"
export PROXMOX_NOTIFY_E2E_HANDLER_SLEEP="1"
"$bin" reconcile --namespace demo &
pid="$!"

while [[ ! -s "$handler_log" ]]; do
  sleep 0.05
done

"$bin" reconcile --namespace demo
wait "$pid"

unset PROXMOX_NOTIFY_E2E_HANDLER_SLEEP
run_count="$(wc -l <"$handler_log" | tr -d ' ')"
test "$run_count" = "2"

"$bin" delete --namespace demo
test ! -e "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml"

if "$bin" publish --namespace ../bad --payload-file "$payload" 2>/dev/null; then
  printf 'invalid namespace was accepted\n' >&2
  exit 1
fi
