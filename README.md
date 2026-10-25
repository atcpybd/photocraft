# PhotoCraft Custom AI

PhotoCraft รุ่นปรับแต่งสำหรับ Windows x64 พร้อม AI Generate, Generative Fill, Generative Expand และ AI Seamless Blend บนเลเยอร์/มาสก์ที่แก้ไขต่อได้ พร้อม local command API และ MCP

พัฒนาต่อจาก [PhotoCraft v0.15.1](https://github.com/storytold/photocraft/releases/tag/v0.15.1) ของ ArtCraft team เป็น custom build ที่แยกการตั้งค่าและ credential store ของตัวเอง ไม่ใช่ release ทางการของโครงการต้นฉบับ

## ดาวน์โหลดและรันบน Windows

1. เปิด [Custom AI release](https://github.com/atcpybd/photocraft/releases/tag/custom-ai-v0.15.1) แล้วดาวน์โหลด **PhotoCraft-Custom-AI-Windows-x64.zip**
2. แตก ZIP ไปยังโฟลเดอร์ที่เขียนไฟล์ได้
3. เปิด **PhotoCraft-Custom-AI.exe** โดยเก็บ `portable.txt` ไว้ข้างไฟล์ exe
4. ไปที่ **Edit > Preferences > Generative AI Settings** แล้วตั้งค่า provider ตามด้านล่าง

ใช้ตัว ZIP ได้ทันทีโดยไม่ต้องติดตั้ง Rust หรือ build ซอร์ส ค่าตั้งค่าอยู่ใน `PhotoCraftData` ข้าง exe ส่วน API key ที่เลือกบันทึกถาวรอยู่ใน Windows Credential Manager

## ตั้งค่า API provider

### OpenAI หรือบริการที่รองรับ OpenAI-compatible Images API

เลือก **OpenAI-compatible image service > Set Up** แล้วกรอก:

| ช่อง | ค่า |
|---|---|
| API base URL | `https://api.openai.com/v1` สำหรับ OpenAI หรือ base URL ของผู้ให้บริการ |
| Model | ชื่อโมเดลสร้างภาพที่บัญชีคุณเข้าถึงได้ เช่น `gpt-image-2` |
| API key | คีย์จากผู้ให้บริการ กรอกในช่อง credential ของแอป |
| Size | เว้นว่างให้บริการเลือก หรือระบุขนาดที่โมเดลรองรับ เช่น `1024x1024` |
| Generate / Edit | เปิดเฉพาะ operation ที่บริการและโมเดลรองรับ |

กด **Save** แล้วเลือกเป็น default provider จากนั้นไปที่ **Image > Generate Image** เพื่อเริ่มสร้างภาพ

ตัว adapter ใช้ `/images/generations` สำหรับสร้างภาพ และ multipart `/images/edits` สำหรับแก้ไขภาพ ต้องเป็นบริการที่มี Images API; endpoint ที่มีเฉพาะ chat completions ใช้แทนไม่ได้ ดู [OpenAI Images documentation](https://developers.openai.com/api/docs/guides/image-generation)

สำหรับ OpenAI text-to-image ให้ใช้แถว **OpenAI-compatible image service** ส่วนแถว **OpenAI GPT Image** เป็น adapter เดิมสำหรับ masked fill

### Google Gemini

เลือก **Google Gemini > Set Up**, ใส่ [Gemini API key](https://aistudio.google.com/apikey), เลือกโมเดลสร้างภาพที่บัญชีรองรับ แล้วกด **Save** / เลือก default provider ดู [Gemini image documentation](https://ai.google.dev/gemini-api/docs/image-generation)

### Local ComfyUI

1. เปิด ComfyUI ที่ติดตั้งไว้และให้ server ทำงาน เช่น `http://127.0.0.1:8188`
2. ใน PhotoCraft เลือก **ComfyUI workflow > Set Up**
3. ใส่ Server URL และวาง workflow ที่ export เป็น **API format** หรือใช้ไฟล์ตัวอย่างด้านล่าง ถ้าวาง configuration wrapper เต็มไฟล์ ให้กด **Apply configuration**
4. เปลี่ยน `ckpt_name` เป็น checkpoint ที่มีอยู่จริงใน ComfyUI ของคุณ ตรวจ node/input mappings แล้วกด **Save**

| งาน | ตัวอย่าง |
|---|---|
| สร้างภาพจากข้อความ | [comfyui-generate.json](docs/examples/custom-ai/comfyui-generate.json) |
| Fill / Expand | [comfyui-inpaint.json](docs/examples/custom-ai/comfyui-inpaint.json) |
| รักษาตัวแบบแล้วปรับให้กลมกลืน | [comfyui-blend.json](docs/examples/custom-ai/comfyui-blend.json) |

เปลี่ยน workflow ที่ตั้งค่าไว้ให้ตรงกับงานนั้น การแก้ไขภาพต้องมีทั้ง image และ mask mapping; mask สีขาวคือส่วนที่ให้สร้างใหม่ และตัวอย่าง `LoadImageMask` ใช้ channel `red` แอปไม่ได้ติดตั้ง ComfyUI หรือดาวน์โหลดโมเดลให้

## ใช้ฟีเจอร์ AI

- **Image > Generate Image**: สร้างภาพเป็นเลเยอร์ใหม่
- **Edit > Generative Fill**: เลือกพื้นที่ก่อน แล้วสั่งสร้าง/แทนที่เนื้อหา
- **Edit > Generative Expand**: กำหนดขอบ canvas ที่ต้องการขยาย
- **Layer > AI Seamless Blend**: เลือกเลเยอร์ตัวแบบที่มีขอบ alpha หรือ mask แล้วสั่งให้ปรับแสง เงา สี มุมมอง และขอบให้เข้ากับฉาก ผลลัพธ์เป็นกลุ่มเลเยอร์/มาสก์แยกจากต้นฉบับ

ผลแต่ละงาน Undo ได้ เลือก variation หรือ Generate More ได้ใน Properties โดย Generate More ใช้ request context ของ session ปัจจุบัน เมื่อเปิดไฟล์ใหม่ ผลที่บันทึกไว้ยังเป็นเลเยอร์/มาสก์ปกติที่แก้ไขต่อได้ คุณภาพและความเหมือนของตัวแบบขึ้นกับโมเดล

เริ่มทดสอบด้วยผลลัพธ์ 1 ภาพ API อาจมีค่าบริการตาม provider ใส่คีย์ในแอปเท่านั้น ไม่ใส่ใน workflow, commit หรือ repo

## ควบคุมผ่าน API / MCP

ถ้าใช้ปุ่มในแอป ตั้งค่า provider ก็เริ่มได้เลย หากให้ script/agent ส่งงาน AI ให้เปิด **Allow command clients and automation to submit AI prompts and images** ใน Generative AI Settings ด้วยตัวเองก่อน

จากโฟลเดอร์ที่แตก ZIP:

```powershell
.\PhotoCraft-Custom-AI.exe --control 7878 --control-token-file .private/control.token --automation-read-root . --automation-write-root .
```

ตั้ง MCP client ให้เรียก:

```powershell
.\photocraft-cli.exe mcp --bridge 127.0.0.1:7878 --control-token-file .private/control.token
```

คำสั่งหลัก: `ai.providers`, `image.generateImage`, `edit.generativeFill`, `edit.generativeExpand`, `layer.aiSeamlessBlend` คำขอที่ใช้เวลานานให้ส่ง `wait:false` แล้วติดตาม `jobs.list` ดูรูปแบบ JSON/authentication ใน [คู่มือใช้งานและ API](docs/custom-ai.md#command-api-and-mcp)

## รันจากซอร์ส

ติดตั้ง Rust MSVC toolchain (ทดสอบด้วย 1.99.1) และ Visual Studio C++ Build Tools พร้อม Windows SDK จากนั้นใช้ **Developer PowerShell for VS 2022**:

```powershell
git clone --branch custom-ai https://github.com/atcpybd/photocraft.git
cd photocraft
cargo run --locked -p photocraft
```

การ compile ครั้งแรกใช้เวลานาน ถ้าต้องการใช้งานโปรแกรม ให้ใช้ ZIP ด้านบน ถ้าต้องการ build portable แบบเดียวกับ release ดู [ขั้นตอน build](docs/custom-ai.md#source-and-build)

## เอกสารและผลตรวจสอบ

- [คู่มือ provider และข้อกำหนด API](docs/custom-ai-providers.md)
- [คู่มือฉบับละเอียด](docs/custom-ai.md)
- [Verification report](docs/custom-ai-verification.md): engine 2,151, UI 3,710, desktop/automation/provider 436 tests ผ่าน; มีข้อจำกัด printer timeout และ Windows WARP driver ตามที่รายงานไว้
- ทดสอบ protocol ด้วย mocks และ local fixtures ยังไม่ได้ใช้คีย์จริงส่งงานที่คิดเงิน หรือรับรองคุณภาพของแต่ละบริการ

โค้ดอยู่ภายใต้ [MIT](LICENSE-MIT) หรือ [Apache-2.0](LICENSE-APACHE) พร้อม [NOTICE](NOTICE) และ [ATTRIBUTION](ATTRIBUTION.md) กราฟิก ArtCraft ที่สงวนสิทธิ์ถูกนำออกจาก branch นี้โดยคงข้อความ license ไว้
