//! Strict share-link conversion. Errors intentionally omit input credentials.
use anyhow::{Result, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;

fn decode(s: &str) -> Result<String> {
    for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
        if let Ok(bytes) = engine.decode(s) {
            return String::from_utf8(bytes)
                .map_err(|_| anyhow::anyhow!("Invalid UTF-8 in encoded link"));
        }
    }
    bail!("Invalid base64 share link")
}
fn unescape(s: &str) -> Result<String> {
    percent_encoding::percent_decode_str(s)
        .decode_utf8()
        .map(|s| s.into_owned())
        .map_err(|_| anyhow::anyhow!("Invalid UTF-8 in share link"))
}
fn port(v: &Value) -> Result<u16> {
    let p = match v {
        Value::String(s) => s.parse::<u16>().ok(),
        Value::Number(n) => n.as_u64().and_then(|n| u16::try_from(n).ok()),
        _ => None,
    };
    match p {
        Some(p) if p > 0 => Ok(p),
        _ => bail!("Invalid node port"),
    }
}
fn transport(out: &mut Value, kind: &str, path: Option<&str>, host: Option<&str>) -> Result<()> {
    match kind {
        "tcp" | "" => {
            if path.is_some_and(|p| !p.is_empty()) || host.is_some_and(|h| !h.is_empty()) {
                bail!("TCP transport does not accept path or host parameters");
            }
        }
        "ws" => {
            out["network"] = json!("ws");
            if let Some(p) = path {
                out["ws-opts"]["path"] = json!(p);
            }
            if let Some(h) = host {
                out["ws-opts"]["headers"]["Host"] = json!(h);
            }
        }
        "grpc" => {
            if host.is_some_and(|h| !h.is_empty()) {
                bail!("gRPC host parameter is unsupported; use sni");
            }
            out["network"] = json!("grpc");
            if let Some(p) = path {
                out["grpc-opts"]["grpc-service-name"] = json!(p);
            }
        }
        _ => bail!("Unsupported share-link transport"),
    }
    Ok(())
}
/// Convert one SS, VMess, VLESS or Trojan link to a Mihomo proxy mapping.
pub fn parse_link(input: &str) -> Result<serde_yaml::Value> {
    let input = input.trim();
    let value = if let Some(encoded) = input.strip_prefix("vmess://") {
        vmess(encoded)?
    } else if let Some(rest) = input.strip_prefix("ss://") {
        ss(rest)?
    } else {
        uri(input)?
    };
    Ok(serde_yaml::to_value(value)?)
}
fn vmess(encoded: &str) -> Result<Value> {
    let data = decode(encoded)?;
    let v: Value =
        serde_json::from_str(&data).map_err(|_| anyhow::anyhow!("Invalid VMess JSON"))?;
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("VMess must contain an object"))?;
    let allowed = [
        "v", "ps", "add", "port", "id", "aid", "scy", "net", "type", "host", "path", "tls", "sni",
        "alpn", "fp",
    ];
    if obj.keys().any(|k| !allowed.contains(&k.as_str())) {
        bail!("Unsupported VMess field");
    }
    if v["type"]
        .as_str()
        .is_some_and(|s| !s.is_empty() && s != "none")
    {
        bail!("Unsupported VMess header type");
    }
    let get = |k: &str| {
        v[k].as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing VMess field: {k}"))
    };
    let server = get("add")?;
    let mut out = json!({"name":v["ps"].as_str().filter(|s| !s.is_empty()).unwrap_or(server),"type":"vmess","server":server,"port":port(&v["port"])? ,"uuid":get("id")?,"cipher":v["scy"].as_str().unwrap_or("auto"),"udp":true});
    let aid = match &v["aid"] {
        Value::Null => 0,
        Value::Number(n) => n
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Invalid alterId"))?,
        Value::String(s) => s.parse().map_err(|_| anyhow::anyhow!("Invalid alterId"))?,
        _ => bail!("Invalid alterId"),
    };
    out["alterId"] = json!(aid);
    transport(
        &mut out,
        v["net"].as_str().unwrap_or("tcp"),
        v["path"].as_str().filter(|s| !s.is_empty()),
        v["host"].as_str().filter(|s| !s.is_empty()),
    )?;
    match v["tls"].as_str().unwrap_or("") {
        "" | "none" => {}
        "tls" => out["tls"] = json!(true),
        _ => bail!("Unsupported VMess TLS mode"),
    }
    for (src, dst) in [("sni", "servername"), ("fp", "client-fingerprint")] {
        if let Some(s) = v[src].as_str().filter(|s| !s.is_empty()) {
            out[dst] = json!(s);
        }
    }
    if let Some(s) = v["alpn"].as_str().filter(|s| !s.is_empty()) {
        out["alpn"] = json!(s.split(',').collect::<Vec<_>>());
    }
    Ok(out)
}
fn ss(rest: &str) -> Result<Value> {
    let (body, frag) = rest.split_once('#').unwrap_or((rest, ""));
    let body = if body.contains('@') {
        body.to_owned()
    } else {
        decode(body)?
    };
    let (auth, server) = body
        .rsplit_once('@')
        .ok_or_else(|| anyhow::anyhow!("SS link requires credentials and server"))?;
    let auth = if auth.contains(':') {
        unescape(auth)?
    } else {
        decode(auth)?
    };
    let (cipher, password) = auth
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid SS credentials"))?;
    if cipher.is_empty() || password.is_empty() {
        bail!("Empty SS credentials");
    }
    let url = Url::parse(&format!("ss://{server}"))
        .map_err(|_| anyhow::anyhow!("Invalid SS server address"))?;
    if url.query().is_some() || !matches!(url.path(), "" | "/") {
        bail!("SS plugins and path parameters are not supported");
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing SS server"))?;
    let port = url
        .port()
        .filter(|p| *p > 0)
        .ok_or_else(|| anyhow::anyhow!("Missing SS port"))?;
    Ok(
        json!({"name":if frag.is_empty(){host.to_owned()}else{unescape(frag)?},"type":"ss","server":host.trim_matches(['[',']']),"port":port,"cipher":cipher,"password":password,"udp":true}),
    )
}
fn uri(input: &str) -> Result<Value> {
    let u = Url::parse(input).map_err(|_| anyhow::anyhow!("Invalid share link"))?;
    if !matches!(u.scheme(), "vless" | "trojan") {
        bail!("Supported links: ss, vmess, vless, trojan");
    }
    if u.password().is_some() || !matches!(u.path(), "" | "/") {
        bail!("Unsupported link credentials or path");
    }
    let host = u
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing node server"))?;
    let port = u
        .port()
        .filter(|p| *p > 0)
        .ok_or_else(|| anyhow::anyhow!("Missing node port"))?;
    let credential = unescape(u.username())?;
    if credential.is_empty() {
        bail!("Missing node credentials");
    }
    let mut q = BTreeMap::new();
    for (k, v) in u.query_pairs() {
        if q.insert(k.into_owned(), v.into_owned()).is_some() {
            bail!("Duplicate share-link parameter");
        }
    }
    let allowed = [
        "type",
        "security",
        "sni",
        "peer",
        "host",
        "path",
        "serviceName",
        "flow",
        "encryption",
        "fp",
        "alpn",
        "pbk",
        "sid",
        "allowInsecure",
    ];
    if q.keys().any(|k| !allowed.contains(&k.as_str())) {
        bail!("Unsupported share-link parameter");
    }
    let get = |k: &str| q.get(k).map(String::as_str);
    if get("encryption").is_some_and(|v| v != "none") {
        bail!("Unsupported VLESS encryption");
    }
    if u.scheme() == "trojan"
        && ["flow", "encryption", "pbk", "sid"]
            .iter()
            .any(|k| q.contains_key(*k))
    {
        bail!("VLESS-only parameter in Trojan link");
    }
    let mut out = json!({"name":match u.fragment(){Some(f)=>unescape(f)?,None=>host.to_owned()},"type":u.scheme(),"server":host.trim_matches(['[',']']),"port":port,"udp":true});
    out[if u.scheme() == "vless" {
        "uuid"
    } else {
        "password"
    }] = json!(credential);
    let security = get("security").unwrap_or(if u.scheme() == "trojan" {
        "tls"
    } else {
        "none"
    });
    match security {
        "none" if u.scheme() == "vless" => {}
        "tls" | "reality" => out["tls"] = json!(true),
        _ => bail!("Unsupported TLS security mode"),
    }
    if security == "reality" {
        out["reality-opts"]["public-key"] =
            json!(get("pbk").ok_or_else(|| anyhow::anyhow!("Reality requires pbk"))?);
        if let Some(s) = get("sid") {
            out["reality-opts"]["short-id"] = json!(s);
        }
    } else if get("pbk").is_some() || get("sid").is_some() {
        bail!("Reality parameters require security=reality");
    }
    if get("sni").is_some() && get("peer").is_some() {
        bail!("Specify only sni or peer");
    }
    if let Some(s) = get("sni").or_else(|| get("peer")) {
        out[if u.scheme() == "trojan" {
            "sni"
        } else {
            "servername"
        }] = json!(s);
    }
    for (src, dst) in [("flow", "flow"), ("fp", "client-fingerprint")] {
        if let Some(s) = get(src) {
            out[dst] = json!(s);
        }
    }
    if let Some(s) = get("alpn") {
        out["alpn"] = json!(s.split(',').collect::<Vec<_>>());
    }
    if let Some(s) = get("allowInsecure") {
        out["skip-cert-verify"] = json!(match s {
            "1" | "true" => true,
            "0" | "false" => false,
            _ => bail!("Invalid allowInsecure boolean"),
        });
    }
    let kind = get("type").unwrap_or("tcp");
    if get("serviceName").is_some() && kind != "grpc" {
        bail!("serviceName requires gRPC");
    }
    if get("serviceName").is_some() && get("path").is_some() {
        bail!("Specify only serviceName or path");
    }
    transport(
        &mut out,
        kind,
        get("serviceName").or_else(|| get("path")),
        get("host"),
    )?;
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ss_encoded_password() {
        let auth = STANDARD_NO_PAD.encode("aes-128-gcm:p:a");
        let v = parse_link(&format!("ss://{auth}@example.com:443#hello%20world")).unwrap();
        assert_eq!(v["password"].as_str(), Some("p:a"));
        assert_eq!(v["name"].as_str(), Some("hello world"));
    }
    #[test]
    fn vless_ws() {
        let v=parse_link("vless://id@example.com:443?security=tls&type=ws&path=%2Fws&host=cdn.example&sni=example.com#node").unwrap();
        assert_eq!(v["ws-opts"]["path"].as_str(), Some("/ws"));
        assert_eq!(v["tls"].as_bool(), Some(true));
    }
    #[test]
    fn reject_unknown_without_secret() {
        let e = parse_link("trojan://SECRET@example.com:443?mystery=SECRET")
            .unwrap_err()
            .to_string();
        assert!(!e.contains("SECRET"));
    }
    #[test]
    fn reject_duplicates() {
        assert!(parse_link("vless://id@host:443?type=ws&type=tcp").is_err());
    }
    #[test]
    fn vmess_base64() {
        let data = json!({"v":"2","ps":"n","add":"example.com","port":"443","id":"id","aid":"0","net":"ws","path":"/ws","tls":"tls"});
        let v = parse_link(&format!("vmess://{}", STANDARD.encode(data.to_string()))).unwrap();
        assert_eq!(v["port"].as_u64(), Some(443));
    }
    #[test]
    fn ipv6_trojan() {
        let v = parse_link("trojan://pass@[::1]:443#n").unwrap();
        assert_eq!(v["server"].as_str(), Some("::1"));
    }
    #[test]
    fn reject_silent_tcp_params() {
        assert!(parse_link("vless://id@host:443?type=tcp&path=/x").is_err());
    }
}
