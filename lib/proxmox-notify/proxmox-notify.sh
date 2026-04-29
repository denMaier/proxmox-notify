#!/usr/bin/env bash

if [[ -z "${PROXMOX_NOTIFY_LIB_DIR:-}" ]]; then
  _proxmox_notify_source="${BASH_SOURCE[0]}"
  PROXMOX_NOTIFY_LIB_DIR="$(CDPATH= cd -- "$(dirname -- "${_proxmox_notify_source}")" && pwd)"
fi

: "${PROXMOX_NOTIFY_CLUSTER_ROOT:=/etc/pve/proxmox-notify}"
: "${PROXMOX_NOTIFY_CONFIG:=/etc/proxmox-notify/config.toml}"
: "${PROXMOX_NOTIFY_RUN_DIR:=/run/proxmox-notify}"
: "${PROXMOX_NOTIFY_HELPER:=${PROXMOX_NOTIFY_LIB_DIR}/helpers/toml.py}"

_proxmox_notify_err() {
  printf 'proxmox-notify: %s\n' "$*" >&2
}

_proxmox_notify_die() {
  _proxmox_notify_err "$*"
  return 1
}

_proxmox_notify_node_name() {
  if [[ -n "${PROXMOX_NOTIFY_NODE_NAME:-}" ]]; then
    printf '%s\n' "$PROXMOX_NOTIFY_NODE_NAME"
  else
    hostname -s
  fi
}

_proxmox_notify_validate_component() {
  local kind="$1"
  local value="$2"

  if [[ -z "$value" ]]; then
    _proxmox_notify_die "$kind must not be empty"
    return 1
  fi
  if ((${#value} > 128)); then
    _proxmox_notify_die "$kind is too long: $value"
    return 1
  fi
  if [[ ! "$value" =~ ^[A-Za-z0-9_.-]+$ ]]; then
    _proxmox_notify_die "$kind contains unsafe characters: $value"
    return 1
  fi
  if [[ "$value" == -* ]]; then
    _proxmox_notify_die "$kind must not start with '-': $value"
    return 1
  fi
  if [[ "$value" =~ ^[.]+$ ]]; then
    _proxmox_notify_die "$kind must not be dots only: $value"
    return 1
  fi
}

_proxmox_notify_helper() {
  python3 "$PROXMOX_NOTIFY_HELPER" "$@"
}

_proxmox_notify_atomic_command() {
  local dest="$1"
  shift
  local dir tmp rc

  dir="$(dirname -- "$dest")"
  mkdir -p -- "$dir" || return 1
  tmp="${dest}.tmp.$$"
  rm -f -- "$tmp"

  if "$@" >"$tmp"; then
    mv -f -- "$tmp" "$dest"
  else
    rc=$?
    rm -f -- "$tmp"
    return "$rc"
  fi
}

_proxmox_notify_atomic_text() {
  local dest="$1"
  local text="$2"
  local dir tmp

  dir="$(dirname -- "$dest")"
  mkdir -p -- "$dir" || return 1
  tmp="${dest}.tmp.$$"
  rm -f -- "$tmp"

  if printf '%s' "$text" >"$tmp"; then
    mv -f -- "$tmp" "$dest"
  else
    local rc=$?
    rm -f -- "$tmp"
    return "$rc"
  fi
}

_proxmox_notify_parse_namespace() {
  local ns_var="$1"
  shift
  local parsed_namespace=""

  while (($#)); do
    case "$1" in
      --namespace)
        if (($# < 2)); then
          _proxmox_notify_die '--namespace needs a value'
          return 2
        fi
        parsed_namespace="$2"
        shift 2
        ;;
      *)
        _proxmox_notify_die "unexpected argument: $1"
        return 2
        ;;
    esac
  done

  if [[ -z "$parsed_namespace" ]]; then
    _proxmox_notify_die '--namespace is required'
    return 2
  fi
  _proxmox_notify_validate_component namespace "$parsed_namespace" || return 1
  printf -v "$ns_var" '%s' "$parsed_namespace"
}

_proxmox_notify_instance_name() {
  local namespace="$1"
  if command -v systemd-escape >/dev/null 2>&1; then
    systemd-escape -- "$namespace"
  else
    printf '%s\n' "$namespace"
  fi
}

proxmox_notify_announce() {
  local node dest
  node="$(_proxmox_notify_node_name)" || return 1
  _proxmox_notify_validate_component node "$node" || return 1
  dest="${PROXMOX_NOTIFY_CLUSTER_ROOT}/${node}/announcements.toml"

  if _proxmox_notify_helper announcement-current \
    --config "$PROXMOX_NOTIFY_CONFIG" \
    --node "$node" \
    --existing "$dest"; then
    return 0
  fi

  _proxmox_notify_atomic_command \
    "$dest" \
    _proxmox_notify_helper write-announcement \
      --config "$PROXMOX_NOTIFY_CONFIG" \
      --node "$node"
}

proxmox_notify_publish() {
  local namespace="" payload_file="" node dest

  while (($#)); do
    case "$1" in
      --namespace)
        (($# >= 2)) || { _proxmox_notify_die '--namespace needs a value'; return 2; }
        namespace="$2"
        shift 2
        ;;
      --payload-file)
        (($# >= 2)) || { _proxmox_notify_die '--payload-file needs a value'; return 2; }
        payload_file="$2"
        shift 2
        ;;
      *)
        _proxmox_notify_die "unexpected argument: $1"
        return 2
        ;;
    esac
  done

  [[ -n "$namespace" ]] || { _proxmox_notify_die '--namespace is required'; return 2; }
  [[ -n "$payload_file" ]] || { _proxmox_notify_die '--payload-file is required'; return 2; }
  [[ -r "$payload_file" ]] || { _proxmox_notify_die "payload file is not readable: $payload_file"; return 1; }

  _proxmox_notify_validate_component namespace "$namespace" || return 1
  _proxmox_notify_helper config-allows-publish --config "$PROXMOX_NOTIFY_CONFIG" --namespace "$namespace" || return 1

  node="$(_proxmox_notify_node_name)" || return 1
  _proxmox_notify_validate_component node "$node" || return 1
  dest="${PROXMOX_NOTIFY_CLUSTER_ROOT}/${node}/manifests/${namespace}.toml"

  if _proxmox_notify_helper manifest-current \
    --namespace "$namespace" \
    --node "$node" \
    --payload-file "$payload_file" \
    --existing "$dest"; then
    return 0
  fi

  _proxmox_notify_atomic_command \
    "$dest" \
    _proxmox_notify_helper write-manifest \
      --namespace "$namespace" \
      --node "$node" \
      --payload-file "$payload_file"
}

proxmox_notify_get() {
  local namespace="" node="" path

  while (($#)); do
    case "$1" in
      --namespace)
        (($# >= 2)) || { _proxmox_notify_die '--namespace needs a value'; return 2; }
        namespace="$2"
        shift 2
        ;;
      --node)
        (($# >= 2)) || { _proxmox_notify_die '--node needs a value'; return 2; }
        node="$2"
        shift 2
        ;;
      *)
        _proxmox_notify_die "unexpected argument: $1"
        return 2
        ;;
    esac
  done

  [[ -n "$namespace" ]] || { _proxmox_notify_die '--namespace is required'; return 2; }
  _proxmox_notify_validate_component namespace "$namespace" || return 1

  if [[ -z "$node" ]]; then
    node="$(_proxmox_notify_node_name)" || return 1
  fi
  _proxmox_notify_validate_component node "$node" || return 1

  path="${PROXMOX_NOTIFY_CLUSTER_ROOT}/${node}/manifests/${namespace}.toml"
  [[ -f "$path" ]] || { _proxmox_notify_die "manifest not found: $node/$namespace"; return 1; }
  cat -- "$path"
}

proxmox_notify_list_manifests() {
  local namespace
  _proxmox_notify_parse_namespace namespace "$@" || return $?
  _proxmox_notify_helper list-manifests --root "$PROXMOX_NOTIFY_CLUSTER_ROOT" --namespace "$namespace"
}

proxmox_notify_list_nodes() {
  _proxmox_notify_helper list-nodes --root "$PROXMOX_NOTIFY_CLUSTER_ROOT"
}

proxmox_notify_delete() {
  local namespace node path
  _proxmox_notify_parse_namespace namespace "$@" || return $?

  node="$(_proxmox_notify_node_name)" || return 1
  _proxmox_notify_validate_component node "$node" || return 1
  path="${PROXMOX_NOTIFY_CLUSTER_ROOT}/${node}/manifests/${namespace}.toml"
  [[ -e "$path" ]] || return 0
  rm -f -- "$path"
}

proxmox_notify_reconcile() {
  local namespace handler lock lockdir rerun rc=0 run_rc=0 lock_backend=""
  _proxmox_notify_parse_namespace namespace "$@" || return $?

  handler="$(_proxmox_notify_helper config-handler --config "$PROXMOX_NOTIFY_CONFIG" --namespace "$namespace")" || return 1
  [[ -n "$handler" ]] || { _proxmox_notify_die "no handler configured for namespace: $namespace"; return 1; }
  [[ -x "$handler" ]] || { _proxmox_notify_die "handler is not executable: $handler"; return 1; }

  mkdir -p -- "$PROXMOX_NOTIFY_RUN_DIR" || return 1
  lock="${PROXMOX_NOTIFY_RUN_DIR}/${namespace}.lock"
  lockdir="${lock}.d"
  rerun="${PROXMOX_NOTIFY_RUN_DIR}/${namespace}.rerun"

  if command -v flock >/dev/null 2>&1; then
    exec 9>"$lock"
    if ! flock -n 9; then
      : >"$rerun"
      exec 9>&-
      return 0
    fi
    lock_backend="flock"
  else
    if ! mkdir -- "$lockdir" 2>/dev/null; then
      : >"$rerun"
      return 0
    fi
    lock_backend="mkdir"
  fi

  rm -f -- "$rerun"
  "$handler" "$namespace" || run_rc=$?
  if ((run_rc != 0)); then
    rc="$run_rc"
  fi

  if [[ -f "$rerun" ]]; then
    rm -f -- "$rerun"
    run_rc=0
    "$handler" "$namespace" || run_rc=$?
    if ((run_rc != 0)); then
      rc="$run_rc"
    fi
  fi

  if [[ "$lock_backend" == "mkdir" ]]; then
    rmdir -- "$lockdir"
  elif [[ "$lock_backend" == "flock" ]]; then
    flock -u 9
    exec 9>&-
  fi

  return "$rc"
}

proxmox_notify_subscribe() {
  local namespace="" handler="" instance interval dropin unit_text

  while (($#)); do
    case "$1" in
      --namespace)
        (($# >= 2)) || { _proxmox_notify_die '--namespace needs a value'; return 2; }
        namespace="$2"
        shift 2
        ;;
      --handler)
        (($# >= 2)) || { _proxmox_notify_die '--handler needs a value'; return 2; }
        handler="$2"
        shift 2
        ;;
      *)
        _proxmox_notify_die "unexpected argument: $1"
        return 2
        ;;
    esac
  done

  [[ -n "$namespace" ]] || { _proxmox_notify_die '--namespace is required'; return 2; }
  [[ -n "$handler" ]] || { _proxmox_notify_die '--handler is required'; return 2; }
  _proxmox_notify_validate_component namespace "$namespace" || return 1
  [[ -x "$handler" ]] || { _proxmox_notify_die "handler is not executable: $handler"; return 1; }

  _proxmox_notify_atomic_command \
    "$PROXMOX_NOTIFY_CONFIG" \
    _proxmox_notify_helper subscribe-config \
      --config "$PROXMOX_NOTIFY_CONFIG" \
      --namespace "$namespace" \
      --handler "$handler" || return 1

  command -v systemctl >/dev/null 2>&1 || { _proxmox_notify_die 'systemctl is required to enable subscriptions'; return 1; }

  interval="$(_proxmox_notify_helper config-interval --config "$PROXMOX_NOTIFY_CONFIG")" || return 1
  instance="$(_proxmox_notify_instance_name "$namespace")" || return 1
  dropin="/etc/systemd/system/proxmox-notify-reconcile@${instance}.timer.d/override.conf"
  unit_text="[Timer]
OnUnitActiveSec=
OnUnitActiveSec=${interval}
"
  _proxmox_notify_atomic_text "$dropin" "$unit_text" || return 1

  systemctl daemon-reload
  systemctl enable --now "proxmox-notify-watch@${instance}.path" "proxmox-notify-reconcile@${instance}.timer"
}

proxmox_notify_prune_nodes() {
  local older_than="" node

  while (($#)); do
    case "$1" in
      --older-than)
        (($# >= 2)) || { _proxmox_notify_die '--older-than needs a value'; return 2; }
        older_than="$2"
        shift 2
        ;;
      *)
        _proxmox_notify_die "unexpected argument: $1"
        return 2
        ;;
    esac
  done

  [[ -n "$older_than" ]] || { _proxmox_notify_die '--older-than is required'; return 2; }

  while IFS= read -r node; do
    [[ -n "$node" ]] || continue
    _proxmox_notify_validate_component node "$node" || return 1
    rm -rf -- "${PROXMOX_NOTIFY_CLUSTER_ROOT}/${node}"
    printf '%s\n' "$node"
  done < <(_proxmox_notify_helper prune-candidates --root "$PROXMOX_NOTIFY_CLUSTER_ROOT" --older-than "$older_than")
}
