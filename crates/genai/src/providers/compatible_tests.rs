use super::*;
use crate::testing::*;
use crate::{ApiKey, ImageSize};
use serde_json::json;

fn config() -> Value {
    json!({"endpoint":"https://images.example/v1", "model":"my-image-model", "size":"1024x1024"})
}

fn generate() -> GenerateRequest {
    GenerateRequest { prompt: "a boat".into(), aspect: (4, 3), size: ImageSize::Small }
}

#[test]
fn custom_model_and_base_path_route_generations_and_edits() {
    let http = FakeHttp::new(vec![
        json_reply(200, json!({"data":[{"b64_json": common::b64(&png(2, 2))}]})),
        json_reply(200, json!({"data":[{"b64_json": common::b64(&png(4, 2))}]})),
    ]);
    let p = build(&config().to_string(), Credentials::api_key(ApiKey::new(SECRET).unwrap()), http.clone()).unwrap();
    assert_eq!(p.info().id, "compatible");
    assert_eq!(p.info().model, "my-image-model");
    assert!(p.capabilities().generate && p.capabilities().edit);
    assert_eq!(p.generate(&generate()).unwrap().bytes, png(2, 2));
    assert_eq!(p.edit(&edit_request("a red boat", 4, 2)).unwrap().bytes, png(4, 2));
    let sent = http.requests();
    assert_eq!(sent[0].url, "https://images.example/v1/images/generations");
    assert_eq!(header(&sent[0], "authorization"), Some(format!("Bearer {SECRET}").as_str()));
    let body: Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(body["model"], "my-image-model");
    assert_eq!(body["size"], "1024x1024");
    assert!(body.get("response_format").is_none(), "GPT Image rejects response_format; accept the service's default image representation");
    assert_eq!(sent[1].url, "https://images.example/v1/images/edits");
    let parts = crate::multipart::parts(header(&sent[1], "content-type").unwrap(), &sent[1].body).unwrap();
    let part = |name| parts.iter().find(|(k, _)| k == name).unwrap().1.clone();
    assert_eq!(part("model"), b"my-image-model");
    assert_eq!(part("image"), png(4, 2));
    let (_, _, rgba) = mask::decode_rgba8(&part("mask")).unwrap();
    assert_eq!(rgba.chunks(4).map(|p| p[3]).collect::<Vec<_>>(), [0, 0, 255, 255, 0, 0, 255, 255]);
}

#[test]
fn local_service_needs_no_key_and_capabilities_can_be_disabled() {
    let mut c = config();
    c["endpoint"] = json!("http://127.0.0.1:8000/v1/");
    c["generate"] = json!(false);
    let http = FakeHttp::new(vec![json_reply(200, json!({"data":[{"b64_json":common::b64(&png(1, 1))}]}))]);
    let p = build(&c.to_string(), Credentials::default(), http.clone()).unwrap();
    assert!(!p.capabilities().generate);
    assert!(matches!(p.generate(&generate()), Err(GenError::Unsupported(_))));
    p.edit(&edit_request("", 2, 2)).unwrap();
    assert_eq!(header(&http.requests()[0], "authorization"), None);
}

#[test]
fn configuration_and_input_errors_never_send_or_echo_credentials() {
    for endpoint in [
        "http://remote.example/v1",
        "file:///tmp",
        "https://user:secret@host/v1",
        "https://host/v1?key=secret",
        "https://host/#x",
        "https://host\\@elsewhere/v1",
        "https://host/\n",
    ] {
        let http = FakeHttp::new(vec![]);
        let mut c = config();
        c["endpoint"] = json!(endpoint);
        let err = build(&c.to_string(), Credentials::default(), http.clone()).err().unwrap().to_string();
        assert!(!err.contains("secret"));
        assert!(http.requests().is_empty());
    }
    for c in [json!({}), json!({"endpoint":"https://host/v1","model":""}), json!({"endpoint":"https://host/v1","model":"m","size":"99999x2"})] {
        assert!(build(&c.to_string(), Credentials::default(), FakeHttp::new(vec![])).is_err());
    }
    let http = FakeHttp::new(vec![]);
    let p = build(&config().to_string(), Credentials::default(), http.clone()).unwrap();
    let mut r = edit_request("", 2, 2);
    r.mask = mask_png(3, 2);
    assert!(p.edit(&r).is_err());
    assert!(http.requests().is_empty());
}

#[test]
fn errors_are_bounded_and_secrets_are_scrubbed() {
    for reply in [json_reply(500, json!({"error":{"message":format!("bad {SECRET}")}})), Err(GenError::Network(format!("transport echoed {SECRET}")))] {
        let p = build(&config().to_string(), Credentials::api_key(ApiKey::new(SECRET).unwrap()), FakeHttp::new(vec![reply])).unwrap();
        assert!(!p.generate(&generate()).unwrap_err().to_string().contains(SECRET));
    }
    let p =
        build(&config().to_string(), Credentials::default(), FakeHttp::new(vec![bytes_reply(200, "application/json", vec![b' '; crate::MAX_RESPONSE + 1])]))
            .unwrap();
    assert!(matches!(p.generate(&generate()), Err(GenError::BadResponse(_))));
}

#[test]
fn result_url_downloads_never_forward_authorization() {
    let http =
        FakeHttp::new(vec![json_reply(200, json!({"data":[{"url":"https://cdn.example/image.png?signature=xyz"}]})), bytes_reply(200, "image/png", png(1, 1))]);
    let p = build(&config().to_string(), Credentials::api_key(ApiKey::new(SECRET).unwrap()), http.clone()).unwrap();
    p.generate(&generate()).unwrap();
    assert_eq!(header(&http.requests()[1], "authorization"), None);
    for url in ["http://remote.example/image", "https://user:password@cdn.example/image", "file:///etc/passwd"] {
        let http = FakeHttp::new(vec![json_reply(200, json!({"data":[{"url":url}]}))]);
        let p = build(&config().to_string(), Credentials::default(), http.clone()).unwrap();
        assert!(p.generate(&generate()).is_err());
        assert_eq!(http.requests().len(), 1);
    }
}
