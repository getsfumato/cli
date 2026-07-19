# Codex App Server Connector

The local Codex connector consumes the Codex usage included with a compatible
ChatGPT plan. It uses the official App Server JSON-RPC protocol over stdio and
is not an OpenAI-compatible HTTP connector.

## Setup

Authenticate with Codex and configure the App Server connector:

```bash
codex login
codex login status
sfumato connector setup codex
sfumato connector models codex
sfumato model add codex \
  --connector codex \
  --id default \
  --capability text \
  --capability code
```

Assign the profile globally or to one project:

```bash
sfumato model use text codex
sfumato model use reviewer codex --project university
```

`sfumato connector login codex` remains intentionally unsupported. Sfumato
never reads, copies, refreshes, or stores the OAuth credentials owned by Codex.

## Protocol Lifecycle

`CodexAppServerProvider` lazily starts one persistent `codex app-server` process
for a resolved provider instance and communicates through newline-delimited
JSON-RPC:

1. `initialize` advertises Sfumato and opts into the experimental API required
   by dynamic tools, followed by `initialized`.
2. `model/list` discovers authenticated models and their default selection.
3. `thread/start` creates an ephemeral, read-only thread using the selected
   project root, rendered system prompt, and Sfumato dynamic tool definitions.
4. `turn/start` sends the rendered user prompt.
5. `item/tool/call` requests are executed by the request's core `ToolExecutor`.
   Sfumato returns `inputText` content and emits readable requested, succeeded,
   or failed tool events to CLI and TUI frontends.
6. `item/completed` supplies the authoritative agent message and
   `turn/completed` closes the operation.

The adapter validates explicit model IDs against `model/list`. The profile ID
`default` resolves the catalog entry marked `isDefault`; it is not an opaque
fallback delegated to a separate CLI invocation.

App Server owns the native model/tool loop for Codex. `AgentRunner` remains the
core loop for OpenAI-compatible providers that expose isolated completion turns.
Sfumato still enforces its request-specific tool-call budget and returns the
rendered output-contract instruction after exhaustion.

The connector supports text generation only. Image, speech, and video stages
must use capability-specific connectors.
