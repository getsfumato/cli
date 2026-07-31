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

Speech and video stages must use capability-specific connectors.

## Image Input

`turn/start` accepts images alongside the prompt, and the protocol takes a local
path and opens the file itself, so a snapshot travels without any encoding — unlike
the HTTP connectors, which inline base64. Each image is preceded by a text label,
because the protocol carries no caption field and a model cannot report which of
several frames is wrong unless it can name them.

Whether images are accepted is read from the model, not assumed from the connector:
`model/list` reports each model's input modalities, and a model that declares text
only is refused with the reason. The protocol specifies that a missing list means
both modalities are accepted, so an older catalog is not treated as a refusal.

## Image Generation

The connector can serve an `image` capability, with one caveat worth understanding
before configuring it.

The protocol exposes no "generate this image" call. Generation is a Codex-native
tool that the *model* chooses to invoke during a turn: Sfumato starts a thread,
asks for exactly one image, and watches for a completed `imageGeneration` item
carrying the artifact's saved path. The generated PNG is then read from that path.

So this is an agent wrapped in a provider contract, and it can fail in a way a
direct endpoint cannot: the model may answer with a description instead of calling
its tool. That outcome is reported as a failed tool — with the model's own words
quoted and a worked profile for a connector that does return bytes — rather than as
an empty success.

What it buys is billing: generation runs on the Codex usage included with the
ChatGPT plan rather than on a metered image endpoint. For a workload where a failed
attempt is cheap and credits are the constraint, that trade is worth making. For one
where a missing image breaks a pipeline, prefer a direct endpoint.

```bash
sfumato model add codex-image --connector codex --id default --capability image
```
