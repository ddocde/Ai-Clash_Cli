use clash_core::App;
use serde_json::json;
#[test]
fn persistence_and_version_guard() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    let mut state = a.state()?;
    state["mode"] = json!("global");
    a.save(&state)?;
    assert_eq!(a.state()?["mode"], "global");
    state["version"] = json!(99);
    a.save(&state)?;
    assert!(a.state().is_err());
    Ok(())
}
#[tokio::test]
async fn reject_profile_path_traversal() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    assert!(a.add("../escape", "missing").await.is_err());
    Ok(())
}
#[tokio::test]
async fn failed_activation_restores_state() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    let original = a.state()?;
    a.save(&original)?;
    let mut next = original.clone();
    next["active"] = json!("missing");
    assert!(a.activate(next).await.is_err());
    assert_eq!(a.state()?, original);
    Ok(())
}
#[cfg(unix)]
#[test]
fn private_state_permissions() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().join("data"))?;
    a.save(&a.state()?)?;
    assert_eq!(
        std::fs::metadata(a.dir.join("state.json"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn rejected_update_keeps_profile_and_active_state() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    let engine = dir.path().join("reject-engine");
    std::fs::write(&engine, "#!/bin/sh\nexit 1\n")?;
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700))?;
    a.write("test.yaml", b"proxies: []\n")?;
    let mut state = a.state()?;
    state["core"] = json!(engine);
    state["active"] = json!("test");
    state["controller_port"] = json!(1);
    state["profiles"]["test"] = json!("original");
    a.save(&state)?;
    assert!(
        a.put_profile("test", "changed", b"proxies: []\nrules: []\n")
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(a.dir.join("test.yaml"))?, b"proxies: []\n");
    assert_eq!(a.state()?, state);
    Ok(())
}
#[test]
fn mutation_lock_excludes_concurrent_writers() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    let lock = a.lock()?;
    assert!(a.lock().is_err());
    drop(lock);
    a.lock()?;
    Ok(())
}
#[test]
fn corrupt_state_types_are_errors_not_panics() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    for (field, value) in [
        ("profiles", json!([])),
        ("selections", json!(4)),
        ("tun", json!("yes")),
        ("mode", json!(false)),
        ("active", json!("../escape")),
        ("controller_port", json!(65536)),
    ] {
        let mut state = json!({"version":1,"profiles":{},"mode":"rule","tun":false});
        state[field] = value;
        a.save(&state)?;
        assert!(a.state().is_err(), "{field}");
    }
    Ok(())
}
#[test]
fn scalar_profile_rejected_without_panicking() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    let mut state = a.state()?;
    state["profiles"]["test"] = json!("local");
    state["active"] = json!("test");
    a.save(&state)?;
    a.write("test.yaml", b"hello")?;
    assert!(a.prepare().is_err());
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn validating_inactive_profile_leaves_no_runtime_artifact() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir()?;
    let a = App::new(dir.path().to_owned())?;
    let engine = dir.path().join("accept-engine");
    std::fs::write(&engine, "#!/bin/sh\nexit 0\n")?;
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700))?;
    let mut state = a.state()?;
    state["core"] = json!(engine);
    a.save(&state)?;
    a.put_profile("test", "local", b"proxies: []\n").await?;
    assert!(!a.dir.join("runtime.yaml").exists());
    assert!(a.state()?["secret"].is_null());
    Ok(())
}

#[cfg(unix)]
fn acceptance_engine(app: &App) -> anyhow::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let engine = app.dir.join("shape-accept-engine");
    std::fs::write(
        &engine,
        "#!/bin/sh\nprintf invoked > \"$0.invoked\"\nexit 0\n",
    )?;
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700))?;
    let mut state = app.state()?;
    state["core"] = json!(engine);
    app.save(&state)?;
    Ok(app.dir.join("shape-accept-engine.invoked"))
}

#[cfg(unix)]
#[tokio::test]
async fn provider_error_payload_never_replaces_active_profile_or_executes_engine()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let app = App::new(dir.path().to_owned())?;
    let marker = acceptance_engine(&app)?;
    let original = b"mode: direct\nrules: ['MATCH,DIRECT']\n";
    app.write("active.yaml", original)?;
    app.write("runtime.yaml", b"previous runtime")?;
    let mut state = app.state()?;
    state["profiles"]["active"] = json!("original");
    state["active"] = json!("active");
    app.save(&state)?;
    let payloads: &[&[u8]] = &[
        br#"{"error":"subscription expired"}"#,
        b"{}",
        b"unexpected: contents",
        b"[]",
        b"secret-provider-token: [",
    ];
    for payload in payloads {
        let error = app
            .put_profile("active", "replacement", payload)
            .await
            .unwrap_err();
        assert!(!error.to_string().contains("secret-provider-token"));
        assert!(!marker.exists());
        assert_eq!(app.state()?, state);
        assert_eq!(std::fs::read(app.dir.join("active.yaml"))?, original);
        assert_eq!(
            std::fs::read(app.dir.join("runtime.yaml"))?,
            b"previous runtime"
        );
    }
    let source = app.dir.join("provider-error.json");
    std::fs::write(&source, payloads[0])?;
    assert!(app.add("active", source.to_str().unwrap()).await.is_err());
    assert_eq!(std::fs::read(app.dir.join("active.yaml"))?, original);
    // prepare also guards files edited externally, before generating a secret.
    app.write("active.yaml", payloads[0])?;
    assert!(app.prepare().is_err());
    assert!(!marker.exists());
    assert_eq!(app.state()?, state);
    assert_eq!(
        std::fs::read(app.dir.join("runtime.yaml"))?,
        b"previous runtime"
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn minimal_direct_dns_listener_and_extension_profiles_remain_accepted() -> anyhow::Result<()>
{
    let dir = tempfile::tempdir()?;
    let app = App::new(dir.path().to_owned())?;
    let marker = acceptance_engine(&app)?;
    for payload in [
        "mode: direct\n",
        "rules: ['MATCH,DIRECT']\n",
        "dns: {enable: true}\n",
        "listeners: []\n",
        "mode: direct\nfuture-extension: preserved\n",
    ] {
        app.put_profile("minimal", "local", payload.as_bytes())
            .await?;
        assert!(marker.exists());
        assert_eq!(
            std::fs::read(app.dir.join("minimal.yaml"))?,
            payload.as_bytes()
        );
        std::fs::remove_file(&marker)?;
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn merge_only_profile_is_accepted_without_rewriting_source() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let app = App::new(dir.path().to_owned())?;
    let marker = acceptance_engine(&app)?;
    let payload =
        b"# Preserve this comment and anchor\ndefaults: &base\n  mixed-port: 7890\n<<: *base\n";
    app.put_profile("merged", "local", payload).await?;
    assert!(marker.exists());
    assert_eq!(std::fs::read(app.dir.join("merged.yaml"))?, payload);
    std::fs::remove_file(&marker)?;
    let state = app.state()?;
    let error = app
        .put_profile(
            "merged",
            "replacement",
            b"mode: direct\n<<: private-subscription-token\n",
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("Invalid YAML merge"));
    assert!(!error.to_string().contains("private-subscription-token"));
    assert_eq!(std::fs::read(app.dir.join("merged.yaml"))?, payload);
    assert_eq!(app.state()?, state);
    assert!(!marker.exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn merged_runtime_settings_are_sanitized_without_changing_source() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let app = App::new(dir.path().to_owned())?;
    let marker = acceptance_engine(&app)?;
    let source = br#"dns-defaults: &dns
  enable: true
  listen: 0.0.0.0:5353
unsafe-defaults: &defaults
  allow-lan: true
  bind-address: '*'
  mixed-port: 7897
  external-controller: 0.0.0.0:9097
  external-controller-tls: 0.0.0.0:9443
  external-controller-unix: /tmp/foreign-controller.sock
  external-controller-pipe: foreign-controller
  listeners: [{name: foreign, type: http, port: 8888, listen: 0.0.0.0}]
  port: 8880
  socks-port: 8881
  redir-port: 8882
  tproxy-port: 8883
  dns:
    <<: *dns
<<: *defaults
"#;
    app.write("merged.yaml", source)?;
    let mut state = app.state()?;
    state["profiles"]["merged"] = json!("local");
    state["active"] = json!("merged");
    state["secret"] = json!("managed-test-secret");
    state["mixed_port"] = json!(17890);
    state["controller_port"] = json!(19090);
    app.save(&state)?;
    let runtime = app.prepare()?;
    assert!(marker.exists());
    let mut config: serde_yaml::Value = serde_yaml::from_slice(&std::fs::read(runtime)?)?;
    // Reapplying YAML merge as a consumer would must not restore unsafe keys.
    config.apply_merge()?;
    for key in [
        "<<",
        "external-controller-tls",
        "external-controller-unix",
        "external-controller-pipe",
        "listeners",
        "port",
        "socks-port",
        "redir-port",
        "tproxy-port",
    ] {
        assert!(config[key].is_null(), "{key}");
    }
    assert!(config["dns"]["listen"].is_null());
    assert_eq!(config["dns"]["enable"].as_bool(), Some(true));
    assert_eq!(config["allow-lan"].as_bool(), Some(false));
    assert_eq!(config["bind-address"].as_str(), Some("127.0.0.1"));
    assert_eq!(
        config["external-controller"].as_str(),
        Some("127.0.0.1:19090")
    );
    assert_eq!(config["mixed-port"].as_u64(), Some(17890));
    assert_eq!(config["secret"].as_str(), Some("managed-test-secret"));
    assert_eq!(std::fs::read(app.dir.join("merged.yaml"))?, source);
    Ok(())
}
