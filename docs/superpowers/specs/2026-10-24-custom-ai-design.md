# Custom PhotoCraft AI build

Date: 24 October 2026
Status: proposed design; implementation awaits review of this scope.

## Purpose

Create a personal Windows desktop build based on PhotoCraft v0.15.1. Bring image generation and editing into the editor so the user can work with layers and selections without exporting to another application. The user explicitly requested multiple API providers, including local ComfyUI. The prior requested workflow is blending an imported subject naturally into its background while retaining editable layers.

## First deliverable

A runnable Windows x64 portable application, modified source, build instructions, and test results. Use the existing native Rust/egui UI and engine. Keep the installed upstream application and its settings separate by using a dedicated configuration directory. No public repository, release publication, or upstream PR is required for this personal build.

Builder choice: base on the v0.15.1 tag, adopt useful MIT/Apache-licensed portions of the open AI contributions only after reviewing them, and resolve integration against this stable base. Do not replace the editor with a separate web application. Preserve upstream copyright, license and attribution notices. Replace restricted ArtCraft branding in the distributed custom application and identify it clearly as a personal build based on PhotoCraft. Internal crate names may remain unchanged.

## Providers and setup

The first version supports three configurable provider types:

1. OpenAI-compatible image API: editable base URL, model, API key, and generation/edit capabilities. Compatibility means the image generation and multipart image-edit endpoints; chat-only endpoints do not qualify. Official and custom compatible hosts use the same adapter.
2. Google Gemini image API: editable supported image model and API key. Inspect current official documentation before choosing the endpoint and model; do not assume that older contribution endpoints remain supported.
3. ComfyUI: configurable local server URL, plus an imported API-format workflow and node/input mappings for prompt, image, mask, seed and output. Provide a documented standard workflow template. Workflow validation must explain missing nodes or inputs. Use upload, queue, history and output endpoints, with bounded polling and cancellation. Local plain HTTP is permitted for loopback; remote endpoints use HTTPS. No automatic model or ComfyUI installation.

Each configuration declares its actual capabilities. Unsupported commands are disabled with an explanation. Allow a default provider and a provider choice for each operation. Persist API keys in Windows Credential Manager, with a memory-only option. Non-secret endpoint, model and workflow settings can be stored in the dedicated configuration directory. Never place keys in project files, logs, command inspection or ordinary preference exports.

## AI operations

- Generate Image: prompt to a new raster layer in the current document, or a new document when no document is open. Expose size and number of results within provider limits.
- Generative Fill: selected region plus surrounding context and prompt. Empty prompts request contextual removal where supported. Place each successful variation in a new layer/group masked to the original selection. Retain all original pixels.
- Generative Expand: explicitly choose enlarged canvas bounds, then generate only the exposed area with surrounding image context. Keep original content intact. Canvas changes and generated layers undo together; cancellation/failure leaves the document unchanged.
- AI Seamless Blend: require an imported/selected subject layer and a subject mask or alpha boundary, and use the visible background as context. Ask the model to harmonize lighting, shadows, colors, perspective and edges while preserving subject identity. Unlike removal, keep the subject visible in the submitted input. Include a bounded surrounding region for cast/contact shadows. Put the result on a new masked layer/group and keep the original subject available beneath it. Subject identity and perfect perspective are model-dependent and are not guaranteed.

An optional prompt guides each edit. Default to one requested result; allow more explicitly, since cloud providers can bill per result. Show provider, endpoint destination, and what image region will be sent before the first submission. The app does not contact providers at startup. No billed live API requests until the user configures the provider and initiates a generation or explicitly authorizes a test.

## Editing behavior

Run requests as cancellable background jobs. Provide progress, errors and retry controls without freezing the editor. One completed operation is one undo step. Capture document/layer identity and the input revision when the job begins. Refuse to apply stale results to a changed/closed document; report the reason rather than silently applying to the current layer. Do not flatten the project. Generated raster layers and masks must survive native save/reopen; use ordinary groups/layers for PSD compatibility.

Convert provider input through the existing color-management paths to an appropriate image format, then convert results back into the document mode/depth. Preserve original dimensions and selected region placement; resample returned output deliberately when needed. Cap input/output pixels, downloaded bytes, workflow size and request duration. Validate returned images before allocation. Report HTTP, model, quota, moderation, malformed-response and cancellation failures without exposing secrets. Do not send API credentials to image-download hosts or follow unchecked redirects.

## API support

Reuse PhotoCraft's engine commands, authenticated loopback control protocol, CLI and MCP entry points. Add generation, fill, expand, blend, provider capabilities/status and job cancellation through that same command registry. API operations must use already configured credentials; inspection never returns secrets. Provider configuration and any change allowing image uploads remain user-controlled. Explicitly enable remote image submission for automation in this custom app's settings before a command client can upload pixels. Local ComfyUI also requires an explicit automation opt-in.

Builder choice: no separate public REST server in the first version. The user-facing app and command API share the same implementation. If the user meant a separate HTTP REST service, that changes scope and can be added after clarification.

## Validation and completion

Install the Rust/MSVC prerequisites if missing, then build the portable Windows application. Retain setup scripts and logs in the workspace. Run meaningful provider protocol tests using mock responses and a local fake ComfyUI server, including mask polarity, node mapping, errors, cancellation, size limits and credential redaction. Test engine invariants: unchanged originals, correct result mask/position, atomic undo, no edits after failure/cancellation, and stale document rejection. Check affected crates, formatting, lint, layer rules and WebAssembly compilation per repository instructions.

Verify the real desktop UI by opening the custom build and inspecting screenshots of provider setup and the AI operation flow. Verify native save/reopen and an authenticated command-API operation using a deterministic mock provider. Keep any test-only mock provider unavailable in production settings. A real provider output is verified only if the user supplies a configured provider and authorizes the request; otherwise report protocol and application testing separately from live model quality.

Completion means source and a runnable Windows build with all three provider adapters and the four operations, documented configuration and API examples, passing relevant checks, and clear disclosure of any checks blocked by the build environment. An uncompiled patch is not a finished application. If full build prerequisites cannot be installed automatically, preserve progress and identify the exact missing prerequisite rather than presenting source as a working binary.

## Explicit limits

No model training, bundled local GPU model, subscription-login workaround, custom general-purpose plug-in framework, hosted cloud service, or promised perfect AI output. ComfyUI is supplied and operated by the user. Provider/model availability and billing are determined by the configured service. Keep compatibility changes narrow enough to bring future upstream releases into the personal build.
