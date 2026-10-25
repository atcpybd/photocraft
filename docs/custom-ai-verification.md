# PhotoCraft Custom AI — verification

Built on 25 October 2026 as a private Windows x64 edition of PhotoCraft v0.15.1. This is an independently modified build, not an official PhotoCraft release.

Code commit: `268b9a7701c9384a8ef4020380a6ee7f5a49b44a`. Base: `60c87b9884fdae0e464614d482a3bc435cbbc578`. Subsequent documentation-only changes do not change the executable code.

## Delivered behavior

Generate Image, Generative Fill, Generative Expand and AI Seamless Blend use the native engine, editable layers/masks and undo. The primary configurable backends are an OpenAI-compatible Images API, Gemini and local ComfyUI workflows. Other inherited fill adapters remain available. Credentials are separate from preferences. AI requests from command clients require explicit native settings consent.

## Checks completed

| Check | Result |
|---|---|
| Engine library, including 60 AI cases | 2,151 passed; 14 ignored |
| Entire UI suite, Vulkan on NVIDIA RTX 5050 | 3,710 passed; 16 ignored; native exit 0 |
| Desktop app unit/integration tests | 174 + 11 passed; 2 ignored |
| Automation unit/integration tests | 81 + 75 passed |
| Provider protocol tests | 95 passed |
| Focused AI UI / custom title tests | 34 / 8 passed (also included in full UI suite) |
| Clippy, affected crates and all targets | Passed with warnings denied |
| Workspace formatting / Git whitespace | Passed |
| Dependency layers | 32 crates, no violations |
| WebAssembly compatibility | All 25 required crates, plus HEIF feature check passed |
| Menu wiring / scorecard generation | 632/632 dispatch entries; generated reports updated |
| Native executable / CLI | Both launch and report version; x64 GUI/console PE headers verified |
| Portable runtime dependencies | Static C runtime; imports require Windows OS DLLs only |
| Authenticated desktop command API | Six checks passed, including protected-consent denial and a masked editable group surviving native save/reopen |
| Packaged CLI MCP bridge | Handshake, 25 tools, provider status and default AI-request denial passed |
| Real desktop HTTP transport | Local Images API generation/download round trip passed through the actual transport |
| Review | Provider, job/undo, credential and authorization fixes reviewed and tested |

The selected native test suites account for 6,297 passed tests and 32 intentionally ignored tests. The focused subsets are not added a second time to that count. Tests requiring a paid image service were not run. Protocol mocks and local fixtures establish request handling and edit invariants; they do not establish real model quality.

## Known verification limits

The adversarial `panic_hunt --ignored` gate failed on three unchanged printer commands: `file.print.page`, `file.printers` and `file.printOneCopy`. Each exceeded the four-second limit twice, with `dryRun:true`, on this Windows machine. A recheck reproduced the same three timeouts. No AI command failed this gate. Printer code was not changed to hide the failure.

One repeat full UI run using the default Windows software renderer terminated inside `d3d10warp.dll` with native exception `0xc0000005`. A supported `WGPU_BACKEND=vulkan` override selected the NVIDIA hardware adapter, and the complete suite then passed. No application code or assertions were changed for that rerun.

Native verification also logged an FXC shader diagnostic. The subsequent app status reported NVIDIA/DX12 GPU rendering with no fallback notice; the tested document and masked group rendered correctly and survived save/reopen. This report does not claim exhaustive graphics-driver coverage.

AI automation permission stayed off during desktop/MCP smoke testing. Successful AI operations were covered by provider and engine tests, including the actual desktop loopback transport. Real provider credentials, billing and output quality need verification with the user's chosen service. Generate More request context lasts for the current session; saved results remain editable layers/masks after reopening.

## Build and package

Rust 1.99.1, Microsoft C++ Build Tools 2022/MSVC 14.44. Release profile: optimization level 2, LTO off, 16 codegen units, static C runtime. The version text says `dev build` because official CI version stamping was not used; these are optimized release-profile executables. The private binaries are unsigned.

The portable folder contains the GUI executable, CLI, portable marker, setup/API guides, three ComfyUI workflow examples and license notices. It contains no API keys, test preferences, model checkpoints or ComfyUI server. Optional HEIF support and the separate craft-fonts bundle are not included. The source archive omits restricted upstream `docs/brand` artwork; all files needed for this custom native build are included.
