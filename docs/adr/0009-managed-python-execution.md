# ADR-0009: Run Generated Python in Managed Disposable Environments

**Decision status:** Accepted and implemented for v0.2.

## Context

Two workflows need to execute Python a model wrote: the Manim video engine, and
the `chart-gen` tool, which plots data with matplotlib so a quantitative figure
is computed rather than imagined. Manim's environment was previously created
inline by the video renderer adapter — a `uv venv` and a pinned install
hardcoded beside the Hyperframes npm install — which gave one workflow an
interpreter and left the next one to invent its own.

Three properties matter and none of them are specific to a workflow. The
interpreter and its dependencies must be pinned, or two runs of the same
generated chart produce different pictures. The code must be screened and
compiled before it executes, so a model that writes something dangerous or
malformed is refused with a clear reason rather than at runtime. And the code
must not survive the call: a revision should carry the chart, not the script
that drew it.

## Decision

Introduce a `PythonRuntime` port in the core with a `uv`-backed adapter.

1. Environments are declared in a bundled manifest, fully pinned, and
   provisioned on demand under `~/.sfumato/python/<layer>`.
2. A layer is keyed by the exact requirement set it was built from and stamped
   with it, so asking for the same environment twice is free, a manifest pin
   bump rebuilds rather than silently reusing, and a half-built environment is
   rebuilt rather than trusted.
3. Extra requirements requested by a workflow are installed into a *derived*
   layer named for the hash of those extras, leaving the pinned base intact.
   Extras are sorted and deduplicated so two callers naming the same packages
   in different orders share one layer.
4. Every requirement is validated as a plain, optionally pinned package name.
   Anything that could read as an installer flag, a URL, or a local path is
   refused rather than escaped.
5. `run` writes the generated files into a temporary directory, compiles them,
   executes there, copies out only the outputs the caller declared, and drops
   the directory.

The Manim renderer delegates its interpreter to this runtime, so
`sfumato renderer install manim` and the chart tool provision the same way.

Authorization is a single project setting, `security.allow_python`, covering
every workflow that executes generated code. It replaces `security.allow_manim`,
which is still read as the same consent so a project that already agreed is not
asked again.

## Consequences

- A new Python-backed capability declares an environment and gets pinning,
  screening, compilation, disposal, and the authorization gate for free.
- Generated Python never reaches the artifact store; a committed revision holds
  the picture, not the script.
- Layering an extra package installs from an index during generation, so it is
  gated on an explicit project allowlist (`security.python_packages`) rather
  than left to the model.
- The first use of an environment pays a provisioning cost; subsequent runs do
  not. Derived layers duplicate their base's packages on disk, which is the
  price of keeping the base exactly as declared.
- This is an authorization boundary, not a strong sandbox. The screen rejects
  the obvious escapes and the run directory limits the blast radius, but a
  project that opens the gate is trusting the model with an interpreter.
