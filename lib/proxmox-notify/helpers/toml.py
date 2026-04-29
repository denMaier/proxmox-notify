#!/usr/bin/env python3
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import sys
from pathlib import Path
from typing import Any

try:
    import tomllib
except ModuleNotFoundError:  # pragma: no cover - fallback for older Python
    import tomli as tomllib  # type: ignore


SAFE_COMPONENT_RE = re.compile(r"^[A-Za-z0-9_.-]+$")


def die(message: str, code: int = 1) -> None:
    print(f"proxmox-notify: {message}", file=sys.stderr)
    raise SystemExit(code)


def validate_component(kind: str, value: str) -> None:
    if not value:
        die(f"{kind} must not be empty")
    if len(value) > 128:
        die(f"{kind} is too long: {value}")
    if not SAFE_COMPONENT_RE.fullmatch(value):
        die(f"{kind} contains unsafe characters: {value}")
    if value.startswith("-"):
        die(f"{kind} must not start with '-': {value}")
    if set(value) == {"."}:
        die(f"{kind} must not be dots only: {value}")


def load_toml(path: Path, *, required: bool = True) -> dict[str, Any]:
    if not path.exists():
        if required:
            die(f"TOML file not found: {path}")
        return {}
    try:
        with path.open("rb") as fh:
            data = tomllib.load(fh)
    except tomllib.TOMLDecodeError as exc:
        die(f"invalid TOML in {path}: {exc}")
    except OSError as exc:
        die(f"cannot read {path}: {exc}")
    if not isinstance(data, dict):
        die(f"TOML document must be a table: {path}")
    return data


def warn(message: str) -> None:
    print(f"proxmox-notify: warning: {message}", file=sys.stderr)


def utc_now() -> str:
    return dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def as_str_list(config: dict[str, Any], key: str) -> list[str]:
    value = config.get(key, [])
    if value is None:
        return []
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        die(f"{key} must be a list of strings")
    return list(dict.fromkeys(value))


def handlers(config: dict[str, Any]) -> dict[str, str]:
    value = config.get("handlers", {})
    if value is None:
        return {}
    if not isinstance(value, dict):
        die("handlers must be a table")
    result: dict[str, str] = {}
    for key, handler in value.items():
        if not isinstance(key, str) or not isinstance(handler, str):
            die("handlers must map namespace strings to handler path strings")
        result[key] = handler
    return result


def toml_quote_string(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def toml_key(key: str) -> str:
    if re.fullmatch(r"[A-Za-z0-9_-]+", key):
        return key
    return toml_quote_string(key)


def toml_scalar(value: Any) -> str:
    if isinstance(value, str):
        return toml_quote_string(value)
    if isinstance(value, dict):
        items = [f"{toml_key(key)} = {toml_scalar(item)}" for key, item in sorted(value.items())]
        return "{ " + ", ".join(items) + " }"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int) and not isinstance(value, bool):
        return str(value)
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, dt.datetime):
        if value.tzinfo is None:
            return value.isoformat()
        return value.astimezone(dt.UTC).isoformat().replace("+00:00", "Z")
    if isinstance(value, dt.date):
        return value.isoformat()
    if isinstance(value, dt.time):
        return value.isoformat()
    if isinstance(value, list):
        return "[" + ", ".join(toml_scalar(item) for item in value) + "]"
    die(f"unsupported TOML scalar type: {type(value).__name__}")


def split_table_values(table: dict[str, Any]) -> tuple[list[tuple[str, Any]], list[tuple[str, dict[str, Any]]]]:
    scalars: list[tuple[str, Any]] = []
    tables: list[tuple[str, dict[str, Any]]] = []
    for key in sorted(table):
        value = table[key]
        if isinstance(value, dict):
            tables.append((key, value))
        else:
            scalars.append((key, value))
    return scalars, tables


def dump_table(table: dict[str, Any], prefix: list[str] | None = None) -> list[str]:
    prefix = prefix or []
    lines: list[str] = []
    scalars, tables = split_table_values(table)

    if prefix:
        lines.append(f"[{'.'.join(toml_key(part) for part in prefix)}]")
    for key, value in scalars:
        lines.append(f"{toml_key(key)} = {toml_scalar(value)}")
    if scalars and tables:
        lines.append("")

    for index, (key, child) in enumerate(tables):
        if lines and lines[-1] != "":
            lines.append("")
        lines.extend(dump_table(child, [*prefix, key]))
        if index != len(tables) - 1:
            lines.append("")

    return lines


def dumps_toml(table: dict[str, Any]) -> str:
    return "\n".join(dump_table(table)).rstrip() + "\n"


def read_payload(path: Path) -> dict[str, Any]:
    data = load_toml(path)
    return data


def command_write_announcement(args: argparse.Namespace) -> None:
    validate_component("node", args.node)
    config = load_toml(Path(args.config))
    document = {
        "node": args.node,
        "announced_at": utc_now(),
        "publishes": as_str_list(config, "publishes"),
        "subscribes": as_str_list(config, "subscribes"),
    }
    print(dumps_toml(document), end="")


def load_current(path: Path) -> dict[str, Any] | None:
    if not path.exists():
        return None
    try:
        return load_toml(path)
    except SystemExit:
        return None


def command_announcement_current(args: argparse.Namespace) -> None:
    validate_component("node", args.node)
    config = load_toml(Path(args.config))
    current = load_current(Path(args.existing))
    if current is None:
        raise SystemExit(1)

    if current.get("node") != args.node:
        raise SystemExit(1)
    if current.get("publishes") != as_str_list(config, "publishes"):
        raise SystemExit(1)
    if current.get("subscribes") != as_str_list(config, "subscribes"):
        raise SystemExit(1)


def command_write_manifest(args: argparse.Namespace) -> None:
    validate_component("namespace", args.namespace)
    validate_component("node", args.node)
    payload = read_payload(Path(args.payload_file))
    document = {
        "namespace": args.namespace,
        "node": args.node,
        "timestamp": utc_now(),
        "payload": payload,
    }
    print(dumps_toml(document), end="")


def command_manifest_current(args: argparse.Namespace) -> None:
    validate_component("namespace", args.namespace)
    validate_component("node", args.node)
    payload = read_payload(Path(args.payload_file))
    current = load_current(Path(args.existing))
    if current is None:
        raise SystemExit(1)

    if current.get("namespace") != args.namespace:
        raise SystemExit(1)
    if current.get("node") != args.node:
        raise SystemExit(1)
    if current.get("payload") != payload:
        raise SystemExit(1)


def command_config_allows_publish(args: argparse.Namespace) -> None:
    validate_component("namespace", args.namespace)
    config = load_toml(Path(args.config))
    if args.namespace not in as_str_list(config, "publishes"):
        die(f"namespace is not in publishes list: {args.namespace}")


def command_config_handler(args: argparse.Namespace) -> None:
    validate_component("namespace", args.namespace)
    config = load_toml(Path(args.config))
    handler = handlers(config).get(args.namespace)
    if not handler:
        die(f"no handler configured for namespace: {args.namespace}")
    print(handler)


def command_config_interval(args: argparse.Namespace) -> None:
    config = load_toml(Path(args.config))
    interval = config.get("reconcile_interval", "60s")
    if not isinstance(interval, str) or not interval.strip() or "\n" in interval:
        die("reconcile_interval must be a non-empty string")
    print(interval.strip())


def parse_manifest(path: Path, namespace: str) -> dict[str, Any] | None:
    try:
        data = load_toml(path)
    except SystemExit:
        warn(f"skipping invalid manifest: {path}")
        return None
    if data.get("namespace") != namespace:
        warn(f"skipping manifest with mismatched namespace: {path}")
        return None
    node = data.get("node")
    if not isinstance(node, str):
        warn(f"skipping manifest without node field: {path}")
        return None
    try:
        validate_component("node", node)
    except SystemExit:
        warn(f"skipping manifest with unsafe node field: {path}")
        return None
    return data


def command_list_manifests(args: argparse.Namespace) -> None:
    validate_component("namespace", args.namespace)
    root = Path(args.root)
    results: list[dict[str, Any]] = []
    if root.exists():
        for node_dir in sorted(item for item in root.iterdir() if item.is_dir()):
            try:
                validate_component("node", node_dir.name)
            except SystemExit:
                warn(f"skipping unsafe node directory: {node_dir}")
                continue
            manifest_path = node_dir / "manifests" / f"{args.namespace}.toml"
            if not manifest_path.exists():
                continue
            manifest = parse_manifest(manifest_path, args.namespace)
            if manifest is not None:
                results.append(manifest)
    print(json.dumps(results, indent=2, sort_keys=True))


def parse_announcement(path: Path) -> dict[str, Any] | None:
    try:
        data = load_toml(path)
    except SystemExit:
        warn(f"skipping invalid announcement: {path}")
        return None
    node = data.get("node")
    if not isinstance(node, str):
        warn(f"skipping announcement without node field: {path}")
        return None
    try:
        validate_component("node", node)
    except SystemExit:
        warn(f"skipping announcement with unsafe node field: {path}")
        return None
    try:
        data["publishes"] = as_str_list(data, "publishes")
        data["subscribes"] = as_str_list(data, "subscribes")
    except SystemExit:
        warn(f"skipping announcement with invalid publishes/subscribes: {path}")
        return None
    return data


def command_list_nodes(args: argparse.Namespace) -> None:
    root = Path(args.root)
    results: list[dict[str, Any]] = []
    if root.exists():
        for node_dir in sorted(item for item in root.iterdir() if item.is_dir()):
            try:
                validate_component("node", node_dir.name)
            except SystemExit:
                warn(f"skipping unsafe node directory: {node_dir}")
                continue
            announcement_path = node_dir / "announcements.toml"
            if not announcement_path.exists():
                continue
            announcement = parse_announcement(announcement_path)
            if announcement is not None:
                results.append(announcement)
    print(json.dumps(results, indent=2, sort_keys=True))


def command_subscribe_config(args: argparse.Namespace) -> None:
    validate_component("namespace", args.namespace)
    config_path = Path(args.config)
    config = load_toml(config_path, required=False)

    publishes = as_str_list(config, "publishes")
    subscribes = as_str_list(config, "subscribes")
    if args.namespace not in subscribes:
        subscribes.append(args.namespace)

    handler_map = handlers(config)
    handler_map[args.namespace] = args.handler

    output: dict[str, Any] = {}
    if "reconcile_interval" in config:
        output["reconcile_interval"] = config["reconcile_interval"]
    else:
        output["reconcile_interval"] = "60s"
    output["publishes"] = publishes
    output["subscribes"] = subscribes
    output["handlers"] = handler_map
    print(dumps_toml(output), end="")


def parse_duration_seconds(value: str) -> int:
    match = re.fullmatch(r"\s*(\d+)\s*([smhd]?|min)\s*", value)
    if not match:
        die(f"invalid duration: {value}")
    amount = int(match.group(1))
    unit = match.group(2) or "s"
    factors = {"s": 1, "m": 60, "min": 60, "h": 3600, "d": 86400}
    return amount * factors[unit]


def parse_timestamp(value: Any, path: Path) -> dt.datetime | None:
    if isinstance(value, dt.datetime):
        if value.tzinfo is None:
            return value.replace(tzinfo=dt.UTC)
        return value.astimezone(dt.UTC)
    if not isinstance(value, str):
        warn(f"announcement has no string announced_at: {path}")
        return None
    try:
        parsed = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        warn(f"announcement has invalid announced_at: {path}")
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt.UTC)
    return parsed.astimezone(dt.UTC)


def command_prune_candidates(args: argparse.Namespace) -> None:
    root = Path(args.root)
    cutoff = dt.datetime.now(dt.UTC) - dt.timedelta(seconds=parse_duration_seconds(args.older_than))
    if not root.exists():
        return
    for node_dir in sorted(item for item in root.iterdir() if item.is_dir()):
        try:
            validate_component("node", node_dir.name)
        except SystemExit:
            warn(f"skipping unsafe node directory: {node_dir}")
            continue
        announcement_path = node_dir / "announcements.toml"
        if not announcement_path.exists():
            continue
        announcement = parse_announcement(announcement_path)
        if announcement is None:
            continue
        announced_at = parse_timestamp(announcement.get("announced_at"), announcement_path)
        if announced_at is not None and announced_at < cutoff:
            print(node_dir.name)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="toml.py")
    sub = parser.add_subparsers(dest="command", required=True)

    write_announcement = sub.add_parser("write-announcement")
    write_announcement.add_argument("--config", required=True)
    write_announcement.add_argument("--node", required=True)
    write_announcement.set_defaults(func=command_write_announcement)

    announcement_current = sub.add_parser("announcement-current")
    announcement_current.add_argument("--config", required=True)
    announcement_current.add_argument("--node", required=True)
    announcement_current.add_argument("--existing", required=True)
    announcement_current.set_defaults(func=command_announcement_current)

    write_manifest = sub.add_parser("write-manifest")
    write_manifest.add_argument("--namespace", required=True)
    write_manifest.add_argument("--node", required=True)
    write_manifest.add_argument("--payload-file", required=True)
    write_manifest.set_defaults(func=command_write_manifest)

    manifest_current = sub.add_parser("manifest-current")
    manifest_current.add_argument("--namespace", required=True)
    manifest_current.add_argument("--node", required=True)
    manifest_current.add_argument("--payload-file", required=True)
    manifest_current.add_argument("--existing", required=True)
    manifest_current.set_defaults(func=command_manifest_current)

    config_allows = sub.add_parser("config-allows-publish")
    config_allows.add_argument("--config", required=True)
    config_allows.add_argument("--namespace", required=True)
    config_allows.set_defaults(func=command_config_allows_publish)

    config_handler = sub.add_parser("config-handler")
    config_handler.add_argument("--config", required=True)
    config_handler.add_argument("--namespace", required=True)
    config_handler.set_defaults(func=command_config_handler)

    config_interval = sub.add_parser("config-interval")
    config_interval.add_argument("--config", required=True)
    config_interval.set_defaults(func=command_config_interval)

    list_manifests = sub.add_parser("list-manifests")
    list_manifests.add_argument("--root", required=True)
    list_manifests.add_argument("--namespace", required=True)
    list_manifests.set_defaults(func=command_list_manifests)

    list_nodes = sub.add_parser("list-nodes")
    list_nodes.add_argument("--root", required=True)
    list_nodes.set_defaults(func=command_list_nodes)

    subscribe_config = sub.add_parser("subscribe-config")
    subscribe_config.add_argument("--config", required=True)
    subscribe_config.add_argument("--namespace", required=True)
    subscribe_config.add_argument("--handler", required=True)
    subscribe_config.set_defaults(func=command_subscribe_config)

    prune_candidates = sub.add_parser("prune-candidates")
    prune_candidates.add_argument("--root", required=True)
    prune_candidates.add_argument("--older-than", required=True)
    prune_candidates.set_defaults(func=command_prune_candidates)

    return parser


def main() -> None:
    args = build_parser().parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
