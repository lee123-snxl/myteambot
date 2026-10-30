pub fn choose_proxy(url: &str, enable: bool, server: &str, bypass: &str) -> Option<String> {
    if !enable || server.trim().is_empty() {
        return None;
    }
    let target = url::Url::parse(url).ok()?;
    let host = target.host_str()?.to_string();
    if is_local_host(&host) || bypassed(&host, bypass) {
        return None;
    }
    proxy_url(server, target.scheme())
}

pub fn current_for(url: &str) -> Option<String> {
    let (enable, server, bypass) = read_settings()?;
    choose_proxy(url, enable, &server, &bypass)
}

fn is_local_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost"
        || host == "::1"
        || host == "0.0.0.0"
        || host.starts_with("127.")
        || host.ends_with(".localhost")
}

fn bypassed(host: &str, bypass: &str) -> bool {
    let host = host.to_ascii_lowercase();
    for raw in bypass.split([';', ',']) {
        let item = raw.trim().to_ascii_lowercase();
        if item.is_empty() {
            continue;
        }
        if item == "<local>" && (is_local_host(&host) || !host.contains('.')) {
            return true;
        }
        if let Some(suffix) = item.strip_prefix("*.") {
            if host == suffix || host.ends_with(&format!(".{suffix}")) {
                return true;
            }
        } else if host == item {
            return true;
        }
    }
    false
}

fn proxy_url(server: &str, scheme: &str) -> Option<String> {
    let server = server.trim();
    if server.is_empty() {
        return None;
    }
    if server.contains('=') {
        let mut fallback = None;
        for part in server.split(';') {
            let mut pair = part.splitn(2, '=');
            let key = pair.next()?.trim();
            let value = pair.next()?.trim();
            if key.eq_ignore_ascii_case(scheme) {
                return normalize_proxy(value);
            }
            if key.eq_ignore_ascii_case("http") {
                fallback = normalize_proxy(value);
            }
        }
        return fallback;
    }
    normalize_proxy(server)
}

fn normalize_proxy(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://")
        || value.starts_with("https://")
        || value.starts_with("socks5://")
    {
        Some(value.to_string())
    } else {
        Some(format!("http://{value}"))
    }
}

#[cfg(windows)]
fn read_settings() -> Option<(bool, String, String)> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings")
        .ok()?;
    let enable: u32 = key.get_value("ProxyEnable").unwrap_or(0);
    let server: String = key.get_value("ProxyServer").unwrap_or_default();
    let bypass: String = key.get_value("ProxyOverride").unwrap_or_default();
    Some((enable != 0, server, bypass))
}

#[cfg(not(windows))]
fn read_settings() -> Option<(bool, String, String)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traverse_proxy_choice() {
        let cases = [
            ("http://127.0.0.1:11434/api/tags", true, "127.0.0.1:7890", "", None),
            ("http://localhost:1234/v1", true, "127.0.0.1:7890", "", None),
            ("https://api.openai.com/v1", false, "127.0.0.1:7890", "", None),
            (
                "https://api.openai.com/v1",
                true,
                "127.0.0.1:7890",
                "",
                Some("http://127.0.0.1:7890".to_string()),
            ),
            (
                "https://api.openai.com/v1",
                true,
                "http=127.0.0.1:7890;https=127.0.0.1:7891",
                "",
                Some("http://127.0.0.1:7891".to_string()),
            ),
            ("https://api.openai.com/v1", true, "127.0.0.1:7890", "api.openai.com", None),
            ("https://a.example.com/v1", true, "127.0.0.1:7890", "*.example.com", None),
            ("http://printer/status", true, "127.0.0.1:7890", "<local>", None),
            ("https://socks.example", true, "socks5://127.0.0.1:1080", "", Some("socks5://127.0.0.1:1080".to_string())),
        ];
        for (url, enable, server, bypass, expect) in cases {
            assert_eq!(choose_proxy(url, enable, server, bypass), expect, "{url}");
        }
    }
}
