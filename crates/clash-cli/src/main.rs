use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use clash_core::App;
use serde_json::json;
use std::{path::PathBuf, process::Command};
#[derive(Parser)]
#[command(
    version,
    about = "Headless Mihomo proxy manager",
    after_long_help = "Examples:\n  clash-cli core install\n  clash-cli profile add home /path/to/config.yaml\n  clash-cli profile use home\n  clash-cli run\n  clash-cli node test --all\n  clash-cli node select NODE --group PROXY\n  clash-cli --json status\n\nUse COMMAND --help for arguments. TUN and Linux services require elevated permissions."
)]
struct Cli {
    /// Persistent state directory; defaults to the platform user data directory
    #[arg(long, global = true, value_name = "DIRECTORY")]
    data_dir: Option<PathBuf>,
    /// Emit compact JSON for automation (diagnostics remain on stderr)
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    /// Install or select the proxy engine
    #[command(
        after_long_help = "Examples:\n  clash-cli core install\n  clash-cli core install --path /opt/mihomo\n  clash-cli core update\n  clash-cli core rollback"
    )]
    Core {
        #[command(subcommand)]
        command: Core,
    },
    /// Manage subscription profiles
    #[command(
        after_long_help = "Examples:\n  clash-cli profile add home /path/to/config.yaml\n  clash-cli profile add remote https://example.com/subscription\n  clash-cli profile use home\n  clash-cli profile update remote"
    )]
    Profile {
        #[command(subcommand)]
        command: Profile,
    },
    /// Inspect and select proxies
    #[command(
        after_long_help = "Examples:\n  clash-cli node list\n  clash-cli node test --all --concurrency 4\n  clash-cli node select NODE --group PROXY\n  clash-cli node import SHARE_URL --profile local"
    )]
    Node {
        #[command(subcommand)]
        command: Node,
    },
    /// List proxy groups
    Group {
        #[command(subcommand)]
        command: Group,
    },
    /// Run the engine in the foreground
    Run,
    /// Manage Linux system service (install requires sudo and explicit --data-dir)
    Service {
        #[arg(value_parser=["install","uninstall","start","stop","restart","status"])]
        action: String,
    },
    /// Read service logs
    Logs,
    /// Enable or disable TUN capture
    #[command(
        after_long_help = "Example:\n  sudo clash-cli --data-dir /var/lib/clash-cli tun on --exclude 192.0.2.10/32\n\nExclude your real SSH management peer or subnet; the address above is a placeholder."
    )]
    Tun {
        #[arg(value_parser=["on","off"])]
        action: String,
        #[arg(long)]
        exclude: Vec<String>,
    },
    /// Query engine version and availability
    Status,
    /// Check platform and configuration
    Doctor,
    /// Configure local listener ports (restart required)
    Config {
        #[arg(long)]
        mixed_port: u16,
        #[arg(long)]
        controller_port: u16,
    },
    /// Set routing mode
    Mode {
        #[arg(value_parser=["rule","global","direct"])]
        mode: String,
    },
    /// Print proxy environment assignments
    Env {
        #[arg(long,default_value="bash",value_parser=["bash","zsh"])]
        shell: String,
    },
    /// Run a program with proxy environment variables
    Exec {
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
}
#[derive(Subcommand)]
enum Core {
    Rollback,
    /// Install the release bundle offline, or select an existing core with --path
    Install {
        /// Use an existing independently verified executable
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Explicitly download and install an upstream core release
    Update {
        #[arg(long, default_value=clash_core::installer::DEFAULT_VERSION)]
        version: String,
    },
}
#[derive(Subcommand)]
enum Profile {
    Add { name: String, source: String },
    List,
    Use { name: String },
    Update { name: String },
    Remove { name: String },
}
#[derive(Subcommand)]
enum Node {
    /// Import a single share URL into a local profile
    Import {
        link: String,
        #[arg(long)]
        profile: String,
    },
    List,
    Select {
        name: String,
        #[arg(long)]
        group: String,
    },
    #[command(
        after_long_help = "Examples:\n  clash-cli node test NODE --timeout 5000\n  clash-cli node test --group PROXY\n  clash-cli --json node test --all --concurrency 8\n\nTimeouts are milliseconds. Results are sorted by latency; failures produce a nonzero exit code after results."
    )]
    Test {
        /// Test one named node; choose this, --group, or --all
        name: Option<String>,
        /// Test the members of a named proxy group
        #[arg(long, conflicts_with = "name")]
        group: Option<String>,
        #[arg(long, conflicts_with_all=["name","group"])]
        all: bool,
        /// Maximum simultaneous tests (1..256)
        #[arg(long, default_value_t = 4)]
        concurrency: usize,
        #[arg(long, default_value = "https://www.gstatic.com/generate_204")]
        url: String,
        /// Per-node timeout in milliseconds (1..300000)
        #[arg(long, default_value_t = 5000)]
        timeout: u64,
    },
}
#[derive(Subcommand)]
enum Group {
    List,
}
fn ensure_test_nodes(names: &[String]) -> Result<()> {
    if names.is_empty() {
        bail!(
            "No nodes available to test; wait for core/provider readiness, check node list, or select a nonempty group"
        );
    }
    Ok(())
}

/// Built-in routing policies have no remote endpoint to latency-test.
/// Match API types, not user-controlled names (a real node may be named PASS).
fn is_network_node(proxy: &serde_json::Value) -> bool {
    proxy["all"].is_null()
        && proxy["type"].as_str().is_some_and(|kind| {
            !matches!(
                kind,
                "Direct" | "Reject" | "RejectDrop" | "Compatible" | "Pass" | "PassRule" | "Dns"
            )
        })
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn needs_mutation_lock(command: &Cmd) -> bool {
    // systemd serializes lifecycle jobs; holding this lock across start/restart
    // prevents the spawned service from acquiring its own preparation lock.
    !(matches!(
        command,
        Cmd::Run
            | Cmd::Status
            | Cmd::Doctor
            | Cmd::Logs
            | Cmd::Env { .. }
            | Cmd::Exec { .. }
            | Cmd::Node {
                command: Node::Test { .. } | Node::List
            }
            | Cmd::Group { .. }
            | Cmd::Profile {
                command: Profile::List
            }
    ) || matches!(command,Cmd::Service{action} if matches!(action.as_str(),"status"|"start"|"stop"|"restart")))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(&cli.command,Cmd::Service{action} if action=="install") && cli.data_dir.is_none() {
        bail!("service install requires explicit --data-dir");
    }
    let a = App::new(match cli.data_dir {
        Some(p) => p,
        None => clash_core::platform::default_data_dir()?,
    })?;
    let _lock = if needs_mutation_lock(&cli.command) {
        Some(a.lock()?)
    } else {
        None
    };
    let out = match cli.command {
        Cmd::Core {
            command: Core::Install { path },
        } => {
            let p = match path {
                Some(p) => clash_core::absolute(&p)?,
                None => clash_core::installer::install_bundled(&a.dir).await?,
            };
            let output = Command::new(&p).arg("-v").output()?;
            if !output.status.success() {
                bail!("Core version check failed");
            }
            let mut s = a.state()?;
            s["core"] = json!(p);
            a.save(&s)?;
            json!({"installed":p})
        }
        Cmd::Core {
            command: Core::Update { version },
        } => {
            let p = clash_core::installer::install(&a.dir, &version).await?;
            let mut state = a.state()?;
            state["core"] = json!(p);
            a.save(&state)?;
            json!({"installed":p,"version":version})
        }
        Cmd::Core {
            command: Core::Rollback,
        } => {
            let p = clash_core::installer::rollback(&a.dir)?;
            let mut state = a.state()?;
            state["core"] = json!(p);
            a.save(&state)?;
            json!({"core":p})
        }
        Cmd::Service { action } => clash_core::platform::service(&action, &a.dir)?,
        Cmd::Logs => {
            let status = Command::new("journalctl")
                .args(["-u", "clash-cli.service", "-n", "100", "--no-pager"])
                .status()?;
            if !status.success() {
                bail!("journalctl failed");
            }
            return Ok(());
        }
        Cmd::Tun { action, exclude } => {
            let mut state = a.state()?;
            let exclusions = if action == "on" {
                clash_core::platform::tun_preflight(&exclude)?
            } else {
                vec![]
            };
            state["tun"] = json!(action == "on");
            state["tun_exclusions"] = json!(exclusions);
            a.activate(state).await?;
            json!({"tun":action})
        }
        Cmd::Profile { command } => match command {
            Profile::Add { name, source } => {
                a.add(&name, &source).await?;
                json!({"added":name})
            }
            Profile::List => {
                let s = a.state()?;
                json!({"profiles":s["profiles"].as_object().context("Invalid profile registry")?.keys().collect::<Vec<_>>(),"active":s["active"]})
            }
            Profile::Use { name } => {
                let mut s = a.state()?;
                if s["profiles"][&name].is_null() {
                    bail!("Unknown profile");
                }
                s["active"] = json!(name);
                a.activate(s).await?;
                json!({"active":name})
            }
            Profile::Update { name } => {
                let s = a.state()?;
                let source = s["profiles"][&name].as_str().context("Unknown profile")?;
                a.add(&name, source).await?;
                json!({"updated":name})
            }
            Profile::Remove { name } => {
                let mut s = a.state()?;
                if s["active"] == name {
                    bail!("Cannot remove active profile");
                }
                s["profiles"]
                    .as_object_mut()
                    .context("Invalid registry")?
                    .remove(&name)
                    .context("Unknown profile")?;
                a.save(&s)?;
                std::fs::remove_file(a.dir.join(format!("{name}.yaml")))?;
                json!({"removed":name})
            }
        },
        Cmd::Run => {
            a.run().await?;
            json!({"stopped":true})
        }
        Cmd::Status => a.api("GET", "/version", None).await?,
        Cmd::Config {
            mixed_port,
            controller_port,
        } => {
            if mixed_port == 0 || controller_port == 0 || mixed_port == controller_port {
                bail!("Ports must be nonzero and different");
            }
            let mut state = a.state()?;
            if std::net::TcpStream::connect_timeout(
                &format!(
                    "127.0.0.1:{}",
                    state["controller_port"].as_u64().unwrap_or(9090)
                )
                .parse()?,
                std::time::Duration::from_millis(200),
            )
            .is_ok()
            {
                bail!("Stop the core before changing listener ports");
            }
            state["mixed_port"] = json!(mixed_port);
            state["controller_port"] = json!(controller_port);
            a.save(&state)?;
            json!({"restart_required":true})
        }
        Cmd::Doctor => {
            use clash_core::platform::Platform;
            let state = a.state()?;
            json!({"capabilities":clash_core::platform::NativePlatform.capabilities(),"data_dir":a.dir,"state_version":state["version"],"core_configured":state["core"].is_string(),"core_exists":state["core"].as_str().is_some_and(|p|std::path::Path::new(p).is_file()),"active_profile":state["active"],"profile_count":state["profiles"].as_object().context("Invalid profiles")?.len(),"mixed_port":state["mixed_port"].as_u64().unwrap_or(7890),"controller_port":state["controller_port"].as_u64().unwrap_or(9090)})
        }
        Cmd::Node { command } => match command {
            Node::Import { link, profile } => {
                let node = clash_core::links::parse_link(&link)?;
                let state = a.state()?;
                if state["profiles"][&profile]
                    .as_str()
                    .is_some_and(|s| s.starts_with("https://"))
                {
                    bail!("Import into a local profile, not a subscription");
                }
                if profile.is_empty()
                    || !profile
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    bail!("Invalid profile name");
                }
                let path = a.dir.join(format!("{profile}.yaml"));
                let mut config: serde_json::Value = if path.exists() {
                    serde_json::from_value(serde_json::to_value(serde_yaml::from_slice::<
                        serde_yaml::Value,
                    >(
                        &std::fs::read(&path)?
                    )?)?)?
                } else {
                    json!({"proxies":[],"proxy-groups":[{"name":"PROXY","type":"select","proxies":[]}],"rules":["MATCH,PROXY"]})
                };
                let node = serde_json::to_value(node)?;
                if config["proxies"]
                    .as_array()
                    .is_some_and(|nodes| nodes.iter().any(|n| n["name"] == node["name"]))
                {
                    bail!("Duplicate node name");
                }
                config["proxies"]
                    .as_array_mut()
                    .context("proxies must be an array")?
                    .push(node.clone());
                if let Some(groups) = config["proxy-groups"].as_array_mut() {
                    for group in groups {
                        if group["type"] == "select" {
                            group["proxies"]
                                .as_array_mut()
                                .context("Invalid group proxies")?
                                .push(node["name"].clone());
                        }
                    }
                }
                a.put_profile(
                    &profile,
                    path.to_str().context("Non-UTF8 profile path")?,
                    serde_yaml::to_string(&config)?.as_bytes(),
                )
                .await?;
                json!({"imported":node["name"],"profile":profile})
            }
            Node::List => a.api("GET", "/proxies", None).await?,
            Node::Select { name, group } => {
                a.api(
                    "PUT",
                    &format!("/proxies/{}", encode(&group)),
                    Some(json!({"name":name})),
                )
                .await?;
                let mut s = a.state()?;
                s["selections"][group] = json!(name);
                a.save(&s)?;
                json!({"selected":name})
            }
            Node::Test {
                name,
                group,
                all,
                url,
                timeout,
                concurrency,
            } => {
                if !(1..=256).contains(&concurrency) || !(1..=300_000).contains(&timeout) {
                    bail!("Concurrency must be 1..256 and timeout 1..300000 milliseconds");
                }
                let proxies = a.api("GET", "/proxies", None).await?;
                let map = proxies["proxies"]
                    .as_object()
                    .context("Invalid proxy response")?;
                let names: Vec<String> = if let Some(name) = name {
                    vec![name]
                } else if let Some(group) = group {
                    map.get(&group).context("Unknown group")?["all"]
                        .as_array()
                        .context("Not a proxy group")?
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .context("Invalid group member")
                        })
                        .collect::<Result<_>>()?
                } else if all {
                    map.iter()
                        .filter(|(_, proxy)| is_network_node(proxy))
                        .map(|(name, _)| name.clone())
                        .collect()
                } else {
                    bail!("Specify NAME, --group GROUP, or --all");
                };
                ensure_test_nodes(&names)?;
                let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
                let mut tasks = tokio::task::JoinSet::new();
                for name in names {
                    let permit = semaphore.clone().acquire_owned().await?;
                    let app = App::new(a.dir.clone())?;
                    let url = url.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let path = format!(
                            "/proxies/{}/delay?url={}&timeout={timeout}",
                            encode(&name),
                            encode(&url)
                        );
                        let request = app.api("GET", &path, None);
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout.saturating_add(1000)),
                            request,
                        )
                        .await
                        {
                            Ok(Ok(result)) => {
                                json!({"name":name,"delay":result["delay"],"status":"ok"})
                            }
                            Ok(Err(error)) => json!({"name":name,"status":if clash_core::is_timeout(&error){"timeout"}else{"failed"}}),
                            Err(_) => json!({"name":name,"status":"timeout"}),
                        }
                    });
                }
                let mut results = Vec::new();
                while let Some(result) = tasks.join_next().await {
                    results.push(result?);
                }
                results.sort_by_key(|v| v["delay"].as_u64().unwrap_or(u64::MAX));
                json!(results)
            }
        },
        Cmd::Group {
            command: Group::List,
        } => a.api("GET", "/group", None).await?,
        Cmd::Mode { mode } => {
            let mut s = a.state()?;
            s["mode"] = json!(mode);
            a.activate(s).await?;
            json!({"mode":mode})
        }
        Cmd::Env { .. } => {
            let port = a.state()?["mixed_port"].as_u64().unwrap_or(7890);
            println!(
                "export http_proxy='http://127.0.0.1:{port}'\nexport https_proxy='http://127.0.0.1:{port}'\nexport all_proxy='socks5h://127.0.0.1:{port}'"
            );
            return Ok(());
        }
        Cmd::Exec { command } => {
            let port = a.state()?["mixed_port"].as_u64().unwrap_or(7890);
            let status = Command::new(&command[0])
                .args(&command[1..])
                .env("http_proxy", format!("http://127.0.0.1:{port}"))
                .env("https_proxy", format!("http://127.0.0.1:{port}"))
                .env("all_proxy", format!("socks5h://127.0.0.1:{port}"))
                .status()?;
            std::process::exit(status.code().unwrap_or(1));
        }
    };
    println!(
        "{}",
        if cli.json {
            serde_json::to_string(&out)?
        } else {
            serde_json::to_string_pretty(&out)?
        }
    );
    if out.as_array().is_some_and(|rows| {
        rows.iter()
            .any(|v| v["status"] == "failed" || v["status"] == "timeout")
    }) {
        bail!("Some node tests failed; inspect results above");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_jobs_do_not_hold_the_childs_configuration_lock() {
        for action in ["status", "start", "stop", "restart"] {
            assert!(
                !needs_mutation_lock(&Cmd::Service {
                    action: action.into()
                }),
                "{action}"
            );
        }
        for action in ["install", "uninstall"] {
            assert!(
                needs_mutation_lock(&Cmd::Service {
                    action: action.into()
                }),
                "{action}"
            );
        }
    }
    #[test]
    fn empty_test_selection_is_an_error() {
        let error = ensure_test_nodes(&[]).unwrap_err();
        assert!(error.to_string().contains("No nodes available"));
        assert!(ensure_test_nodes(&["remote".to_owned()]).is_ok());
    }
    #[test]
    fn all_tests_only_remote_nodes_not_routing_policies() {
        for kind in [
            "Direct",
            "Reject",
            "RejectDrop",
            "Compatible",
            "Pass",
            "PassRule",
            "Dns",
        ] {
            assert!(
                !is_network_node(&json!({"name":"custom policy name","type":kind})),
                "{kind}"
            );
        }
        for kind in ["Vless", "Trojan", "Shadowsocks", "WireGuard", "Hysteria2"] {
            assert!(
                is_network_node(&json!({"name":"PASS","type":kind})),
                "{kind}"
            );
        }
        assert!(!is_network_node(&json!({"type":"Selector","all":["node"]})));
        assert!(!is_network_node(&json!({"name":"missing type"})));
        let explicit = Cli::try_parse_from(["clash-cli", "node", "test", "PASS"]).unwrap();
        assert!(
            matches!(explicit.command,Cmd::Node{command:Node::Test{name:Some(name),..}} if name=="PASS")
        );
    }
    #[test]
    fn automation_and_batch_arguments_parse() {
        let cli = Cli::try_parse_from([
            "clash-cli",
            "--json",
            "node",
            "test",
            "--all",
            "--concurrency",
            "8",
        ])
        .unwrap();
        assert!(cli.json);
        assert!(matches!(
            cli.command,
            Cmd::Node {
                command: Node::Test {
                    all: true,
                    concurrency: 8,
                    ..
                }
            }
        ));
        assert!(Cli::try_parse_from(["clash-cli", "node", "test", "one", "--all"]).is_err());
    }
    #[test]
    fn command_arguments_preserve_child_flags() {
        let cli = Cli::try_parse_from([
            "clash-cli",
            "exec",
            "--",
            "curl",
            "-I",
            "https://example.com",
        ])
        .unwrap();
        assert!(
            matches!(cli.command,Cmd::Exec{command} if command==["curl","-I","https://example.com"])
        );
    }
}
