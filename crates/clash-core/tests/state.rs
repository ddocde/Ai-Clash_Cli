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
