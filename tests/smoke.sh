#!/usr/bin/env bash
set -euo pipefail

repo_root="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf -- "$tmp_dir"' EXIT

export PROXMOX_NOTIFY_CLUSTER_ROOT="${tmp_dir}/pve/proxmox-notify"
export PROXMOX_NOTIFY_CONFIG="${tmp_dir}/etc/proxmox-notify/config.toml"
export PROXMOX_NOTIFY_RUN_DIR="${tmp_dir}/run/proxmox-notify"
export PROXMOX_NOTIFY_NODE_NAME="pve-01"
bin="${PROXMOX_NOTIFY_BIN:-${repo_root}/target/debug/proxmox-notify}"

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

{
  printf '#!%s\n' "$(command -v bash)"
  cat <<'SH'
set -euo pipefail
printf '%s\n' "$1" >>"${PROXMOX_NOTIFY_TEST_HANDLER_LOG}"
SH
} >"$handler"
chmod +x "$handler"
export PROXMOX_NOTIFY_TEST_HANDLER_LOG="${tmp_dir}/handler.log"

payload="${tmp_dir}/payload.toml"
cat >"$payload" <<'TOML'
revision = "abc123"
enabled = true
peers = ["pve-02", "pve-03"]
TOML

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
"$bin" list-manifests --namespace demo | python3 -m json.tool >/dev/null
"$bin" list-nodes | python3 -m json.tool >/dev/null

"$bin" reconcile --namespace demo
grep -qx demo "$PROXMOX_NOTIFY_TEST_HANDLER_LOG"

>"$PROXMOX_NOTIFY_TEST_HANDLER_LOG"
"$bin" agent --once
grep -qx demo "$PROXMOX_NOTIFY_TEST_HANDLER_LOG"

install_root="${tmp_dir}/install-root"
"$bin" install --destdir "$install_root" --prefix /usr/local --sysconfdir /etc --no-systemctl
test -x "${install_root}/usr/local/bin/proxmox-notify"
test -f "${install_root}/etc/proxmox-notify/config.toml"
test -f "${install_root}/usr/local/lib/systemd/system/proxmox-notify-agent.service"
grep -qx 'ExecStart=/usr/local/bin/proxmox-notify agent' "${install_root}/usr/local/lib/systemd/system/proxmox-notify-agent.service"

printf '# keep me\n' >"${install_root}/etc/proxmox-notify/config.toml"
"$bin" install --destdir "$install_root" --prefix /usr/local --sysconfdir /etc --no-systemctl --no-binary --no-systemd-unit
grep -qx '# keep me' "${install_root}/etc/proxmox-notify/config.toml"

"$bin" uninstall --destdir "$install_root" --prefix /usr/local --sysconfdir /etc --no-systemctl
test ! -e "${install_root}/usr/local/lib/systemd/system/proxmox-notify-agent.service"
test -x "${install_root}/usr/local/bin/proxmox-notify"
test -f "${install_root}/etc/proxmox-notify/config.toml"

"$bin" uninstall --destdir "$install_root" --prefix /usr/local --sysconfdir /etc --no-systemctl --remove-binary --purge-config
test ! -e "${install_root}/usr/local/bin/proxmox-notify"
test ! -e "${install_root}/etc/proxmox-notify/config.toml"

if "$bin" publish --namespace ../bad --payload-file "$payload" 2>/dev/null; then
  printf 'invalid namespace was accepted\n' >&2
  exit 1
fi

"$bin" delete --namespace demo
test ! -e "${PROXMOX_NOTIFY_CLUSTER_ROOT}/pve-01/manifests/demo.toml"
