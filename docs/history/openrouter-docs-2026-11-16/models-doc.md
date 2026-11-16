> ## Documentation Index
> Fetch the complete documentation index at: https://openrouter.ai/docs/llms.txt
> Use this file to discover all available pages before exploring further.

# Models API

> Complete API reference for OpenRouter's Models API. Discover models, filter by input and output modalities, query supported parameters, sort by price and latency, and resolve aliases and variants.

The OpenRouter Models API provides programmatic access to metadata, capabilities, context limits, pricing, and provider endpoints for every model available on OpenRouter. The catalog updates continuously as providers deploy new models, adjust pricing, and add hardware endpoints.

## Catalog endpoints

OpenRouter provides several endpoints for discovering and inspecting models:

| Method | Endpoint | Auth | Purpose |
| - | - | - | - |
| `GET` | `/api/v1/models` | Optional | Public catalog with server-side filtering, sorting, and pagination. |
| `GET` | `/api/v1/models/user` | Required | Catalog filtered to models accessible by your API key, reflecting your workspace guardrails, provider preferences, and privacy settings. |
| `GET` | `/api/v1/model/{author}/{slug}` | Optional | Detailed record for a single model, resolving aliases and variants automatically. |
| `GET` | `/api/v1/models/{author}/{slug}/endpoints` | Optional | Per-provider endpoints for a model with quantization, pricing, context limits, and availability. |
| `GET` | `/api/v1/models/count` | Optional | Total count of models matching an `output_modalities` filter. |
| `GET` | `/api/v1/endpoints/zdr` | Optional | Preview of provider endpoints that support Zero Data Retention (ZDR). |
| `GET` | `/api/v1/images/models` | Optional | Dedicated image generation catalog with resolution and aspect ratio constraints. |
| `GET` | `/api/v1/videos/models` | Optional | Dedicated video generation catalog with duration, resolution, and SKU pricing. |
| `GET` | `/api/v1/embeddings/models` | Optional | Dedicated text and multimodal embedding catalog with vector dimensions. |

## Query parameters (`GET /api/v1/models`)

The primary catalog endpoint accepts the following query parameters:

| Parameter | Type | Default | Description |
| - | - | - | - |
| `output_modalities` | string | `text` | Comma-separated output modalities to return, or `all`. Options: `text`, `image`, `video`, `speech`, `transcription`, `audio`, `embeddings`, `rerank`, `decisions`, `all`. |
| `input_modalities` | string | (all) | Comma-separated input modalities required: `text`, `image`, `audio`, `file`. |
| `endpoints` | string | (all) | Comma-separated API paths the model must serve. Examples: `/api/v1/chat/completions`, `/api/v1/audio/speech`, `/api/alpha/extractions`. |
| `supported_parameters` | string | (all) | Comma-separated API parameters required (e.g. `tools`, `structured_outputs`, `reasoning`). |
| `sort` | string | (default) | Server-side ordering: `pricing-low-to-high`, `pricing-high-to-low`, `latency-low-to-high`, `throughput-high-to-low`, `context-high-to-low`, `newest`, `most-popular`. |
| `min_price` | number | (none) | Minimum prompt price in $/M tokens. Useful for excluding `$0`free models or`-1\` dynamic routers. |
| `max_price` | number | (none) | Maximum prompt price in \$/M tokens. |
| `context` | integer | (none) | Minimum context window size in tokens. |
| `zdr` | string (`true`) | (none) | When set to `true`, returns only models with at least one Zero Data Retention (ZDR) endpoint. This filters the catalog only; it doesn't restrict routing, so other endpoints of the same model may still retain prompts. To enforce ZDR on a request, set `provider.zdr: true`; see [Zero Data Retention](https://openrouter.ai/docs/guides/features/zdr.md). |
| `region` | string | (none) | Returns only models with at least one endpoint in the given data region (`eu` or `us`). This filters the catalog only; it doesn't restrict routing, so a matched model can still be served by an endpoint outside the region. To keep inference in a region, send requests to the regional base URL (`https://eu.openrouter.ai` or `https://us.openrouter.ai`) and enforce it with a guardrail's `allowed_data_regions`; see [In-Region Routing](https://openrouter.ai/docs/guides/features/in-region-routing.md). Requests served by your own BYOK key go to wherever that deployment runs, so provision BYOK deployments in the same region; see [BYOK with In-Region Routing](https://openrouter.ai/docs/guides/features/in-region-routing.md#byok-with-in-region-routing). |
| `q` | string | (none) | Case-insensitive search string matching model name, author, or description. |
| `offset` | integer | `0` | Number of models to skip for pagination. |
| `limit` | integer | (none) | Maximum number of models to return (up to `1000`). |

## Modality filtering guide

By default, `GET /api/v1/models` returns **text-output models only**. If you omit `output_modalities`, image generators, embedding models, dedicated speech models, and decision models are excluded.

Pass `output_modalities=all` to receive the complete multi-modal catalog, or target specific capabilities:

```bash theme={null}
# Text generation models only (default)
curl -fsS "https://openrouter.ai/api/v1/models"

# Every model across all modalities (image, video, text, speech, etc.)
curl -fsS "https://openrouter.ai/api/v1/models?output_modalities=all"

# Image generation models
curl -fsS "https://openrouter.ai/api/v1/models?output_modalities=image"

# Dedicated text-to-speech models
curl -fsS "https://openrouter.ai/api/v1/models?output_modalities=speech"

# Dedicated speech-to-text models
curl -fsS "https://openrouter.ai/api/v1/models?output_modalities=transcription"
```

### Important distinction: `audio` vs `speech` vs `transcription`

OpenRouter categorizes audio capabilities into three distinct output modalities:

1. **`speech`**: Dedicated Text-to-Speech (TTS) models that run on the `POST /api/v1/audio/speech` endpoint (e.g. `assemblyai/assemblyai-tts-1`, `heygen/heygen-voice-1`). They return raw binary audio files (MP3, WAV, PCM).
2. **`transcription`**: Dedicated Speech-to-Text (STT) models that run on the `POST /api/v1/audio/transcriptions` endpoint (e.g. `x-ai/grok-stt-2.0`, `nvidia/nemotron-3.5-transcribe`, `openai/whisper-large-v3`). They accept audio file uploads and return transcribed text.
3. **`audio`**: General conversational multimodal models that process or emit audio chunks via `POST /api/v1/chat/completions` or `POST /api/v1/responses` (e.g. `openai/gpt-audio`, `openai/gpt-audio-mini`, `google/lyria-3-clip-preview`).

## Sentinel prices and safe cheapest-model sorting

When building model selectors or cost optimizers, be aware of two sentinel price values in the `pricing` object:

1. **`-1` (Dynamic Router models)**: Routers such as `openrouter/auto`, `openrouter/auto-code`, `openrouter/fusion`, and `typesafe/jev-router` set `pricing.prompt: "-1"` and `pricing.completion: "-1"`. Because these models dynamically delegate to underlying models at runtime, their final cost depends on the selected downstream provider.
2. **`0` (Free models and variants)**: Models ending in `:free` (e.g. `liquid/lfm-2.5-2.6b:free`) and experimental preview models set prompt and completion pricing to `0`.

### The naive sort trap

If client code performs a naive numeric sort (`models.sort((a, b) => a.pricing.prompt - b.pricing.prompt)`), dynamic router models with `-1` will sort to the very top, followed by free models with `0`. If your application expects a standard paid LLM with direct provider endpoints, sending requests to `-1` routers or rate-limited `:free` variants will produce unexpected routing behavior.

### Safe server-side query

To query the cheapest paid production chat models server-side in one call, combine three filters:

* `sort=pricing-low-to-high` (orders from lowest price to highest)
* `min_price=0.000001` (excludes `-1` dynamic routers and `$0` free variants)
* `endpoints=/api/v1/chat/completions` (excludes models that only serve batches, embeddings, or media)

```bash theme={null}
curl -fsS "https://openrouter.ai/api/v1/models?sort=pricing-low-to-high&min_price=0.000001&endpoints=/api/v1/chat/completions"
```

### TypeScript client-side filtering example

```typescript theme={null}
interface OpenRouterModel {
  id: string;
  name: string;
  pricing: {
    prompt: string;
    completion: string;
  };
  endpoints?: string[];
}

async function getCheapestStableChatModel(): Promise<OpenRouterModel> {
  const res = await fetch(
    'https://openrouter.ai/api/v1/models?sort=pricing-low-to-high&min_price=0.000001&endpoints=/api/v1/chat/completions',
  );
  if (!res.ok) {
    throw new Error(`Failed to fetch models: ${res.status}`);
  }

  const { data } = (await res.json()) as { data: OpenRouterModel[] };

  // Filter out rolling aliases (~ prefix) and non-standard variants if a pinned model is required
  const stableModels = data.filter((m) => {
    const isPaid = Number(m.pricing.prompt) > 0 && Number(m.pricing.completion) >= 0;
    const isNotFreeVariant = !m.id.endsWith(':free');
    const isNotBatchVariant = !m.id.endsWith(':batch');
    const isNotAlias = !m.id.startsWith('~');
    return isPaid && isNotFreeVariant && isNotBatchVariant && isNotAlias;
  });

  if (stableModels.length === 0) {
    throw new Error('No models matched the selection criteria');
  }

  return stableModels[0];
}
```

## Model identifiers, aliases, and variants

Model identifiers follow structured naming conventions (see [Model Variants](https://openrouter.ai/docs/guides/routing/model-variants/overview.md) for details):

### 1. Canonical slugs vs. display IDs

Every model record contains both an `id` and a `canonical_slug`:

* `id`: The routable slug used in API requests (e.g. `anthropic/claude-3.5-sonnet`).
* `canonical_slug`: The permanent canonical reference for the underlying model checkpoint (e.g. `anthropic/claude-3-5-sonnet-20241022`).

OpenRouter automatically resolves known aliases. If you request `anthropic/claude-3-5-sonnet`, OpenRouter routes it to `anthropic/claude-3.5-sonnet`.

### 2. Rolling aliases (`~` prefix)

Aliases prefixed with `~` (e.g. `~deepseek/deepseek-pro-latest`, `~typesafe/jev-latest`) point dynamically to the latest approved stable version of a model family. Use rolling aliases when you want automatic updates, or use pinned version slugs when reproducibility is critical. See [Latest Resolution](https://openrouter.ai/docs/guides/features/latest-resolution.md) for details.

### 3. Catalog variants vs. routing suffixes

OpenRouter distinguishes between catalog variants and request-time routing suffixes:

* **Catalog variants** appear as separate entries in `GET /api/v1/models`. They include:
  * `:free`: Free rate-limited endpoints provided for testing and evaluation.
  * `:batch`: Discounted asynchronous batch processing endpoints.
  * `:thinking` (deprecated): Reasoning enabled by default on models that previously shipped separate reasoning endpoints (use the `reasoning` parameter instead).
  * `:extended` (deprecated): Larger context window on models that previously offered one.
* **Routing suffixes** do not appear as separate catalog entries. They are modifier flags appended to model IDs at request time on `POST /api/v1/chat/completions`:
  * `:nitro`: Prioritizes the lowest latency providers.
  * `:floor`: Selects the cheapest available provider for the model.
  * `:exacto`: Sorts providers by quality signals tuned for tool-calling reliability (mapped to `ProviderSort.Exacto`). See [Exacto](https://openrouter.ai/docs/guides/routing/model-variants/exacto.md).
  * `:online` (deprecated): Attaches web search results (use the `openrouter:web_search` server tool instead).

## Inspecting provider endpoints

A single model (such as `meta-llama/llama-3.3-70b-instruct`) is frequently hosted across dozens of backend providers (Together, Groq, Fireworks, DeepInfra, Lepton, etc.). To inspect the live providers, pricing, quantizations, and status for a model:

```bash theme={null}
curl -fsS "https://openrouter.ai/api/v1/models/meta-llama/llama-3.3-70b-instruct/endpoints"
```

The response nests the model record and endpoint records under the top-level `data` envelope:

```json theme={null}
{
  "data": {
    "id": "meta-llama/llama-3.3-70b-instruct",
    "name": "Meta: Llama 3.3 70B Instruct",
    "endpoints": [
      {
        "provider_name": "DeepInfra",
        "tag": "deepinfra/turbo",
        "context_length": 131072,
        "pricing": {
          "prompt": "0.0000001",
          "completion": "0.00000032"
        },
        "quantization": "fp8",
        "status": 0,
        "uptime_last_30m": 100,
        "supported_parameters": [
          "tools",
          "tool_choice",
          "structured_outputs",
          "response_format"
        ]
      }
    ]
  }
}
```

Use endpoint metadata when you want to pin requests to specific providers or enforce quantization standards using the `provider` request parameter in Chat Completions.
