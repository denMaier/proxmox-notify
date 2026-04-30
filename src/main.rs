use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};
use std::thread;
use std::time::Duration as StdDuration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use toml::Value;

const DEFAULT_CLUSTER_ROOT: &str = "/etc/pve/proxmox-notify";
const DEFAULT_CONFIG: &str = "/etc/proxmox-notify/config.toml";
const DEFAULT_PREFIX: &str = "/usr/local";
const DEFAULT_RUN_DIR: &str = "/run/proxmox-notify";
const DEFAULT_SYSCONFDIR: &str = "/etc";
const DEFAULT_RECONCILE_INTERVAL: &str = "60s";
const SERVICE_NAME: &str = "proxmox-notify-agent.service";
const DEFAULT_CONFIG_TEMPLATE: &str = include_str!("../config/config.toml");
const SYSTEMD_UNIT_TEMPLATE: &str = include_str!("../systemd/proxmox-notify-agent.service");

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
    Agent(AgentArgs),
    Install(InstallArgs),
    Uninstall(UninstallArgs),
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
struct AgentArgs {
    #[arg(long)]
    once: bool,
    #[arg(long)]
    poll_interval: Option<String>,
}

#[derive(Debug, Args)]
struct InstallArgs {
    #[arg(long, default_value = DEFAULT_PREFIX)]
    prefix: PathBuf,
    #[arg(long, default_value = DEFAULT_SYSCONFDIR)]
    sysconfdir: PathBuf,
    #[arg(long)]
    systemd_dir: Option<PathBuf>,
    #[arg(long)]
    destdir: Option<PathBuf>,
    #[arg(long)]
    no_binary: bool,
    #[arg(long)]
    no_config: bool,
    #[arg(long)]
    no_systemd_unit: bool,
    #[arg(long)]
    no_systemctl: bool,
    #[arg(long)]
    enable_now: bool,
}

#[derive(Debug, Args)]
struct UninstallArgs {
    #[arg(long, default_value = DEFAULT_PREFIX)]
    prefix: PathBuf,
    #[arg(long, default_value = DEFAULT_SYSCONFDIR)]
    sysconfdir: PathBuf,
    #[arg(long)]
    systemd_dir: Option<PathBuf>,
    #[arg(long)]
    destdir: Option<PathBuf>,
    #[arg(long)]
    no_systemctl: bool,
    #[arg(long)]
    remove_binary: bool,
    #[arg(long)]
    purge_config: bool,
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

#[derive(Debug, Clone)]
struct ClusterWriteStatus {
    node: String,
    writable: bool,
    degraded_reason: Option<String>,
}

impl ClusterWriteStatus {
    fn writable(node: String) -> Self {
        Self {
            node,
            writable: true,
            degraded_reason: None,
        }
    }

    fn degraded(node: String, reason: impl Into<String>) -> Self {
        Self {
            node,
            writable: false,
            degraded_reason: Some(reason.into()),
        }
    }

    fn degraded_env(&self) -> &'static str {
        if self.writable {
            "0"
        } else {
            "1"
        }
    }

    fn writable_env(&self) -> &'static str {
        if self.writable {
            "1"
        } else {
            "0"
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
        Commands::Agent(args) => agent(&paths, &args),
        Commands::Install(args) => install_system(&args),
        Commands::Uninstall(args) => uninstall_system(&args),
    }
}

fn env_path(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

struct InstallLayout {
    host_binary: PathBuf,
    binary: PathBuf,
    config: PathBuf,
    unit: PathBuf,
}

impl InstallLayout {
    fn new(
        prefix: &Path,
        sysconfdir: &Path,
        systemd_dir: Option<&Path>,
        destdir: Option<&Path>,
    ) -> Result<Self> {
        require_absolute_path("prefix", prefix)?;
        require_absolute_path("sysconfdir", sysconfdir)?;

        let host_systemd_dir = match systemd_dir {
            Some(path) => {
                require_absolute_path("systemd-dir", path)?;
                path.to_path_buf()
            }
            None => prefix.join("lib/systemd/system"),
        };

        let host_binary = prefix.join("bin/proxmox-notify");
        let host_config = sysconfdir.join("proxmox-notify/config.toml");
        let host_unit = host_systemd_dir.join(SERVICE_NAME);

        Ok(Self {
            binary: rooted_path(destdir, &host_binary),
            config: rooted_path(destdir, &host_config),
            unit: rooted_path(destdir, &host_unit),
            host_binary,
        })
    }
}

fn require_absolute_path(name: &str, path: &Path) -> Result<()> {
    if !path.is_absolute() {
        bail!("{name} must be an absolute path: {}", path.display());
    }
    Ok(())
}

fn rooted_path(destdir: Option<&Path>, path: &Path) -> PathBuf {
    let Some(destdir) = destdir.filter(|path| !path.as_os_str().is_empty()) else {
        return path.to_path_buf();
    };

    match path.strip_prefix("/") {
        Ok(relative) => destdir.join(relative),
        Err(_) => destdir.join(path),
    }
}

fn install_system(args: &InstallArgs) -> Result<()> {
    let layout = InstallLayout::new(
        &args.prefix,
        &args.sysconfdir,
        args.systemd_dir.as_deref(),
        args.destdir.as_deref(),
    )?;

    if !args.no_binary {
        if install_current_exe(&layout.binary)? {
            println!("installed {}", layout.binary.display());
        } else {
            println!("kept existing {}", layout.binary.display());
        }
    }

    if !args.no_config {
        if write_text_if_missing(&layout.config, DEFAULT_CONFIG_TEMPLATE, 0o644)? {
            println!("installed {}", layout.config.display());
        } else {
            println!("kept existing {}", layout.config.display());
        }
    }

    if !args.no_systemd_unit {
        let unit = render_systemd_unit(&layout.host_binary)?;
        write_text_with_mode(&layout.unit, &unit, 0o644)?;
        println!("installed {}", layout.unit.display());
    }

    if should_run_systemctl(args.destdir.as_deref(), args.no_systemctl) {
        systemctl_best_effort(&["daemon-reload"]);
        if args.enable_now {
            systemctl_required(&["enable", "--now", SERVICE_NAME])?;
        }
    }

    Ok(())
}

fn uninstall_system(args: &UninstallArgs) -> Result<()> {
    let layout = InstallLayout::new(
        &args.prefix,
        &args.sysconfdir,
        args.systemd_dir.as_deref(),
        args.destdir.as_deref(),
    )?;

    if should_run_systemctl(args.destdir.as_deref(), args.no_systemctl) {
        systemctl_best_effort(&["disable", "--now", SERVICE_NAME]);
    }

    if remove_file_if_exists(&layout.unit)? {
        println!("removed {}", layout.unit.display());
    }

    if args.remove_binary && remove_file_if_exists(&layout.binary)? {
        println!("removed {}", layout.binary.display());
    }

    if args.purge_config && remove_file_if_exists(&layout.config)? {
        println!("removed {}", layout.config.display());
    }

    if should_run_systemctl(args.destdir.as_deref(), args.no_systemctl) {
        systemctl_best_effort(&["daemon-reload"]);
    }

    Ok(())
}

fn render_systemd_unit(binary: &Path) -> Result<String> {
    let needle = "ExecStart=/usr/local/bin/proxmox-notify agent";
    let replacement = format!("ExecStart={} agent", binary.display());
    if !SYSTEMD_UNIT_TEMPLATE.contains(needle) {
        bail!("systemd unit template is missing the expected ExecStart line");
    }
    Ok(SYSTEMD_UNIT_TEMPLATE.replace(needle, &replacement))
}

fn install_current_exe(path: &Path) -> Result<bool> {
    let source = std::env::current_exe().context("cannot locate current executable")?;
    if same_file(&source, path) {
        return Ok(false);
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    fs::copy(&source, path)
        .with_context(|| format!("cannot copy {} to {}", source.display(), path.display()))?;
    set_file_mode(path, 0o755)?;
    Ok(true)
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn write_text_if_missing(path: &Path, text: &str, mode: u32) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    write_text_with_mode(path, text, mode)?;
    Ok(true)
}

fn write_text_with_mode(path: &Path, text: &str, mode: u32) -> Result<()> {
    write_text_atomic(path, text)?;
    set_file_mode(path, mode)
}

fn set_file_mode(path: &Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("cannot set mode on {}", path.display()))?;
    }

    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }

    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err).with_context(|| format!("cannot remove {}", path.display())),
    }
}

fn should_run_systemctl(destdir: Option<&Path>, no_systemctl: bool) -> bool {
    !no_systemctl
        && match destdir {
            Some(path) => path.as_os_str().is_empty(),
            None => true,
        }
}

fn command_in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn systemctl_best_effort(args: &[&str]) {
    if !command_in_path("systemctl") {
        return;
    }

    match ProcessCommand::new("systemctl").args(args).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!(
            "proxmox-notify: warning: systemctl {} exited with {status}",
            args.join(" ")
        ),
        Err(err) => eprintln!(
            "proxmox-notify: warning: cannot run systemctl {}: {err}",
            args.join(" ")
        ),
    }
}

fn systemctl_required(args: &[&str]) -> Result<()> {
    if !command_in_path("systemctl") {
        bail!("systemctl is required for systemctl {}", args.join(" "));
    }

    let status = ProcessCommand::new("systemctl")
        .args(args)
        .status()
        .with_context(|| format!("cannot run systemctl {}", args.join(" ")))?;
    if !status.success() {
        bail!("systemctl {} exited with {status}", args.join(" "));
    }
    Ok(())
}

fn node_name() -> Result<String> {
    if let Ok(value) = std::env::var("PROXMOX_NOTIFY_NODE_NAME") {
        return Ok(value);
    }

    let raw = hostname::get().context("cannot read hostname")?;
    let raw = raw.to_string_lossy();
    let short = raw.split_once('.').map(|(s, _)| s).unwrap_or(raw.as_ref());
    Ok(short.to_string())
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
    Ok(read_optional_toml::<Config>(path)?.unwrap_or_default())
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

fn cluster_write_status(paths: &Paths) -> Result<ClusterWriteStatus> {
    let node = node_name()?;
    validate_component("node", &node)?;

    if let Some(reason) = cluster_degraded_reason(&paths.cluster_root) {
        Ok(ClusterWriteStatus::degraded(node, reason))
    } else {
        Ok(ClusterWriteStatus::writable(node))
    }
}

fn cluster_degraded_reason(cluster_root: &Path) -> Option<String> {
    match pmxcfs_quorate(cluster_root) {
        Ok(Some(false)) => return Some("pmxcfs reports cluster is not quorate".to_string()),
        Ok(Some(true) | None) => {}
        Err(err) => return Some(format!("{err:#}")),
    }

    readonly_mount_reason(cluster_root)
}

fn pmxcfs_quorate(cluster_root: &Path) -> Result<Option<bool>> {
    let Some(pve_root) = cluster_root.parent() else {
        return Ok(None);
    };
    let members_path = pve_root.join(".members");
    let members = match fs::read_to_string(&members_path) {
        Ok(members) => members,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| format!("cannot read {}", members_path.display()))
        }
    };
    let value: serde_json::Value = serde_json::from_str(&members)
        .with_context(|| format!("invalid JSON in {}", members_path.display()))?;
    Ok(value
        .get("quorate")
        .and_then(|value| value.as_i64())
        .map(|quorate| quorate != 0))
}

#[cfg(target_os = "linux")]
fn readonly_mount_reason(path: &Path) -> Option<String> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").ok()?;
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mount = mountinfo
        .lines()
        .filter_map(parse_mountinfo_line)
        .filter(|mount| path.starts_with(&mount.mount_point))
        .max_by_key(|mount| mount.mount_point.as_os_str().len())?;

    if mount.readonly {
        Some(format!(
            "{} is mounted read-only",
            mount.mount_point.display()
        ))
    } else {
        None
    }
}

#[cfg(not(target_os = "linux"))]
fn readonly_mount_reason(_path: &Path) -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
struct MountInfo {
    mount_point: PathBuf,
    readonly: bool,
}

#[cfg(target_os = "linux")]
fn parse_mountinfo_line(line: &str) -> Option<MountInfo> {
    let (before_separator, _) = line.split_once(" - ")?;
    let fields: Vec<&str> = before_separator.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }

    Some(MountInfo {
        mount_point: PathBuf::from(unescape_mountinfo_path(fields[4])),
        readonly: fields[5].split(',').any(|option| option == "ro"),
    })
}

#[cfg(target_os = "linux")]
fn unescape_mountinfo_path(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn announce(paths: &Paths) -> Result<()> {
    let config = read_config(&paths.config)?;
    announce_with_config(paths, &config)
}

fn announce_with_config(paths: &Paths, config: &Config) -> Result<()> {
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
        publishes: config.publishes.clone(),
        subscribes: config.subscribes.clone(),
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
            Ok(Some(manifest)) if manifest.namespace == namespace && manifest.node == node => {
                manifests.push(manifest)
            }
            Ok(Some(_)) => eprintln!(
                "proxmox-notify: warning: skipping manifest with mismatched fields: {}",
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
            Ok(Some(announcement)) if announcement.node == node => announcements.push(announcement),
            Ok(Some(_)) => eprintln!(
                "proxmox-notify: warning: skipping announcement with mismatched node: {}",
                path.display()
            ),
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
    let cluster_status = cluster_write_status(paths)?;
    reconcile_with_handler(paths, namespace, handler_path, &cluster_status)
}

fn reconcile_with_handler(
    paths: &Paths,
    namespace: &str,
    handler_path: &Path,
    cluster_status: &ClusterWriteStatus,
) -> Result<()> {
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

    match lock.try_lock_exclusive() {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
            File::create(&rerun_path)
                .with_context(|| format!("cannot create {}", rerun_path.display()))?;
            return Ok(());
        }
        Err(err) => {
            return Err(err).with_context(|| format!("cannot lock {}", lock_path.display()));
        }
    }

    let result = {
        let _ = fs::remove_file(&rerun_path);
        let mut exit = run_handler(handler_path, namespace, cluster_status);

        if rerun_path.exists() {
            let _ = fs::remove_file(&rerun_path);
            let second_exit = run_handler(handler_path, namespace, cluster_status);
            if second_exit.is_err() {
                exit = second_exit;
            }
        }

        exit
    };

    let _ = lock.unlock();
    result
}

fn agent(paths: &Paths, args: &AgentArgs) -> Result<()> {
    loop {
        match agent_cycle(paths, args.once) {
            Ok(()) => {}
            Err(err) if args.once => return Err(err),
            Err(err) => eprintln!("proxmox-notify: agent cycle failed: {err:#}"),
        }

        if args.once {
            return Ok(());
        }

        let interval = match agent_poll_interval(paths, args.poll_interval.as_deref()) {
            Ok(interval) => interval,
            Err(err) => {
                eprintln!("proxmox-notify: cannot read agent poll interval: {err:#}");
                parse_duration(DEFAULT_RECONCILE_INTERVAL)?
            }
        };
        thread::sleep(interval);
    }
}

fn agent_cycle(paths: &Paths, strict: bool) -> Result<()> {
    let config = read_config(&paths.config)?;

    let mut failures = 0usize;
    let mut cluster_status = cluster_write_status(paths)?;
    if !cluster_status.writable {
        failures += 1;
        let reason = cluster_status
            .degraded_reason
            .as_deref()
            .unwrap_or("cluster state is not writable");
        eprintln!("proxmox-notify: cluster state degraded: {reason}");
    } else if let Err(err) = announce_with_config(paths, &config) {
        failures += 1;
        eprintln!("proxmox-notify: announce failed: {err:#}");
        cluster_status =
            ClusterWriteStatus::degraded(cluster_status.node.clone(), format!("{err:#}"));
    }

    for namespace in &config.subscribes {
        let result = (|| -> Result<()> {
            validate_component("namespace", namespace)?;
            let handler = config
                .handlers
                .get(namespace)
                .ok_or_else(|| anyhow!("no handler configured for namespace: {namespace}"))?;
            reconcile_with_handler(paths, namespace, Path::new(handler), &cluster_status)
        })();

        if let Err(err) = result {
            failures += 1;
            eprintln!("proxmox-notify: reconcile failed for {namespace}: {err:#}");
        }
    }

    if strict && failures > 0 {
        bail!("{failures} agent step(s) failed");
    }
    Ok(())
}

fn agent_poll_interval(paths: &Paths, override_interval: Option<&str>) -> Result<StdDuration> {
    if let Some(value) = override_interval {
        return parse_duration(value);
    }

    let config = read_optional_toml::<Config>(&paths.config)?.unwrap_or_default();
    parse_duration(
        config
            .reconcile_interval
            .as_deref()
            .unwrap_or(DEFAULT_RECONCILE_INTERVAL),
    )
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

fn run_handler(handler: &Path, namespace: &str, cluster_status: &ClusterWriteStatus) -> Result<()> {
    let status = ProcessCommand::new(handler)
        .arg(namespace)
        .env("PROXMOX_NOTIFY_NAMESPACE", namespace)
        .env("PROXMOX_NOTIFY_NODE", &cluster_status.node)
        .env(
            "PROXMOX_NOTIFY_CLUSTER_WRITABLE",
            cluster_status.writable_env(),
        )
        .env("PROXMOX_NOTIFY_DEGRADED", cluster_status.degraded_env())
        .env(
            "PROXMOX_NOTIFY_DEGRADED_REASON",
            cluster_status.degraded_reason.as_deref().unwrap_or(""),
        )
        .status()
        .with_context(|| format!("cannot run handler {}", handler.display()))?;
    if !status.success() {
        bail!("handler {} exited with {status}", handler.display());
    }
    Ok(())
}

fn parse_duration(value: &str) -> Result<StdDuration> {
    let trimmed = value.trim();
    let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digit_count == 0 {
        bail!("invalid duration: {value}");
    }

    let amount: u64 = trimmed[..digit_count]
        .parse()
        .with_context(|| format!("invalid duration: {value}"))?;
    if amount == 0 {
        bail!("duration must be positive: {value}");
    }
    let multiplier: u64 = match trimmed[digit_count..].trim() {
        "" | "s" => 1,
        "m" | "min" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => bail!("invalid duration: {value}"),
    };
    let seconds = amount
        .checked_mul(multiplier)
        .with_context(|| format!("duration overflows: {value}"))?;
    Ok(StdDuration::from_secs(seconds))
}
