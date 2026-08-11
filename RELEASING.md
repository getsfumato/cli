# Releasing

The installer at [sfumato.sh/install.sh](https://sfumato.sh/install.sh) lives in
the `getsfumato/site` repository and is the contract this process satisfies. It
resolves the version from `releases/latest`, downloads
`sfumato-v<version>-<target>.tar.gz`, verifies the `.sha256` beside it, and runs
`sfumato --version` when it is done. Nothing here is free to change unilaterally.

## Cutting a release

**Normally you do not.** `auto-release.yml` runs on every push to master, reads the
Conventional Commits since the last tag, and cuts the release if they call for one:

| Commits since the last tag | Result |
| --- | --- |
| a `feat:`, or anything breaking | minor — `0.3.0` → `0.4.0` |
| a `fix:` or `perf:` | patch — `0.3.0` → `0.3.1` |
| only `docs`/`chore`/`ci`/`refactor`/`test`/`style` | nothing |

While the major is `0`, a break takes the minor: there is no major to spend, and
semver says anything may change in `0.x`. That collapses breaks and features onto
one bump, which is a real loss of signal — the changelog still separates them, and
the mapping changes on its own once the version reaches `1.0`.

When there is a release to cut, the workflow runs the full gate first — fmt,
clippy, the suite — then bumps all four version strings, updates the lock, writes
the changelog section, commits as `chore(release): <version>`, tags, and builds the
artifacts. `ci.yml` runs on the same push but nothing joins the two, so the gate is
repeated here deliberately: without it a release can be cut from a commit whose CI
is red, which is how 0.3.1 shipped (from a flake, not a regression).

A commit whose type it does not recognise is reported as a warning rather than
ignored: `feet:` parses cleanly and releases nothing.

To release commits that would not trigger one, run `auto-release` from the Actions
tab with **force** — it takes a patch.

### By hand

Still supported, and the path to use for a prerelease, which the automatic one
never produces.

1. **Bump the version.** `python3 .github/scripts/bump_version.py 0.4.0` sets all
   four places: `version` under `[workspace.package]` and the three
   `[workspace.dependencies]` entries. They must agree — `release.yml` refuses the
   tag otherwise, because `cargo publish --dry-run` cannot see a stale one.
2. **Update `Cargo.lock`.** `cargo check --workspace`, then commit it. Skipping
   this is the most common way to fail at tag time, because every build passes
   `--locked`.
3. **Update `CHANGELOG.md`.**
4. Commit, push, and let CI go green.
5. **Tag and push it.**

   ```bash
   git tag v0.4.0 && git push origin v0.4.0
   ```

6. Watch the run. It builds six targets into a **draft** release and only
   publishes it once all twelve assets are present.
7. **Check the draft before it goes live** — download one artifact and run it:

   ```bash
   gh release download v0.3.0 -p '*aarch64-apple-darwin*'
   shasum -a 256 -c *.sha256
   tar xzf *.tar.gz && ./sfumato-v0.3.0-aarch64-apple-darwin/sfumato --version
   ```

8. `finalize` flips the draft. Confirm the installer sees it:

   ```bash
   curl -s https://api.github.com/repos/getsfumato/sfumato/releases/latest | jq -r .tag_name
   ```

9. **Install it the way a user would**, on clean machines:

   ```bash
   docker run --rm debian:12 sh -c 'apt-get update && apt-get install -y curl ca-certificates >/dev/null && curl -fsSL https://sfumato.sh/install.sh | sh && ~/.local/bin/sfumato --version'
   docker run --rm alpine  sh -c 'apk add --no-cache curl >/dev/null && curl -fsSL https://sfumato.sh/install.sh | sh && ~/.local/bin/sfumato --version'
   ```

   To pin a version, the assignment goes on the `sh`, not the `curl` — otherwise it
   is set for the download and not for the script, which silently resolves
   `releases/latest` instead:

   ```bash
   curl -fsSL https://sfumato.sh/install.sh | SFUMATO_VERSION=0.3.0 sh
   ```

   Debian 12 exercises the glibc floor; Alpine is the only thing that exercises
   the musl artifacts.

## Rehearsing

A tag with a suffix publishes as a prerelease, which GitHub excludes from
`releases/latest`. The installer therefore cannot reach it unless asked by name,
which makes it a rehearsal with no blast radius:

```bash
# bump the manifest to 0.3.1-rc.1 first, so the tag, the assets and the binary
# all report the same version
git tag v0.3.1-rc.1 && git push origin v0.3.1-rc.1
curl -fsSL https://sfumato.sh/install.sh | SFUMATO_VERSION=0.3.1-rc.1 sh
```

Delete and retag an rc freely: `gh release delete <tag> --yes --cleanup-tag`.

## Publishing to crates.io

**After** the GitHub release is validated, never before. GitHub releases can be
deleted; crates.io versions can be yanked but never deleted and never reused. If
crates.io goes first and the macOS binary turns out to be broken, that version
number is spent.

Order follows the dependency graph, and the loop is idempotent so a rerun after a
partial failure makes progress instead of erroring. crates.io rate-limits *new*
crate creation to roughly one per ten minutes after a short burst, which matters
the first time four crates are created at once.

```bash
VERSION=0.3.0
published() {
  curl -fsS "https://index.crates.io/${1:0:2}/${1:2:2}/$1" 2>/dev/null \
    | grep -q "\"vers\":\"$2\""
}
for crate in sfumato-domain sfumato-core sfumato-adapters sfumato; do
  published "$crate" "$VERSION" && { echo "skip $crate"; continue; }
  cargo publish -p "$crate" --locked
done
```

Between `sfumato-core` and `sfumato-adapters`, run
`cargo package -p sfumato-adapters --locked`. Once `core` is published this works
without any override and compiles the extracted copy, which is the real proof that
the compiled-in assets survived packaging. Before `core` exists on crates.io it
cannot run at all, which is why CI checks the packaged *file list* instead.

Then prove an outside consumer can use it:

```bash
cd "$(mktemp -d)" && cargo new probe && cd probe
cargo add sfumato-core@0.3.0 && cargo build
cargo install sfumato --locked --version 0.3.0
```

Only once that last command works should `cargo_fallback()` in the site's
`install.sh` move from `cargo install --git` to `cargo install sfumato --locked`.

## When something goes wrong

| Situation | What to do | Safe? |
| --- | --- | --- |
| `--locked` fails in `verify` | The lock was not bumped. Fix, `gh release delete <tag> --yes --cleanup-tag`, retag. | Yes, the release was a draft |
| One matrix leg fails | `finalize` blocks on the asset count. Fix and re-run the failed job, or delete and retag. | Yes, still a draft |
| A bad tag, before the draft is published | `gh release delete <tag> --yes --cleanup-tag`, then retag. | Yes |
| A bad release, after it went live | `gh release edit <tag> --prerelease`. This is the kill switch: it drops out of `releases/latest` immediately without deleting anything or moving the tag. Then ship `0.3.1`. | Yes |
| Wanting to delete a published release | Don't. With one release, `releases/latest` 404s, the installer's version lookup returns empty, and every install silently falls through to a 5–15 minute build from source. | **No** |
| A bad version on crates.io | `cargo yank`. It does not delete, does not break existing lockfiles, and the number can never be reused. Roll forward. | Forward only |

**Never move a tag crates.io has already seen.** Before that step tags are cheap;
after it they are load-bearing history.

## How the workflows fit together

```text
push to master  ──►  auto-release.yml   plan → bump, tag ─┐
push a v* tag   ──►  release.yml        verify ───────────┤
                                                          ▼
                                     release-artifacts.yml  (workflow_call)
                                        draft → build ×6 → finalize
```

`release-artifacts.yml` is reusable rather than tag-triggered for a reason worth
knowing before changing it: **a tag pushed with `GITHUB_TOKEN` does not start a
workflow.** GitHub refuses to let a run beget a run, so the automatic path cannot
rely on its own tag being dispatched — it invokes the artifacts workflow directly.
Making `release-artifacts.yml` trigger on tags instead would silently break the
automatic release while leaving the manual one working.

The same rule is what stops the release commit from looping: pushed with
`GITHUB_TOKEN`, it starts nothing. `auto-release.yml` also skips any head commit
starting with `chore(release):`, because depending on that rule alone would make an
infinite release loop the failure mode if it ever changed.

## Adding a target

`install.sh`'s `detect_target` decides which triples can ever be downloaded, so a
new target needs a matching case there, a matrix entry in `release.yml`, and the
expected asset count in `finalize` raised by two.
