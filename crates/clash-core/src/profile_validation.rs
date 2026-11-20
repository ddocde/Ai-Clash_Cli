//! Reject provider error envelopes before Mihomo silently treats them as defaults.
use anyhow::{Context, Result, bail};
use serde_yaml::Value;

// This is a minimum shape guard, not a strict schema: extension fields remain
// permitted beside a recognized setting. The engine validates actual values.
const SETTINGS: &[&str] = &[
    "proxies",
    "proxy-providers",
    "proxy-groups",
    "rules",
    "rule-providers",
    "sub-rules",
    "dns",
    "tun",
    "listeners",
    "mode",
    "port",
    "socks-port",
    "mixed-port",
    "redir-port",
    "tproxy-port",
    "allow-lan",
    "bind-address",
    "authentication",
    "skip-auth-prefixes",
    "lan-allowed-ips",
    "lan-disallowed-ips",
    "ipv6",
    "log-level",
    "external-controller",
    "external-controller-tls",
    "external-controller-unix",
    "external-controller-pipe",
    "secret",
    "external-ui",
    "external-ui-url",
    "external-ui-name",
    "interface-name",
    "routing-mark",
    "hosts",
    "profile",
    "sniffer",
    "ntp",
    "geodata-mode",
    "geodata-loader",
    "geox-url",
    "geo-auto-update",
    "geo-update-interval",
    "find-process-mode",
    "global-client-fingerprint",
    "global-ua",
    "unified-delay",
    "tcp-concurrent",
    "keep-alive-idle",
    "keep-alive-interval",
    "disable-keep-alive",
    "tunnels",
];

pub(crate) fn parse(data: &[u8]) -> Result<Value> {
    let value: Value = serde_yaml::from_slice(data)
        .map_err(|_| anyhow::anyhow!("Invalid YAML profile (contents redacted)"))?;
    // Resolve aliases only for shape inspection. Preserve the parsed structure
    // and original profile bytes for the normal configuration pipeline.
    let mut inspected = value.clone();
    inspected
        .apply_merge()
        .map_err(|_| anyhow::anyhow!("Invalid YAML merge in profile (contents redacted)"))?;
    let mapping = inspected
        .as_mapping()
        .context("Profile must be a YAML mapping")?;
    if !mapping
        .keys()
        .any(|key| key.as_str().is_some_and(|key| SETTINGS.contains(&key)))
    {
        bail!(
            "Profile contains no recognized Mihomo settings; subscription may contain a provider error instead of configuration (contents redacted)"
        );
    }
    Ok(value)
}
