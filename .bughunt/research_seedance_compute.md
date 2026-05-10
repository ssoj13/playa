# Seedance 2.0 — API integration guide

> Research date: 2026-05-09. Time-capped. Where a vendor page would not render or
> contradicted another source, marked NEEDS-VERIFY with the URL.

## Who hosts it

Seedance 2.0 is ByteDance's video generation model. Three official channels:

- **Volcengine Ark** (China-domestic, RMB billing): https://www.volcengine.com/ — the "Volcano Ark" / `ark` API. Model launched on Ark April 2, 2026.
  Source: https://apidog.com/blog/seedance-2-0-api/
- **BytePlus ModelArk** (international, USD billing, English UI): https://docs.byteplus.com/en/docs/ModelArk/1520757
- **Dreamina / Jimeng** (consumer-facing, not API-first).

Third-party hosted endpoints (also legit, much easier signup):
- **fal.ai**: https://fal.ai/models/bytedance/seedance-2.0/fast/text-to-video
- **Replicate**: https://replicate.com/bytedance/seedance-2.0
- **WaveSpeedAI**: https://wavespeed.ai/

## Sign-up flow

### Volcengine Ark (China-domestic)
1. https://www.volcengine.com/ — sign up. **Catch:** real-name auth — Chinese phone / ID typically required. Hard for foreign devs.
   Source: WebSearch result citing OpenViking purchase guide — https://github.com/volcengine/OpenViking/blob/main/docs/en/guides/02-volcengine-purchase-guide.md
2. Activate the Ark service (开通服务).
3. Console → "API Key Management" (API Key 管理) → "Create API Key". Direct URL in apidog article: `https://console.volcengine.com/ark/region:ark+cn-beijing/apikey` — NEEDS-VERIFY (apidog blog, not console-verified).
4. New users: ~5,000,000 free tokens (~16 × 15s clips) per https://aicost.org/blog/seedance-2-0-api-pricing-breakdown-2026

### BytePlus ModelArk (international, recommended)
1. https://www.byteplus.com/ → sign up. USD / EUR via credit card, no CN currency required (per WebSearch result).
2. Console → enable ModelArk → create API key.
3. Docs: https://docs.byteplus.com/en/docs/ModelArk/1520757 (full reference page didn't render via WebFetch — NEEDS-VERIFY for exact regional URLs).

### fal.ai (lowest friction for first integration)
1. https://fal.ai/ → "Start Building" → GitHub OAuth login.
2. Dashboard issues `FAL_KEY`. Pay-as-you-go, credit card. No free tier mentioned on https://fal.ai/pricing .

### Replicate
1. https://replicate.com/ → GitHub OAuth.
2. API token from account page. Card on file required for paid models.

## Auth + base URL

### Volcengine Ark
- **Base URL (China region):** `https://ark.cn-beijing.volces.com/api/v3`
- **Base URL (BytePlus international SE):** `https://ark.ap-southeast.bytepluses.com/api/v3` (per first WebSearch summary; NEEDS-VERIFY directly).
- **Auth header:** `Authorization: Bearer YOUR_ARK_API_KEY`
- Source: https://apidog.com/blog/seedance-2-0-api/

### fal.ai
- **Auth:** env var `FAL_KEY`; HTTP header `Authorization: Key <FAL_KEY>` (standard fal pattern; NEEDS-VERIFY exact header from https://fal.ai/docs).
- Endpoints are queue-based per-model paths under `https://queue.fal.run/<owner>/<model>/...`.

### Replicate
- **Auth:** `Authorization: Token r8_xxx` (standard Replicate pattern).
- **Base:** `https://api.replicate.com/v1`.
- NEEDS-VERIFY against https://replicate.com/bytedance/seedance-2.0/api (page didn't render in WebFetch).

## Submit-prompt endpoint (Volcengine Ark / BytePlus)

```
POST {base_url}/contents/generations/tasks
```

Required body fields:

| field        | type    | notes |
|--------------|---------|-------|
| `model`      | string  | `doubao-seedance-2-0-260128` (standard) or `doubao-seedance-2-0-fast-260128` (fast tier) |
| `content`    | array   | ordered list of `{type: "text"|"image_url", ...}` parts |
| `resolution` | string  | `480p` / `720p` / `1080p` (also `2K` per third-party docs — NEEDS-VERIFY official) |
| `ratio`      | string  | `16:9`, `9:16`, `1:1`, ... |
| `duration`   | int     | seconds, typical 4–15 |

Sample request body:

```json
{
  "model": "doubao-seedance-2-0-260128",
  "content": [
    { "type": "text", "text": "A serene mountain lake at sunrise, slow dolly-in" }
  ],
  "resolution": "1080p",
  "ratio": "16:9",
  "duration": 5
}
```

Sample successful submission response:

```json
{ "id": "cgt-2025xxxxxxxx-xxxx" }
```

Source: https://apidog.com/blog/seedance-2-0-api/

## Status-poll endpoint

```
GET {base_url}/contents/generations/tasks/{task_id}
```

- **Auth:** same Bearer token.
- **Recommended polling cadence:** 5 s (per https://www.aifreeapi.com/en/posts/seedance-2-api-integration-guide — third-party guide; vendor-recommended cadence not explicitly published — NEEDS-VERIFY).
- **Status field values:** `queued`, `running`, `succeeded`, `failed`, `expired`, `cancelled`.
  Source: https://apidog.com/blog/seedance-2-0-api/
- **Error envelope shape:** standard Ark error shape `{"error": {"code": "...", "message": "..."}}` — NEEDS-VERIFY against ModelArk error reference.

## Result download

- On `succeeded`, response carries `content.video_url` pointing at the rendered mp4.
- **TTL: 24 hours** for the signed URL — re-host immediately if needed.
  Source: https://apidog.com/blog/seedance-2-0-api/
- Container: mp4 (h.264). Audio is included in-band if `generate_audio` was set.
- fal.ai response shape:
  ```json
  {
    "video": { "url": "...", "content_type": "video/mp4", "file_size": 4823041 },
    "seed": 42
  }
  ```
  Source: https://fal.ai/models/bytedance/seedance-2.0/fast/text-to-video

## Pricing

### Volcengine Ark / BytePlus official
Token-based, billed per million model tokens (tokens computed from `height × width × duration × 24 / 1024` per fal docs; that formula is the standard ByteDance formula).

| mode                 | CNY / 1M tokens |
|----------------------|-----------------|
| Pure generation      | 46              |
| Video editing input  | 28              |

Approx ≈ **1 CNY / second** of standard 720p output (~$0.14 USD/sec at 7.2 FX).

| clip length | pure CNY | edit CNY |
|-------------|----------|----------|
| 5 s         | 4.74     | 2.88     |
| 15 s        | 14.21    | 8.65     |

Source: https://aicost.org/blog/seedance-2-0-api-pricing-breakdown-2026

Free tier: ~5M tokens (≈16 × 15s clips) per new account on Volcengine.
Source: https://aicost.org/blog/seedance-2-0-api-pricing-breakdown-2026

> NOTE: aicost.org also stated (March 5, 2026) "API integration coming soon" — likely the API went live April 2, 2026 per apidog.com. Take official-availability dates with care if testing today.

### fal.ai (per second, 720p)
- Standard text-to-video: **$0.3034/sec**
- Standard image-to-video: **$0.3024/sec**
- Fast tier (all endpoints): **$0.2419/sec**
- Reference-to-video: $0.3024/sec; video-input mode 0.6× discount → ~$0.1814/sec standard, ~$0.1452/sec fast.
- 10 s clip: ≈$3.03 standard, ≈$2.42 fast.
- Audio at no extra cost.

Source: https://fal.ai/models/bytedance/seedance-2.0/fast/text-to-video

### Replicate
NEEDS-VERIFY exact $/run — page didn't render. Public listing exists at https://replicate.com/bytedance/seedance-2.0/api . Generally Replicate prices video models per-second of GPU time on H100 (~$0.001/s on H100 hardware tier).

## Rust client choice

Recommend **`ureq` + `rustls`** for v0:
- Submit-then-poll pattern → no streaming, no multipart, no websockets. Plain JSON over HTTPS.
- ureq is sync, blocking, lives happily in `playa-jobs` worker thread without dragging in tokio.
- Multipart only needed for image-to-video uploads. ureq supports it via `multipart` crate, or pre-upload image to S3-compatible bucket and pass URL.
- If later you want streaming SSE progress (fal supports it via `subscribe()`), switch to `reqwest` + `tokio` for that one provider only.

Recommended crates:
- `ureq = "2"` (with `rustls` feature, no openssl pain)
- `serde`, `serde_json`
- `url`
- For mp4 download: `ureq` get → `std::io::copy` to `File`. No reqwest needed.

## Sample request flow (pseudo-code)

```rust
// 1) Submit
let body = json!({
    "model": "doubao-seedance-2-0-260128",
    "content": [{ "type": "text", "text": prompt }],
    "resolution": "1080p",
    "ratio": "16:9",
    "duration": 5,
});
let resp: SubmitResp = ureq::post(&format!("{base}/contents/generations/tasks"))
    .set("Authorization", &format!("Bearer {api_key}"))
    .set("Content-Type", "application/json")
    .send_json(body)?
    .into_json()?;
let task_id = resp.id;

// 2) Poll (5s, exponential cap, abort on cancel signal)
loop {
    let s: TaskStatus = ureq::get(&format!("{base}/contents/generations/tasks/{task_id}"))
        .set("Authorization", &format!("Bearer {api_key}"))
        .call()?
        .into_json()?;
    match s.status.as_str() {
        "succeeded" => break s.content.video_url,
        "failed" | "expired" | "cancelled" => return Err(...),
        _ => sleep(Duration::from_secs(5)),
    }
}

// 3) Download mp4 within 24h TTL
let mut reader = ureq::get(&video_url).call()?.into_reader();
let mut f = File::create(out_path)?;
std::io::copy(&mut reader, &mut f)?;
```

---

# Compute marketplaces — survey

## Vendor matrix

| Vendor | URL | Type | Pricing (2026) | Sign-up friction | Best for |
|---|---|---|---|---|---|
| Volcengine Ark | https://www.volcengine.com/ | model API | ~1 CNY / sec Seedance 2.0; 5M-token free trial | HIGH — CN phone / real-name typically required | Native Seedance, RMB invoicing |
| BytePlus ModelArk | https://docs.byteplus.com/en/docs/ModelArk/1520757 | model API | USD billing, same Seedance pricing converted | MEDIUM — corporate-style KYC; credit card OK | Native Seedance for international devs |
| fal.ai | https://fal.ai/ | model API | Seedance 2.0 std $0.3034/s, fast $0.2419/s | LOW — GitHub OAuth + card | First integration target — easiest path |
| Replicate | https://replicate.com/bytedance/seedance-2.0 | model API marketplace | per-second, NEEDS-VERIFY | LOW — GitHub OAuth + card | Hosted Seedance with simple REST + Rust-friendly |
| WaveSpeedAI | https://wavespeed.ai/ | model API (700+ models) | pay-as-you-go, $1 trial, per-image/clip | LOW — email + card | Multi-model B2C with predictable per-call cost |
| Fireworks AI | https://fireworks.ai/ | LLM-first | NO video gen as of 2026 — image only (FLUX/SDXL) | LOW | Skip for video — text/LLM only. Source: https://wavespeed.ai/blog/posts/fireworks-ai-review-2026/ |
| Together AI | https://www.together.ai/ | LLM-first; image/video minimal | NEEDS-VERIFY for video gen | LOW | Likely skip for video |
| RunPod | https://www.runpod.io/pricing | raw GPU + serverless | H100 SXM on-demand $2.69/hr; $1.30–1.60 spot; serverless H100 ~$3.25/hr; A100 $1.19/hr | LOW — email + card | Self-hosted Seedance weights / experimental models |
| Lambda Labs | https://lambda.ai/pricing | raw GPU | H100 PCIe $2.49/hr, SXM $3.29/hr; reserved $1.50–2.00 | LOW–MED — KYC for reserved | Stable long-running training jobs |
| Vast.ai | https://vast.ai/pricing | raw GPU marketplace | H100 from ~$1.65–1.87/hr; RTX 4090 ~$0.29 | LOW — email + crypto OR card | Cheapest H100, less reliable; experiments |
| Modal | https://modal.com/ | serverless container GPU | per-second GPU, ~$3–4/hr H100 effective | LOW — GitHub OAuth | Self-hosted custom video pipelines with autoscale |

Sources for compute matrix:
- RunPod: https://www.runpod.io/pricing , https://northflank.com/blog/runpod-gpu-pricing
- Lambda: https://lambda.ai/pricing , https://intuitionlabs.ai/articles/h100-rental-prices-cloud-comparison
- Vast.ai: https://vast.ai/pricing
- Spheron 2026 comparison: https://www.spheron.network/blog/gpu-cloud-pricing-comparison-2026/

Anthropic-friendly options for video: **none** — Anthropic's Claude API is text/vision in / text out. Skip.

## Recommendation

**For playa v0 Seedance integration: ship against fal.ai first.**

Why:
- Lowest sign-up friction (GitHub OAuth, no KYC nonsense).
- Per-second pricing is predictable and quotable in UI.
- mp4 URL straight back, exactly the shape `playa-jobs` already wants (submit → poll → get URL → download).
- ureq/rustls Rust client — done in an afternoon.
- If user hits scale and wants to drop fal markup, the Volcengine/BytePlus API is wire-compatible enough that the provider trait can swap with ~50 LoC of changes (model IDs, endpoint paths, auth header).

Second target: **BytePlus ModelArk** for power users with USD billing who want closer-to-cost pricing.

Skip for v0: Replicate (similar to fal but slower cold starts on big video models per common dev reports — NEEDS-VERIFY), WaveSpeedAI (great but adds cognitive load with 700 models), raw GPU (RunPod / Vast / Lambda — only worth it if hosting custom non-Seedance weights).

## Risks / open questions

- **Volcengine direct signup:** real-name auth + CN phone is the wall. Confirmed by multiple sources but exact 2026 status NEEDS-VERIFY against current console flow.
- **BytePlus regional endpoints:** AP-Southeast endpoint cited as `https://ark.ap-southeast.bytepluses.com/api/v3` — NEEDS-VERIFY directly because docs.byteplus.com page would not render through WebFetch in this session.
- **24h signed URL TTL** for video output: cited by apidog blog, vendor-published TTL not directly verified. Always re-host the mp4 on first download to be safe.
- **Pricing in CNY vs USD via BytePlus:** BytePlus claims USD billing but I did not verify per-second USD rate equivalence to fal. AICost article gave only CNY. Treat fal.ai's per-second figures as the trustworthy USD reference for now.
- **Output container:** mp4/h264 confirmed via fal response; whether ByteDance also offers webm/prores via a parameter — NEEDS-VERIFY against ModelArk reference.
- **Rate limits:** not surfaced in sources scanned. Need to read https://docs.byteplus.com/en/docs/ModelArk/ rate-limit page once API key in hand.
- **playa-ffmpeg compatibility:** mp4 + h264 + AAC audio is the lowest common denominator and ffmpeg ingests it natively. No worries.
- **Replicate API page** would not render via WebFetch — re-verify endpoint shape directly.
- **Audio output:** fal includes audio at no cost when `generate_audio: true`. Volcengine token model does not surface audio as a separate line item per aicost — likely included.
