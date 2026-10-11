//! AUTOMATIC1111-compatible local image/mask editing over an injected transport.
//! No model is installed by this adapter. The desktop transport must enforce loopback,
//! disable proxies and redirects, and bound its response before returning it here.
use crate::{Capabilities, EditRequest, GenError, GeneratedImage, HttpRequest, HttpTransport, ImageProvider, ProviderInfo, Result};
use base64::Engine as _;
use serde_json::{Value, json};
pub const ID: &str = "webui-local";
pub const NAME: &str = "Local WebUI";
/// Accept numeric loopback with an explicit port only; no DNS, path, credentials or query.
pub fn endpoint(base: &str) -> Result<String> {
    let base = base.trim_end_matches('/');
    let port = base.strip_prefix("http://127.0.0.1:").or_else(|| base.strip_prefix("http://[::1]:"));
    if port.filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())).and_then(|p| p.parse::<u16>().ok()).is_none_or(|p| p == 0) {
        return Err(GenError::Invalid("local URL must be http://127.0.0.1:PORT or http://[::1]:PORT".into()));
    }
    Ok(format!("{base}/sdapi/v1/img2img"))
}
pub struct LocalWebUi<T: HttpTransport> {
    endpoint: String,
    model: String,
    strength: f32,
    steps: u32,
    seed: i64,
    transport: T,
}
impl<T: HttpTransport> LocalWebUi<T> {
    pub fn new(base: &str, model: &str, strength: f32, steps: u32, seed: i64, transport: T) -> Result<Self> {
        let endpoint = endpoint(base)?;
        if model.len() > 256 || model.chars().any(char::is_control) || !strength.is_finite() || !(0.0..=1.0).contains(&strength) || !(1..=100).contains(&steps)
        {
            return Err(GenError::Invalid("invalid local model, strength or step count".into()));
        }
        Ok(Self { endpoint, model: model.to_owned(), strength, steps, seed, transport })
    }
    pub fn transport(&self) -> &T {
        &self.transport
    }
}
impl<T: HttpTransport> ImageProvider for LocalWebUi<T> {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: ID,
            name: NAME,
            model: if self.model.is_empty() { "Server default (unverified)".into() } else { self.model.clone() },
            host: "numeric loopback",
        }
    }
    fn endpoint(&self) -> String {
        self.endpoint.clone()
    }
    fn local_only(&self) -> bool {
        true
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { edit: true, ..Default::default() }
    }
    // Default edit_guided rejects semantic reference images. Selection/protection are
    // represented by the edit mask; other semantics require a compatible guided adapter.
    fn edit(&self, r: &EditRequest) -> Result<GeneratedImage> {
        let (w, h) = crate::guidance::check_edit(r)?;
        if w % 8 != 0 || h % 8 != 0 {
            return Err(GenError::Invalid("local WebUI input dimensions must be multiples of eight".into()));
        }
        let b64 = &base64::engine::general_purpose::STANDARD;
        let mut payload = json!({"init_images":[b64.encode(&r.image)],"mask":b64.encode(&r.mask),"prompt":r.prompt,"width":w,"height":h,"denoising_strength":self.strength,"steps":self.steps,"seed":self.seed,"batch_size":1,"n_iter":1,"resize_mode":0,"inpainting_fill":1,"inpaint_full_res":false,"inpainting_mask_invert":0,"mask_blur":0,"send_images":true,"save_images":false,"do_not_save_samples":true,"do_not_save_grid":true,"override_settings_restore_afterwards":true});
        if !self.model.is_empty() {
            payload["override_settings"] = json!({"sd_model_checkpoint":self.model});
        }
        let body = serde_json::to_vec(&payload).map_err(|_| GenError::Invalid("could not encode local edit".into()))?;
        let reply = self.transport.post(HttpRequest { url: self.endpoint.clone(), headers: vec![("content-type", "application/json".into())], body })?;
        if reply.body.len() > crate::MAX_RESPONSE {
            return Err(GenError::BadResponse("local response too large"));
        }
        if !(200..300).contains(&reply.status) {
            return Err(GenError::Http { status: reply.status, message: "local image edit failed; check the local backend".into() });
        }
        let json: Value = serde_json::from_slice(&reply.body).map_err(|_| GenError::BadResponse("local response is not JSON"))?;
        let images = json.get("images").and_then(Value::as_array).filter(|v| v.len() == 1).ok_or(GenError::BadResponse("expected one local result"))?;
        let encoded = images.first().and_then(Value::as_str).ok_or(GenError::BadResponse("missing local image"))?;
        let encoded = encoded.strip_prefix("data:image/png;base64,").or_else(|| encoded.strip_prefix("data:image/jpeg;base64,")).unwrap_or(encoded);
        if encoded.len() > crate::MAX_RESPONSE {
            return Err(GenError::BadResponse("local image too large"));
        }
        let bytes = b64.decode(encoded).map_err(|_| GenError::BadResponse("invalid local base64"))?;
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8]) {
            "image/jpeg"
        } else {
            return Err(GenError::BadResponse("local result must be PNG or JPEG"));
        };
        Ok(GeneratedImage { bytes, mime: mime.into() })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    struct Fake {
        calls: Mutex<Vec<HttpRequest>>,
        body: Vec<u8>,
        status: u16,
    }
    impl HttpTransport for Fake {
        fn post(&self, r: HttpRequest) -> Result<crate::HttpResponse> {
            self.calls.lock().unwrap().push(r);
            Ok(crate::HttpResponse { status: self.status, body: self.body.clone() })
        }
    }
    fn png() -> Vec<u8> {
        let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        p.extend_from_slice(&64u32.to_be_bytes());
        p.extend_from_slice(&64u32.to_be_bytes());
        p
    }
    fn fake() -> Fake {
        Fake {
            calls: Mutex::new(Vec::new()),
            body: serde_json::to_vec(&json!({"images":[base64::engine::general_purpose::STANDARD.encode(png())]})).unwrap(),
            status: 200,
        }
    }
    fn request() -> EditRequest {
        EditRequest { image: png(), mask: png(), prompt: "Synthetic protocol fixture".into(), size: crate::ImageSize::Small }
    }
    #[test]
    fn local_endpoint_excludes_remote_hosts_dns_credentials_paths_and_bad_ports() {
        for url in [
            "http://example.com:7860",
            "http://localhost:7860",
            "https://127.0.0.1:7860",
            "http://127.0.0.1:0",
            "http://127.0.0.1:70000",
            "http://127.0.0.1:7860/elsewhere",
            "http://127.0.0.1:7860?target=remote",
            "http://user@127.0.0.1:7860",
        ] {
            assert!(endpoint(url).is_err(), "{url}");
        }
        assert_eq!(endpoint("http://[::1]:7860/").unwrap(), "http://[::1]:7860/sdapi/v1/img2img");
    }
    #[test]
    fn local_wire_contract_sends_only_image_mask_prompt_and_explicit_generation_settings() {
        let p = LocalWebUi::new("http://127.0.0.1:7860", "test-checkpoint", 0.4, 20, 7, fake()).unwrap();
        assert_eq!(p.edit(&request()).unwrap().bytes, png());
        let calls = p.transport().calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].url, p.endpoint());
        let v: Value = serde_json::from_slice(&calls[0].body).unwrap();
        assert_eq!(v["width"], 64);
        assert_eq!(v["height"], 64);
        assert_eq!(v["batch_size"], 1);
        assert_eq!(v["save_images"], false);
        assert_eq!(v["override_settings_restore_afterwards"], true);
        assert_eq!(v["override_settings"]["sd_model_checkpoint"], "test-checkpoint");
        assert_eq!(v["mask_blur"], 0);
        assert!(p.local_only());
    }
    #[test]
    fn unsupported_guidance_is_rejected_before_any_local_request() {
        let p = LocalWebUi::new("http://127.0.0.1:7860", "", 0.5, 20, 0, fake()).unwrap();
        let r = crate::GuidedEditRequest {
            edit: request(),
            guidance: vec![crate::GuidanceImage { semantic: "foldDirection".into(), mask: png(), annotations: vec!["From (10,10) toward (30,40)".into()] }],
        };
        assert!(p.edit_guided(&r).is_err());
        assert!(p.transport().calls.lock().unwrap().is_empty());
    }
    #[test]
    fn local_errors_do_not_echo_response_bodies_or_accept_malformed_images() {
        for body in [b"not json".to_vec(), br#"{"images":[]}"#.to_vec(), br#"{"images":["bad"]}"#.to_vec()] {
            let mut f = fake();
            f.body = body;
            let p = LocalWebUi::new("http://127.0.0.1:7860", "", 0.5, 20, 0, f).unwrap();
            assert!(p.edit(&request()).is_err());
        }
        let mut f = fake();
        f.status = 500;
        f.body = b"private server content".to_vec();
        let p = LocalWebUi::new("http://127.0.0.1:7860", "", 0.5, 20, 0, f).unwrap();
        assert!(!p.edit(&request()).unwrap_err().to_string().contains("private"));
    }
}
