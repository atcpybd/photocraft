//! ComfyUI API workflows, with explicit mappings and prompt-specific queue cancellation.

use serde_json::{Value, json};
use std::{hash::BuildHasher, sync::Arc, time::Duration};

use super::{common, configured};
use crate::{
    API_KEY_FIELD, CancelToken, Capabilities, Credentials, EditRequest, GenError, GenerateRequest, GeneratedImage, HttpRequest, HttpResponse, HttpTransport,
    ImageProvider, ProviderInfo, ProviderSpec, Result, check_prompt, mask,
    multipart::Form,
    poll::{Clock, SystemClock},
};

pub const ID: &str = "comfyui";
const NAME: &str = "ComfyUI workflow";
const HOST: &str = "configured endpoint";

pub const SPEC: ProviderSpec = ProviderSpec {
    id: ID,
    name: NAME,
    host: HOST,
    result_hosts: &[],
    models: &[],
    fields: &[API_KEY_FIELD],
    key_page: "https://docs.comfy.org/development/comfyui-server/comms_routes",
    note: "Uploads images to your configured ComfyUI server. API key is optional.",
    build,
};

#[derive(Clone)]
struct Mapping {
    node: String,
    input: String,
}

struct ComfyUi {
    endpoint: String,
    workflow: Value,
    prompt: Mapping,
    image: Option<Mapping>,
    mask: Option<Mapping>,
    seed: Option<Mapping>,
    output: String,
    poll: Duration,
    creds: Credentials,
    http: Arc<dyn HttpTransport>,
}

fn invalid(message: &str) -> GenError {
    GenError::Invalid(message.into())
}

fn build(raw: &str, creds: Credentials, http: Arc<dyn HttpTransport>) -> Result<Arc<dyn ImageProvider>> {
    Ok(Arc::new(ComfyUi::new(raw, creds, http)?))
}

impl ComfyUi {
    fn new(raw: &str, creds: Credentials, http: Arc<dyn HttpTransport>) -> Result<Self> {
        let c = configured::config(raw)?;
        let endpoint = configured::endpoint(configured::string(&c, "endpoint")?)?;
        let workflow = c.get("workflow").filter(|v| v.is_object()).ok_or_else(|| invalid("workflow must be an exported ComfyUI API graph"))?;
        let nodes = workflow.as_object().ok_or_else(|| invalid("workflow must be an API graph"))?;
        if nodes.is_empty()
            || nodes.len() > 4096
            || nodes.values().any(|v| v.get("class_type").and_then(Value::as_str).is_none_or(str::is_empty) || !v.get("inputs").is_some_and(Value::is_object))
        {
            return Err(invalid("workflow must contain API nodes with class_type and inputs"));
        }
        let mappings = c.get("mappings").filter(|v| v.is_object()).ok_or_else(|| invalid("mappings must identify prompt and output nodes"))?;
        let mapping = |name: &str| -> Result<Option<Mapping>> {
            let Some(v) = mappings.get(name) else { return Ok(None) };
            let node = configured::string(v, "node")?;
            let input = configured::string(v, "input")?;
            let current = workflow
                .get(node)
                .and_then(|v| v.get("inputs"))
                .and_then(|v| v.get(input))
                .ok_or_else(|| invalid("a mapping refers to a missing workflow node or input"))?;
            let valid = if name == "seed" { current.is_u64() } else { current.is_string() };
            if !valid {
                return Err(invalid("mapped prompt/image/mask inputs must be text; seed must be an unsigned integer"));
            }
            Ok(Some(Mapping { node: node.into(), input: input.into() }))
        };
        let prompt = mapping("prompt")?.ok_or_else(|| invalid("a prompt mapping is required"))?;
        let image = mapping("image")?;
        let mask = mapping("mask")?;
        let seed = mapping("seed")?;
        let all: Vec<&Mapping> = std::iter::once(&prompt).chain(image.iter()).chain(mask.iter()).chain(seed.iter()).collect();
        for (i, a) in all.iter().enumerate() {
            if all.iter().skip(i + 1).any(|b| a.node == b.node && a.input == b.input) {
                return Err(invalid("workflow mappings must target different inputs"));
            }
        }
        let output = configured::string(mappings, "output")?;
        if workflow.get(output).is_none() {
            return Err(invalid("the output mapping refers to a missing workflow node"));
        }
        let poll = match c.get("pollMs") {
            None => Duration::from_millis(500),
            Some(v) => Duration::from_millis(v.as_u64().filter(|n| (1..=10_000).contains(n)).ok_or_else(|| invalid("pollMs must be between 1 and 10000"))?),
        };
        configured::validate_credentials(&creds)?;
        Ok(Self { endpoint, workflow: workflow.clone(), prompt, image, mask, seed, output: output.into(), poll, creds, http })
    }

    fn send(&self, req: HttpRequest) -> Result<HttpResponse> {
        configured::send(&*self.http, configured::authorize(req, &self.creds), &self.creds)
    }

    fn inject(workflow: &mut Value, mapping: &Mapping, value: Value) -> Result<()> {
        let slot = workflow
            .get_mut(&mapping.node)
            .and_then(|v| v.get_mut("inputs"))
            .and_then(|v| v.get_mut(&mapping.input))
            .ok_or_else(|| invalid("workflow mapping no longer exists"))?;
        *slot = value;
        Ok(())
    }

    fn upload(&self, bytes: &[u8], kind: &str, cancel: &CancelToken) -> Result<String> {
        cancel.check()?;
        let nonce = std::collections::hash_map::RandomState::new().hash_one(0u8);
        let mut form = Form::new();
        form.text("type", "input").text("overwrite", "false").file("image", &format!("photocraft-{kind}-{nonce:016x}.png"), "image/png", bytes);
        let (ct, body) = form.finish();
        let reply = self.send(HttpRequest::post(format!("{}/upload/image", self.endpoint), body).header("content-type", ct))?;
        cancel.check()?;
        let v = common::json(&reply)?;
        let name = v.get("name").and_then(Value::as_str).ok_or(GenError::BadResponse("upload returned no filename"))?;
        let subfolder = v.get("subfolder").and_then(Value::as_str).unwrap_or("");
        if !safe_filename(name) || !safe_subfolder(subfolder) || v.get("type").and_then(Value::as_str).is_some_and(|t| t != "input") {
            return Err(GenError::BadResponse("upload returned an invalid image location"));
        }
        Ok(if subfolder.is_empty() { name.into() } else { format!("{subfolder}/{name}") })
    }

    fn queue_delete(&self, id: &str) {
        // Never use a global /interrupt: it could stop another client's work.
        let _ = self.send(HttpRequest::post(format!("{}/queue", self.endpoint), Vec::new()).json(&json!({"delete":[id]})));
    }

    fn run(&self, prompt: &str, edit: Option<&EditRequest>, cancel: &CancelToken, clock: &dyn Clock, limit: Duration) -> Result<GeneratedImage> {
        check_prompt(prompt, edit.is_none())?;
        cancel.check()?;
        let mut workflow = self.workflow.clone();
        Self::inject(&mut workflow, &self.prompt, json!(common::prompt_or_description(prompt)))?;
        if let Some(seed) = &self.seed {
            let value = std::collections::hash_map::RandomState::new().hash_one(0u8) & ((1u64 << 53) - 1);
            Self::inject(&mut workflow, seed, json!(value))?;
        }
        if let Some(r) = edit {
            let (Some(image), Some(mask)) = (&self.image, &self.mask) else { return Err(GenError::Unsupported(NAME)) };
            common::same_size(&r.image, &r.mask)?;
            // Explicit mask image uses white=generate; use LoadImageMask's red channel.
            let white = mask::white_fill(&r.mask)?;
            mask::decode_rgba8(&r.image)?;
            Self::inject(&mut workflow, image, json!(self.upload(&r.image, "image", cancel)?))?;
            Self::inject(&mut workflow, mask, json!(self.upload(&white, "mask", cancel)?))?;
        }
        cancel.check()?;
        let reply = self.send(HttpRequest::post(format!("{}/prompt", self.endpoint), Vec::new()).json(&json!({"prompt":workflow})))?;
        let v = common::json(&reply)?;
        let id = v
            .get("prompt_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
            .ok_or(GenError::BadResponse("the workflow was not accepted"))?;
        let output = self.wait(id, cancel, clock, limit);
        if matches!(output, Err(GenError::Cancelled | GenError::Timeout)) {
            self.queue_delete(id);
        }
        output
    }

    fn wait(&self, id: &str, cancel: &CancelToken, clock: &dyn Clock, limit: Duration) -> Result<GeneratedImage> {
        let start = clock.now();
        loop {
            cancel.check()?;
            let reply = self.send(HttpRequest::get(format!("{}/history/{id}", self.endpoint)))?;
            cancel.check()?;
            let v = common::json(&reply)?;
            if let Some(job) = v.get(id) {
                if job.pointer("/status/status_str").and_then(Value::as_str) == Some("error") {
                    return Err(GenError::Refused("the ComfyUI workflow failed; inspect its server log".into()));
                }
                let image =
                    job.get("outputs").and_then(|v| v.get(&self.output)).and_then(|v| v.get("images")).and_then(Value::as_array).and_then(|a| a.first());
                if let Some(image) = image {
                    let filename =
                        image.get("filename").and_then(Value::as_str).filter(|s| safe_filename(s)).ok_or(GenError::BadResponse("invalid output filename"))?;
                    let subfolder = image.get("subfolder").and_then(Value::as_str).unwrap_or("");
                    let kind = image.get("type").and_then(Value::as_str).unwrap_or("output");
                    if !safe_subfolder(subfolder) || !matches!(kind, "output" | "temp" | "input") {
                        return Err(GenError::BadResponse("invalid output location"));
                    }
                    let query = common::form_urlencode(&[("filename", filename), ("subfolder", subfolder), ("type", kind)]);
                    cancel.check()?;
                    let reply = self.send(HttpRequest::get(format!("{}/view?{query}", self.endpoint)))?;
                    cancel.check()?;
                    return common::image(reply.body);
                }
                if job.pointer("/status/completed").and_then(Value::as_bool) == Some(true) {
                    return Err(GenError::BadResponse("the mapped output node returned no image"));
                }
            }
            let elapsed = clock.now().saturating_sub(start);
            if elapsed.saturating_add(self.poll) > limit {
                return Err(GenError::Timeout);
            }
            let mut remaining = self.poll;
            while !remaining.is_zero() {
                cancel.check()?;
                let slice = remaining.min(Duration::from_millis(100));
                clock.sleep(slice);
                remaining = remaining.saturating_sub(slice);
            }
        }
    }
}

fn safe_filename(s: &str) -> bool {
    !s.is_empty() && s.len() <= 1024 && !matches!(s, "." | "..") && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':'))
}

fn safe_subfolder(s: &str) -> bool {
    s.is_empty() || (s.len() <= 2048 && s.split('/').all(safe_filename))
}

impl ImageProvider for ComfyUi {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: ID, name: NAME, model: "API workflow".into(), host: HOST }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { generate: true, edit: self.image.is_some() && self.mask.is_some(), variations: false }
    }
    fn generate(&self, r: &GenerateRequest) -> Result<GeneratedImage> {
        self.run(&r.prompt, None, &CancelToken::default(), &SystemClock::new(), crate::poll::LIMIT)
    }
    fn edit(&self, r: &EditRequest) -> Result<GeneratedImage> {
        if !self.capabilities().edit {
            return Err(GenError::Unsupported(NAME));
        }
        self.run(&r.prompt, Some(r), &r.cancel, &SystemClock::new(), crate::poll::LIMIT)
    }
}

#[cfg(test)]
#[path = "comfyui_tests.rs"]
mod tests;
