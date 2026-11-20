//! Verified, explicit Mihomo installation; no privilege escalation.
use anyhow::{Context, Result, bail};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const DEFAULT_VERSION: &str = "v1.19.34";
const MAX_ARCHIVE_BYTES: usize = 100 * 1024 * 1024;
const MAX_ENGINE_BYTES: u64 = 200 * 1024 * 1024;

/// Resolve an upstream asset without exposing operating-system assumptions to callers.
pub fn asset_name(os: &str, arch: &str, version: &str) -> Result<String> {
    validate_version(version)?;
    let target = match (os, arch) {
        ("linux", "x86_64") => "linux-amd64-compatible",
        ("linux", "aarch64") => "linux-arm64",
        _ => {
            bail!("Automatic engine installation is not implemented for this platform; use --path")
        }
    };
    Ok(format!("mihomo-{target}-{version}.gz"))
}
fn validate_version(version: &str) -> Result<()> {
    let Some(v) = version.strip_prefix('v') else {
        bail!("Version must have format vMAJOR.MINOR.PATCH");
    };
    let parts: Vec<_> = v.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        bail!("Version must have format vMAJOR.MINOR.PATCH");
    }
    Ok(())
}
fn verify(bytes: &[u8], digest: &str) -> Result<()> {
    let expected = digest.strip_prefix("sha256:").context(
        "Upstream release has no SHA-256 digest; use an independently verified offline core",
    )?;
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("Malformed upstream SHA-256 digest");
    }
    let actual = format!("{:x}", Sha256::digest(bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("Downloaded engine checksum mismatch; existing engine was not changed");
    }
    Ok(())
}
async fn limited_bytes(mut response: reqwest::Response) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|n| n > MAX_ARCHIVE_BYTES as u64)
    {
        bail!("Engine download exceeds size limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > MAX_ARCHIVE_BYTES {
            bail!("Engine download exceeds size limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn persist(dir: &Path, name: &str, data: &[u8], executable: bool) -> Result<PathBuf> {
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(if executable {
                0o700
            } else {
                0o600
            }))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    temp.as_file().sync_all()?;
    let destination = dir.join(name);
    temp.persist(&destination).map_err(|e| e.error)?;
    Ok(destination)
}
/// Download an exact release and verify GitHub's published SHA-256 before executing it.
pub async fn install(dir: &Path, version: &str) -> Result<PathBuf> {
    let name = asset_name(std::env::consts::OS, std::env::consts::ARCH, version)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent("clash-cli")
        .build()?;
    let metadata: Value = client
        .get(format!(
            "https://api.github.com/repos/MetaCubeX/mihomo/releases/tags/{version}"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let asset = metadata["assets"]
        .as_array()
        .context("Malformed upstream release metadata")?
        .iter()
        .find(|a| a["name"] == name)
        .context("Engine asset is unavailable for this release")?;
    let digest = asset["digest"].as_str().context(
        "Upstream release has no SHA-256 digest; use an independently verified offline core",
    )?;
    // Construct the URL ourselves; release metadata cannot redirect us to an arbitrary asset URL.
    let bytes = limited_bytes(
        client
            .get(format!(
                "https://github.com/MetaCubeX/mihomo/releases/download/{version}/{name}"
            ))
            .send()
            .await?
            .error_for_status()?,
    )
    .await?;
    install_archive(dir, &bytes, digest, version).await
}

async fn install_archive(dir: &Path, bytes: &[u8], digest: &str, version: &str) -> Result<PathBuf> {
    verify(bytes, digest)?;
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_ENGINE_BYTES + 1)
        .read_to_end(&mut decoded)?;
    if decoded.is_empty() || decoded.len() as u64 > MAX_ENGINE_BYTES {
        bail!("Invalid engine decompressed size");
    }
    fs::create_dir_all(dir)?;
    let mut candidate = tempfile::NamedTempFile::new_in(dir)?;
    candidate.write_all(&decoded)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        candidate
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o700))?;
    }
    candidate.as_file().sync_all()?;
    // Close the writable descriptor before execution (Linux ETXTBSY).
    let path = candidate.into_temp_path();
    let output = tokio::process::Command::new(&path)
        .arg("-v")
        .output()
        .await
        .context("Downloaded engine could not execute")?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .any(|part| part == version)
    {
        bail!("Engine failed version check or does not match the requested version");
    }
    let current = dir.join("mihomo");
    if current.exists() {
        persist(dir, "mihomo.previous", &fs::read(&current)?, true)?;
    }
    path.persist(&current).map_err(|e| e.error)?;
    Ok(fs::canonicalize(current)?)
}
/// Install only the packaged archive adjacent to this CLI; never fetch implicitly.
pub async fn install_bundled(dir: &Path) -> Result<PathBuf> {
    let executable = std::env::current_exe()?;
    let bundle = executable
        .parent()
        .context("CLI executable has no parent directory")?
        .join("bundled");
    if bundle.exists() {
        return install_bundle_from(dir, &bundle).await;
    }
    let prefix = executable
        .parent()
        .and_then(Path::parent)
        .context("CLI executable has no installation prefix")?;
    install_bundle_from(dir, &prefix.join("lib/clash-cli/bundled")).await
}

/// Explicit bundle directory also allows deterministic offline installation tests.
pub async fn install_bundle_from(dir: &Path, bundle: &Path) -> Result<PathBuf> {
    let manifest_path = bundle.join("manifest.json");
    if !manifest_path.is_file() || !bundle.join("mihomo.gz").is_file() {
        bail!(
            "Bundled core not found beside clash-cli; retain the bundled directory from the release package, run core update to download, or use core install --path /path/to/mihomo"
        );
    }
    if fs::metadata(&manifest_path)?.len() > 16 * 1024 {
        bail!("Bundle manifest exceeds size limit");
    }
    let manifest: Value = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let version = manifest["version"]
        .as_str()
        .context("Bundle manifest missing version")?;
    if version != DEFAULT_VERSION {
        bail!("Bundle version differs from the core pinned to this CLI release");
    }
    let expected_asset = asset_name(std::env::consts::OS, std::env::consts::ARCH, version)?;
    if manifest["asset"] != expected_asset {
        bail!("Bundle asset does not match this platform and version");
    }
    let digest = manifest["digest"]
        .as_str()
        .context("Bundle manifest missing SHA-256 digest")?;
    let archive = fs::File::open(bundle.join("mihomo.gz"))?;
    let mut bytes = Vec::new();
    archive
        .take(MAX_ARCHIVE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_ARCHIVE_BYTES {
        bail!("Bundled archive exceeds size limit");
    }
    install_archive(dir, &bytes, digest, version).await
}

/// Swap the current and previous verified binaries. The caller must stop its engine first.
pub fn rollback(dir: &Path) -> Result<PathBuf> {
    let previous = dir.join("mihomo.previous");
    let old = fs::read(&previous).context("No previous engine is available")?;
    let current = dir.join("mihomo");
    let now = fs::read(&current).context("No managed engine is installed")?;
    persist(dir, "mihomo", &old, true)?;
    if let Err(error) = persist(dir, "mihomo.previous", &now, true) {
        persist(dir, "mihomo", &now, true)
            .context("Rollback metadata write and engine restoration both failed")?;
        return Err(error);
    }
    Ok(fs::canonicalize(current)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets() {
        assert_eq!(
            asset_name("linux", "x86_64", DEFAULT_VERSION).unwrap(),
            format!("mihomo-linux-amd64-compatible-{DEFAULT_VERSION}.gz")
        );
        assert!(asset_name("windows", "x86_64", DEFAULT_VERSION).is_err());
        assert!(asset_name("linux", "x86_64", "../../bad").is_err());
    }
    #[test]
    fn checksums() {
        let d = format!("sha256:{:x}", Sha256::digest(b"engine"));
        verify(b"engine", &d).unwrap();
        assert!(verify(b"tampered", &d).is_err());
        assert!(verify(b"engine", "sha256:bad").is_err());
    }
    #[test]
    fn rollback_swaps() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mihomo"), b"new").unwrap();
        fs::write(dir.path().join("mihomo.previous"), b"old").unwrap();
        rollback(dir.path()).unwrap();
        assert_eq!(fs::read(dir.path().join("mihomo")).unwrap(), b"old");
        assert_eq!(
            fs::read(dir.path().join("mihomo.previous")).unwrap(),
            b"new"
        );
    }
    #[cfg(target_os = "linux")]
    fn fixture_bundle(bundle: &Path, program: &[u8]) -> Result<()> {
        use flate2::{Compression, write::GzEncoder};
        fs::create_dir_all(bundle)?;
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(program)?;
        let bytes = gzip.finish()?;
        fs::write(bundle.join("mihomo.gz"), &bytes)?;
        fs::write(
            bundle.join("manifest.json"),
            serde_json::to_vec(
                &serde_json::json!({"version":DEFAULT_VERSION,"asset":asset_name(std::env::consts::OS,std::env::consts::ARCH,DEFAULT_VERSION)?,"digest":format!("sha256:{:x}",Sha256::digest(&bytes))}),
            )?,
        )?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn bundle_install_is_offline_verified_and_preserves_previous() -> Result<()> {
        let root = tempfile::tempdir()?;
        let bundle = root.path().join("bundle");
        let data = root.path().join("data");
        fs::create_dir(&data)?;
        fs::write(data.join("mihomo"), b"previous")?;
        fixture_bundle(
            &bundle,
            format!("#!/bin/sh\necho Mihomo Meta {DEFAULT_VERSION}\n").as_bytes(),
        )?;
        let installed = install_bundle_from(&data, &bundle).await?;
        assert!(installed.is_file());
        assert_eq!(fs::read(data.join("mihomo.previous"))?, b"previous");
        Ok(())
    }
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn bundle_rejects_tampering_platform_and_binary_version() -> Result<()> {
        let root = tempfile::tempdir()?;
        let bundle = root.path().join("bundle");
        let data = root.path().join("data");
        fs::create_dir(&data)?;
        fs::write(data.join("mihomo"), b"original")?;
        fixture_bundle(&bundle, b"#!/bin/sh\necho wrong-version\n")?;
        assert!(install_bundle_from(&data, &bundle).await.is_err());
        let manifest_path = bundle.join("manifest.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
        manifest["asset"] = serde_json::json!("wrong-platform");
        fs::write(&manifest_path, serde_json::to_vec(&manifest)?)?;
        assert!(install_bundle_from(&data, &bundle).await.is_err());
        fixture_bundle(
            &bundle,
            format!("#!/bin/sh\necho {DEFAULT_VERSION}\n").as_bytes(),
        )?;
        fs::write(bundle.join("mihomo.gz"), b"tampered")?;
        assert!(install_bundle_from(&data, &bundle).await.is_err());
        assert_eq!(fs::read(data.join("mihomo"))?, b"original");
        assert!(!data.join("mihomo.previous").exists());
        Ok(())
    }
    #[tokio::test]
    async fn missing_bundle_does_not_download() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let error = install_bundle_from(dir.path(), &dir.path().join("absent"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("core update"));
        Ok(())
    }
}
