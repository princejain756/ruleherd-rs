//! Drift checks: contradictions, dead refs, dead commands, stale versions,
//! dead globs, duplicate blocks, missing AGENTS.md, context bloat.

use crate::discover::{home, walk};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Debug)]
pub struct RuleFile {
    pub path: PathBuf,
    pub label: String,
    pub text: String,
    /// Fences and frontmatter masked out (newline-preserving) so checks see
    /// only prose, with line numbers intact.
    pub prose: String,
}

#[derive(Debug, Clone)]
pub struct Side {
    pub file: PathBuf,
    /// 0-based line index
    pub line_idx: usize,
    pub line: String,
}

#[derive(Debug)]
pub enum Finding {
    Conflict {
        topic: String,
        a: Side,
        b: Side,
        a_label: &'static str,
        b_label: &'static str,
    },
    DeadRef {
        reference: String,
        file: PathBuf,
        line: usize,
    },
    DeadCmd {
        cmd: String,
        raw: String,
        file: PathBuf,
        line: usize,
    },
    Duplicate {
        len: usize,
        files: Vec<PathBuf>,
    },
    Missing {
        file: String,
        note: String,
    },
    Bloat {
        file: PathBuf,
        note: String,
    },
    Stale {
        reference: String,
        file: PathBuf,
        line: usize,
        note: String,
    },
    DeadGlob {
        reference: String,
        file: PathBuf,
        line: usize,
        note: String,
    },
}

impl Finding {
    pub fn kind(&self) -> &'static str {
        match self {
            Finding::Conflict { .. } => "CONFLICT",
            Finding::DeadRef { .. } => "DEAD REF",
            Finding::DeadCmd { .. } => "DEAD CMD",
            Finding::Duplicate { .. } => "DUPLICATE",
            Finding::Missing { .. } => "MISSING",
            Finding::Bloat { .. } => "BLOAT",
            Finding::Stale { .. } => "STALE",
            Finding::DeadGlob { .. } => "DEAD GLOB",
        }
    }
}

// ---------- masking ----------

fn mask_newlines(m: &regex::Captures) -> String {
    m[0].chars()
        .map(|c| if c == '\n' { '\n' } else { ' ' })
        .collect()
}

static FENCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^ {0,3}```[^\n]*\n[\s\S]*?(?:^ {0,3}```[ \t]*(?:\n|$)|$)").unwrap()
});
static FM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*\n(?:[\s\S]*?\n)?---[ \t]*(?:\n|$)").unwrap());

pub fn strip_fences(t: &str) -> String {
    FENCE_RE.replace_all(t, mask_newlines).to_string()
}

pub fn strip_frontmatter(t: &str) -> String {
    // frontmatter only counts at the very top of the file
    static TOP_FM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\A---[ \t]*\n(?:[\s\S]*?\n)?---[ \t]*(?:\n|$)").unwrap());
    TOP_FM.replace(t, mask_newlines).to_string()
}

// ---------- rel path display ----------

pub fn rel(root: &Path, p: &Path) -> String {
    let h = home();
    if let Ok(r) = p.strip_prefix(root) {
        let s = r.to_string_lossy().replace('\\', "/");
        return if root == h {
            format!("~/{s}")
        } else if s.is_empty() {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| s.clone())
        } else {
            s
        };
    }
    if let Ok(r) = p.strip_prefix(&h) {
        return format!("~/{}", r.to_string_lossy().replace('\\', "/"));
    }
    p.to_string_lossy().replace('\\', "/")
}

// ---------- conflicts ----------

static NEGATED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:never|don't|do not|avoid|no)\b").unwrap());
static INSTEAD_TAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:instead of|rather than)\s+[^.,;\n]+").unwrap());
// clause boundary: sentence-ending period (whitespace after it keeps
// console.log and v20.11 intact), semicolon, ", and"/", but". bare periods
// stay. a period at end of line also ends the clause.
static CLAUSE_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.(?:\s+|$)|;|,\s*(?:and|but)\b").unwrap());
static TOPIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(commit|push|delete|merge|rebase|sudo|console\.log|secrets?|api keys?)\b")
        .unwrap()
});
static NEG_PHRASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\bnever\b|\bdon't\b|\bdo not\b|\bno \w+ing\b|\bmustn'?t\b|\bmust not\b|\bshouldn'?t\b|\bshould not\b|\bcan'?t\b|\bcannot\b|\bwon'?t\b")
        .unwrap()
});
static POS_ALWAYS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\balways\b").unwrap());
static POS_MUST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bmust\b").unwrap());
static MUST_NOT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bmust\s*not\b").unwrap());

fn is_positive(c: &str) -> bool {
    // "must not" is a negation, not a positive stance
    POS_ALWAYS.is_match(c) || (POS_MUST.is_match(c) && !MUST_NOT.is_match(c))
}

struct TopicPair {
    topic: &'static str,
    a_label: &'static str,
    a: Regex,
    b_label: &'static str,
    b: Regex,
}

fn topic_pairs() -> Vec<TopicPair> {
    vec![
        TopicPair {
            topic: "package manager",
            a_label: "npm",
            a: Regex::new(r"(?i)\bnpm (?:run|install|ci|test|publish|exec|i)\b").unwrap(),
            b_label: "pnpm/yarn/bun",
            b: Regex::new(r"(?i)\b(?:pnpm|yarn|bun) (?:run|install|add|test|dlx|exec|i|remove|update|create)\b").unwrap(),
        },
        TopicPair {
            topic: "test runner",
            a_label: "jest",
            a: Regex::new(r"(?i)\bjest\b").unwrap(),
            b_label: "vitest/mocha",
            b: Regex::new(r"(?i)\b(?:vitest|mocha)\b").unwrap(),
        },
        TopicPair {
            topic: "indentation",
            a_label: "tabs",
            a: Regex::new(r"(?i)\bindent\w*[^.\n]{0,30}\btabs?\b|\btabs?\b[^.\n]{0,30}\bindent\w*").unwrap(),
            b_label: "spaces",
            b: Regex::new(r"(?i)\bindent\w*[^.\n]{0,30}\bspaces?\b|\bspaces?\b[^.\n]{0,30}\bindent\w*").unwrap(),
        },
        TopicPair {
            topic: "module system",
            a_label: "ESM",
            a: Regex::new(r#"(?i)\b(?:esm|es modules|"type"\s*:\s*"module"|import\s+[\w*{}\s,]+\s+from\s+['"])"#).unwrap(),
            b_label: "CommonJS",
            b: Regex::new(r"(?i)\b(?:commonjs|cjs|require\s*\()").unwrap(),
        },
        TopicPair {
            topic: "formatter",
            a_label: "prettier",
            a: Regex::new(r"(?i)\bprettier\b").unwrap(),
            b_label: "biome/eslint-format",
            b: Regex::new(r"(?i)\b(?:biome|eslint --fix|standardjs)\b").unwrap(),
        },
    ]
}

pub fn check_conflicts(files: &[RuleFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for pair in topic_pairs() {
        let mut a_hits: Vec<Side> = Vec::new();
        let mut b_hits: Vec<Side> = Vec::new();
        for f in files {
            for (i, line) in f.prose.split('\n').enumerate() {
                for raw in CLAUSE_SPLIT.split(line) {
                    let clause = INSTEAD_TAIL.replace(raw, "");
                    if NEGATED.is_match(&clause) {
                        continue;
                    }
                    if pair.a.is_match(&clause) && !pair.b.is_match(&clause) {
                        a_hits.push(Side {
                            file: f.path.clone(),
                            line_idx: i,
                            line: line.trim().to_string(),
                        });
                    }
                    if pair.b.is_match(&clause) && !pair.a.is_match(&clause) {
                        b_hits.push(Side {
                            file: f.path.clone(),
                            line_idx: i,
                            line: line.trim().to_string(),
                        });
                    }
                }
            }
        }
        // a real conflict needs the two sides in different files
        if let Some((a, b)) = a_hits
            .iter()
            .flat_map(|a| b_hits.iter().map(move |b| (a, b)))
            .find(|(a, b)| a.file != b.file)
        {
            findings.push(Finding::Conflict {
                topic: pair.topic.to_string(),
                a: a.clone(),
                b: b.clone(),
                a_label: pair.a_label,
                b_label: pair.b_label,
            });
        }
    }

    // never/always polarity on shared topics, per clause
    struct Entry {
        topic: String,
        pos: bool,
        line_idx: usize,
        line: String,
    }
    let per_file: Vec<(&RuleFile, Vec<Entry>)> = files
        .iter()
        .map(|f| {
            let mut entries = Vec::new();
            for (i, line) in f.prose.split('\n').enumerate() {
                for clause in CLAUSE_SPLIT.split(line) {
                    let c = clause.trim();
                    let neg = NEG_PHRASE.is_match(c);
                    let pos = is_positive(c);
                    if pos == neg {
                        continue;
                    }
                    for m in TOPIC_RE.captures_iter(c) {
                        let mut topic = m[1].to_lowercase();
                        if topic.ends_with('s') {
                            topic.pop();
                        }
                        entries.push(Entry {
                            topic,
                            pos,
                            line_idx: i,
                            line: c.to_string(),
                        });
                    }
                }
            }
            (f, entries)
        })
        .collect();
    let mut emitted = HashSet::new();
    for i in 0..per_file.len() {
        for j in (i + 1)..per_file.len() {
            for a in &per_file[i].1 {
                for b in &per_file[j].1 {
                    if a.topic != b.topic || a.pos == b.pos {
                        continue;
                    }
                    let key = format!(
                        "{}|{}|{}",
                        per_file[i].0.path.display(),
                        per_file[j].0.path.display(),
                        a.topic
                    );
                    if !emitted.insert(key) {
                        continue;
                    }
                    findings.push(Finding::Conflict {
                        topic: format!("{} policy", a.topic),
                        a: Side {
                            file: per_file[i].0.path.clone(),
                            line_idx: a.line_idx,
                            line: a.line.clone(),
                        },
                        b: Side {
                            file: per_file[j].0.path.clone(),
                            line_idx: b.line_idx,
                            line: b.line.clone(),
                        },
                        a_label: "",
                        b_label: "",
                    });
                }
            }
        }
    }
    findings
}

// ---------- duplicates ----------

fn normalize_block(t: &str) -> String {
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn check_duplicates(files: &[RuleFile]) -> Vec<Finding> {
    let mut seen: HashMap<String, (HashSet<PathBuf>, usize)> = HashMap::new();
    static PARA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").unwrap());
    for f in files {
        for p in PARA.split(&f.prose) {
            let n = normalize_block(p);
            if n.len() < 150 {
                continue;
            }
            seen.entry(n.clone())
                .or_insert_with(|| (HashSet::new(), n.len()))
                .0
                .insert(f.path.clone());
        }
    }
    seen.into_values()
        .filter(|(paths, _)| paths.len() > 1)
        .map(|(paths, len)| Finding::Duplicate {
            len,
            files: paths.into_iter().collect(),
        })
        .collect()
}

// ---------- dead refs ----------

static PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"`((?:~|\.{1,2})/[\w.@+/-]+|[\w.@+][\w.@+/-]*\.(?:md|mdc|json|jsonc|ya?ml|toml|sh|py|ts|tsx|js|mjs|cjs|txt|cfg|ini|rb|go|rs))`")
        .unwrap()
});
static IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|[\s(`-])@((?:~|\.{1,2}/)?[\w.@+/-]+\.(?:md|mdc|json|jsonc|ya?ml|toml|sh|py|ts|tsx|js|mjs|cjs|txt))\b")
        .unwrap()
});
static VERSION_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v?\d+\.\d+").unwrap());

const KNOWN_FILES: &[&str] = &[
    "README.md",
    "LICENSE",
    "Makefile",
    "CONTRIBUTING.md",
    "CHANGELOG.md",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "tsconfig.json",
    "Cargo.toml",
    "pyproject.toml",
    "go.mod",
    "Dockerfile",
    "docker-compose.yml",
    "docker-compose.yaml",
    "compose.yml",
    "AGENTS.md",
    "CLAUDE.md",
    "GEMINI.md",
    ".cursorrules",
    ".windsurfrules",
];

const EXAMPLE_DIRS: &[&str] = &[
    "foo",
    "bar",
    "baz",
    "qux",
    "path",
    "your-project",
    "my-project",
    "my-app",
    "example",
    "examples",
];

fn path_exists(root: &Path, from_dir: &Path, p: &str) -> bool {
    if p.starts_with("http://") || p.starts_with("https://") {
        return true;
    }
    if let Some(r) = p.strip_prefix("~/") {
        return home().join(r).exists();
    }
    if p.starts_with('/') {
        return Path::new(p).exists();
    }
    from_dir.join(p).exists() || root.join(p).exists() || root.join("node_modules").join(p).exists()
}

pub fn check_dead_refs(files: &[RuleFile], root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    for f in files {
        let dir = f.path.parent().unwrap_or_else(|| Path::new(""));
        for (i, line) in f.prose.split('\n').enumerate() {
            for m in PATH_RE.captures_iter(line) {
                let p = &m[1];
                let first_seg = p
                    .trim_start_matches(['~', '.'])
                    .trim_start_matches('/')
                    .split('/')
                    .next()
                    .unwrap_or("");
                if first_seg == "node_modules" || EXAMPLE_DIRS.contains(&first_seg) {
                    continue;
                }
                if VERSION_TAG.is_match(p) {
                    continue;
                }
                let has_slash = p.contains('/');
                if !has_slash && !KNOWN_FILES.contains(&p) {
                    continue;
                }
                if !path_exists(root, dir, p) {
                    findings.push(Finding::DeadRef {
                        reference: p.to_string(),
                        file: f.path.clone(),
                        line: i + 1,
                    });
                }
            }
            for m in IMPORT_RE.captures_iter(line) {
                let p = &m[1];
                let first_seg = p
                    .trim_start_matches(['~', '.'])
                    .trim_start_matches('/')
                    .split('/')
                    .next()
                    .unwrap_or("");
                if EXAMPLE_DIRS.contains(&first_seg) {
                    continue;
                }
                if !path_exists(root, dir, p) && !path_exists(root, dir, &format!("@{p}")) {
                    findings.push(Finding::DeadRef {
                        reference: format!("@{p}"),
                        file: f.path.clone(),
                        line: i + 1,
                    });
                }
            }
        }
    }
    findings
}

// ---------- dead commands ----------

const PM_BUILTINS: &[&str] = &[
    "install",
    "add",
    "remove",
    "rm",
    "uninstall",
    "dlx",
    "exec",
    "init",
    "create",
    "publish",
    "cache",
    "config",
    "why",
    "list",
    "ls",
    "outdated",
    "update",
    "upgrade",
    "link",
    "unlink",
    "audit",
    "pack",
    "info",
    "help",
    "version",
    "run",
    "-r",
    "--recursive",
    "import",
    "rebuild",
    "prune",
    "test",
    "t",
    "start",
    "stop",
    "restart",
    "ci",
    "env",
    "store",
    "patch",
    "patch-commit",
    "dedupe",
    "find-dupes",
    "workspaces",
    "workspace",
    "bin",
    "dir",
    "root",
    "doctor",
    "fetch",
    "licenses",
    "login",
    "logout",
    "whoami",
    "setup",
    "pkg",
    "build",
    "x",
    "pm",
    "repl",
    "up",
    "set",
    "deploy",
    "approve-builds",
    "self-update",
];

pub fn collect_pkgs(root: &Path) -> Vec<serde_json::Value> {
    let mut files = Vec::new();
    walk(root, 0, &mut files);
    files
        .iter()
        .filter(|f| f.file_name().is_some_and(|n| n == "package.json"))
        .filter_map(|f| serde_json::from_str(&std::fs::read_to_string(f).ok()?).ok())
        .collect()
}

pub fn collect_scripts(pkgs: &[serde_json::Value]) -> HashSet<String> {
    let mut scripts = HashSet::new();
    for pkg in pkgs {
        if let Some(s) = pkg.get("scripts").and_then(|v| v.as_object()) {
            for k in s.keys() {
                scripts.insert(k.clone());
            }
        }
    }
    scripts
}

pub fn check_dead_cmds(files: &[RuleFile], scripts: &HashSet<String>) -> Vec<Finding> {
    if scripts.is_empty() {
        return Vec::new();
    }
    static EXPLICIT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"`(?:npm run|pnpm run|yarn run|bun run) ([\w:.-]+)`").unwrap()
    });
    static BARE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"`(?:pnpm|yarn|bun) ([\w:.-]+)`").unwrap());
    static FILE_ARG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\.(?:[cm]?[jt]sx?|sh|py|rb)$").unwrap());
    let mut findings = Vec::new();
    for f in files {
        for (i, line) in f.prose.split('\n').enumerate() {
            for m in EXPLICIT.captures_iter(line) {
                if !scripts.contains(&m[1]) {
                    findings.push(Finding::DeadCmd {
                        cmd: m[1].to_string(),
                        raw: m[0].replace('`', ""),
                        file: f.path.clone(),
                        line: i + 1,
                    });
                }
            }
            for m in BARE.captures_iter(line) {
                let cmd = &m[1];
                // bare `pnpm x` runs script x, but builtin subcommands aren't
                // scripts; neither are flags (`pnpm -v`) or file args
                if cmd.starts_with('-') || FILE_ARG.is_match(cmd) {
                    continue;
                }
                if !PM_BUILTINS.contains(&cmd) && !scripts.contains(cmd) {
                    findings.push(Finding::DeadCmd {
                        cmd: cmd.to_string(),
                        raw: m[0].replace('`', ""),
                        file: f.path.clone(),
                        line: i + 1,
                    });
                }
            }
        }
    }
    findings
}

// ---------- stale versions ----------

const VERSIONED: &[&str] = &[
    "react",
    "next",
    "vue",
    "svelte",
    "angular",
    "tailwindcss",
    "typescript",
    "node",
    "prisma",
    "zod",
    "vite",
];

static NONSEMVER_DEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:file|link|portal|catalog|workspace|npm:|git|github:|https?):").unwrap()
});
static COUNT_NOUN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s+(?:items?|steps?|lines?|files?|times?|days?|hours?|minutes|seconds|weeks?|months?|years?|sections?|chapters?|parts?|phases?|stages?|commits?)\b")
        .unwrap()
});
static UPPER_BOUND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<\s*\d+(?:\.\d+)*").unwrap());
// candidate majors: separator + optional v + digits. the trailing boundary
// (., space, |, end) is checked in code since lookaround is unavailable.
static MAJOR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[~^>=|\s])v?(\d+)").unwrap());

pub fn check_stale_versions(files: &[RuleFile], pkgs: &[serde_json::Value]) -> Vec<Finding> {
    let mut installed: HashMap<String, HashSet<String>> = HashMap::new();
    for pkg in pkgs {
        for section in ["dependencies", "devDependencies", "engines"] {
            let Some(deps) = pkg.get(section).and_then(|v| v.as_object()) else {
                continue;
            };
            for name in VERSIONED {
                let Some(v) = deps.get(*name).and_then(|v| v.as_str()) else {
                    continue;
                };
                if NONSEMVER_DEP.is_match(v.trim()) {
                    continue;
                }
                // drop exclusive upper bounds first, then take every remaining
                // major: ">=18 <20" -> {18}; "^18 || ^20" -> {18,20}
                let spec = UPPER_BOUND.replace_all(v, "");
                for m in MAJOR_RE.captures_iter(&spec) {
                    // trailing boundary: next char must be ., space, | or end
                    let end = m.get(0).unwrap().end();
                    let ok = spec[end..]
                        .chars()
                        .next()
                        .is_none_or(|c| c == '.' || c.is_whitespace() || c == '|');
                    if ok {
                        installed
                            .entry((*name).to_string())
                            .or_default()
                            .insert(m[1].to_string());
                    }
                }
            }
        }
    }
    if installed.is_empty() {
        return Vec::new();
    }
    static VER_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(react|next|vue|svelte|angular|tailwindcss|typescript|node|prisma|zod|vite|tailwind|nodejs)(?:\.js)?[ -]?v?(\d{1,2})\b")
            .unwrap()
    });
    static NEXT_GUARD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(Next|next\.js|next[ -]v)").unwrap());
    let mut findings = Vec::new();
    let mut emitted = HashSet::new();
    for f in files {
        for (i, line) in f.prose.split('\n').enumerate() {
            for m in VER_RE.captures_iter(line) {
                let mut name = m[1].to_lowercase();
                if name == "tailwind" {
                    name = "tailwindcss".to_string();
                } else if name == "nodejs" {
                    name = "node".to_string();
                }
                // lowercase "next" is usually the English word; require Next,
                // next.js, or next vN
                if name == "next" && !NEXT_GUARD.is_match(&m[0]) {
                    continue;
                }
                if COUNT_NOUN.is_match(&line[m.get(0).unwrap().end()..]) {
                    continue;
                }
                let Some(majors) = installed.get(&name) else {
                    continue;
                };
                if majors.contains(&m[2]) {
                    continue;
                }
                let key = format!("{}|{}|{}", name, &m[2], f.path.display());
                if !emitted.insert(key) {
                    continue;
                }
                let mut sorted: Vec<&String> = majors.iter().collect();
                sorted.sort();
                findings.push(Finding::Stale {
                    reference: format!("{} {}", &m[1], &m[2]),
                    file: f.path.clone(),
                    line: i + 1,
                    note: format!(
                        "package.json has {} {}",
                        name,
                        sorted
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("/")
                    ),
                });
            }
        }
    }
    findings
}

// ---------- dead globs ----------

/// Split a glob list on top-level commas/semicolons only - commas inside
/// brace expansions ({ts,tsx}) must not split.
fn split_globs(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' | ';' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn clean_glob(s: &str) -> String {
    let s = s.trim();
    let s = Regex::new(r"\s+#.*$").unwrap().replace(s, "");
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    s.trim().trim_matches(|c| c == '"' || c == '\'').to_string()
}

pub fn check_dead_globs(files: &[RuleFile], root: &Path) -> Vec<Finding> {
    static INLINE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?im)^[ \t]*(?:globs?|applyTo)[ \t]*:[ \t]*(\S.*?)[ \t]*$").unwrap()
    });
    static LIST_KEY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[ \t]*(?:globs?|applyTo)[ \t]*(?::[ \t]*(?:#.*)?)$").unwrap()
    });
    static LIST_ITEM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[ \t]*-[ \t]*(.+)$").unwrap());
    static COMMENT_BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t]*(#|$)").unwrap());
    static WILD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*?{\[]").unwrap());

    let mut findings = Vec::new();
    let mut check = |f: &RuleFile, glob: &str, line: usize| {
        if glob.is_empty() || glob.starts_with('#') || glob.starts_with('!') {
            return;
        }
        // leading "/" is repo-root-relative intent, not the OS root
        let g = glob.trim_start_matches('/');
        // static prefix = everything before the first wildcard, trimmed back
        // to the last complete path segment (src/Button*.tsx -> src)
        let prefix = match g.find(|c: char| "*?{[".contains(c)) {
            None => g.trim_end_matches('/').to_string(),
            Some(wild) => {
                let seg = &g[..wild];
                match seg.rfind('/') {
                    Some(i) => seg[..i].to_string(),
                    None => String::new(),
                }
            }
        };
        if prefix.is_empty() || prefix == "." {
            return;
        }
        // cursor/copilot globs are workspace-root-relative
        if !root.join(&prefix).exists() {
            findings.push(Finding::DeadGlob {
                reference: glob.to_string(),
                file: f.path.clone(),
                line,
                note: "glob prefix does not exist; rule never loads".to_string(),
            });
        }
    };

    for f in files {
        let fname = f.path.to_string_lossy();
        if !(fname.ends_with(".mdc") || fname.ends_with(".instructions.md")) {
            continue;
        }
        let Some(fm) = FM_RE.find(&f.text) else {
            continue;
        };
        let body = fm.as_str().trim_start_matches(['-', ' ', '\t', '\n']);
        let body = body.strip_suffix("---").unwrap_or(body);
        let body = body.trim_end_matches(['-', ' ', '\t']);
        for m in INLINE.captures_iter(body) {
            let line = body[..m.get(0).unwrap().start()].matches('\n').count() + 2;
            for glob in split_globs(&clean_glob(&m[1])) {
                check(f, &clean_glob(&glob), line);
            }
        }
        let fm_lines: Vec<&str> = body.split('\n').collect();
        for (li, l) in fm_lines.iter().enumerate() {
            if !LIST_KEY.is_match(l) {
                continue;
            }
            for (k, l2) in fm_lines.iter().enumerate().skip(li + 1) {
                match LIST_ITEM.captures(l2) {
                    Some(item) => {
                        for glob in split_globs(&clean_glob(&item[1])) {
                            check(f, &clean_glob(&glob), k + 2);
                        }
                    }
                    None => {
                        if COMMENT_BLANK.is_match(l2) {
                            continue;
                        }
                        break;
                    }
                }
            }
        }
        let _ = &WILD; // wildcard detection is inline via char search
    }
    findings
}
