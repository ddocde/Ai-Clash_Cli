//! Operating-system boundary. No privileged operation runs implicitly.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    env, fs,
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
};

const SERVICE_NAME: &str = "clash-cli.service";
const SERVICE_FILE: &str = "/etc/systemd/system/clash-cli.service";
const SERVICE_MARKER: &str = "# Managed by clash-cli";

#[derive(Debug, Clone)]
pub struct Directories {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub logs: PathBuf,
}

/// Add native implementations here without exposing OS commands to business code.
pub trait Platform {
    fn capabilities(&self) -> Value;
    fn directories(&self, explicit: Option<&Path>) -> Result<Directories>;
    fn service(&self, action: &str, data_dir: &Path) -> Result<Value>;
    fn tun_preflight(&self, exclusions: &[String]) -> Result<Vec<String>>;
    fn system_proxy(&self, _enabled: bool) -> Result<()> {
        bail!("System proxy settings are not implemented on this platform; use env, exec, or TUN")
    }
}

pub struct NativePlatform;
impl Platform for NativePlatform {
    fn capabilities(&self) -> Value {
        json!({"os":env::consts::OS,"arch":env::consts::ARCH,
            "foreground":true,"system_service":cfg!(target_os="linux"),
            "tun":cfg!(target_os="linux"),"system_proxy":false})
    }
    fn directories(&self, explicit: Option<&Path>) -> Result<Directories> {
        let data = match explicit {
            Some(path) => absolute_path(path)?,
            None => default_data_dir()?,
        };
        Ok(Directories {
            config: data.clone(),
            cache: data.join("cache"),
            logs: data.join("logs"),
            data,
        })
    }
    fn service(&self, action: &str, data_dir: &Path) -> Result<Value> {
        service(action, data_dir)
    }
    fn tun_preflight(&self, exclusions: &[String]) -> Result<Vec<String>> {
        tun_preflight(exclusions)
    }
}

pub fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

pub fn default_data_dir() -> Result<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        PathBuf::from(
            env::var_os("LOCALAPPDATA").context("LOCALAPPDATA missing; supply --data-dir")?,
        )
    } else if cfg!(target_os = "macos") {
        PathBuf::from(env::var_os("HOME").context("HOME missing; supply --data-dir")?)
            .join("Library/Application Support")
    } else {
        match env::var_os("XDG_DATA_HOME") {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => PathBuf::from(env::var_os("HOME").context("HOME missing; supply --data-dir")?)
                .join(".local/share"),
        }
    };
    if !base.is_absolute() {
        bail!("Platform data directory must be absolute; supply --data-dir");
    }
    Ok(base.join("clash-cli"))
}

/// A process handle isolates platform termination semantics from callers.
pub struct EngineProcess(Child);
impl EngineProcess {
    pub fn spawn(executable: &Path, data_dir: &Path, config: &Path) -> Result<Self> {
        Ok(Self(
            Command::new(executable)
                .arg("-d")
                .arg(data_dir)
                .arg("-f")
                .arg(config)
                .spawn()
                .context("Start Mihomo engine")?,
        ))
    }
    pub fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.0.wait()?)
    }
    pub fn running(&mut self) -> Result<bool> {
        Ok(self.0.try_wait()?.is_none())
    }
    pub fn stop(&mut self) -> Result<()> {
        if self.running()? {
            self.0.kill()?;
            self.0.wait()?;
        }
        Ok(())
    }
}

/// Let Mihomo restore routes and DNS before resorting to forced termination.
pub async fn terminate_engine(child: &mut tokio::process::Child) -> Result<ExitStatus> {
    if let Some(status) = child.try_wait()? {
        return Ok(status);
    }
    #[cfg(unix)]
    {
        use nix::{
            errno::Errno,
            sys::signal::{Signal, kill},
            unistd::Pid,
        };
        let id = child.id().context("Engine process has no process ID")?;
        let pid = i32::try_from(id).context("Engine process ID is out of range")?;
        match kill(Pid::from_raw(pid), Signal::SIGTERM) {
            Ok(()) => {}
            Err(Errno::ESRCH) => return Ok(child.wait().await?),
            Err(error) => return Err(error.into()),
        }
        match tokio::time::timeout(std::time::Duration::from_secs(10), child.wait()).await {
            Ok(status) => Ok(status?),
            Err(_elapsed) => {
                // The engine did not honor graceful termination within the deadline.
                child.kill().await?;
                Ok(child.wait().await?)
            }
        }
    }
    #[cfg(not(unix))]
    {
        child.kill().await?;
        Ok(child.wait().await?)
    }
}

fn systemd_quote(value: &str) -> Result<String> {
    if value.chars().any(|c| c.is_control()) {
        bail!("Service paths must not contain control characters");
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    ))
}

pub fn service_unit(executable: &Path, data_dir: &Path) -> Result<String> {
    if !executable.is_absolute() || !data_dir.is_absolute() {
        bail!("Service executable and data directory must be absolute");
    }
    let exe = systemd_quote(
        executable
            .to_str()
            .context("Service executable must be UTF-8")?,
    )?;
    let data = systemd_quote(
        data_dir
            .to_str()
            .context("Service data directory must be UTF-8")?,
    )?;
    Ok(format!(
        "{SERVICE_MARKER}\n[Unit]\nDescription=clash-cli Mihomo proxy\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nExecStart={exe} --data-dir {data} run\nRestart=on-failure\nRestartSec=3\nKillMode=control-group\nTimeoutStopSec=15\nUMask=0077\n\n[Install]\nWantedBy=multi-user.target\n"
    ))
}

fn require_linux() -> Result<()> {
    if !cfg!(target_os = "linux") {
        bail!("This operation is currently supported only on Linux");
    }
    Ok(())
}
fn require_root() -> Result<()> {
    let output = Command::new("id")
        .arg("-u")
        .output()
        .context("Check effective user ID")?;
    if !output.status.success() {
        bail!("Cannot determine effective user ID");
    }
    if output.stdout != b"0\n" {
        bail!("Root permission required: rerun with sudo and the same explicit --data-dir path");
    }
    Ok(())
}
fn checked(command: &mut Command) -> Result<()> {
    let status = command.status().context("Run platform command")?;
    if !status.success() {
        bail!("Platform command failed ({status})");
    }
    Ok(())
}
fn ensure_owned_service() -> Result<()> {
    let text = fs::read_to_string(SERVICE_FILE)
        .context("Service not installed; run service install first")?;
    if !text.starts_with(SERVICE_MARKER) {
        bail!("Refusing to modify a service not managed by clash-cli");
    }
    Ok(())
}

/// Root services must never execute or consume state writable by another user.
#[cfg(unix)]
fn trusted_root_path(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() {
        bail!("Privileged service paths must be absolute");
    }
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).context("Check privileged service path")?;
        if metadata.file_type().is_symlink() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0
        {
            bail!(
                "Service executable, data, core, and all ancestors must be root-owned, not symlinks, and not group/world writable. Install the CLI in /usr/local/bin and initialize --data-dir /var/lib/clash-cli with sudo"
            );
        }
    }
    Ok(())
}
#[cfg(not(unix))]
fn trusted_root_path(_path: &Path) -> Result<()> {
    bail!("Privileged services are not implemented on this platform")
}
fn trusted_data_tree(dir: &Path) -> Result<()> {
    trusted_root_path(dir)?;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            trusted_root_path(&entry.path())?;
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    Ok(())
}
fn validate_service_trust(executable: &Path, data_dir: &Path) -> Result<()> {
    trusted_root_path(executable)?;
    trusted_data_tree(data_dir)?;
    let state: Value = serde_json::from_slice(&fs::read(data_dir.join("state.json"))?)?;
    let core = state["core"]
        .as_str()
        .context("Install a core before installing the service")?;
    trusted_root_path(Path::new(core))?;
    Ok(())
}

pub fn service(action: &str, data_dir: &Path) -> Result<Value> {
    require_linux()?;
    if !Path::new("/run/systemd/system").is_dir() {
        bail!("systemd is not running; use run with your process supervisor");
    }
    match action {
        "install" => {
            require_root()?;
            if Path::new(SERVICE_FILE).exists() {
                ensure_owned_service()?;
            }
            let data = fs::canonicalize(data_dir)
                .context("Initialize the data directory before installing service")?;
            let executable = env::current_exe()?;
            validate_service_trust(&executable, &data)?;
            let unit = service_unit(&executable, &data)?;
            // Fixed system path; never accept a user-controlled unit name.
            fs::write(SERVICE_FILE, unit)?;
            checked(Command::new("systemctl").arg("daemon-reload"))?;
            checked(Command::new("systemctl").args(["enable", SERVICE_NAME]))?;
            Ok(json!({"installed":true,"enabled":true,"started":false,"data_dir":data}))
        }
        "uninstall" => {
            require_root()?;
            ensure_owned_service()?;
            checked(Command::new("systemctl").args(["disable", "--now", SERVICE_NAME]))?;
            fs::remove_file(SERVICE_FILE)?;
            checked(Command::new("systemctl").arg("daemon-reload"))?;
            Ok(json!({"uninstalled":true,"data_preserved":true}))
        }
        "start" | "stop" | "restart" => {
            require_root()?;
            ensure_owned_service()?;
            checked(Command::new("systemctl").args([action, SERVICE_NAME]))?;
            Ok(json!({"action":action,"success":true}))
        }
        "status" => {
            let output = Command::new("systemctl")
                .args([
                    "show",
                    SERVICE_NAME,
                    "--property=LoadState,ActiveState,SubState,MainPID",
                ])
                .output()?;
            if !output.status.success() {
                bail!("Cannot query system service");
            }
            let mut fields = serde_json::Map::new();
            for line in std::str::from_utf8(&output.stdout)?.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    fields.insert(key.into(), json!(value));
                }
            }
            Ok(Value::Object(fields))
        }
        "logs" => {
            checked(Command::new("journalctl").args([
                "--unit",
                SERVICE_NAME,
                "--no-pager",
                "--lines",
                "100",
            ]))?;
            Ok(json!({"source":"journal"}))
        }
        _ => bail!("Unknown service action: {action}"),
    }
}

/// Pure parser so SSH safety can be tested without mutating process environment.
pub fn ssh_exclusions(connection: Option<&str>, extra: &[String]) -> Result<Vec<String>> {
    let mut result = Vec::new();
    if let Some(connection) = connection {
        let fields: Vec<_> = connection.split_whitespace().collect();
        if fields.len() != 4 {
            bail!("Invalid SSH_CONNECTION; supply explicit management exclusions instead");
        }
        let peer: IpAddr = fields[0].parse().context("Invalid SSH client IP")?;
        result.push(format!("{peer}/{}", if peer.is_ipv4() { 32 } else { 128 }));
    }
    for cidr in extra {
        let (ip, prefix) = cidr
            .split_once('/')
            .context("Management exclusion must be CIDR, e.g. 192.0.2.1/32")?;
        let ip: IpAddr = ip.parse().context("Invalid exclusion IP")?;
        let prefix: u8 = prefix.parse().context("Invalid exclusion prefix")?;
        if prefix > if ip.is_ipv4() { 32 } else { 128 } {
            bail!("Invalid exclusion prefix length");
        }
        let normalized = format!("{ip}/{prefix}");
        if !result.contains(&normalized) {
            result.push(normalized);
        }
    }
    Ok(result)
}

pub fn tun_preflight(extra_exclusions: &[String]) -> Result<Vec<String>> {
    require_linux()?;
    require_root()?;
    if !Path::new("/dev/net/tun").exists() {
        bail!("TUN device missing: ask the administrator to enable /dev/net/tun");
    }
    if !Path::new("/etc/resolv.conf").exists() {
        bail!("DNS configuration /etc/resolv.conf is missing");
    }
    let output = Command::new("ip")
        .args(["route", "show", "default"])
        .output()
        .context("Install iproute2 to check default routing")?;
    if !output.status.success() || output.stdout.is_empty() {
        bail!("No IPv4 default route detected; configure routing before enabling TUN");
    }
    let ssh = env::var("SSH_CONNECTION")
        .map(Some)
        .or_else(|err| match err {
            env::VarError::NotPresent => Ok(None),
            other => Err(other),
        })?;
    if ssh.is_none() && extra_exclusions.is_empty() {
        bail!(
            "No SSH connection metadata available; preserve SSH_CONNECTION through sudo or provide --exclude MANAGEMENT_IP/32 before enabling TUN"
        );
    }
    ssh_exclusions(ssh.as_deref(), extra_exclusions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(unix)]
    fn quote_systemd_expansions_and_spaces() -> Result<()> {
        let unit = service_unit(
            Path::new("/opt/My App/$cli%/bin"),
            Path::new("/var/lib/a b"),
        )?;
        assert!(unit.contains("\"/opt/My App/$$cli%%/bin\" --data-dir \"/var/lib/a b\" run"));
        assert!(service_unit(Path::new("relative"), Path::new("/data")).is_err());
        assert!(systemd_quote("bad\npath").is_err());
        Ok(())
    }
    #[tokio::test]
    #[cfg(unix)]
    async fn graceful_termination_allows_cleanup() -> Result<()> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut child = tokio::process::Command::new("sh")
            .args([
                "-c",
                "trap 'exit 0' TERM; echo ready; while :; do sleep 1; done",
            ])
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take().context("Missing test child output")?;
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).await?;
        assert_eq!(line.trim(), "ready");
        assert!(terminate_engine(&mut child).await?.success());
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn root_service_rejects_user_writable_tree() -> Result<()> {
        let directory = tempfile::tempdir()?;
        assert!(trusted_root_path(directory.path()).is_err());
        assert!(trusted_root_path(Path::new("relative")).is_err());
        Ok(())
    }
    #[test]
    fn ssh_ipv4_ipv6_and_deduplication() -> Result<()> {
        assert_eq!(
            ssh_exclusions(
                Some("192.0.2.7 1234 192.0.2.8 22"),
                &["192.0.2.7/32".into()]
            )?,
            vec!["192.0.2.7/32"]
        );
        assert_eq!(
            ssh_exclusions(Some("2001:db8::1 1234 2001:db8::2 22"), &[])?,
            vec!["2001:db8::1/128"]
        );
        assert!(ssh_exclusions(Some("broken"), &[]).is_err());
        assert!(ssh_exclusions(None, &["192.0.2.1/33".into()]).is_err());
        Ok(())
    }
}
