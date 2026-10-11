//! Generative AI services for the desktop app (#41): API keys in the OS credential store and an
//! HTTPS transport for `photocraft-genai` providers. Nothing here runs until the user sets up a
//! provider in Generative AI Settings.

use std::io::Read as _;
use std::sync::Arc;
use std::time::Duration;

use photocraft_genai::{ApiKey, GenError, HttpRequest, HttpResponse, HttpTransport};
use photocraft_ui_egui::GenAiServices;

/// The credential store's service name; the account is the provider id (`gemini`).
#[cfg(any(target_os = "macos", target_os = "windows"))]
const SERVICE: &str = "ai.storyteller.photocraft.genai";
/// An image request can take a while (large outputs, busy service).
const TIMEOUT: Duration = Duration::from_secs(180);

/// Exact-destination transport. Local mode has numeric loopback, no proxy and no
/// redirect. Both modes have bounded bodies and a global timeout. A cancelled engine
/// job discards late output; this blocking HTTP call can remain until timeout.
struct RestrictedHttp {
    agent: ureq::Agent,
    endpoint: String,
    local: bool,
}
impl RestrictedHttp {
    fn remote() -> Self {
        let endpoint = "https://generativelanguage.googleapis.com/v1beta/interactions".to_owned();
        let config =
            ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).https_only(true).proxy(None).max_redirects(0).http_status_as_error(false).build();
        Self { agent: config.new_agent(), endpoint, local: false }
    }
    fn local(base: &str) -> photocraft_genai::Result<Self> {
        let endpoint = photocraft_genai::local_webui::endpoint(base)?;
        let config =
            ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).https_only(false).proxy(None).max_redirects(0).http_status_as_error(false).build();
        Ok(Self { agent: config.new_agent(), endpoint, local: true })
    }
}
impl HttpTransport for RestrictedHttp {
    fn post(&self, request: HttpRequest) -> photocraft_genai::Result<HttpResponse> {
        if request.url != self.endpoint
            || request.body.len() > 64 * 1024 * 1024
            || request.headers.iter().any(|(name, _)| *name != "content-type" && (*name != "x-goog-api-key" || self.local))
        {
            return Err(GenError::Invalid("transport destination, headers or body exceed the configured scope".into()));
        }
        let mut call = self.agent.post(&self.endpoint);
        for (name, value) in &request.headers {
            call = call.header(*name, value.as_str());
        }
        let mut response = call.send(&request.body[..]).map_err(|_| GenError::Network("request failed or timed out".into()))?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(photocraft_genai::MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|_| GenError::Network("reading the response failed".into()))?;
        if body.len() > photocraft_genai::MAX_RESPONSE {
            return Err(GenError::BadResponse("response exceeds the byte limit"));
        }
        Ok(HttpResponse { status, body })
    }
}
fn build(provider: &str, model: &str, key: Option<ApiKey>, config: &serde_json::Value) -> Result<photocraft_engine::genai_cmds::SharedProvider, String> {
    match provider {
        photocraft_genai::gemini::ID => {
            let key = key.ok_or("an API key is required")?;
            let p = photocraft_genai::gemini::GeminiProvider::new(key, model, RestrictedHttp::remote()).map_err(|e| e.to_string())?;
            Ok(Arc::new(p))
        }
        photocraft_genai::local_webui::ID => {
            let base = config.get("endpoint").and_then(serde_json::Value::as_str).unwrap_or("http://127.0.0.1:7860");
            let strength = config.get("strength").and_then(serde_json::Value::as_f64).unwrap_or(0.45) as f32;
            let steps = config.get("steps").and_then(serde_json::Value::as_u64).unwrap_or(20);
            let steps = u32::try_from(steps).map_err(|_| "too many local steps")?;
            let transport = RestrictedHttp::local(base).map_err(|e| e.to_string())?;
            let p = photocraft_genai::local_webui::LocalWebUi::new(base, model, strength, steps, -1, transport).map_err(|e| e.to_string())?;
            Ok(Arc::new(p))
        }
        _ => Err("unknown image provider".into()),
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod store {
    fn entry(provider: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(super::SERVICE, provider).map_err(|e| e.to_string())
    }
    pub fn load(provider: &str) -> Result<Option<String>, String> {
        match entry(provider)?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("could not read the API key: {e}")),
        }
    }
    pub fn save(provider: &str, key: &str) -> Result<(), String> {
        entry(provider)?.set_password(key).map_err(|e| format!("could not save the API key: {e}"))
    }
    pub fn delete(provider: &str) -> Result<(), String> {
        match entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("could not remove the API key: {e}")),
        }
    }
    #[cfg(target_os = "macos")]
    pub const NAME: &str = "the macOS Keychain";
    #[cfg(target_os = "windows")]
    pub const NAME: &str = "Windows Credential Manager";
}

/// Elsewhere (Linux, the BSDs) the key lives in process memory only and is asked for again
/// after a restart, rather than written anywhere in plain text.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod store {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    fn keys() -> &'static Mutex<HashMap<String, String>> {
        static KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        KEYS.get_or_init(Mutex::default)
    }
    pub fn load(provider: &str) -> Result<Option<String>, String> {
        Ok(keys().lock().map_err(|_| "key store unavailable")?.get(provider).cloned())
    }
    pub fn save(provider: &str, key: &str) -> Result<(), String> {
        keys().lock().map_err(|_| "key store unavailable")?.insert(provider.into(), key.into());
        Ok(())
    }
    pub fn delete(provider: &str) -> Result<(), String> {
        keys().lock().map_err(|_| "key store unavailable")?.remove(provider);
        Ok(())
    }
    pub const NAME: &str = "memory until PhotoCraft quits";
}

pub fn services() -> GenAiServices {
    GenAiServices {
        load_key: Box::new(store::load),
        save_key: Box::new(store::save),
        delete_key: Box::new(store::delete),
        provider: Box::new(build),
        key_store_name: store::NAME,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn factories_are_offline_and_validate_provider_configuration() {
        let key = || Some(ApiKey::new("synthetic-test-key").unwrap());
        let config = serde_json::json!({});
        assert!(build("gemini", "", key(), &config).is_ok());
        assert!(build("gemini", "../bad", key(), &config).is_err());
        assert!(build("gemini", "", None, &config).is_err());
        assert!(build("webui-local", "", None, &config).unwrap().local_only());
        assert!(build("webui-local", "", None, &serde_json::json!({"endpoint":"https://example.invalid"})).is_err());
        assert!(build("unknown", "", None, &config).is_err());
    }
    #[test]
    fn unrelated_destinations_and_local_credentials_are_rejected_before_sending() {
        for transport in [RestrictedHttp::remote(), RestrictedHttp::local("http://127.0.0.1:1").unwrap()] {
            let result = transport.post(HttpRequest {
                url: "http://example.invalid/".into(),
                headers: vec![("x-goog-api-key", "synthetic-secret".into())],
                body: vec![],
            });
            let e = result.err().unwrap();
            assert!(matches!(e, GenError::Invalid(_)));
            assert!(!e.to_string().contains("synthetic-secret"));
        }
        let local = RestrictedHttp::local("http://127.0.0.1:1").unwrap();
        assert!(local.post(HttpRequest { url: local.endpoint.clone(), headers: vec![("x-goog-api-key", "synthetic".into())], body: vec![] }).is_err());
    }
    #[test]
    fn local_transport_posts_to_a_synthetic_loopback_server_and_does_not_follow_redirects() {
        use std::io::Write;
        use std::net::TcpListener;
        for (response, want) in [
            ("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}", 200),
            ("HTTP/1.1 302 Found\r\nLocation: https://example.invalid/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", 302),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let thread = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut data = [0u8; 4096];
                let n = stream.read(&mut data).unwrap();
                assert!(std::str::from_utf8(&data[..n]).unwrap().starts_with("POST /sdapi/v1/img2img"));
                stream.write_all(response.as_bytes()).unwrap();
            });
            let local = RestrictedHttp::local(&format!("http://{address}")).unwrap();
            let result =
                local.post(HttpRequest { url: local.endpoint.clone(), headers: vec![("content-type", "application/json".into())], body: b"{}".to_vec() });
            if want == 200 {
                assert_eq!(result.unwrap().status, want);
            } else {
                assert!(result.is_err() || result.unwrap().status == 302);
            }
            thread.join().unwrap();
        }
    }
}
