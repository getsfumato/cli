# Connector Native Capabilities

Generation protocols and connector-native management APIs are separate.
`ProviderFactory` uses a shared OpenAI-compatible transport for generic HTTP,
OpenRouter, and Ollama model calls. `ConnectorIntrospection` exposes optional
catalog and status operations without forcing unsupported methods onto every
connector.

| Kind | Shared generation | Native model catalog | Native status |
| --- | --- | --- | --- |
| `openai_compatible` | Chat and image endpoints | None unless a legacy OpenRouter/Ollama URL is recognized | None |
| `openrouter` | OpenAI-compatible | `GET /api/v1/models`, including modalities, context, pricing, and parameters | `GET /api/v1/key`, including usage and key limits |
| `ollama` | OpenAI-compatible `/v1` | `GET /api/tags`, including size, digest, family, parameter size, and quantization | `GET /api/version` and `GET /api/ps` |
| `codex_app_server` | Native JSON-RPC threads | `model/list` | `account/read` and `account/rateLimits/read` |

```bash
sfumato connector capabilities openrouter
sfumato connector models openrouter
sfumato connector status openrouter
```

The Ratatui Connectors section exposes the same operations as asynchronous
actions. Results replace the browse rows temporarily; press `r` to return to the
configured connector list. Frontends should query capabilities before offering
an operation. Status DTOs contain only presentation-safe fields and never raw
credentials.

## Provider References

- [OpenRouter models API](https://openrouter.ai/docs/api/api-reference/models/get-models)
- [OpenRouter key usage and limits](https://openrouter.ai/docs/api/reference/limits)
- [Ollama model list](https://docs.ollama.com/api/tags)
- [Ollama API reference](https://docs.ollama.com/api)
- [Codex App Server](https://developers.openai.com/codex/app-server/)
