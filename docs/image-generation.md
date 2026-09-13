# Image generation

In a channel, send `/imagegen "your description"`. This queues an image
without posting the command or prompt to channel history.
The private image inbox above the composer shows progress and the completed
preview. It follows the account across refreshes and devices while Sprøyt is
open. It is an in-app whisper; it does not send an OS push notification.

**Godta** (accept) creates an ordinary image attachment owned by the requester
in the original channel and adds it to the draft. Existing caption text is
preserved. The requester still presses Send to publish. If the channel changed
while accepting, return to the original channel and choose **Legg i utkast**.
Accepted images remain recoverable in the inbox until posted, dismissed or
expired. **Avslå** (decline) removes the private preview without posting.

## Configuration

Set `SPROYT_COMFYUI_URL` to the private ComfyUI image server, for example
`http://192.168.68.152:8188`. Leave it unset to disable generation. Helm exposes
this as `config.comfyuiUrl`, with `networkPolicy.comfyuiCidrs` and
`networkPolicy.comfyuiPort` for narrowly scoped network access. ComfyUI must
remain a trusted private service: Sprøyt does not expose its API to browsers.

The fixed server graph uses `qwen_image_edit_2511_fp8mixed.safetensors`,
`qwen_2.5_vl_7b_fp8_scaled.safetensors`, `qwen_image_vae.safetensors`, and
`Qwen-Image-Edit-2511-Lightning-4steps-V1.0-bf16.safetensors`. It generates
1024×768 PNG images with four Euler/simple steps, CFG 1 and AuraFlow shift 3.1.
The Lightning LoRA accelerates generation; the previous Heartsync
nudity-oriented LoRA is no longer part of the application graph. Users supply
text; arbitrary graph nodes and image URLs are not accepted.

Copy the two reviewed photos from `assets/imagegen-references/` into
ComfyUI's `input/sproyt-references/`. Their source links, attribution, licence
and checksums are recorded in the adjacent README. Paroikia scenes receive
both the actual Artemis ferry and the real waterfront as image inputs;
unspecified Greek/Paros coastal scenes receive the ferry only. Other explicit
settings receive neither. Each reference is scaled to 0.5 megapixels and
supplied through TextEncodeQwenImageEditPlus for visual and VAE encoding. These are actual image inputs,
not just text descriptions. Replace/update references through a reviewed
server deployment; this change does not add a general browser upload field.

The private inbox exposes **Sjå referansefoto**, with photo credits and links.
PNG metadata retains the same provenance. Images are generated compositions,
not geographic reconstructions or evidence of a ferry's current position.

Migration 0035 stores durable requests in SQLite or PostgreSQL. Unique indexes
limit each account to one unreviewed request and the shared GPU queue to eight
outstanding requests. A database lease serializes polling across replicas.
Submission IDs prevent duplicate admission on network retries. If the server
loses confirmation during submission, it records a failure instead of risking
a duplicate GPU job. ComfyUI failures and 30-minute timeouts appear privately.

Previews are owner-only authenticated routes, checked against current channel
membership, with `private, no-store` headers. Preview bytes are retained for
seven days in Sprøyt and removed on decline/dismissal. Accepted attachments
follow the existing chat media storage lifecycle. ComfyUI retains its own
outputs on Santorini; declining in Sprøyt does not erase those server files.
Only a fixed-size PNG up to 8 MiB is retrieved from the configured backend.

## Prompt expansion and reference lookups

Set `SPROYT_VLLM_URL` to the OpenAI-compatible base URL (including `/v1`)
and `SPROYT_VLLM_API_KEY` to the existing vLLM credential. Sprøyt discovers
the served model through `/models` for each job. The first inference interprets
the request and chooses cartoon or realistic representation; the second
produces a concise, composed image prompt. Explicit subjects, actions, media
and locations take precedence. When no setting is given or implied, the
default is Paroikia's seafront on Paros one hour before sunset, with the
Artemis passenger ferry small in the distance behind the main subject.
Ordinary scenes use context-appropriate clothing. Non-sexual adult nudity is
preserved only when explicitly requested, including naturism and oil painting.
The prompt interpreter preserves group size and distinguishes an empty coast
from the town waterfront. Explicit locations and artistic media take priority.
Visual reference directions are appended deterministically for the selected
scene so the model receives the correct relationship between text and photos.

`SPROYT_IMAGEGEN_WEB_RESEARCH=true` enables up to two short public-reference
queries through Wikipedia's REST search API. It does not send the full prompt
to Wikipedia or fetch model-provided URLs. Retrieved excerpts are treated as
untrusted context. These are reference lookups, not live ferry tracking or
exhaustive web research. Lookup failure does not stop expansion. If vLLM fails,
times out or returns malformed output, generation uses the original prompt
and explains the fallback in the private prompt details.

The expanded text, style, actual model ID and consulted references are stored
with the private job and shown under **Sjå utvida biletprompt**. They are cleared
with the preview on decline, dismissal or expiry. Expansion has an 80-second
total deadline, leaving time within the worker lease for ComfyUI admission.

Helm equivalents are `config.vllmUrl`, `config.imagegenWebResearch`,
`secret.vllmExistingSecret`, `secret.vllmApiKeyKey`, and the narrowly scoped
`networkPolicy.vllmCidrs` / `networkPolicy.vllmPort`. Do not put API keys in values.

Image and video workflows share Santorini's memory with vLLM. Avoid running
large manual video jobs simultaneously. The integration does not restart
ComfyUI, evict other users' jobs, or stop vLLM.
