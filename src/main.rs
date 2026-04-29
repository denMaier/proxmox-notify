use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use clap::{Args, Parser, Subcommand};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use toml::Value;

const DEFAULT_CLUSTER_ROOT: &str = "/etc/pve/proxmox-notify";
const DEFAULT_CONFIG: &str = "/etc/proxmox-notify/config.toml";
const DEFAULT_RUN_DIR: &str = "/run/proxmox-notify";
const DEFAULT_RECONCILE_INTERVAL: &str = "60s";

#[derive(Debug, Parser)]
#[command(name = "proxmox-notify")]
#[command(about = "pmxcfs-backed node-to-node state announcements for Proxmox")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Announce,
    Publish(PublishArgs),
    Get(GetArgs),
    ListManifests(NamespaceArgs),
    ListNodes,
    Delete(NamespaceArgs),
    Reconcile(NamespaceArgs),
    Subscribe(SubscribeArgs),
    PruneNodes(PruneArgs),
}

#[derive(Debug, Args)]
struct NamespaceArgs {
    #[arg(long)]
    namespace: String,
}

#[derive(Debug, Args)]
struct PublishArgs {
    #[arg(long)]
    namespace: String,
    #[arg(long)]
    payload_file: PathBuf,
}

#[derive(Debug, Args)]
struct GetArgs {
    #[arg(long)]
    namespace: String,
    #[arg(long)]
    node: Option<String>,
}

#[derive(Debug, Args)]
struct SubscribeArgs {
    #[arg(long)]
    namespace: String,
    #[arg(long)]
    handler: PathBuf,
}

#[derive(Debug, Args)]
struct PruneArgs {
    #[arg(long)]
    older_than: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
struct Config {
    #[serde(default)]
    reconcile_interval: Option<String>,
    #[serde(default)]
    publishes: Vec<String>,
    #[serde(default)]
    subscribes: Vec<String>,
    #[serde(default)]
    handlers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Announcement {
    node: String,
    announced_at: String,
    publishes: Vec<String>,
    subscribes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Manifest {
    namespace: String,
    node: String,
    timestamp: String,
    payload: Value,
}

struct Paths {
    cluster_root: PathBuf,
    config: PathBuf,
    run_dir: PathBuf,
}

impl Paths {
    fn from_env() -> Self {
        Self {
            cluster_root: env_path("PROXMOX_NOTIFY_CLUSTER_ROOT", DEFAULT_CLUSTER_ROOT),
            config: env_path("PROXMOX_NOTIFY_CONFIG", DEFAULT_CONFIG),
            run_dir: env_path("PROXMOX_NOTIFY_RUN_DIR", DEFAULT_RUN_DIR),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("proxmox-notify: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env();

    match cli.command {
        Commands::Announce => announce(&paths),
        Commands::Publish(args) => publish(&paths, &args.namespace, &args.payload_file),
        Commands::Get(args) => get(&paths, &args.namespace, args.node.as_deref()),
        Commands::ListManifests(args) => list_manifests(&paths, &args.namespace),
        Commands::ListNodes => list_nodes(&paths),
        Commands::Delete(args) => delete_manifest(&paths, &args.namespace),
        Commands::Reconcile(args) => reconcile(&paths, &args.namespace),
        Commands::Subscribe(args) => subscribe(&paths, &args.namespace, &args.handler),
        Commands::PruneNodes(args) => prune_nodes(&paths, &args.older_than),
    }
}

fn env_path(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

fn node_name() -> Result<String> {
    if let Ok(value) = std::env::var("PROXMOX_NOTIFY_NODE_NAME") {
        return Ok(value);
    }

    let name = hostname::get().context("cannot read hostname")?;
    let name = name
        .to_string_lossy()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_string();
    Ok(name)
}

fn validate_component(kind: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{kind} must not be empty");
    }
    if value.len() > 128 {
        bail!("{kind} is too long: {value}");
    }
    if value.starts_with('-') {
        bail!("{kind} must not start with '-': {value}");
    }
    if value.chars().all(|ch| ch == '.') {
        bail!("{kind} must not be dots only: {value}");
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    {
        bail!("{kind} contains unsafe characters: {value}");
    }
    Ok(())
}

fn now_utc() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn read_config(path: &Path) -> Result<Config> {
    read_toml(path)
}

fn read_toml<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid TOML in {}", path.display()))
}

fn read_optional_toml<T>(path: &Path) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    match fs::read_to_string(path) {
        Ok(text) => {
            Ok(Some(toml::from_str(&text).with_context(|| {
                format!("invalid TOML in {}", path.display())
            })?))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn read_payload(path: &Path) -> Result<Value> {
    let value: Value = read_toml(path)?;
    if !value.is_table() {
        bail!("payload TOML must be a table: {}", path.display());
    }
    Ok(value)
}

fn write_toml_atomic<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let text = toml::to_string_pretty(value).context("cannot serialize TOML")?;
    write_text_atomic(path, &text)
}

fn write_text_atomic(path: &Path, text: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;

    let file_name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("path has no file name: {}", path.display()))?;
    let tmp = parent.join(format!("{file_name}.tmp.{}", std::process::id()));

    let write_result = (|| -> Result<()> {
        let mut file =
            File::create(&tmp).with_context(|| format!("cannot create {}", tmp.display()))?;
        file.write_all(text.as_bytes())
            .with_context(|| format!("cannot write {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("cannot sync {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("cannot rename {} to {}", tmp.display(), path.display()))?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&tmp);
    }

    write_result
}

fn announce(paths: &Paths) -> Result<()> {
    let config = read_config(&paths.config)?;
    let node = node_name()?;
    validate_component("node", &node)?;

    let path = paths.cluster_root.join(&node).join("announcements.toml");
    if let Some(current) = read_optional_toml::<Announcement>(&path)? {
        if current.node == node
            && current.publishes == config.publishes
            && current.subscribes == config.subscribes
        {
            return Ok(());
        }
    }

    let announcement = Announcement {
        node,
        announced_at: now_utc(),
        publishes: config.publishes,
        subscribes: config.subscribes,
    };
    write_toml_atomic(&path, &announcement)
}

fn publish(paths: &Paths, namespace: &str, payload_file: &Path) -> Result<()> {
    validate_component("namespace", namespace)?;
    let config = read_config(&paths.config)?;
    if !config.publishes.iter().any(|item| item == namespace) {
        bail!("namespace is not in publishes list: {namespace}");
    }

    let payload = read_payload(payload_file)?;
    let node = node_name()?;
    validate_component("node", &node)?;
    let path = paths
        .cluster_root
        .join(&node)
        .join("manifests")
        .join(format!("{namespace}.toml"));

    if let Some(current) = read_optional_toml::<Manifest>(&path)? {
        if current.namespace == namespace && current.node == node && current.payload == payload {
            return Ok(());
        }
    }

    let manifest = Manifest {
        namespace: namespace.to_string(),
        node,
        timestamp: now_utc(),
        payload,
    };
    write_toml_atomic(&path, &manifest)
}

fn get(paths: &Paths, namespace: &str, node: Option<&str>) -> Result<()> {
    validate_component("namespace", namespace)?;
    let node = match node {
        Some(node) => node.to_string(),
        None => node_name()?,
    };
    validate_component("node", &node)?;

    let path = paths
        .cluster_root
        .join(&node)
        .join("manifests")
        .join(format!("{namespace}.toml"));
    let text = fs::read_to_string(&path)
        .with_context(|| format!("manifest not found: {node}/{namespace}"))?;
    print!("{text}");
    Ok(())
}

fn list_manifests(paths: &Paths, namespace: &str) -> Result<()> {
    validate_component("namespace", namespace)?;
    let mut manifests = Vec::new();

    for node_dir in node_dirs(&paths.cluster_root)? {
        let node = node_dir
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        if let Err(err) = validate_component("node", node) {
            eprintln!(
                "proxmox-notify: warning: skipping unsafe node directory {}: {err}",
                node_dir.display()
            );
            continue;
        }

        let path = node_dir.join("manifests").join(format!("{namespace}.toml"));
        if !path.exists() {
            continue;
        }
        match read_optional_toml::<Manifest>(&path) {
            Ok(Some(manifest)) if manifest.namespace == namespace => manifests.push(manifest),
            Ok(Some(_)) => eprintln!(
                "proxmox-notify: warning: skipping manifest with mismatched namespace: {}",
                path.display()
            ),
            Ok(None) => {}
            Err(err) => eprintln!(
                "proxmox-notify: warning: skipping invalid manifest {}: {err:#}",
                path.display()
            ),
        }
    }

    println!("{}", serde_json::to_string_pretty(&manifests)?);
    Ok(())
}

fn list_nodes(paths: &Paths) -> Result<()> {
    let mut announcements = Vec::new();

    for node_dir in node_dirs(&paths.cluster_root)? {
        let node = node_dir
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        if let Err(err) = validate_component("node", node) {
            eprintln!(
                "proxmox-notify: warning: skipping unsafe node directory {}: {err}",
                node_dir.display()
            );
            continue;
        }

        let path = node_dir.join("announcements.toml");
        if !path.exists() {
            continue;
        }
        match read_optional_toml::<Announcement>(&path) {
            Ok(Some(announcement)) => announcements.push(announcement),
            Ok(None) => {}
            Err(err) => eprintln!(
                "proxmox-notify: warning: skipping invalid announcement {}: {err:#}",
                path.display()
            ),
        }
    }

    println!("{}", serde_json::to_string_pretty(&announcements)?);
    Ok(())
}

fn node_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut dirs = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("cannot read {}", root.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.path());
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn delete_manifest(paths: &Paths, namespace: &str) -> Result<()> {
    validate_component("namespace", namespace)?;
    let node = node_name()?;
    validate_component("node", &node)?;
    let path = paths
        .cluster_root
        .join(node)
        .join("manifests")
        .join(format!("{namespace}.toml"));

    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| format!("cannot delete {}", path.display())),
    }
}

fn reconcile(paths: &Paths, namespace: &str) -> Result<()> {
    validate_component("namespace", namespace)?;
    let config = read_config(&paths.config)?;
    let handler = config
        .handlers
        .get(namespace)
        .ok_or_else(|| anyhow!("no handler configured for namespace: {namespace}"))?;
    let handler_path = Path::new(handler);
    if !is_executable(handler_path) {
        bail!("handler is not executable: {}", handler_path.display());
    }

    fs::create_dir_all(&paths.run_dir)
        .with_context(|| format!("cannot create {}", paths.run_dir.display()))?;
    let lock_path = paths.run_dir.join(format!("{namespace}.lock"));
    let rerun_path = paths.run_dir.join(format!("{namespace}.rerun"));

    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("cannot open {}", lock_path.display()))?;

    if lock.try_lock_exclusive().is_err() {
        File::create(&rerun_path)
            .with_context(|| format!("cannot create {}", rerun_path.display()))?;
        return Ok(());
    }

    let result = {
        let _ = fs::remove_file(&rerun_path);
        let mut exit = run_handler(handler_path, namespace);

        if rerun_path.exists() {
            let _ = fs::remove_file(&rerun_path);
            let second_exit = run_handler(handler_path, namespace);
            if second_exit.is_err() {
                exit = second_exit;
            }
        }

        exit
    };

    let _ = lock.unlock();
    result
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(path) {
            Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
            Err(_) => false,
        }
    }

    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn run_handler(handler: &Path, namespace: &str) -> Result<()> {
    let status = ProcessCommand::new(handler)
        .arg(namespace)
        .status()
        .with_context(|| format!("cannot run handler {}", handler.display()))?;
    if !status.success() {
        bail!("handler {} exited with {status}", handler.display());
    }
    Ok(())
}

fn subscribe(paths: &Paths, namespace: &str, handler: &Path) -> Result<()> {
    validate_component("namespace", namespace)?;
    if !is_executable(handler) {
        bail!("handler is not executable: {}", handler.display());
    }

    let mut config = read_optional_toml::<Config>(&paths.config)?.unwrap_or_default();
    if !config.subscribes.iter().any(|item| item == namespace) {
        config.subscribes.push(namespace.to_string());
    }
    config
        .handlers
        .insert(namespace.to_string(), handler.display().to_string());
    if config.reconcile_interval.is_none() {
        config.reconcile_interval = Some(DEFAULT_RECONCILE_INTERVAL.to_string());
    }
    write_toml_atomic(&paths.config, &config)?;

    let interval = config
        .reconcile_interval
        .as_deref()
        .unwrap_or(DEFAULT_RECONCILE_INTERVAL);
    let instance = systemd_escape(namespace);
    let dropin = PathBuf::from(format!(
        "/etc/systemd/system/proxmox-notify-reconcile@{instance}.timer.d/override.conf"
    ));
    write_text_atomic(
        &dropin,
        &format!("[Timer]\nOnUnitActiveSec=\nOnUnitActiveSec={interval}\n"),
    )?;

    run_systemctl(["daemon-reload"])?;
    run_systemctl([
        "enable",
        "--now",
        &format!("proxmox-notify-watch@{instance}.path"),
        &format!("proxmox-notify-reconcile@{instance}.timer"),
    ])
}

fn systemd_escape(namespace: &str) -> String {
    match ProcessCommand::new("systemd-escape")
        .arg("--")
        .arg(namespace)
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => namespace.to_string(),
    }
}

fn run_systemctl<const N: usize>(args: [&str; N]) -> Result<()> {
    let status = ProcessCommand::new("systemctl")
        .args(args)
        .status()
        .context("systemctl is required to enable subscriptions")?;
    if !status.success() {
        bail!("systemctl exited with {status}");
    }
    Ok(())
}

fn prune_nodes(paths: &Paths, older_than: &str) -> Result<()> {
    let cutoff = Utc::now() - parse_duration(older_than)?;
    for node_dir in node_dirs(&paths.cluster_root)? {
        let node = node_dir
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default();
        validate_component("node", node)?;
        let path = node_dir.join("announcements.toml");
        let Some(announcement) = read_optional_toml::<Announcement>(&path)? else {
            continue;
        };
        let announced_at = DateTime::parse_from_rfc3339(&announcement.announced_at)
            .with_context(|| format!("invalid announced_at in {}", path.display()))?
            .with_timezone(&Utc);
        if announced_at < cutoff {
            fs::remove_dir_all(&node_dir)
                .with_context(|| format!("cannot prune {}", node_dir.display()))?;
            println!("{node}");
        }
    }
    Ok(())
}

fn parse_duration(value: &str) -> Result<Duration> {
    let trimmed = value.trim();
    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        bail!("invalid duration: {value}");
    }

    let amount: i64 = trimmed[..digit_count]
        .parse()
        .with_context(|| format!("invalid duration: {value}"))?;
    let unit = trimmed[digit_count..].trim();
    match unit {
        "" | "s" => Ok(Duration::seconds(amount)),
        "m" | "min" => Ok(Duration::minutes(amount)),
        "h" => Ok(Duration::hours(amount)),
        "d" => Ok(Duration::days(amount)),
        _ => bail!("invalid duration: {value}"),
    }
}
