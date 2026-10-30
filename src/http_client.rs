use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub timeout_secs: u64,
    /// 为 0 时最多读 8MB。
    pub max_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub bytes: Vec<u8>,
}

impl HttpResponse {
    pub fn text(status: u16, body: impl Into<String>) -> Self {
        let body = body.into();
        let bytes = body.as_bytes().to_vec();
        Self { status, body, bytes }
    }

    pub fn binary(status: u16, bytes: Vec<u8>) -> Self {
        let body = String::from_utf8(bytes.clone()).unwrap_or_default();
        Self { status, body, bytes }
    }
}

pub fn send(request: &HttpRequest) -> Result<HttpResponse, String> {
    let timeout = Duration::from_secs(request.timeout_secs.max(1));
    // ureq 的 native-tls 特性只引入库，不会自动用于 HTTPS。
    let connector = ureq::native_tls::TlsConnector::new()
        .map_err(|err| format!("无法初始化系统 TLS：{err}"))?;
    let mut builder = ureq::AgentBuilder::new()
        .timeout(timeout)
        .tls_connector(Arc::new(connector));
    if let Some(proxy) = crate::proxy::current_for(&request.url) {
        if let Ok(proxy) = ureq::Proxy::new(proxy) {
            builder = builder.proxy(proxy);
        }
    }
    let agent = builder.build();
    let mut call = agent.request(&request.method, &request.url);
    let mut saw_user_agent = false;
    for (key, value) in &request.headers {
        if key.eq_ignore_ascii_case("user-agent") {
            saw_user_agent = true;
        }
        call = call.set(key, value);
    }
    if !saw_user_agent {
        call = call.set("user-agent", "LocalBot/0.1");
    }
    let result = if let Some(body) = &request.body {
        call.send_string(body)
    } else {
        call.call()
    };
    let limit = if request.max_bytes == 0 { 8 * 1024 * 1024 } else { request.max_bytes };
    match result {
        Ok(response) => read_response(response.status(), response, limit),
        Err(ureq::Error::Status(code, response)) => read_response(code, response, limit),
        Err(err) => Err(format!("网络错误：{err}")),
    }
}

fn read_response(status: u16, response: ureq::Response, limit: usize) -> Result<HttpResponse, String> {
    let mut reader = response.into_reader().take(limit as u64 + 1);
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .map_err(|err| format!("网络错误：{err}"))?;
    if buf.len() as u64 > limit as u64 {
        return Err("响应过大".into());
    }
    let body = String::from_utf8(buf.clone()).unwrap_or_default();
    Ok(HttpResponse { status, body, bytes: buf })
}
