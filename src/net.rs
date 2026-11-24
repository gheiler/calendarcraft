//! HTTP for CalDAV and `.ics` feeds: the browser's `fetch` in WebAssembly, `ureq` natively.
//! Redirects are returned, not followed, so discovery can follow them through a proxy.

#[derive(Debug, Clone, Default)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
    pub etag: Option<String>,
    pub location: Option<String>,
}

impl HttpResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn send(req: HttpRequest) -> Result<HttpResponse, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let init = web_sys::RequestInit::new();
    init.set_method(&req.method);
    init.set_credentials(web_sys::RequestCredentials::SameOrigin);
    init.set_redirect(web_sys::RequestRedirect::Follow);
    let headers = web_sys::Headers::new().map_err(|_| "headers".to_string())?;
    for (k, v) in &req.headers {
        headers.set(k, v).map_err(|_| format!("bad header {k}"))?;
    }
    init.set_headers(&headers);
    if let Some(body) = &req.body {
        init.set_body(&wasm_bindgen::JsValue::from_str(body));
    }
    let request = web_sys::Request::new_with_str_and_init(&req.url, &init).map_err(|e| format!("{e:?}"))?;
    let window = web_sys::window().ok_or("no window")?;
    let resp = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| format!("could not reach {} (network or CORS)", host_of(&req.url)))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "not a response".to_string())?;
    let status = resp.status();
    let etag = resp.headers().get("etag").ok().flatten();
    let location = resp.headers().get("location").ok().flatten();
    let text = JsFuture::from(resp.text().map_err(|_| "body".to_string())?).await.map_err(|_| "body".to_string())?;
    Ok(HttpResponse { status, body: text.as_string().unwrap_or_default(), etag, location })
}

#[cfg(not(target_arch = "wasm32"))]
pub async fn send(req: HttpRequest) -> Result<HttpResponse, String> {
    use std::io::Read;
    let agent = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(30)).build();
    let mut call = agent.request(&req.method, &req.url);
    for (k, v) in &req.headers {
        call = call.set(k, v);
    }
    let result = match &req.body {
        Some(body) => call.send_string(body),
        None => call.call(),
    };
    let resp = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(err) => return Err(format!("could not reach {}: {err}", host_of(&req.url))),
    };
    let status = resp.status();
    let etag = resp.header("etag").map(str::to_string);
    let location = resp.header("location").map(str::to_string);
    let mut body = String::new();
    resp.into_reader().take(32 * 1024 * 1024).read_to_string(&mut body).map_err(|e| e.to_string())?;
    Ok(HttpResponse { status, body, etag, location })
}

pub fn host_of(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split('/').next().unwrap_or(rest).to_string()
}

/// Run a future in the background: the browser's microtask queue, or a native thread.
#[cfg(target_arch = "wasm32")]
pub fn spawn(fut: impl std::future::Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn(fut: impl std::future::Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(fut));
}
