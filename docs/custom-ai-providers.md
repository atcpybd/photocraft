# Custom image services in PhotoCraft

PhotoCraft uses native Rust image providers for Generative Fill, Generative Expand,
Generate Image and AI Seamless Blend. Fill, Expand and Seamless Blend use the provider's
masked edit operation. Generate Image uses its text-to-image operation. A service must
actually support the requested operation; PhotoCraft does not add image capabilities
to a text-only model.

Configure providers in Generative AI Settings. API keys belong in the credential field,
which is saved in the operating system's credential store. Keep keys out of endpoints,
model names, workflow JSON and prompts. Local services may leave the key empty.

## OpenAI-compatible image API

Select `compatible` and enter an endpoint base, model ID, and optional output size.
For example, the serialized configuration stored in `models["compatible"]` is:

```json
{
  "endpoint": "http://127.0.0.1:8000/v1",
  "model": "gpt-image-2",
  "size": "1024x1024",
  "generate": true,
  "edit": true
}
```

The endpoint is the API base, including `/v1` when the service uses it. PhotoCraft appends
`/images/generations` or `/images/edits`. Generation sends JSON. Editing sends multipart
`image`, `mask`, `model`, `prompt`, and `n` fields. The mask has transparent
pixels in the area to generate, following the OpenAI convention. An omitted size lets the
service choose its default. Supported explicit sizes are `WIDTHxHEIGHT` with each side
between 1 and 8192 pixels. The adapter accepts either `b64_json` or an image URL result,
downloaded without forwarding the service's Authorization header. It leaves
`response_format` unset because GPT Image models do not support that legacy field.

Both capability flags default to `true`. Disable an operation if your service or model
does not implement it. A gateway that only supports chat completions will not work.
An API key, when supplied, is sent as `Authorization: Bearer …`.

## ComfyUI

Select `comfyui`. Run a ComfyUI server and export a workflow in **API format**, containing
node IDs mapped to objects with `class_type` and `inputs`. The regular editor JSON with
`nodes` and `links` is a different format. Set an endpoint such as
`http://127.0.0.1:8188` and provide the graph and mappings:

```json
{
  "endpoint": "http://127.0.0.1:8188",
  "workflow": {
    "6": {"class_type": "CLIPTextEncode", "inputs": {"text": "a landscape", "clip": ["4", 1]}},
    "4": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": "your-installed-checkpoint.safetensors"}},
    "7": {"class_type": "CLIPTextEncode", "inputs": {"text": "", "clip": ["4", 1]}},
    "5": {"class_type": "EmptyLatentImage", "inputs": {"width": 512, "height": 512, "batch_size": 1}},
    "3": {"class_type": "KSampler", "inputs": {"seed": 1, "steps": 20, "cfg": 7, "sampler_name": "euler", "scheduler": "normal", "denoise": 1, "model": ["4", 0], "positive": ["6", 0], "negative": ["7", 0], "latent_image": ["5", 0]}},
    "8": {"class_type": "VAEDecode", "inputs": {"samples": ["3", 0], "vae": ["4", 2]}},
    "9": {"class_type": "SaveImage", "inputs": {"images": ["8", 0], "filename_prefix": "PhotoCraft"}}
  },
  "mappings": {
    "prompt": {"node": "6", "input": "text"},
    "seed": {"node": "3", "input": "seed"},
    "output": "9"
  }
}
```

Complete editable examples are in [`examples/custom-ai`](examples/custom-ai). Replace the
checkpoint filename with one installed on your server. These original stock-node examples
describe the protocol; model installation, licence and suitability remain your choice.

The prompt mapping and an existing output node are required. The output must produce
`images` in ComfyUI history, usually through `SaveImage` or `PreviewImage`. The optional
seed mapping is refreshed per request. Node IDs and input names must exist in the exported
graph. Prompt, image and mask mappings target string inputs; seed targets an unsigned
integer. Separate mappings cannot overwrite the same input.

Masked edits require **both** `image` and `mask` mappings. PhotoCraft uploads the source
PNG and a grayscale PNG mask using `/upload/image`, then injects the filenames returned
by ComfyUI. White means generate and black means keep. For the stock `LoadImageMask`
node, set `channel` to **`red`**; its default alpha channel is unsuitable for this opaque
mask. Connect the mask to your workflow's inpainting or latent noise mask input. The
inpainting example uses `VAEEncodeForInpaint`; choose a checkpoint appropriate for it.

ComfyUI controls resolution through its workflow. The adapter does not rewrite arbitrary
resolution nodes to match PhotoCraft's requested size. Keep the graph appropriate for
the images you edit. Generation with an editing graph uses its configured image input
defaults; use the generation example for a standalone text-to-image graph.

PhotoCraft queues `/prompt`, polls `/history/{prompt_id}`, and downloads the first image
from the mapped output node through `/view`. Requests time out after two minutes plus
any in-flight transport timeout. Cancelling or timing out attempts to remove only that
prompt from the pending queue. Already running server work may finish: PhotoCraft never
sends a global interrupt that could stop another client's workflow. Optional `pollMs`
(1–10000, default 500) controls polling frequency.

## Connection and privacy

Remote endpoints require HTTPS. HTTP is accepted only for parsed loopback IP addresses
or exactly `localhost`. An endpoint cannot contain a username, password, query, fragment,
whitespace or backslash. Use the API key field for authentication; configure custom proxy
authentication on the server if it requires a different scheme.

The configured service receives the prompt and, for edits, the image and mask. A workflow
may itself contact other services through its nodes. Returned image URLs may refer to a
CDN. HTTP and image responses are limited to 48 MiB. PhotoCraft validates image signatures
and scrubs configured credential values from adapter errors. The desktop transport must
also enforce a streaming body limit and must not log request headers.

The provider registry retains the `BuildFn` interface: `model: &str` contains serialized
JSON for `compatible` and `comfyui`; other providers continue to accept a plain model ID.
Their stable registry IDs are also their exact `ProviderInfo.id` values.

Protocol references: [ComfyUI routes](https://docs.comfy.org/development/comfyui-server/comms_routes),
[ComfyUI server implementation](https://github.com/Comfy-Org/ComfyUI/blob/master/server.py),
[OpenAI Images API](https://platform.openai.com/docs/api-reference/images), and
[Gemini image generation](https://ai.google.dev/gemini-api/docs/image-generation).

The existing Gemini adapter was checked against the current Gemini documentation: its
Interactions endpoint, image/text input blocks, `response_format` image sizing and
`steps[].content[]` image outputs match that protocol. The image/mask prompt is an
instruction-based edit, since Gemini does not accept a separate native mask parameter.
