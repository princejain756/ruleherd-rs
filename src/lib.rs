pub mod checks;
pub mod discover;
pub mod report;

use checks::{Finding, RuleFile};
use std::path::{Path, PathBuf};

/// Read discovered files into RuleFile entries with prose masked.
pub fn load_files(found: Vec<discover::FoundFile>) -> Vec<RuleFile> {
    found
        .into_iter()
        .filter_map(|f| {
            let bytes = std::fs::read(&f.path).ok()?;
            if bytes.contains(&0) {
                return None; // binary
            }
            let text = String::from_utf8_lossy(&bytes)
                .trim_start_matches('\u{FEFF}')
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            let prose = checks::strip_fences(&checks::strip_frontmatter(&text));
            Some(RuleFile {
                path: f.path,
                label: f.label,
                text,
                prose,
            })
        })
        .collect()
}

pub fn run(root: &Path, user_mode: bool) -> report::Report {
    let h = discover::home();
    let scan_root: PathBuf = if user_mode { h } else { root.to_path_buf() };
    let files = load_files(if user_mode {
        discover::discover_user()
    } else {
        discover::discover(root)
    });

    let pkgs = if user_mode {
        Vec::new()
    } else {
        checks::collect_pkgs(&scan_root)
    };
    let scripts = checks::collect_scripts(&pkgs);

    let mut findings: Vec<Finding> = Vec::new();
    findings.extend(checks::check_conflicts(&files));
    findings.extend(checks::check_dead_refs(&files, &scan_root));
    findings.extend(checks::check_dead_cmds(&files, &scripts));
    findings.extend(checks::check_duplicates(&files));
    findings.extend(checks::check_dead_globs(&files, &scan_root));
    if !user_mode {
        findings.extend(checks::check_stale_versions(&files, &pkgs));
    }

    let has_agents = files
        .iter()
        .any(|f| f.path.file_name().is_some_and(|n| n == "AGENTS.md"));
    if !files.is_empty() && !has_agents && !user_mode {
        findings.push(Finding::Missing {
            file: "AGENTS.md".to_string(),
            note: "30+ agents read AGENTS.md; only Claude reads CLAUDE.md".to_string(),
        });
    }

    let report_files: Vec<(RuleFile, usize)> = files
        .into_iter()
        .map(|f| {
            let tokens = f.text.len() / 4;
            (f, tokens)
        })
        .collect();
    report::collect_bloat(&report_files, &mut findings);

    let files_only: Vec<RuleFile> = report_files.into_iter().map(|(f, _)| f).collect();
    report::build(files_only, findings)
}
