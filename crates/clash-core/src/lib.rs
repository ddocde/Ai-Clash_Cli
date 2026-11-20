pub mod installer;
pub mod links;
pub mod platform;
mod profile_validation;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
pub struct App {
    pub dir: PathBuf,
}
impl App {
    pub fn lock(&self) -> Result<std::fs::File> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.dir.join("mutation.lock"))?;
        fs2::FileExt::try_lock_exclusive(&file)
            .context("Another configuration operation is in progress")?;
        Ok(file)
    }

    pub fn new(dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            dir: fs::canonicalize(dir)?,
        })
    }
    pub fn state(&self) -> Result<Value> {
        let p = self.dir.join("state.json");
        if p.exists() {
            let s: Value = serde_json::from_slice(&fs::read(p)?)?;
            if s["version"] != 1 {
                bail!("Unsupported state version; migrate with a compatible CLI");
            }
            let profiles = s["profiles"]
                .as_object()
                .context("State profiles must be an object")?;
            for (name, source) in profiles {
                validate_name(name)?;
                if !source.is_string() {
                    bail!("Invalid profile source type");
                }
            }
            if let Some(active) = s.get("active") {
                let name = active.as_str().context("State active must be a string")?;
                validate_name(name)?;
                if !profiles.contains_key(name) {
                    bail!("Active profile is missing from registry");
                }
            }
            if !matches!(s["mode"].as_str(), Some("rule" | "global" | "direct"))
                || !s["tun"].is_boolean()
            {
                bail!("Invalid mode or tun state");
            }
            if let Some(selections) = s.get("selections")
                && !selections
                    .as_object()
                    .is_some_and(|m| m.values().all(Value::is_string))
            {
                bail!("Selections must map group names to strings");
            }
            for key in ["core", "secret"] {
                if let Some(value) = s.get(key)
                    && !value
                        .as_str()
                        .is_some_and(|v| !v.is_empty() && !v.chars().any(char::is_control))
                {
                    bail!("Invalid {key} state");
                }
            }
            for key in ["mixed_port", "controller_port"] {
                if let Some(value) = s.get(key)
                    && !value.as_u64().is_some_and(|p| (1..=65535).contains(&p))
                {
                    bail!("Invalid {key} state");
                }
            }
            if s["mixed_port"].as_u64().unwrap_or(7890)
                == s["controller_port"].as_u64().unwrap_or(9090)
            {
                bail!("Listener ports must differ");
            }
            if let Some(value) = s.get("tun_exclusions")
                && !value
                    .as_array()
                    .is_some_and(|a| a.iter().all(Value::is_string))
            {
                bail!("Invalid TUN exclusions");
            }
            Ok(s)
        } else {
            Ok(json!({"version":1,"profiles":{},"mode":"rule","tun":false}))
        }
    }
    pub fn save(&self, s: &Value) -> Result<()> {
        self.write("state.json", &serde_json::to_vec_pretty(s)?)
    }
    pub fn write(&self, name: &str, data: &[u8]) -> Result<()> {
        let p = self.dir.join(name);
        let mut tmp = tempfile::NamedTempFile::new_in(&self.dir)?;
        tmp.write_all(data)?;
        tmp.as_file().sync_all()?;
        tmp.persist(p)?;
        Ok(())
    }
    pub fn core(&self) -> Result<PathBuf> {
        let s = self.state()?;
        s["core"]
            .as_str()
            .map(PathBuf::from)
            .context("Install a core first: core install --path /path/to/mihomo")
    }
    pub async fn api(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let s = self.state()?;
        let secret = s["secret"].as_str().context("Run the core first")?;
        let port = u16::try_from(s["controller_port"].as_u64().unwrap_or(9090))?;
        let client = clash_api::Client::new(port, secret)?;
        if method != "GET" {
            client.verify_controller().await?;
        }
        Ok(client.request(method, path, body).await?)
    }
    pub async fn verify_controller(&self) -> Result<()> {
        let state = self.state()?;
        clash_api::Client::new(
            u16::try_from(state["controller_port"].as_u64().unwrap_or(9090))?,
            state["secret"]
                .as_str()
                .context("No managed controller secret")?,
        )?
        .verify_controller()
        .await?;
        Ok(())
    }

    pub async fn add(&self, name: &str, source: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!("Profile name must contain letters, numbers, - or _");
        }
        let data = if source.starts_with("https://") {
            let mut response = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent("clash-cli/0.1 (Mihomo)")
                .https_only(true)
                .build()?
                .get(source)
                .send()
                .await
                .map_err(|e| e.without_url())
                .context("Subscription request failed")?
                .error_for_status()
                .map_err(|e| e.without_url())
                .context("Subscription HTTP error")?;
            const MAX_PROFILE: usize = 16 * 1024 * 1024;
            if response
                .content_length()
                .is_some_and(|n| n > MAX_PROFILE as u64)
            {
                bail!("Subscription exceeds 16 MiB");
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|e| e.without_url())
                .context("Subscription body read failed")?
            {
                if bytes.len().saturating_add(chunk.len()) > MAX_PROFILE {
                    bail!("Subscription exceeds 16 MiB");
                }
                bytes.extend_from_slice(&chunk);
            }
            bytes
        } else {
            fs::read(source)?
        };
        let source = if source.starts_with("https://") {
            source.to_owned()
        } else {
            fs::canonicalize(source)?
                .to_str()
                .context("Non-UTF8 profile path")?
                .to_owned()
        };
        self.put_profile(name, &source, &data).await
    }
    pub async fn put_profile(&self, name: &str, source: &str, data: &[u8]) -> Result<()> {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            bail!("Invalid profile name");
        }
        profile_validation::parse(data)?;
        let previous = self.state()?;
        let file = format!("{name}.yaml");
        let old = if self.dir.join(&file).exists() {
            Some(fs::read(self.dir.join(&file))?)
        } else {
            None
        };
        self.write(&file, data)?;
        let result = async {
            let mut next = previous.clone();
            next["profiles"][name] = json!(source);
            if next["active"] == name {
                self.activate(next).await?;
            } else {
                let mut validation = next.clone();
                validation["active"] = json!(name);
                let old_runtime = if self.dir.join("runtime.yaml").exists() {
                    Some(fs::read(self.dir.join("runtime.yaml"))?)
                } else {
                    None
                };
                self.save(&validation)?;
                let validation_result = self.prepare();
                self.save(&previous)?;
                if let Some(bytes) = old_runtime {
                    self.write("runtime.yaml", &bytes)?;
                } else if self.dir.join("runtime.yaml").exists() {
                    fs::remove_file(self.dir.join("runtime.yaml"))?;
                }
                validation_result?;
                self.save(&next)?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            self.save(&previous)?;
            match old {
                Some(bytes) => self.write(&file, &bytes)?,
                None => fs::remove_file(self.dir.join(&file))?,
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn prepare(&self) -> Result<PathBuf> {
        let mut s = self.state()?;
        let name = s["active"]
            .as_str()
            .context("Select a profile with profile use NAME")?;
        let mut c = profile_validation::parse(&fs::read(self.dir.join(format!("{name}.yaml")))?)?;
        // Expand inherited settings before enforcing listener restrictions, so
        // Mihomo cannot reintroduce stripped keys when it reads the runtime file.
        c.apply_merge()
            .map_err(|_| anyhow::anyhow!("Invalid YAML merge in profile (contents redacted)"))?;
        if s["secret"].is_null() {
            s["secret"] = json!(uuid::Uuid::new_v4().to_string());
            self.save(&s)?;
        }
        for (k, v) in [
            (
                "mixed-port",
                json!(s["mixed_port"].as_u64().unwrap_or(7890)),
            ),
            ("allow-lan", json!(false)),
            ("bind-address", json!("127.0.0.1")),
            (
                "external-controller",
                json!(format!(
                    "127.0.0.1:{}",
                    s["controller_port"].as_u64().unwrap_or(9090)
                )),
            ),
            ("secret", s["secret"].clone()),
            ("mode", s["mode"].clone()),
        ] {
            c[k] = serde_yaml::to_value(v)?;
        }
        for key in [
            "external-controller-tls",
            "external-controller-unix",
            "external-controller-pipe",
            "listeners",
            "port",
            "socks-port",
            "redir-port",
            "tproxy-port",
        ] {
            c.as_mapping_mut()
                .context("Profile must be a mapping")?
                .remove(serde_yaml::Value::String(key.into()));
        }
        if c["dns"].is_mapping() {
            c["dns"]
                .as_mapping_mut()
                .context("Invalid DNS mapping")?
                .remove(serde_yaml::Value::String("listen".into()));
        }
        if !c["tun"].is_mapping() {
            c["tun"] = serde_yaml::Value::Mapping(Default::default());
        }
        c["tun"]["auto-route"] = serde_yaml::to_value(true)?;
        c["tun"]["auto-detect-interface"] = serde_yaml::to_value(true)?;
        if let Some(exclusions) = s["tun_exclusions"].as_array() {
            c["tun"]["route-exclude-address"] = serde_yaml::to_value(exclusions)?;
        }
        if s["tun"] == true {
            c["tun"]["dns-hijack"] = serde_yaml::to_value(["any:53", "tcp://any:53"])?;
            if !c["dns"].is_mapping() {
                c["dns"] = serde_yaml::Value::Mapping(Default::default());
            }
            c["dns"]["enable"] = serde_yaml::to_value(true)?;
            if c["dns"]["nameserver"].is_null() {
                c["dns"]["nameserver"] = serde_yaml::to_value(["https://1.1.1.1/dns-query"])?;
            }
        }
        c["tun"]["enable"] = serde_yaml::to_value(s["tun"].clone())?;
        self.write("candidate.yaml", serde_yaml::to_string(&c)?.as_bytes())?;
        let path = self.dir.join("candidate.yaml");
        let output = Command::new(self.core()?)
            .arg("-t")
            .arg("-d")
            .arg(&self.dir)
            .arg("-f")
            .arg(&path)
            .output()?;
        if !output.status.success() {
            bail!("Core rejected configuration");
        }
        self.write("runtime.yaml", &fs::read(&path)?)?;
        Ok(self.dir.join("runtime.yaml"))
    }
    pub async fn activate(&self, next: Value) -> Result<()> {
        let previous = self.state()?;
        let runtime = self.dir.join("runtime.yaml");
        let old_runtime = if runtime.exists() {
            Some(fs::read(&runtime)?)
        } else {
            None
        };
        let running = std::net::TcpStream::connect_timeout(
            &format!(
                "127.0.0.1:{}",
                previous["controller_port"].as_u64().unwrap_or(9090)
            )
            .parse()?,
            std::time::Duration::from_millis(200),
        )
        .is_ok();
        if running {
            self.verify_controller()
                .await
                .context("Listener is not the authenticated managed controller")?;
        }
        self.save(&next)?;
        let result = async {
            self.prepare()?;
            if running {
                self.api("PUT", "/configs?force=true", Some(json!({"path":runtime})))
                    .await?;
                self.restore_selections().await?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            self.save(&previous)?;
            if let Some(bytes) = old_runtime {
                self.write("runtime.yaml", &bytes)?;
                if running {
                    self.api("PUT", "/configs?force=true", Some(json!({"path":runtime})))
                        .await
                        .context(
                            "Reload failed and restoring previous core configuration also failed",
                        )?;
                    self.restore_selections()
                        .await
                        .context("Restoring previous node selections failed")?;
                }
            }
            return Err(error.context("Configuration change failed; restored previous state"));
        }
        Ok(())
    }
    async fn restore_selections(&self) -> Result<()> {
        if let Some(selections) = self.state()?["selections"].as_object() {
            let proxies = self.api("GET", "/proxies", None).await?;
            for (group, node) in selections {
                if proxies["proxies"][group]["all"]
                    .as_array()
                    .is_some_and(|members| members.contains(node))
                {
                    let group = percent_encoding::utf8_percent_encode(
                        group,
                        percent_encoding::NON_ALPHANUMERIC,
                    );
                    self.api(
                        "PUT",
                        &format!("/proxies/{group}"),
                        Some(json!({"name":node})),
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }
    pub async fn run(&self) -> Result<()> {
        let state = self.state()?;
        let port = state["controller_port"].as_u64().unwrap_or(9090);
        if std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse()?,
            std::time::Duration::from_millis(200),
        )
        .is_ok()
        {
            bail!(
                "Controller port already occupied; stop the existing process or configure another port"
            );
        }
        let p = {
            let _lock = self.lock()?;
            self.prepare()?
        };
        let mut child = tokio::process::Command::new(self.core()?)
            .arg("-d")
            .arg(&self.dir)
            .arg("-f")
            .arg(p)
            .kill_on_drop(true)
            .spawn()?;
        let startup = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            let mut ready = false;
            for _ in 0..100 {
                if let Some(status) = child.try_wait()? {
                    bail!("Core exited before readiness: {status}");
                }
                if self.api("GET", "/version", None).await.is_ok() {
                    ready = true;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            if !ready {
                bail!("Core readiness timed out");
            }
            self.restore_selections().await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("Core readiness timed out")
        .and_then(|result| result);
        if let Err(error) = startup {
            platform::terminate_engine(&mut child).await?;
            return Err(error);
        }
        #[cfg(unix)]
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let shutdown = async {
            #[cfg(unix)]
            {
                tokio::select! {result=tokio::signal::ctrl_c()=>{result?;}, _=terminate.recv()=>{}}
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::select! {
            result=child.wait()=>{let status=result?;if !status.success(){bail!("Core exited with {status}");}}
            result=shutdown=>{result?;platform::terminate_engine(&mut child).await?;}
        }
        Ok(())
    }
}
pub fn absolute(p: &Path) -> Result<PathBuf> {
    Ok(fs::canonicalize(p)?)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Invalid profile name");
    }
    Ok(())
}

/// Classify controller latency endpoints without exposing their response bodies.
pub fn is_timeout(error: &anyhow::Error) -> bool {
    matches!(
        error
            .downcast_ref::<clash_api::Error>()
            .map(clash_api::Error::kind),
        Some(clash_api::ErrorKind::HttpStatus(408 | 504))
    )
}
