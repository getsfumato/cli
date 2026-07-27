# Hyperframe troubleshooting

Use this guide for the managed, silent Hyperframe pipeline. It intentionally
does not cover TTS, music, sound effects, or audio-synchronised captions.

## Contract and timeline failures

Run `sfumato renderer doctor hyperframe`, then inspect the saved `source/` bundle.
`index.html` must load only `./vendor/gsap.min.js`, create paused timelines, register
them in `window.__timelines`, and finish at the requested duration. New productions
also require local `compositions/*.html` scene modules assembled from `index.html`.

## Assets and catalog failures

Generated source may reference only local project assets and the catalog blocks
installed with the renderer. Run `sfumato renderer install hyperframe` again if
`doctor` reports a catalog mismatch. A generation never downloads catalog entries;
do not work around that restriction with remote URLs or `fetch`.

## Empty snapshots or preview differences

Inspect `snapshots/` and `contact-sheet.md` in the managed artifact or paused review
session. Empty snapshots usually mean the timeline was not registered, did not cover
the sampled timestamp, or the root has no visible clips. Preview and render must use
the same saved `source/` directory; regenerate rather than copying files into it.

## Version or approval failures

`sfumato video approve <review-id>` verifies the stored plan and source hashes before
it renders. It rejects an edited, incomplete, or already-approved session. Recreate
the review after changing Sfumato or reinstalling an incompatible renderer runtime.
The session also remembers the effective publication destination from generation;
approval publishes there after rendering. Use `sfumato video approve <review-id>
--out <folder>` to override it. If neither generation nor approval has an output
destination, the MP4 remains available only in Sfumato's managed revision history.

## Safe reproduction package

Attach the managed review-session directory, including `review.json`, `plan.json`,
`DESIGN.md`, `STORYBOARD.md`, `SCRIPT.md`, `source/`, `snapshots/`, and renderer
diagnostics. Remove credentials and unrelated project files first. This package is
enough to reproduce validation and preview without re-running planning or authoring.
