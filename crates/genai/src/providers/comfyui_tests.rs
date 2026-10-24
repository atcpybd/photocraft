use super::*;
use crate::testing::*;
use crate::{GenerateRequest, ImageSize};
use serde_json::json;

pub(crate) fn config() -> Value {
    json!({"endpoint":"http://127.0.0.1:8188", "workflow":{
        "6":{"class_type":"CLIPTextEncode","inputs":{"text":"old"}},
        "3":{"class_type":"KSampler","inputs":{"seed":1}},
        "10":{"class_type":"LoadImage","inputs":{"image":"old.png"}},
        "11":{"class_type":"LoadImageMask","inputs":{"image":"old.png","channel":"red"}},
        "9":{"class_type":"SaveImage","inputs":{"images":["3",0]}}
    },"mappings":{"prompt":{"node":"6","input":"text"},"image":{"node":"10","input":"image"},"mask":{"node":"11","input":"image"},"seed":{"node":"3","input":"seed"},"output":"9"},"pollMs":1})
}

fn provider(c: Value, http: Arc<FakeHttp>) -> ComfyUi {
    ComfyUi::new(&c.to_string(), Credentials::default(), http).unwrap()
}

fn generated() -> GenerateRequest {
    GenerateRequest { prompt: "a boat".into(), aspect: (1, 1), size: ImageSize::Small }
}

fn history() -> Result<HttpResponse> {
    json_reply(
        200,
        json!({"job-1":{"status":{"completed":true,"status_str":"success"},"outputs":{"9":{"images":[{"filename":"a b&x.png","subfolder":"PhotoCraft/test","type":"output"}]}}}}),
    )
}

#[test]
fn generation_injects_prompt_seed_polls_and_encodes_output_url() {
    let http =
        FakeHttp::new(vec![json_reply(200, json!({"prompt_id":"job-1"})), json_reply(200, json!({})), history(), bytes_reply(200, "image/png", png(2, 2))]);
    let p = provider(config(), http.clone());
    assert!(p.capabilities().generate && p.capabilities().edit);
    assert_eq!(p.run("a boat", None, &CancelToken::default(), &FakeClock::default(), Duration::from_secs(1)).unwrap().bytes, png(2, 2));
    let sent = http.requests();
    assert_eq!(sent[0].url, "http://127.0.0.1:8188/prompt");
    let body: Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(body["prompt"]["6"]["inputs"]["text"], "a boat");
    assert!(body["prompt"]["3"]["inputs"]["seed"].as_u64().is_some());
    assert_eq!(sent[1].url, "http://127.0.0.1:8188/history/job-1");
    assert_eq!(sent[3].url, "http://127.0.0.1:8188/view?filename=a%20b%26x.png&subfolder=PhotoCraft%2Ftest&type=output");
}

#[test]
fn edits_upload_both_images_and_use_returned_names_and_white_mask() {
    let http = FakeHttp::new(vec![
        json_reply(200, json!({"name":"actual image.png","subfolder":"uploads","type":"input"})),
        json_reply(200, json!({"name":"actual mask.png","subfolder":"","type":"input"})),
        json_reply(200, json!({"prompt_id":"job-1"})),
        history(),
        bytes_reply(200, "image/png", png(2, 2)),
    ]);
    let p = provider(config(), http.clone());
    let r = edit_request("a boat", 4, 2);
    p.run(&r.prompt, Some(&r), &r.cancel, &FakeClock::default(), Duration::from_secs(1)).unwrap();
    let sent = http.requests();
    assert_eq!(sent[0].url, "http://127.0.0.1:8188/upload/image");
    assert_eq!(sent[1].url, "http://127.0.0.1:8188/upload/image");
    let parts = crate::multipart::parts(header(&sent[1], "content-type").unwrap(), &sent[1].body).unwrap();
    let mask = &parts.iter().find(|(k, _)| k == "image").unwrap().1;
    let (_, _, coverage) = mask::coverage(mask).unwrap();
    assert_eq!(coverage, [255, 255, 0, 0, 255, 255, 0, 0]);
    let body: Value = serde_json::from_slice(&sent[2].body).unwrap();
    assert_eq!(body["prompt"]["10"]["inputs"]["image"], "uploads/actual image.png");
    assert_eq!(body["prompt"]["11"]["inputs"]["image"], "actual mask.png");
}

#[test]
fn mappings_and_workflow_are_validated_before_network_and_edit_requires_both() {
    for pointer in ["/mappings/prompt/node", "/mappings/image/node", "/mappings/mask/input", "/mappings/seed/input", "/mappings/output"] {
        let mut c = config();
        *c.pointer_mut(pointer).unwrap() = json!("missing");
        let http = FakeHttp::new(vec![]);
        assert!(ComfyUi::new(&c.to_string(), Credentials::default(), http.clone()).is_err(), "{pointer}");
        assert!(http.requests().is_empty());
    }
    let mut c = config();
    c["mappings"].as_object_mut().unwrap().remove("mask");
    let p = provider(c, FakeHttp::new(vec![]));
    assert!(p.capabilities().generate);
    assert!(!p.capabilities().edit);
    assert!(matches!(p.edit(&edit_request("", 2, 2)), Err(GenError::Unsupported(_))));
    let http = FakeHttp::new(vec![]);
    let p = provider(config(), http.clone());
    let mut r = edit_request("", 2, 2);
    r.mask = mask_png(3, 2);
    assert!(p.edit(&r).is_err());
    assert!(http.requests().is_empty());
}

#[test]
fn cancellation_deletes_only_own_queued_job_and_timeout_is_bounded() {
    let http = FakeHttp::new(vec![json_reply(200, json!({"prompt_id":"job-1"})), json_reply(200, json!({}))]);
    let seen = http.clone();
    let cancel = CancelToken::new(move || !seen.requests().is_empty());
    let p = provider(config(), http.clone());
    assert_eq!(p.run("x", None, &cancel, &FakeClock::default(), Duration::from_secs(1)), Err(GenError::Cancelled));
    let sent = http.requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].url, "http://127.0.0.1:8188/queue");
    assert_eq!(serde_json::from_slice::<Value>(&sent[1].body).unwrap(), json!({"delete":["job-1"]}));
    assert!(!sent.iter().any(|r| r.url.ends_with("/interrupt")));

    let http = FakeHttp::new(vec![json_reply(200, json!({"prompt_id":"job-1"})), json_reply(200, json!({})), json_reply(200, json!({}))]);
    let p = provider(config(), http.clone());
    assert_eq!(p.run("x", None, &CancelToken::default(), &FakeClock::default(), Duration::ZERO), Err(GenError::Timeout));
    assert_eq!(http.requests().last().unwrap().url, "http://127.0.0.1:8188/queue");
}

#[test]
fn failed_jobs_bad_replies_and_output_paths_are_safe_errors() {
    for reply in [
        json_reply(200, json!({"job-1":{"status":{"completed":true,"status_str":"error"}}})),
        json_reply(200, json!({"job-1":{"status":{"completed":true},"outputs":{}}})),
        history().map(|mut r| {
            r.body = vec![0; crate::MAX_RESPONSE + 1];
            r
        }),
    ] {
        let http = FakeHttp::new(vec![json_reply(200, json!({"prompt_id":"job-1"})), reply]);
        let p = provider(config(), http);
        assert!(p.run("x", None, &CancelToken::default(), &FakeClock::default(), Duration::from_secs(1)).is_err());
    }
    let http = FakeHttp::new(vec![json_reply(200, json!({"prompt_id":"../other"}))]);
    let p = provider(config(), http.clone());
    assert!(p.generate(&generated()).is_err());
    assert_eq!(http.requests().len(), 1);

    for location in [
        json!({"filename":"../secret.png","type":"output"}),
        json!({"filename":"image.png","subfolder":"../private","type":"output"}),
        json!({"filename":"image.png","type":"other"}),
    ] {
        let http =
            FakeHttp::new(vec![json_reply(200, json!({"prompt_id":"job-1"})), json_reply(200, json!({"job-1":{"outputs":{"9":{"images":[location]}}}}))]);
        let p = provider(config(), http.clone());
        assert!(p.run("x", None, &CancelToken::default(), &FakeClock::default(), Duration::from_secs(1)).is_err());
        assert_eq!(http.requests().len(), 2, "unsafe locations never reach /view");
    }
}

#[test]
fn documented_workflow_templates_build_without_credentials_or_network() {
    for (raw, edit) in [
        (include_str!("../../../../docs/examples/custom-ai/comfyui-generate.json"), false),
        (include_str!("../../../../docs/examples/custom-ai/comfyui-inpaint.json"), true),
    ] {
        let http = FakeHttp::new(vec![]);
        let p = ComfyUi::new(raw, Credentials::default(), http.clone()).unwrap();
        assert!(p.capabilities().generate);
        assert_eq!(p.capabilities().edit, edit);
        assert!(http.requests().is_empty());
    }
}
