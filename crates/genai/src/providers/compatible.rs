//! OpenAI image protocol with an explicit user configured endpoint and model.

use serde_json::{Value, json};
use std::sync::Arc;

use super::{common, configured};
use crate::{
    API_KEY_FIELD, Capabilities, Credentials, EditRequest, GenError, GenerateRequest, GeneratedImage, HttpRequest, HttpTransport, ImageProvider, ProviderInfo,
    ProviderSpec, Result, check_prompt, mask, multipart::Form,
};

pub const ID: &str = "compatible";
const NAME: &str = "OpenAI-compatible image service";
const HOST: &str = "configured endpoint";

pub const SPEC: ProviderSpec = ProviderSpec {
    id: ID,
    name: NAME,
    host: HOST,
    result_hosts: &[],
    models: &[],
    fields: &[API_KEY_FIELD],
    key_page: "https://platform.openai.com/docs/api-reference/images",
    note: "Uses your configured endpoint. API key is optional for local services.",
    build,
};

struct Compatible {
    endpoint: String,
    model: String,
    size: Option<String>,
    capabilities: Capabilities,
    creds: Credentials,
    http: Arc<dyn HttpTransport>,
}

fn build(raw: &str, creds: Credentials, http: Arc<dyn HttpTransport>) -> Result<Arc<dyn ImageProvider>> {
    let c = configured::config(raw)?;
    let endpoint = configured::endpoint(configured::string(&c, "endpoint")?)?;
    let model = configured::string(&c, "model")?;
    if model.len() > 256 || model.chars().any(char::is_control) {
        return Err(GenError::Invalid("the model name is too long or contains control characters".into()));
    }
    let size = match c.get("size") {
        None => None,
        Some(value) => {
            let text = value.as_str().ok_or_else(|| GenError::Invalid("size must be WIDTHxHEIGHT".into()))?;
            let valid = text
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
                .is_some_and(|(w, h)| (1..=8192).contains(&w) && (1..=8192).contains(&h));
            if !valid {
                return Err(GenError::Invalid("size must be WIDTHxHEIGHT, with each side at most 8192".into()));
            }
            Some(text.to_owned())
        }
    };
    let flag = |name| match c.get(name) {
        None => Ok(true),
        Some(v) => v.as_bool().ok_or_else(|| GenError::Invalid(format!("{name} must be true or false"))),
    };
    let capabilities = Capabilities { generate: flag("generate")?, edit: flag("edit")?, variations: false };
    configured::validate_credentials(&creds)?;
    Ok(Arc::new(Compatible { endpoint, model: model.into(), size, capabilities, creds, http }))
}

impl Compatible {
    fn result(&self, response: &crate::HttpResponse) -> Result<GeneratedImage> {
        let v = common::json(response)?;
        let item = v.get("data").and_then(Value::as_array).and_then(|a| a.first()).ok_or(GenError::BadResponse("no image in the reply"))?;
        if let Some(data) = item.get("b64_json").and_then(Value::as_str) {
            return common::image(common::decode_b64(data)?);
        }
        let raw = item.get("url").and_then(Value::as_str).ok_or(GenError::BadResponse("no image in the reply"))?;
        let url = configured::checked_url(raw, true).map_err(|_| GenError::BadResponse("an unsafe result URL"))?;
        // Results may live on a CDN; never send the service's credential there.
        let reply = configured::send(&*self.http, HttpRequest::get(url.as_str()), &self.creds)?;
        common::image(reply.body)
    }
}

impl ImageProvider for Compatible {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: ID, name: NAME, model: self.model.clone(), host: HOST }
    }
    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
    fn generate(&self, r: &GenerateRequest) -> Result<GeneratedImage> {
        if !self.capabilities.generate {
            return Err(GenError::Unsupported(NAME));
        }
        check_prompt(&r.prompt, true)?;
        // GPT Image rejects the legacy response_format field; accept either inline or URL
        // output, using each compatible service's default representation.
        let mut body = json!({"model":self.model,"prompt":r.prompt,"n":1});
        if let Some(size) = &self.size {
            body["size"] = json!(size);
        }
        let req = HttpRequest::post(format!("{}/images/generations", self.endpoint), Vec::new()).json(&body);
        let reply = configured::send(&*self.http, configured::authorize(req, &self.creds), &self.creds)?;
        self.result(&reply)
    }
    fn edit(&self, r: &EditRequest) -> Result<GeneratedImage> {
        if !self.capabilities.edit {
            return Err(GenError::Unsupported(NAME));
        }
        check_prompt(&r.prompt, false)?;
        common::same_size(&r.image, &r.mask)?;
        r.cancel.check()?;
        let hole = mask::alpha_hole(&r.image, &r.mask)?;
        let mut form = Form::new();
        form.text("model", &self.model).text("prompt", common::prompt_or_continue(&r.prompt)).text("n", "1");
        if let Some(size) = &self.size {
            form.text("size", size);
        }
        form.file("image", "image.png", "image/png", &r.image).file("mask", "mask.png", "image/png", &hole);
        let (ct, body) = form.finish();
        let req = HttpRequest::post(format!("{}/images/edits", self.endpoint), body).header("content-type", ct);
        let reply = configured::send(&*self.http, configured::authorize(req, &self.creds), &self.creds)?;
        r.cancel.check()?;
        let output = self.result(&reply)?;
        r.cancel.check()?;
        Ok(output)
    }
}

#[cfg(test)]
#[path = "compatible_tests.rs"]
mod tests;
