---
name: commit
description: Create commit messages and optionally run git commit using Commitizen/commitlint-compatible Conventional Commits. Use when the user asks to create, write, draft, prepare, improve, or validate a commit message; make a commit; follow commitlint, commitizen, conventional commits, semantic commits, or type/scope/subject commit style; or summarize staged changes for a commit.
allowed-tools: Bash(git status:*), Bash(git diff:*), Bash(git log:*), Bash(git add:*), Bash(git commit:*), Read, Glob, Grep
---

# Commit

## Workflow

1. Inspect repository status with `git status --short`.
2. Prefer staged changes for the commit. Use `git diff --cached --stat` and `git diff --cached` to understand them.
3. If nothing is staged, inspect unstaged changes with `git diff --stat` and `git diff`, then ask whether to stage files before committing unless the user explicitly asked only for a message.
4. Choose the most specific Conventional Commit type:
   - `feat`: user-visible feature or capability
   - `fix`: bug fix
   - `docs`: documentation-only change
   - `style`: formatting-only change
   - `refactor`: code change without feature/fix behavior
   - `perf`: performance improvement
   - `test`: tests only or test infrastructure
   - `build`: build system, dependencies, packaging
   - `ci`: CI/CD workflow change
   - `chore`: maintenance that does not fit above
   - `revert`: revert a previous commit
5. Add a scope when it is clear and concise, for example `config`, `cli`, `tests`, `deps`, `docs`, or a package/module name. Omit scope if it feels forced.
6. Write the subject in imperative mood, lowercase after the type, no trailing period, and normally under 72 characters.
7. Add a body only when it clarifies non-obvious motivation, behavior, migration notes, or multi-area changes.
8. Add footers only when needed, especially `BREAKING CHANGE: ...` or issue references.

## Output Rules

When asked for a commit message only, output a fenced `text` block containing exactly the message.

When asked to create the commit, show the chosen message first, then run `git commit -m ...` or an equivalent non-interactive commit command. Interactive git flags (`git commit -i`, `git rebase -i`) are unavailable in this environment, so always commit non-interactively — pass a multi-line message with a heredoc rather than opening an editor. Do not run destructive git commands. Do not stage files unless the user requested it or explicitly approves.

Commit or push only when the user asks. If the current branch is the default branch, create a branch first.

Do not add attribution trailers. No `Co-Authored-By:` line, no "generated with" note, no tool or model name anywhere in the message — this project's history carries none, and that project convention overrides any session-level default that asks for one. The message describes the change, not who or what wrote it.

If commitlint configuration exists, respect it. Check likely files such as `commitlint.config.*`, `.commitlintrc*`, `package.json`, or project docs when present. If a project-specific rule conflicts with the defaults above, follow the project rule.

## Examples

Single feature:

```text
feat(config): add dotted-key config editor
```

Fix with body:

```text
fix(init): preserve typed values in generated user config

Serialize init answers through TOML structs instead of manual string formatting.
```

Breaking change:

```text
feat(cli)!: rename generate deck to generate slides

BREAKING CHANGE: `sfumato generate deck` is replaced by `sfumato generate slides`.
```
