//! Instruction-file discovery for repos and global config dirs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const ROOT_FILES: &[&str] = &[
    "AGENTS.md",
    "AGENTS.override.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    "GEMINI.md",
    "QWEN.md",
    ".cursorrules",
    ".windsurfrules",
    ".clinerules",
    ".aider.conf.yml",
    ".github/copilot-instructions.md",
    "CONVENTIONS.md",
];

const RULE_DIRS: &[(&str, &str)] = &[
    (".cursor/rules", "cursor"),
    (".windsurf/rules", "windsurf"),
    (".clinerules", "cline"),
    (".agents/rules", "agents"),
    (".github/instructions", "copilot"),
];

const USER_FILES: &[(&str, &str)] = &[
    ("~/.claude/CLAUDE.md", "claude code"),
    ("~/AGENTS.md", "agents"),
    ("~/CLAUDE.md", "claude code"),
    ("~/GEMINI.md", "gemini cli"),
    ("~/.gemini/GEMINI.md", "gemini cli"),
    ("~/.config/devin/AGENTS.md", "devin"),
    ("~/.codeium/windsurf/memories/global_rules.md", "windsurf"),
];

const USER_DIRS: &[(&str, &str)] = &[
    ("~/.agents/rules", "agents"),
    ("~/.cursor/rules", "cursor"),
    ("~/.claude/rules", "claude"),
    ("~/.config/agents/rules", "agents"),
];

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "dist",
    "build",
    ".next",
    "vendor",
    "out",
    "coverage",
];

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn expand_home(p: &str) -> PathBuf {
    p.strip_prefix("~/")
        .map(|r| home().join(r))
        .unwrap_or_else(|| PathBuf::from(p))
}

#[derive(Debug, Clone)]
pub struct FoundFile {
    pub path: PathBuf,
    pub label: String,
}

/// Walk a directory to depth 3, skipping vendor/output dirs. Symlinks are
/// followed once via metadata.
pub fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let full = e.path();
        let Ok(meta) = std::fs::metadata(&full) else {
            continue;
        };
        if meta.is_dir() {
            if !SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()) {
                walk(&full, depth + 1, out);
            }
        } else if meta.is_file() {
            out.push(full);
        }
    }
}

fn add(found: &mut HashMap<PathBuf, FoundFile>, p: &Path, label: &str) {
    let Ok(st) = std::fs::metadata(p) else {
        return;
    };
    if !st.is_file() || st.len() == 0 || st.len() > 5_000_000 {
        return;
    }
    let real = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if found.contains_key(&real) {
        return; // symlink aliases are one file, not a duplicate
    }
    found.insert(
        real,
        FoundFile {
            path: p.to_path_buf(),
            label: label.to_string(),
        },
    );
}

pub fn discover(root: &Path) -> Vec<FoundFile> {
    let mut found = HashMap::new();
    for rel in ROOT_FILES {
        add(&mut found, &root.join(rel), rel);
    }
    for (dir, label) in RULE_DIRS {
        let abs = root.join(dir);
        if !abs.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk(&abs, 0, &mut files);
        for f in files {
            add(&mut found, &f, label);
        }
    }
    // nested copies of the big-name files up to depth 3
    let mut all = Vec::new();
    walk(root, 0, &mut all);
    for f in all {
        if let Some(name) = f.file_name().and_then(|n| n.to_str()) {
            if matches!(name, "CLAUDE.md" | "AGENTS.md" | "GEMINI.md") {
                add(&mut found, &f, name);
            }
        }
    }
    found.into_values().collect()
}

pub fn discover_user() -> Vec<FoundFile> {
    let mut found = HashMap::new();
    for (file, label) in USER_FILES {
        add(&mut found, &expand_home(file), label);
    }
    for (dir, label) in USER_DIRS {
        let abs = expand_home(dir);
        if !abs.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        walk(&abs, 0, &mut files);
        for f in files {
            add(&mut found, &f, label);
        }
    }
    found.into_values().collect()
}
