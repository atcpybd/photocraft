//! Generation, atomic canvas expansion and subject-preserving image edits.

use photocraft_genai::GenerateRequest;
use photocraft_geom::Size;

use super::*;

pub const GENERATE: &str = "image.generateImage";
pub const EXPAND: &str = "edit.generativeExpand";
pub const BLEND: &str = "layer.aiSeamlessBlend";
pub const PROVIDERS: &str = "ai.providers";

fn bad_for(cmd: &str, msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: cmd.into(), msg: msg.into() }
}

fn uint(p: &Value, key: &str, default: u32, min: u32, max: u32, cmd: &str) -> Result<u32> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| (u64::from(min)..=u64::from(max)).contains(n))
            .map(|n| n as u32)
            .ok_or_else(|| bad_for(cmd, format!("`{key}` must be a whole number from {min} to {max}"))),
    }
}

fn params(p: &Value, cmd: &str, required: bool) -> Result<(String, usize)> {
    if !p.is_object() {
        return Err(bad_for(cmd, "parameters must be an object"));
    }
    let prompt = match p.get("prompt") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(t)) => t.trim().to_owned(),
        Some(_) => return Err(bad_for(cmd, "`prompt` must be text")),
    };
    photocraft_genai::check_prompt(&prompt, required).map_err(|e| bad_for(cmd, e.to_string()))?;
    let count = variations_param(p, "variations").map_err(|_| bad_for(cmd, "`variations` must be a whole number from 1 to 4"))?;
    Ok((prompt, count))
}

pub(super) fn check_dimensions(w: u32, h: u32, cmd: &str) -> Result<()> {
    if w == 0 || h == 0 || w > RESULT_SIDE || h > RESULT_SIDE || u64::from(w) * u64::from(h) > MAX_WORK_PIXELS {
        return Err(bad_for(cmd, "AI image regions must be 1..4096 pixels per side and at most 16 megapixels"));
    }
    Ok(())
}

pub(super) fn generate_enabled(s: &Session) -> std::result::Result<(), String> {
    if s.genai.by_id.values().any(|p| p.capabilities().generate) {
        Ok(())
    } else {
        Err("set up a provider capable of generating images first (Generative AI Settings)".into())
    }
}

pub(super) fn check_can_generate(provider: &dyn ImageProvider, cmd: &str) -> Result<()> {
    if !provider.capabilities().generate {
        return Err(bad_for(cmd, format!("{} can't generate images", provider.info().name)));
    }
    Ok(())
}

fn edit_enabled(s: &Session) -> std::result::Result<(), String> {
    any_editor(s)?;
    s.active().ok_or_else(|| "no document open".to_owned()).map(|_| ())
}

fn blend_enabled(s: &Session) -> std::result::Result<(), String> {
    edit_enabled(s)?;
    let st = s.active().ok_or("no document open")?;
    let id = st.active_layer.ok_or("select a subject pixel layer first")?;
    let layer = st.doc.layer(id).ok_or("the selected layer is gone")?;
    let surface = layer.surface().ok_or("select a subject pixel layer first")?;
    if !layer.visible || layer.opacity <= 0.0 || layer.fill_opacity <= 0.0 {
        return Err("the subject layer must be visible".into());
    }
    let masked = layer.mask.as_ref().is_some_and(|m| m.enabled);
    if (!surface.format().alpha || st.doc.is_background(id)) && !masked {
        return Err("the subject needs a mask or a transparency boundary".into());
    }
    if let Some(path) = st.doc.path_of(id) {
        for end in 1..path.len() {
            if st.doc.layer_at(path.get(..end).ok_or("invalid subject layer path")?).is_some_and(|l| !l.visible || l.opacity <= 0.0) {
                return Err("the subject's containing groups must be visible".into());
            }
        }
    }
    Ok(())
}

/// Decode before placing, with the shared download/dimension/allocation limits. Invalid
/// variations are dropped if another is usable. No colour correction: relighting is requested.
pub(super) fn place_results(results: &[Vec<u8>], area: Rect) -> Result<Vec<Surface>> {
    let mut placed = Vec::new();
    let mut error = None;
    for bytes in results {
        match decode(bytes) {
            Ok((surface, bounds)) => {
                let fitted = scaled(&surface, bounds, area.width(), area.height());
                let mut out = Surface::new(fitted.format());
                out.write_region(area, &fitted.read_region(Rect::from_xywh(0, 0, area.width(), area.height())));
                placed.push(out);
            }
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    if placed.is_empty() {
        return Err(error.unwrap_or_else(|| other("the image service returned no images")));
    }
    Ok(placed)
}

pub(super) fn generate_results(
    ctx: &crate::jobs::JobCtx,
    provider: &dyn ImageProvider,
    request: &GenerateRequest,
    count: usize,
    area: Rect,
) -> Result<Vec<Surface>> {
    let mut results = Vec::new();
    let mut error = None;
    let job = ctx.clone();
    let cancel = photocraft_genai::CancelToken::new(move || job.cancelled());
    for i in 0..count {
        ctx.check()?;
        ctx.progress(0.15 + 0.65 * i as f32 / count as f32, &format!("Waiting for {}", provider.info().name));
        match provider.generate_cancellable(request, cancel.clone()) {
            Ok(image) => results.push(image.bytes),
            Err(e) => {
                error.get_or_insert(e);
            }
        }
    }
    ctx.check()?;
    if results.is_empty() {
        return Err(other(error.map_or_else(|| "the image service returned nothing".into(), |e| e.to_string())));
    }
    ctx.progress(0.9, "Placing the results");
    let placed = place_results(&results, area)?;
    ctx.check()?;
    Ok(placed)
}

fn result_group(doc: &Document, name: String, placed: &[Surface], mask: Option<Surface>) -> Result<(Layer, Vec<u64>)> {
    let children = placed.iter().enumerate().map(|(i, p)| variation_layer(doc, p, i + 1, i == 0)).collect::<Result<Vec<_>>>()?;
    let ids = children.iter().map(|l| l.id.0).collect();
    let mut group = Layer::group(name, children);
    group.mask = mask.map(|surface| LayerMask { surface, ..LayerMask::reveal_all() });
    Ok((group, ids))
}

/// One transaction moves original content, resizes the canvas and adds the results. Top-level
/// placement prevents a subject's parent mask from cutting off new cast shadows or canvas area.
fn apply(
    s: &mut Session,
    label: &str,
    name: String,
    placed: Vec<Surface>,
    mask: Option<Surface>,
    source: Source,
    expansion: Option<(u32, u32, u32, u32)>,
) -> Result<Value> {
    let area = source.area;
    let (group, ids) = s.edit(label, move |doc, active| {
        if let Some((left, top, width, height)) = expansion {
            crate::image_cmds::translate_doc(doc, EXPAND, left as i32, top as i32)?;
            doc.size = Size::new(width, height);
        }
        let (group, ids) = result_group(doc, name, &placed, mask)?;
        let id = doc.insert_above(None, group);
        crate::layer_multi_cmds::check_group_depth(doc, label)?;
        *active = Some(id);
        Ok((id, ids))
    })?;
    s.genai_sources.insert(group, Arc::new(source));
    Ok(json!({"layer":group.0,"variations":ids,"bounds":[area.x0,area.y0,area.width(),area.height()]}))
}

fn generate(s: &mut Session, p: &Value) -> Result<Value> {
    let (prompt, count) = params(p, GENERATE, true)?;
    let width = uint(p, "width", 1024, 1, RESULT_SIDE, GENERATE)?;
    let height = uint(p, "height", 1024, 1, RESULT_SIDE, GENERATE)?;
    check_dimensions(width, height, GENERATE)?;
    let provider = chosen_provider(s, p, GENERATE, None)?;
    check_can_generate(provider.as_ref(), GENERATE)?;
    let provider_id = provider.info().id.to_owned();
    let has_document = s.active().is_some();
    let request = GenerateRequest { prompt: prompt.clone(), aspect: (width, height), size: ImageSize::for_long_side(width.max(height)) };
    let area = Rect::from_xywh(0, 0, width, height);
    let source = Source { prompt: prompt.clone(), area, input: SourceInput::Generate(request.clone()), provider: provider_id };
    crate::jobs::run(
        s,
        "Generate Image",
        has_document,
        move |ctx| generate_results(ctx, provider.as_ref(), &request, count, area),
        move |s, placed| {
            let name = layer_name(&prompt);
            if has_document {
                apply(s, "Generate Image", name, placed, None, source, None)
            } else {
                // Build and convert everything before admitting a document; a failure leaves none.
                // Never target a document that the user opened while this independent job ran.
                let doc = Document::new("Generated Image", Size::new(width, height), ColorMode::Rgb, SampleType::U8);
                let (group, ids) = result_group(&doc, name, &placed, None)?;
                let document = doc.id;
                let previous = s.active;
                let index = s.add_document(doc, None);
                let inserted = s.edit("Generate Image", move |doc, active| {
                    let id = doc.insert_above(None, group);
                    *active = Some(id);
                    Ok(id)
                });
                let id = match inserted {
                    Ok(id) => id,
                    Err(e) => {
                        s.close(index);
                        s.active = previous;
                        return Err(e);
                    }
                };
                s.genai_sources.insert(id, Arc::new(source));
                Ok(json!({"document":document.0,"layer":id.0,"variations":ids,"bounds":[0,0,width,height]}))
            }
        },
    )
}

/// Preserve the visible subject and existing canvas in the submitted image. Fill's
/// `request_images` intentionally removes the subject, so it must never be used here.
fn preserving_request(doc: &Document, sel: &Surface, area: Rect, provider: &dyn ImageProvider, prompt: String) -> Result<EditRequest> {
    let k = upload_scale(f64::from(area.width().max(area.height())), provider.max_side(), provider.min_side());
    let w = ((f64::from(area.width()) * k).round() as u32).max(1);
    let h = ((f64::from(area.height()) * k).round() as u32).max(1);
    let local = Rect::from_xywh(0, 0, w, h);
    let image = scaled(&srgb_composite(doc, area)?, area, w, h).to_interleaved(local);
    let coverage = scaled(sel, area, w, h).read_region(local);
    let mut opaque = Vec::with_capacity(image.len());
    let mut mask = Vec::with_capacity(image.len());
    for (pixel, coverage) in image.as_chunks::<4>().0.iter().zip(coverage) {
        let a = f32::from(pixel[3]) / 255.0;
        for c in pixel.iter().take(3) {
            opaque.push((f32::from(*c) * a + HOLE * (1.0 - a)).round().clamp(0.0, 255.0) as u8);
        }
        opaque.push(255);
        let g = (coverage.clamp(0.0, 1.0) * 255.0).round() as u8;
        mask.extend_from_slice(&[g, g, g, 255]);
    }
    Ok(EditRequest {
        image: encode_rgba(opaque, w, h)?,
        mask: encode_rgba(mask, w, h)?,
        prompt,
        size: ImageSize::for_long_side(w.max(h)),
        cancel: Default::default(),
        preserve_source: true,
    })
}

fn expand(s: &mut Session, p: &Value) -> Result<Value> {
    let (prompt, count) = params(p, EXPAND, false)?;
    let left = uint(p, "left", 0, 0, RESULT_SIDE, EXPAND)?;
    let top = uint(p, "top", 0, 0, RESULT_SIDE, EXPAND)?;
    let right = uint(p, "right", 256, 0, RESULT_SIDE, EXPAND)?;
    let bottom = uint(p, "bottom", 256, 0, RESULT_SIDE, EXPAND)?;
    if left + top + right + bottom == 0 {
        return Err(bad_for(EXPAND, "add some canvas area to generate"));
    }
    edit_enabled(s).map_err(|e| bad_for(EXPAND, e))?;
    let provider = chosen_provider(s, p, EXPAND, None)?;
    check_can_edit(provider.as_ref(), EXPAND)?;
    check_prompt_for(provider.as_ref(), &prompt, EXPAND)?;
    let original = s.active().ok_or(EngineError::NoDocument)?.doc.clone();
    let width = original.size.width.checked_add(left).and_then(|v| v.checked_add(right)).ok_or_else(|| bad_for(EXPAND, "expanded width is too large"))?;
    let height = original.size.height.checked_add(top).and_then(|v| v.checked_add(bottom)).ok_or_else(|| bad_for(EXPAND, "expanded height is too large"))?;
    check_dimensions(width, height, EXPAND)?;
    let area = Rect::from_xywh(0, 0, width, height);
    let provider_id = provider.info().id.to_owned();
    crate::jobs::run(
        s,
        "Generative Expand",
        true,
        move |ctx| {
            ctx.progress(0.05, "Preparing expanded canvas");
            let mut expanded = (*original).clone();
            crate::image_cmds::translate_doc(&mut expanded, EXPAND, left as i32, top as i32)?;
            expanded.size = Size::new(width, height);
            let old = original.bounds().translate(left as i32, top as i32);
            let mask: Vec<f32> = (0..height).flat_map(|y| (0..width).map(move |x| if old.contains(x as i32, y as i32) { 0.0 } else { 1.0 })).collect();
            let mut selection = Surface::new(PixelFormat::GRAY8);
            selection.write_region(area, &mask);
            ctx.check()?;
            let request = preserving_request(&expanded, &selection, area, provider.as_ref(), prompt)?;
            let results = request_all(ctx, provider.as_ref(), &request, count)?;
            let placed = place_results(&results, area)?;
            ctx.check()?;
            let source = Source { prompt: request.prompt.clone(), area, input: SourceInput::Edit { request, reference: None }, provider: provider_id };
            Ok((placed, selection, source))
        },
        move |s, (placed, mask, source)| {
            apply(s, "Generative Expand", "Generative Expand".into(), placed, Some(mask), source, Some((left, top, width, height)))
        },
    )
}

fn subject_mask(doc: &Document, id: LayerId, margin: u32, ctx: &crate::jobs::JobCtx) -> Result<Surface> {
    let layer = doc.layer(id).ok_or_else(|| bad_for(BLEND, "the subject is gone"))?;
    let surface = layer.surface().ok_or_else(|| bad_for(BLEND, "select a subject pixel layer"))?;
    let area = doc.bounds();
    let mut coverage = Vec::with_capacity((u64::from(area.width()) * u64::from(area.height())) as usize);
    let (mut min, mut max) = (1.0f32, 0.0f32);
    for y in area.y0..area.y1 {
        ctx.check()?;
        for x in area.x0..area.x1 {
            let alpha = if surface.format().alpha { surface.sample_channel(x, y, surface.format().mode.color_channels()) } else { 1.0 };
            let value = alpha * layer.mask.as_ref().map_or(1.0, |m| m.value(x, y));
            let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
            min = min.min(value);
            max = max.max(value);
            coverage.push(value);
        }
    }
    if max <= 0.0 || min >= max {
        return Err(bad_for(BLEND, "the subject needs a nonempty mask or transparency boundary inside the canvas"));
    }
    let grown = if margin == 0 {
        coverage
    } else {
        let mut grown = sel_algo::expand(&coverage, area.width() as usize, area.height() as usize, margin as f32);
        grown.iter_mut().zip(coverage).for_each(|(g, a)| *g = g.max(a));
        grown
    };
    let mut mask = Surface::new(PixelFormat::GRAY8);
    mask.write_region(area, &grown);
    Ok(mask)
}

fn blend(s: &mut Session, p: &Value) -> Result<Value> {
    let (guidance, count) = params(p, BLEND, false)?;
    let margin = uint(p, "margin", 32, 0, 1024, BLEND)?;
    blend_enabled(s).map_err(|e| bad_for(BLEND, e))?;
    let provider = chosen_provider(s, p, BLEND, None)?;
    check_can_edit(provider.as_ref(), BLEND)?;
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let original = st.doc.clone();
    let subject = st.active_layer.ok_or_else(|| bad_for(BLEND, "select a subject pixel layer"))?;
    check_dimensions(original.size.width, original.size.height, BLEND)?;
    let mut prompt = "Harmonize the visible selected subject with its background. Preserve subject identity and composition. Match lighting, colors, perspective and edges; add natural contact and cast shadows in the masked surrounding region. Keep unmasked surroundings unchanged.".to_owned();
    if !guidance.is_empty() {
        prompt.push_str(" Additional guidance: ");
        prompt.push_str(&guidance);
    }
    photocraft_genai::check_prompt(&prompt, true).map_err(|e| bad_for(BLEND, e.to_string()))?;
    let provider_id = provider.info().id.to_owned();
    crate::jobs::run(
        s,
        "AI Seamless Blend",
        true,
        move |ctx| {
            ctx.progress(0.05, "Preparing subject and background");
            let mask = subject_mask(&original, subject, margin, ctx)?;
            let canvas = original.bounds();
            let selected = mask.content_bounds().intersect(&canvas);
            let area = context_area(selected, canvas, |w, h| provider.edit_aspect(w, h));
            let request = preserving_request(&original, &mask, area, provider.as_ref(), prompt)?;
            ctx.check()?;
            let results = request_all(ctx, provider.as_ref(), &request, count)?;
            let placed = place_results(&results, area)?;
            ctx.check()?;
            let mask = blended_mask(&mask, selected, canvas);
            let source = Source { prompt: request.prompt.clone(), area, input: SourceInput::Edit { request, reference: None }, provider: provider_id };
            Ok((placed, mask, source))
        },
        move |s, (placed, mask, source)| apply(s, "AI Seamless Blend", "AI Seamless Blend".into(), placed, Some(mask), source, None),
    )
}

fn providers(s: &mut Session, _: &Value) -> Result<Value> {
    let providers: Vec<_> = s.genai.by_id.iter().map(|(id,p)| {
        let info = p.info();
        let caps = p.capabilities();
        json!({"id":id,"name":info.name,"model":info.model,"host":info.host,"capabilities":{"generate":caps.generate,"edit":caps.edit,"variations":caps.variations}})
    }).collect();
    Ok(json!({"default":s.genai.default,"providers":providers}))
}

pub(super) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: GENERATE,
            label: "Generate Image…",
            menu: &["Image"],
            shortcut: None,
            params: r#"{"prompt":str,"provider":str?,"width":1..4096=1024,"height":1..4096=1024,"variations":1..4=1} generates a new group with one raster layer per result. Creates a document on success when none was open; retains originals in an existing document."#,
            enabled: generate_enabled,
            run: generate,
            journal: true,
        },
        CommandSpec {
            id: EXPAND,
            label: "Generative Expand…",
            menu: &["Edit"],
            shortcut: None,
            params: r#"{"prompt":str?,"provider":str?,"left":uint=0,"top":uint=0,"right":uint=256,"bottom":uint=256,"variations":1..4=1} enlarges the canvas by these pixel margins and generates only the exposed area. Final dimensions are limited to 4096 per side. Canvas and new masked layers undo together; originals remain intact."#,
            enabled: edit_enabled,
            run: expand,
            journal: true,
        },
        CommandSpec {
            id: BLEND,
            label: "AI Seamless Blend…",
            menu: &["Layer"],
            shortcut: None,
            params: r#"{"prompt":str?,"provider":str?,"margin":0..1024=32,"variations":1..4=1} harmonizes a visible subject pixel layer with a mask or alpha boundary and its background. Sends the visible subject plus surrounding context, includes margin for shadows, and adds masked results above the originals. Identity and perspective depend on the model."#,
            enabled: blend_enabled,
            run: blend,
            journal: true,
        },
        CommandSpec {
            id: PROVIDERS,
            label: "AI Providers",
            menu: &[],
            shortcut: None,
            params: "{} returns configured provider ids, names, models, destinations and capabilities, plus the default. Never returns credentials or contacts providers.",
            enabled: |_| Ok(()),
            run: providers,
            journal: false,
        },
    ]
}
