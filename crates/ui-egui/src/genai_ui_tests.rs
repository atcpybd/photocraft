use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use photocraft_engine::Session;
use photocraft_genai::{ApiKey, Capabilities, EditRequest, GeneratedImage, ImageProvider, ProviderInfo};

use egui_kittest::kittest::Queryable;

use super::*;
use crate::{GenAiServices, Services};

const SECRET: &str = "AIza-ui-test-secret";

/// Fills with solid red at the request's size; built for any registered id.
struct Fake {
    id: &'static str,
    model: String,
}

impl ImageProvider for Fake {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: self.id, name: self.id, model: self.model.clone(), host: "example.invalid" }
    }
    fn capabilities(&self) -> Capabilities {
        let config = serde_json::from_str::<Value>(&self.model).unwrap_or(Value::Null);
        Capabilities {
            generate: config.get("generate").and_then(Value::as_bool).unwrap_or(true),
            edit: config.get("edit").and_then(Value::as_bool).unwrap_or(true),
            variations: false,
        }
    }
    fn needs_prompt(&self) -> bool {
        self.id == "firefly" || self.id == "ideogram-v3"
    }
    fn edit(&self, r: &EditRequest) -> photocraft_genai::Result<GeneratedImage> {
        let (w, h) = photocraft_genai::png_size(&r.image).unwrap();
        // Red with a coarse checkerboard of detail: a flat result is rejected as an empty fill.
        let block = (w.max(h) / 16).max(1);
        let px: Vec<u8> =
            (0..w * h).flat_map(|i| if ((i % w) / block + (i / w) / block).is_multiple_of(2) { [255u8, 0, 0, 255] } else { [192, 0, 0, 255] }).collect();
        let img = photocraft_codecs::Image::from_u8(w, h, photocraft_codecs::ChannelLayout::Rgba, px).unwrap();
        let bytes = photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &Default::default()).unwrap();
        Ok(GeneratedImage { bytes, mime: "image/png".into() })
    }
}

type Store = Rc<RefCell<HashMap<String, String>>>;

fn services(store: &Store) -> GenAiServices {
    let (a, b, c) = (store.clone(), store.clone(), store.clone());
    GenAiServices {
        load_key: Box::new(move |p| Ok(a.borrow().get(p).cloned())),
        save_key: Box::new(move |p, k| {
            b.borrow_mut().insert(p.into(), k.into());
            Ok(())
        }),
        delete_key: Box::new(move |p| {
            c.borrow_mut().remove(p);
            Ok(())
        }),
        provider: Box::new(|id, model, creds: photocraft_genai::Credentials| {
            let spec = photocraft_genai::spec(id).ok_or("unknown provider")?;
            if !optional_key(id) && !creds.has_all(spec.fields) {
                return Err(format!("{id}: missing credentials"));
            }
            Ok(Arc::new(Fake { id: spec.id, model: model.into() }))
        }),
        key_store_name: "the test store",
    }
}

fn secret(fields: &[(&str, &str)]) -> String {
    let mut c = photocraft_genai::Credentials::default();
    for (n, v) in fields {
        c.insert(n, ApiKey::new(v).unwrap());
    }
    c.to_secret()
}

/// An app with `ids` configured (first = default), each with an API key unless the store already
/// holds something for it, restored, and a selection made.
fn configured(store: &Store, ids: &[&str]) -> (PhotocraftApp, Result<(), String>) {
    for id in ids {
        store.borrow_mut().entry((*id).to_owned()).or_insert_with(|| secret(&[("api_key", SECRET)]));
    }
    let mut app = app_with(store);
    let saved = Saved {
        default: ids.first().map(|s| (*s).to_owned()).unwrap_or_default(),
        models: Default::default(),
        configured: ids.iter().map(|s| (*s).to_owned()).collect(),
    };
    app.session.prefs.edit(|p| p.dialogs.insert(PREF.into(), saved.to_json()));
    let restored = restore(&mut app);
    app.run("select.rect", json!({"x": 8, "y": 8, "width": 16, "height": 16})).unwrap();
    (app, restored)
}

fn app_with(store: &Store) -> PhotocraftApp {
    let mut app = PhotocraftApp::new(Session::new(), Services { genai: Some(services(store)), ..Default::default() });
    app.run("file.new", json!({"width": 64, "height": 64, "background": "#ffffff"})).unwrap();
    app
}

fn fill_in_menu(app: &PhotocraftApp) -> bool {
    crate::menus::menu_items(app).iter().any(|i| i.id == FILL)
}

fn settings_dialog(app: &PhotocraftApp) -> Option<u64> {
    app.ui.dialogs.iter().find(|d| d.fields.contains_key(SETTINGS_MARK)).map(|d| d.id)
}

#[test]
fn off_by_default_with_no_menu_item_and_no_key_read() {
    let store = Store::default();
    let app = app_with(&store);
    assert!(app.session.image_provider().is_none());
    assert!(!fill_in_menu(&app), "no \"coming soon\" item before a provider is set up");
    // The web build (no services) has no settings at all.
    let mut web = PhotocraftApp::new(Session::new(), Services::default());
    assert!(restore(&mut web).is_ok());
    assert!(web.session.image_provider().is_none());
}

#[test]
fn the_prompt_dialog_runs_the_fill_and_remembers_the_prompt() {
    let store = Store::default();
    store.borrow_mut().insert("gemini".into(), SECRET.into());
    let mut app = app_with(&store);
    app.session.prefs.edit(|p| p.dialogs.insert(PREF.into(), json!({"provider": "gemini", "model": "m"})));
    restore(&mut app).unwrap();
    let ctx = egui::Context::default();
    // No selection: Generative Fill can't run, and says why.
    assert!(crate::menus::invoke(&mut app, &ctx, FILL, json!({})).is_err());
    app.run("select.rect", json!({"x": 8, "y": 8, "width": 16, "height": 16})).unwrap();
    let r = crate::menus::invoke(&mut app, &ctx, FILL, json!({})).unwrap();
    let id = r["dialog"].as_u64().unwrap();
    assert_eq!(crate::genai_ui::ok_label(&app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields), Some("Generate"));
    app.ui.dialog_mut(id).unwrap().fields.insert("prompt".into(), json!("a red square"));
    let layers = app.session.active().unwrap().doc.layers.len();
    crate::dialogs::confirm(&mut app, id).unwrap();
    let doc = &app.session.active().unwrap().doc;
    assert_eq!(doc.layers.len(), layers + 1);
    assert_eq!(doc.layers.last().unwrap().name, "a red square");
    assert_eq!(app.session.prefs().dialogs.get(FILL), Some(&json!({"prompt": "a red square"})));
}

/// Renders Generative AI Settings, a provider's Set Up dialog and the prompt for a look
/// (`GENAI_SNAPSHOT_DIR=… cargo test`).
#[test]
fn the_dialogs_render() {
    let store = Store::default();
    store.borrow_mut().insert("gemini".into(), SECRET.into());
    let store2 = store.clone();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 760.0)).with_max_steps(64).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        let mut app = app_with(&store2);
        app.session.prefs.edit(|p| p.dialogs.insert(PREF.into(), json!({"provider": "gemini"})));
        restore(&mut app).unwrap();
        open_settings(&mut app);
        app
    });
    h.run_steps(6);
    h.get_by_label("Google Gemini");
    h.get_by_label("Edit…");
    h.get_by_label("Remove");
    let dir = std::env::var_os("GENAI_SNAPSHOT_DIR");
    if let (Some(dir), Ok(img)) = (&dir, h.render()) {
        img.save(std::path::Path::new(dir).join("genai-settings.png")).unwrap();
    }
    let id = settings_dialog(h.state()).unwrap();
    crate::dialogs::cancel(h.state_mut(), id).unwrap();
    let id = open_provider(h.state_mut(), "gemini").unwrap();
    h.run_steps(6);
    h.get_by_label("API key");
    h.get_by_label("Get credentials from Google Gemini");
    h.get_by_label("Save");
    if let (Some(dir), Ok(img)) = (&dir, h.render()) {
        img.save(std::path::Path::new(dir).join("genai-provider.png")).unwrap();
    }
    crate::dialogs::cancel(h.state_mut(), id).unwrap();
    h.state_mut().run("select.rect", json!({"x": 8, "y": 8, "width": 16, "height": 16})).unwrap();
    let ctx = h.ctx.clone();
    crate::menus::invoke(h.state_mut(), &ctx, FILL, json!({})).unwrap();
    h.run_steps(6);
    h.get_by_label("Generate");
    if let (Some(dir), Ok(img)) = (&dir, h.render()) {
        img.save(std::path::Path::new(dir).join("genai-prompt.png")).unwrap();
    }
    // The options bar of a selection tool: the button sits after Select and Mask….
    let prompt = h.state().ui.dialogs[0].id;
    crate::dialogs::cancel(h.state_mut(), prompt).unwrap();
    h.state_mut().ui.tool = crate::state::Tool::RectMarquee;
    h.run_steps(6);
    h.get_by_label("Generative Fill…");
    if let (Some(dir), Ok(img)) = (&dir, h.render()) {
        img.save(std::path::Path::new(dir).join("genai-options-bar.png")).unwrap();
    }
}

/// Properties shows a generative layer's variations: clicking one shows it, and Generate More
/// adds one by default (`GENAI_SNAPSHOT_DIR=… cargo test` saves a look).
#[test]
fn properties_show_the_variations() {
    let store = Store::default();
    store.borrow_mut().insert("gemini".into(), SECRET.into());
    let store2 = store.clone();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 760.0)).with_max_steps(64).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        let mut app = app_with(&store2);
        app.session.prefs.edit(|p| p.dialogs.insert(PREF.into(), json!({"provider": "gemini", "model": "m"})));
        restore(&mut app).unwrap();
        app.run("select.rect", json!({"x": 8, "y": 8, "width": 32, "height": 24})).unwrap();
        app
    });
    h.state_mut().run(FILL, json!({"prompt": "a red kite", "variations": 3})).unwrap();
    for _ in 0..200 {
        h.run_steps(1);
        if h.state().session.generative_layer().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let group = h.state().session.generative_layer().expect("the fill finished");
    h.run_steps(4);
    let shown = |h: &egui_kittest::Harness<'_, PhotocraftApp>| -> Vec<bool> {
        h.state().session.active().unwrap().doc.layer(group).unwrap().children().unwrap().iter().map(|l| l.visible).collect()
    };
    assert_eq!(shown(&h), [true, false, false]);
    h.get_by_label("Variations");
    h.get_by_label("Generate More");
    if let (Some(dir), Ok(img)) = (std::env::var_os("GENAI_SNAPSHOT_DIR"), h.render()) {
        img.save(std::path::Path::new(&dir).join("genai-variations.png")).unwrap();
    }
    // The Layers panel lists "Variation 3" too; the thumbnail is the one in Properties, above it.
    let thumb = h.query_all_by_role_and_label(egui::accesskit::Role::Button, "Variation 3").min_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y)).unwrap();
    thumb.click();
    h.run_steps(4);
    assert_eq!(shown(&h), [false, false, true]);
    h.get_by_label("Generate More").click();
    for _ in 0..200 {
        h.run_steps(1);
        if shown(&h).len() == 4 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(shown(&h), [false, false, false, true]);
}

#[test]
fn saved_settings_from_the_single_provider_version_migrate() {
    let old = json!({"provider": "gemini", "model": "gemini-nano-banana-2.1"});
    let s = parse_saved(Some(&old));
    assert_eq!(s.configured, vec!["gemini".to_owned()]);
    assert_eq!(s.default, "gemini");
    assert_eq!(s.models.get("gemini").map(String::as_str), Some("gemini-nano-banana-2.1"));
    assert_eq!(parse_saved(Some(&json!({"provider": ""}))), Saved::default());
    let new = json!({"default": "gone", "models": {"gemini": ""}, "configured": ["gemini", "not-a-provider"]});
    let s = parse_saved(Some(&new));
    assert_eq!(s.configured, vec!["gemini".to_owned()], "unknown ids are dropped");
    assert_eq!(s.default, "gemini", "a default that isn't configured falls back to the first");
    assert_eq!(parse_saved(Some(&s.to_json())), s);
}

#[test]
fn setting_up_a_provider_saves_its_fields_outside_the_dialog_and_installs_it() {
    let store = Store::default();
    let mut app = app_with(&store);
    let id = open_provider(&mut app, "gemini").unwrap();
    let fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    app.genai.key_inputs.insert("api_key", SECRET.into());
    let r = confirm(&mut app, &fields).unwrap();
    assert_eq!(r["ready"], true);
    assert_eq!(app.session.image_provider().unwrap().info().id, "gemini");
    let saved = store.borrow().get("gemini").cloned().unwrap();
    assert_eq!(photocraft_genai::Credentials::from_secret(&saved).unwrap().require("api_key").unwrap().expose(), SECRET);
    assert!(app.genai.key_inputs.is_empty(), "typed secrets are dropped after saving");
    let inspected = serde_json::to_string(&app.ui.dialogs.iter().map(|d| d.fields.clone()).collect::<Vec<_>>()).unwrap();
    assert!(!inspected.contains(SECRET), "never in dialog fields (ui.inspect)");
    assert!(!serde_json::to_string(&app.session.prefs().dialogs).unwrap().contains(SECRET), "never in preferences");
}

#[test]
fn a_provider_is_not_saved_without_every_field() {
    let store = Store::default();
    let mut app = app_with(&store);
    let id = open_provider(&mut app, "gemini").unwrap();
    let fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    let e = confirm(&mut app, &fields).unwrap_err();
    assert!(e.contains("Google Gemini"), "{e}");
    assert!(store.borrow().is_empty());
    assert!(app.session.image_provider().is_none());
}

#[test]
fn restore_reports_unreadable_credentials_without_panicking() {
    let store = Store::default();
    store.borrow_mut().insert("gemini".into(), "{corrupt".into());
    let (app, restored) = configured(&store, &["gemini"]);
    let e = restored.unwrap_err();
    assert!(e.contains("gemini"), "{e}");
    assert!(app.session.image_provider().is_none());
}

#[test]
fn removing_the_only_provider_deletes_its_credentials_and_turns_fill_off() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini"]);
    restored.unwrap();
    remove_provider(&mut app, "gemini").unwrap();
    assert!(!store.borrow().contains_key("gemini"));
    assert!(app.session.image_provider().is_none());
    assert_eq!(parse_saved(app.session.prefs().dialogs.get(PREF)), Saved::default());
    assert!(!fill_in_menu(&app));
}

#[test]
fn the_prompt_dialog_passes_the_chosen_provider_and_remembers_it() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini"]);
    restored.unwrap();
    let id = open_prompt(&mut app);
    let mut f = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    assert_eq!(f["provider"], "gemini", "starts on the default");
    f.insert("prompt".into(), json!("a lighthouse"));
    confirm(&mut app, &f).unwrap();
    assert_eq!(app.genai.last_provider.as_deref(), Some("gemini"));
    f.insert("prompt".into(), json!(""));
    assert!(can_confirm(&app, &f), "Gemini continues the image without a prompt");
    let mut other = Map::new();
    other.insert("x".into(), json!(1));
    assert!(can_confirm(&app, &other), "other dialogs are not affected");
}

#[test]
fn restore_installs_the_rest_when_one_credential_set_is_broken() {
    let store = Store::default();
    store.borrow_mut().insert("openai".into(), "{corrupt".into());
    let (app, restored) = configured(&store, &["openai", "gemini"]);
    let e = restored.unwrap_err();
    assert!(e.contains("openai"), "{e}");
    assert_eq!(app.session.image_provider_ids(), vec!["gemini".to_owned()]);
    assert_eq!(app.session.image_provider().unwrap().info().id, "gemini", "the default falls back to one that works");
}

#[test]
fn removing_a_provider_moves_the_default() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["openai", "gemini"]);
    restored.unwrap();
    remove_provider(&mut app, "openai").unwrap();
    assert!(!store.borrow().contains_key("openai"));
    assert_eq!(app.session.image_provider_ids(), vec!["gemini".to_owned()]);
    assert_eq!(parse_saved(app.session.prefs().dialogs.get(PREF)).default, "gemini");
}

#[test]
fn the_settings_default_choice_is_saved_on_ok() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini", "openai"]);
    restored.unwrap();
    let id = open_settings(&mut app);
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    fields.insert("default".into(), json!("openai"));
    confirm(&mut app, &fields).unwrap();
    assert_eq!(app.session.image_provider().unwrap().info().id, "openai");
}

#[test]
fn the_prompt_dialog_offers_configured_providers_and_remembers_the_choice() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini", "ideogram-v3"]);
    restored.unwrap();
    let id = open_prompt(&mut app);
    let mut f = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    assert_eq!(f["provider"], "gemini", "starts on the default");
    f.insert("provider".into(), json!("ideogram-v3"));
    f.insert("prompt".into(), json!("a lighthouse"));
    confirm(&mut app, &f).unwrap();
    assert_eq!(app.genai.last_provider.as_deref(), Some("ideogram-v3"));
    app.ui.dialogs.clear();
    let id = open_prompt(&mut app);
    assert_eq!(app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields["provider"], "ideogram-v3", "remembered this session");
}

#[test]
fn ok_is_off_for_an_empty_prompt_on_a_provider_that_needs_one() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini", "ideogram-v3"]);
    restored.unwrap();
    let id = open_prompt(&mut app);
    let mut f = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    f.insert("prompt".into(), json!(""));
    assert!(can_confirm(&app, &f));
    f.insert("provider".into(), json!("ideogram-v3"));
    assert!(!can_confirm(&app, &f));
    f.insert("prompt".into(), json!("a lighthouse"));
    assert!(can_confirm(&app, &f));
}

#[test]
fn editing_keeps_saved_fields_left_empty_and_requires_every_field() {
    let store = Store::default();
    store.borrow_mut().insert("firefly".into(), secret(&[("client_id", "cid")]));
    let mut app = app_with(&store);
    let id = open_provider(&mut app, "firefly").unwrap();
    let fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    let e = confirm(&mut app, &fields).unwrap_err();
    assert!(e.contains("Adobe Firefly"), "{e}");
    app.genai.key_inputs.insert("client_secret", "csecret".into());
    confirm(&mut app, &fields).unwrap();
    let saved = photocraft_genai::Credentials::from_secret(&store.borrow()["firefly"]).unwrap();
    assert_eq!(saved.require("client_id").unwrap().expose(), "cid", "kept");
    assert_eq!(saved.require("client_secret").unwrap().expose(), "csecret", "added");
}

#[test]
fn restore_skips_a_half_saved_two_field_provider() {
    let store = Store::default();
    store.borrow_mut().insert("firefly".into(), secret(&[("client_id", "only-half")]));
    let (app, restored) = configured(&store, &["firefly", "gemini"]);
    assert!(restored.unwrap_err().contains("firefly"));
    assert_eq!(app.session.image_provider_ids(), vec!["gemini".to_owned()]);
}

#[test]
fn generate_more_names_the_provider_that_made_the_layer() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini", "openai"]);
    restored.unwrap();
    assert_eq!(app.session.image_provider().unwrap().info().id, "gemini");
    let r = app.run(FILL, json!({"prompt": "a kite", "variations": 1, "provider": "openai"})).unwrap();
    let group = LayerId(r["layer"].as_u64().unwrap());
    let tip = more_tooltip(&app, group);
    assert!(tip.contains("openai") && !tip.contains("gemini"), "{tip}");
    // OpenAI is removed: the tooltip says so instead of naming the default.
    remove_provider(&mut app, "openai").unwrap();
    let tip = more_tooltip(&app, group);
    assert!(tip.contains("OpenAI") && tip.contains("no longer set up"), "{tip}");
}

#[test]
fn saving_one_provider_succeeds_when_another_cannot_load() {
    let store = Store::default();
    store.borrow_mut().insert("openai".into(), "{corrupt".into());
    let (mut app, restored) = configured(&store, &["gemini", "openai"]);
    assert!(restored.is_err());
    let id = open_provider(&mut app, "gemini").unwrap();
    let fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    app.genai.key_inputs.insert("api_key", "AIza-another-secret".into());
    let r = confirm(&mut app, &fields).expect("the save itself worked");
    assert_eq!(r["ready"], true);
    assert!(app.session.image_provider_by_id("gemini").is_some());
    assert!(app.ui.status.contains("openai"), "{}", app.ui.status);
    // The settings OK reports the same way.
    app.ui.status.clear();
    let id = open_settings(&mut app);
    let fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    confirm(&mut app, &fields).expect("saving the default worked");
    assert!(app.ui.status.contains("openai"), "{}", app.ui.status);
    let saved = saved(&app);
    assert_eq!(row_status(&app, &saved, "gemini"), "Set up");
    assert_eq!(row_status(&app, &saved, "openai"), "Needs credentials");
    assert_eq!(row_status(&app, &saved, "stability"), "Not set up");
}

#[test]
fn the_settings_default_ignores_an_empty_choice() {
    let store = Store::default();
    let (app, restored) = configured(&store, &["gemini", "openai"]);
    restored.unwrap();
    let mut fields = Map::new();
    fields.insert(SETTINGS_MARK.into(), json!(true));
    fields.insert("default".into(), json!(""));
    assert_eq!(default_choice(&fields, &saved(&app)), "gemini", "an empty default (dialog opened before any provider) falls back");
    fields.insert("default".into(), json!("openai"));
    assert_eq!(default_choice(&fields, &saved(&app)), "openai");
    fields.remove("default");
    assert_eq!(default_choice(&fields, &saved(&app)), "gemini");
}

fn comfy_fields() -> Map<String, Value> {
    let workflow = json!({
        "6": {"class_type": "CLIPTextEncode", "inputs": {"text": ""}},
        "10": {"class_type": "LoadImage", "inputs": {"image": "input.png"}},
        "11": {"class_type": "LoadImage", "inputs": {"image": "mask.png"}},
        "3": {"class_type": "KSampler", "inputs": {"seed": 1}},
        "9": {"class_type": "SaveImage", "inputs": {"images": ["3", 0]}}
    });
    let mut fields = Map::new();
    fields.insert("endpoint".into(), json!("http://127.0.0.1:8188"));
    fields.insert("workflow".into(), json!(workflow.to_string()));
    for (name, node, input) in [("prompt", "6", "text"), ("image", "10", "image"), ("mask", "11", "image"), ("seed", "3", "seed")] {
        fields.insert(format!("{name}Node"), json!(node));
        fields.insert(format!("{name}Input"), json!(input));
    }
    fields.insert("outputNode".into(), json!("9"));
    fields
}

#[test]
fn comfy_config_serializes_api_graph_and_checks_mapping_nodes_and_inputs() {
    let mut fields = comfy_fields();
    let config = provider_config(&fields, "comfyui").unwrap();
    assert_eq!(config["mappings"]["prompt"], json!({"node": "6", "input": "text"}));
    assert_eq!(config["mappings"]["output"], "9");
    assert_eq!(config["workflow"]["10"]["inputs"]["image"], "input.png");
    fields.insert("imageNode".into(), json!("missing"));
    assert!(provider_config(&fields, "comfyui").unwrap_err().contains("missing node"));
    fields.insert("imageNode".into(), json!("10"));
    fields.insert("imageInput".into(), json!("unknown"));
    assert!(provider_config(&fields, "comfyui").unwrap_err().contains("no input"));
    fields.insert("imageNode".into(), json!(""));
    assert!(provider_config(&fields, "comfyui").unwrap()["mappings"].get("image").is_none());
    fields.insert("workflow".into(), json!("not JSON"));
    assert!(provider_config(&fields, "comfyui").unwrap_err().contains("Invalid workflow JSON"));
}

#[test]
fn compatible_configuration_persists_without_credentials_and_restores_without_fake_key() {
    let store = Store::default();
    let mut app = app_with(&store);
    let id = open_provider(&mut app, "compatible").unwrap();
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    fields.insert("endpoint".into(), json!("http://127.0.0.1:9000/v1"));
    fields.insert("model".into(), json!("local-image-model"));
    confirm(&mut app, &fields).unwrap();
    assert!(store.borrow().is_empty());
    let configured = saved(&app);
    let config: Value = serde_json::from_str(&configured.models["compatible"]).unwrap();
    assert_eq!(config["endpoint"], "http://127.0.0.1:9000/v1");
    assert_eq!(config["model"], "local-image-model");
    assert_eq!(parse_saved(Some(&configured.to_json())), configured);
    assert_eq!(destination(&app, "compatible").as_deref(), Some("http://127.0.0.1:9000/v1"));
    let mut restarted = app_with(&store);
    restarted.session.prefs.edit(|p| p.dialogs.insert(PREF.into(), configured.to_json()));
    restore(&mut restarted).unwrap();
    assert!(restarted.session.image_provider_by_id("compatible").is_some());
}

#[test]
fn memory_only_keys_survive_restore_without_entering_store_fields_or_preferences() {
    let store = Store::default();
    let mut app = app_with(&store);
    let id = open_provider(&mut app, "gemini").unwrap();
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    fields.insert("memoryOnly".into(), json!(true));
    app.genai.key_inputs.insert("api_key", SECRET.into());
    confirm(&mut app, &fields).unwrap();
    assert!(store.borrow().is_empty());
    restore(&mut app).unwrap();
    assert!(app.session.image_provider_by_id("gemini").is_some());
    assert!(app.genai.key_inputs.is_empty());
    assert!(!serde_json::to_string(&fields).unwrap().contains(SECRET));
    assert!(!serde_json::to_string(&app.session.prefs().dialogs).unwrap().contains(SECRET));
    cancelled(&mut app);
    restore(&mut app).unwrap();
    assert!(app.session.image_provider_by_id("gemini").is_some());
    remove_provider(&mut app, "gemini").unwrap();
    assert!(!app.genai.memory_keys.contains_key("gemini"));
    assert!(app.session.image_provider().is_none());
}

#[test]
fn clearing_credentials_removes_persistent_and_memory_keys_and_disables_required_provider() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini"]);
    restored.unwrap();
    app.genai.memory_keys.insert("gemini".into(), photocraft_genai::Credentials::api_key(ApiKey::new(SECRET).unwrap()));
    let id = open_provider(&mut app, "gemini").unwrap();
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    fields.insert("clearCredentials".into(), json!(true));
    confirm(&mut app, &fields).unwrap();
    assert!(store.borrow().is_empty());
    assert!(!app.genai.memory_keys.contains_key("gemini"));
    assert!(app.session.image_provider().is_none());
}

#[test]
fn operation_dialogs_route_separately_and_default_to_one_result() {
    assert!(crate::dialogs::is_route_marker(OPERATION));
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini"]);
    restored.unwrap();
    for command in [GENERATE, FILL, EXPAND, BLEND] {
        let id = open_operation(&mut app, command);
        let fields = &app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields;
        assert_eq!(operation(fields), command);
        assert_eq!(fields["__label"], operation_label(command));
        assert_eq!(fields["variations"], 1);
        assert_eq!(can_confirm(&app, fields), command != GENERATE);
        if command == GENERATE {
            assert_eq!(fields["width"], 1024);
            assert_eq!(fields["height"], 1024);
        }
        if command == EXPAND {
            assert_eq!(fields["right"], 256);
        }
        if command == BLEND {
            assert_eq!(fields["margin"], 32);
        }
        assert_eq!(open_operation(&mut app, command), id);
        assert!(menu(&mut app, command, &json!({"prompt": "explicit"})).is_none());
    }
    let mut off = app_with(&Store::default());
    for command in [GENERATE, FILL, EXPAND, BLEND] {
        assert!(menu(&mut off, command, &json!({})).unwrap().unwrap().get("dialog").is_some());
    }
}

#[test]
fn automation_requires_explicit_settings_opt_in() {
    let mut app = app_with(&Store::default());
    let id = open_settings(&mut app);
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    assert_eq!(fields["allowAutomation"], false);
    fields.insert("allowAutomation".into(), json!(true));
    confirm(&mut app, &fields).unwrap();
    assert_eq!(app.session.prefs().dialogs["customAi"]["allowAutomation"], true);
    fields.insert("allowAutomation".into(), json!(false));
    confirm(&mut app, &fields).unwrap();
    assert_eq!(app.session.prefs().dialogs["customAi"]["allowAutomation"], false);
}

#[test]
fn custom_ai_dialogs_render() {
    let store = Store::default();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1280.0, 720.0)).with_max_steps(64).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        let mut app = app_with(&store);
        let saved = Saved {
            default: "compatible".into(),
            models: [("compatible".into(), default_config("compatible").to_string())].into(),
            configured: vec!["compatible".into()],
        };
        store_saved(&mut app, &saved);
        restore(&mut app).unwrap();
        app
    });
    let snapshot = |h: &mut egui_kittest::Harness<'_, PhotocraftApp>, name: &str| {
        h.run_steps(24);
        if let Some(dir) = std::env::var_os("GENAI_SNAPSHOT_DIR") {
            h.render().unwrap().save(std::path::Path::new(&dir).join(format!("genai-{name}.png"))).unwrap();
        }
    };
    let settings = open_settings(h.state_mut());
    snapshot(&mut h, "settings-narrow");
    assert!(h.get_by_label("Allow command clients and automation to submit AI prompts and images").rect().bottom() < 720.0);
    assert!(h.get_by_label("OK").rect().bottom() < 720.0);
    crate::dialogs::cancel(h.state_mut(), settings).unwrap();
    let dialog = open_provider(h.state_mut(), "compatible").unwrap();
    snapshot(&mut h, "compatible");
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Server URL");
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Image model");
    h.get_by_label("Keep credentials only until PhotoCraft exits");
    crate::dialogs::cancel(h.state_mut(), dialog).unwrap();
    let dialog = open_provider(h.state_mut(), "comfyui").unwrap();
    let config = provider_config(&comfy_fields(), "comfyui").unwrap();
    h.state_mut().ui.dialog_mut(dialog).unwrap().fields.insert("workflow".into(), json!(config.to_string()));
    h.run_steps(24);
    h.get_by_label("Apply configuration").hover();
    h.run_steps(24);
    h.get_by_label("Apply configuration").click();
    h.run_steps(24);
    let imported: Value = serde_json::from_str(h.state().ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields["workflow"].as_str().unwrap()).unwrap();
    assert!(imported.get("6").is_some(), "the Apply configuration button imports the API graph");
    snapshot(&mut h, "comfyui");
    h.get_by_label("Paste ComfyUI workflow (Export API format)");
    h.get_by_label("Workflow JSON and node/input mappings are valid.");
    assert!(h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Output node ID").rect().bottom() < 720.0);
    assert!(h.get_by_label("Save").rect().bottom() < 720.0);
    crate::dialogs::cancel(h.state_mut(), dialog).unwrap();
    for (command, name) in [(GENERATE, "generate"), (FILL, "fill"), (EXPAND, "expand"), (BLEND, "blend")] {
        let dialog = open_operation(h.state_mut(), command);
        snapshot(&mut h, name);
        h.get_by_role_and_label(egui::accesskit::Role::SpinButton, "Results");
        h.get_by_label("Each result may be billed by the provider. Nothing is sent until you click Generate.");
        crate::dialogs::cancel(h.state_mut(), dialog).unwrap();
    }
    h.state_mut().run("select.rect", json!({"x": 8, "y": 8, "width": 16, "height": 16})).unwrap();
    h.state_mut().ui.tool = crate::state::Tool::RectMarquee;
    snapshot(&mut h, "options-bar-narrow");
    let fill = h.get_by_label("Generative Fill…").rect();
    assert!(fill.right() < 1280.0 && fill.bottom() < 100.0);
}

#[test]
fn compatible_config_omits_unspecified_size() {
    let fields = json!({"endpoint": "https://example.invalid/v1", "model": "images", "size": "  ", "generate": false, "edit": true});
    let config = provider_config(fields.as_object().unwrap(), "compatible").unwrap();
    assert!(config.get("size").is_none());
    assert_eq!(config["generate"], false);
    assert_eq!(config["edit"], true);
}

#[test]
fn operation_choice_falls_back_to_provider_with_the_required_capability() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini"]);
    restored.unwrap();
    let dialog = open_provider(&mut app, "compatible").unwrap();
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    fields.insert("generate".into(), json!(false));
    confirm_provider(&mut app, &fields).unwrap();
    let mut saved = saved(&app);
    saved.default = "compatible".into();
    store_saved(&mut app, &saved);
    restore(&mut app).unwrap();
    assert_eq!(suitable_provider(&app, GENERATE).as_deref(), Some("gemini"));
    assert_eq!(suitable_provider(&app, FILL).as_deref(), Some("compatible"));
    let dialog = open_operation(&mut app, GENERATE);
    let mut prompt = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    assert_eq!(prompt["provider"], "gemini");
    prompt.insert("provider".into(), json!("compatible"));
    prompt.insert("prompt".into(), json!("an apple"));
    assert!(!can_confirm(&app, &prompt));
}

#[test]
fn switching_provider_editors_drops_the_previous_secret_buffer_and_dialog() {
    let mut app = app_with(&Store::default());
    let previous = open_provider(&mut app, "gemini").unwrap();
    app.genai.key_inputs.insert("api_key", SECRET.into());
    let next = open_provider(&mut app, "compatible").unwrap();
    assert_ne!(previous, next);
    assert!(app.ui.dialogs.iter().all(|d| d.id != previous));
    assert!(app.genai.key_inputs.is_empty());
    assert_eq!(app.ui.dialogs.iter().filter(|d| d.fields.contains_key(PROVIDER_MARK)).count(), 1);
}

#[test]
fn remote_dialog_confirmation_cannot_change_automation_consent() {
    let mut app = app_with(&Store::default());
    let dialog = open_settings(&mut app);
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    fields.insert("allowAutomation".into(), json!(true));
    let previous = app.session.prefs().dialogs.clone();
    app.session.authorize = Some(|_, _| Ok(()));
    assert!(confirm(&mut app, &fields).unwrap_err().contains("native settings"));
    assert_eq!(app.session.prefs().dialogs, previous);
    app.session.authorize = None;
    confirm(&mut app, &fields).unwrap();
    app.session.authorize = Some(|_, _| Ok(()));
    assert!(confirm(&mut app, &fields).is_err());
    fields.insert("allowAutomation".into(), json!(false));
    assert!(confirm(&mut app, &fields).is_err());
    assert_eq!(app.session.prefs().dialogs["customAi"]["allowAutomation"], true);
}

#[test]
fn full_comfy_configuration_can_be_pasted_and_applied_without_importing_credentials() {
    let config = provider_config(&comfy_fields(), "comfyui").unwrap();
    let mut pasted = config.clone();
    pasted["endpoint"] = json!("http://localhost:8188");
    pasted["api_key"] = json!(SECRET);
    let mut fields = Map::new();
    fields.insert("workflow".into(), json!(pasted.to_string()));
    import_comfy_config(&mut fields).unwrap();
    assert_eq!(fields["endpoint"], "http://localhost:8188");
    assert_eq!(fields["promptNode"], "6");
    assert_eq!(fields["imageInput"], "image");
    assert_eq!(fields["outputNode"], "9");
    assert!(!serde_json::to_string(&fields).unwrap().contains(SECRET));
    assert_eq!(provider_config(&fields, "comfyui").unwrap()["workflow"], config["workflow"]);
    fields.insert("workflow".into(), json!(json!({"workflow": {}, "mappings": {}}).to_string()));
    let before = fields.clone();
    assert!(import_comfy_config(&mut fields).is_err());
    assert_eq!(fields, before, "invalid imports leave the previous fields intact");
}

#[test]
fn queued_synthetic_input_cannot_save_credentials_change_settings_or_remove_provider() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini"]);
    restored.unwrap();
    let dialog = open_provider(&mut app, "gemini").unwrap();
    let fields = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    app.genai.key_inputs.insert("api_key", "replacement-secret".into());
    let before_store = store.borrow().clone();
    let before_prefs = app.session.prefs().dialogs.clone();
    app.automation_input = true;
    assert!(app.session.authorize.is_none());
    assert!(confirm(&mut app, &fields).is_err());
    assert!(confirm_provider(&mut app, &fields).is_err());
    assert!(remove_provider(&mut app, "gemini").is_err());
    let dialog = open_settings(&mut app);
    let mut settings = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    settings.insert("allowAutomation".into(), json!(true));
    assert!(confirm(&mut app, &settings).is_err());
    assert_eq!(*store.borrow(), before_store);
    assert_eq!(app.session.prefs().dialogs, before_prefs);
    assert!(app.session.image_provider_by_id("gemini").is_some());
    assert_eq!(app.genai.key_inputs.get("api_key").map(String::as_str), Some("replacement-secret"));
}

#[test]
fn removed_provider_in_an_open_prompt_never_silently_uses_another_destination() {
    let store = Store::default();
    let (mut app, restored) = configured(&store, &["gemini", "openai"]);
    restored.unwrap();
    let dialog = open_operation(&mut app, FILL);
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    fields.insert("provider".into(), json!("openai"));
    fields.insert("prompt".into(), json!("a red kite"));
    remove_provider(&mut app, "openai").unwrap();
    let before = app.session.active().unwrap().doc.layers.len();
    assert!(!can_confirm(&app, &fields));
    assert!(confirm_prompt(&mut app, &fields).unwrap_err().contains("no longer set up"));
    assert_eq!(app.session.active().unwrap().doc.layers.len(), before);
    assert!(!app.session.prefs().dialogs.contains_key(FILL));
}
