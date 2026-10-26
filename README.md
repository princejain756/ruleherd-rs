# ruleherd-rs

Audit agent instruction files for drift, contradictions, dead references, and bloat. Rust port of [ruleherd](https://github.com/princejain756/ruleherd).

Your AGENTS.md says `npm run build`. Your CLAUDE.md says `pnpm run build`. Your `.cursor/rules/api.mdc` applies to a directory you deleted three months ago. Agents read all of it, follow whichever file loads first, and quietly disagree with each other. ruleherd finds that before your agents do.

## Install

```bash
cargo install --path .
```

Or build and copy the binary:

```bash
cargo build --release
```

## Usage

```bash
ruleherd                  # audit current directory
ruleherd ./my-repo        # audit a specific repo
ruleherd --user           # audit global config (~/.claude, ~/.cursor, ...)
ruleherd --json           # machine-readable output
ruleherd --format sarif   # SARIF 2.1.0 for code scanning
ruleherd --format github  # GitHub Actions annotations
ruleherd --strict         # exit 1 on any finding (CI gate)
```

## What it finds

| Finding | Example |
|---|---|
| CONFLICT | CLAUDE.md says tabs, AGENTS.md says spaces |
| DEAD REF | rules reference `docs/guide.md`, file was deleted |
| DEAD CMD | rules say `npm run e2e`, no such script exists |
| STALE | rules say "react 17", package.json has react 19 |
| DEAD GLOB | cursor rule's `globs: api/**` points at a deleted dir |
| DUPLICATE | same 200-char block pasted into 3 instruction files |
| MISSING | repo has CLAUDE.md but no AGENTS.md |
| BLOAT | a 4,000-token rules file loads into every prompt |

Checks understand the masks: code fences and YAML frontmatter are excluded (they hold examples and metadata, not rules). Negated clauses don't count as stances - "don't use npm" is not a vote for npm, and "use pnpm instead of npm" keeps only the positive half.

## Scoring

Starts at 100, drops per finding kind: conflicts cost the most (-20 each, capped at -50), then dead commands, dead refs, duplicates, dead globs, stale versions, bloat, missing files. Grade: A >= 90, B >= 80, C >= 70, D >= 60, F below.

## CI

```yaml
- run: ruleherd --strict --format github
```

The `github` format emits `::error`/`::warning`/`::notice` annotations that land on the PR's Files tab. `sarif` output uploads straight to GitHub code scanning.

## Discovered files

Repo scan: `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `QWEN.md`, `.cursorrules`, `.windsurfrules`, `.clinerules`, `.aider.conf.yml`, `.github/copilot-instructions.md`, `CONVENTIONS.md`, plus everything under `.cursor/rules`, `.windsurf/rules`, `.clinerules`, `.agents/rules`, and `.github/instructions`. Nested `CLAUDE.md`/`AGENTS.md`/`GEMINI.md` files up to depth 3.

`--user` scan: `~/.claude/CLAUDE.md`, `~/AGENTS.md`, `~/CLAUDE.md`, `~/GEMINI.md`, `~/.gemini/GEMINI.md`, `~/.config/devin/AGENTS.md`, windsurf global rules, and rule dirs under `~/.agents`, `~/.cursor`, `~/.claude`, `~/.config/agents`.

Symlinked files count once. Binary files and files over 5MB are skipped.

## Exit codes

- `0` - clean, or findings without `--strict`
- `1` - findings present and `--strict` passed
- `2` - bad arguments

## License

MIT
