# PhotoCraft Custom AI

Personal native Windows x64 build based on PhotoCraft v0.15.1. This is an independently modified build, not an official PhotoCraft release. Original PhotoCraft copyright and license notices remain in the package.

## Start

Extract the portable ZIP to a writable folder and open PhotoCraft-Custom-AI.exe. The portable.txt marker makes the app keep settings in its own PhotoCraftData folder. It does not share the installed upstream app's settings. API keys saved persistently use a separate Windows Credential Manager service. Keep the marker beside the executable. Without portable mode, this build uses its separate PhotoCraftCustomAI settings and recovery folders.

## Configure a provider

Open Edit > Preferences > Generative AI Settings. Select Set Up for a provider, enter its settings, save, then choose it as the default.

- OpenAI-compatible image service: enter the API base URL (including /v1 where required), image model name and API key. The service must implement Images generation and/or multipart image editing. A chat-only endpoint cannot generate images. Capabilities can be disabled for services that only implement one operation. Use this entry for OpenAI text-to-image generation; the separate OpenAI GPT Image entry is the inherited masked-fill adapter.
- Google Gemini: enter an image-capable Gemini model and API key. Other built-in fill adapters are also available; only their supported operations are enabled.
- ComfyUI workflow: start your own ComfyUI server, normally http://127.0.0.1:8188. Paste an API-format workflow (not the editor's ordinary workflow export), then map the prompt, optional seed, image and mask inputs and the output node. Both image and mask mappings are required for editing. Choose the actual checkpoint installed on your server. Examples are in ComfyUI examples/. Choose the generation, inpaint or blend example to match the operation and replace the configured workflow when switching between them. No models or server are installed by this app.

Cloud endpoints require HTTPS. Plain HTTP is accepted only for loopback local servers. Keys go into the credential field; do not put them in URLs or workflow JSON. Memory-only credentials are forgotten at exit. The settings and operation dialog identify the destination; pressing Generate initiates the request. Providers may charge per request/result. Start with one result.

## AI operations

- Image > Generate Image: describe a new image, choose a provider and dimensions. A result becomes a new layer/group; with no document open a new document is created after success.
- Edit > Generative Fill: select a region, describe the replacement, and generate. An empty prompt asks compatible providers for contextual removal. Results go into a masked group with separate variations.
- Edit > Generative Expand: choose the number of pixels to add on each side. The original image is retained; the generated border and expanded canvas are one undoable operation.
- Layer > AI Seamless Blend: select an imported subject layer with alpha or a mask (raster or cached Smart Object). Set the surrounding margin for shadows and give an optional instruction. The input includes the subject, so the model can harmonize its appearance with the background. The output is a separate masked group; identity and exact geometry depend on the chosen model.

For ComfyUI blending, start from `comfyui-blend.json`: it encodes the visible subject before applying a noise mask. The fill template deliberately removes masked content and is appropriate for replacement rather than subject harmonization. Adjust denoise and your model to control how much the subject changes.

Each successful operation is undoable. Use the Properties panel to choose a variation or Generate More. Results are ordinary layers/masks and survive native save/reopen. Originals are retained. Generate More keeps its prompt and input context for the current session; after reopening, the saved results remain ordinary editable layers and masks. Cancellation discards pending results. A long network request may finish server-side after cancellation; it can still be billed. ComfyUI cancellation deletes this app's queued prompt and never globally interrupts another client's work.

## Command API and MCP

The same engine commands are available through the authenticated local control channel and through the included CLI's MCP bridge. Enable "Allow command clients and automation to submit AI prompts and images" in Generative AI Settings first. This permission also applies to local ComfyUI. Command clients cannot grant it through prefs.set.

Launch with a control port and token file, for example from this folder:

```powershell
.\PhotoCraft-Custom-AI.exe --control 7878 --control-token-file .private/control.token --automation-read-root . --automation-write-root .
```

Connect an MCP client with:

```powershell
.\photocraft-cli.exe mcp --bridge 127.0.0.1:7878 --control-token-file .private/control.token
```

The control transport uses newline-delimited JSON over loopback TCP. Authenticate first with method auth and params.token read from the private token file. Then use engine.execute with these examples as its params:

```json
{"command":"ai.providers","params":{}}
{"command":"image.generateImage","params":{"provider":"compatible","prompt":"A paper sculpture of a bird","width":1024,"height":1024,"variations":1}}
{"command":"edit.generativeFill","params":{"provider":"comfyui","prompt":"grass matching the scene","variations":1}}
{"command":"edit.generativeExpand","params":{"provider":"compatible","right":256,"bottom":256,"prompt":"continue the landscape"}}
{"command":"layer.aiSeamlessBlend","params":{"provider":"compatible","margin":32,"prompt":"Match the warm light and add a contact shadow"}}
```

For a long image request, pass `"wait": false` beside `command` and `params` in `engine.execute` (or beside `id` and `params` in MCP `command_run`), then poll `jobs.list` to follow the returned job ID. This avoids the control bridge's reply timeout while the provider continues working. Use `jobs.cancel` to discard a pending job. MCP exposes command_run / command_batch and jobs tools over the same engine. Provider credentials are configured in the desktop app and are never returned by inspection. This version provides the existing local command API, not a separate public REST server. The headless CLI does not automatically load the desktop providers; use its MCP bridge to the configured app.

## Source and build

The source archive contains the modified Rust workspace. On Windows, install Rust (MSVC toolchain) and Microsoft C++ Build Tools with the Desktop development with C++ workload, then build the desktop app and CLI. Follow AGENTS.md and docs/development.md. The build is based on tag v0.15.1 and adapts upstream AI work from PR #6209 plus custom providers, engine commands and UI.

The supplied Windows binaries use the static C runtime. Reproduce that build from a Visual Studio Developer PowerShell with Rust on PATH:

```powershell
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:CARGO_PROFILE_RELEASE_LTO = 'false'
$env:CARGO_PROFILE_RELEASE_CODEGEN_UNITS = '16'
$env:CARGO_PROFILE_RELEASE_OPT_LEVEL = '2'
cargo build --release --locked --target x86_64-pc-windows-msvc -p photocraft -p photocraft-cli
```

This build uses the default format features; optional HEIF support and the separate craft-fonts bundle are not included.

A separate verification report records the checks actually completed. Mock provider tests verify integration and invariants; they do not establish real model quality. No paid provider request is made during the build without explicit authorization.
