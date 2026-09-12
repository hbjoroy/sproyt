# Image generation

In a channel, send `/imagegen "your description"`. This queues a Heartsync
FLUX.1 image without posting the command or prompt to channel history.
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

The fixed server graph uses `flux1-dev-fp8.safetensors`,
`Heartsync_Flux_NSFW_uncensored.safetensors`, `clip_l.safetensors`,
`t5xxl_fp16.safetensors`, and `ae.safetensors`. It generates 768×768 PNG images,
28 Euler/simple steps, CFG 1, guidance 3.5, and LoRA strength 1. Users supply
only text; arbitrary graph nodes and image URLs are not accepted.

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

Image and video workflows share Santorini's memory with vLLM. Avoid running
large manual video jobs simultaneously. The integration does not restart
ComfyUI, evict other users' jobs, or stop vLLM.
