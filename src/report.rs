//! Output formats: text, json, github annotations, sarif.

use crate::checks::{rel, Finding, RuleFile};
use std::collections::HashSet;
use std::path::Path;

pub const VERSION: &str = "0.1.0";

pub struct Report {
    pub files: Vec<(RuleFile, usize)>, // file + approx tokens
    pub findings: Vec<Finding>,
    pub total_tokens: usize,
    pub score: i64,
    pub grade: &'static str,
}

fn grade(score: i64) -> &'static str {
    match score {
        90.. => "A",
        80.. => "B",
        70.. => "C",
        60.. => "D",
        _ => "F",
    }
}

pub fn build(files: Vec<RuleFile>, findings: Vec<Finding>) -> Report {
    let mut scored: Vec<(RuleFile, usize)> = files
        .into_iter()
        .map(|f| {
            let tokens = f.text.len() / 4;
            (f, tokens)
        })
        .collect();
    let total_tokens = scored.iter().map(|(_, t)| t).sum();
    let count = |k: &str| findings.iter().filter(|f| f.kind() == k).count() as i64;
    let mut score = 100i64;
    score -= (count("CONFLICT") * 20).min(50);
    score -= (count("DEAD REF") * 6).min(24);
    score -= (count("DEAD CMD") * 8).min(20);
    score -= (count("DUPLICATE") * 5).min(15);
    score -= (count("STALE") * 4).min(10);
    score -= (count("DEAD GLOB") * 5).min(10);
    score -= (count("BLOAT") * 5).min(10);
    score -= (count("MISSING") * 5).min(5);
    let score = score.max(0);
    scored.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    Report {
        files: scored,
        findings,
        total_tokens,
        score,
        grade: grade(score),
    }
}

// ---------- text ----------

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const BLUE: &str = "\x1b[34m";
const MAGENTA: &str = "\x1b[35m";

fn kind_color(k: &str) -> &'static str {
    match k {
        "CONFLICT" => RED,
        "MISSING" => BLUE,
        "DUPLICATE" => MAGENTA,
        _ => YELLOW,
    }
}

pub fn print_text(display_root: &Path, r: &Report) {
    let bar = "═".repeat(58);
    let title = if display_root == crate::discover::home() {
        "~ (global config)".to_string()
    } else {
        display_root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| display_root.display().to_string())
    };
    println!("\n{BOLD}RULEHERD v{VERSION}{RESET} {DIM}· {title}{RESET}");
    println!("{DIM}{bar}{RESET}");
    let gc = match r.grade {
        "A" | "B" => GREEN,
        "C" | "D" => YELLOW,
        _ => RED,
    };
    println!(
        "{BOLD}GRADE: {gc}{}{RESET}{BOLD} ({}/100){RESET}\n",
        r.grade, r.score
    );

    if r.files.is_empty() {
        println!("no agent instruction files found.");
        println!("looked for CLAUDE.md, AGENTS.md, .cursor/rules/, .windsurfrules, copilot-instructions...");
        return;
    }

    println!(
        "{} instruction file{} · ~{} tokens of agent context",
        r.files.len(),
        if r.files.len() > 1 { "s" } else { "" },
        r.total_tokens
    );
    for (f, tok) in &r.files {
        println!(
            "  {:<46} {:>6} tok  {DIM}{}{RESET}",
            rel(display_root, &f.path),
            tok,
            f.label
        );
    }
    println!();

    if r.findings.is_empty() {
        println!(
            "{GREEN}no drift found. your files agree with each other and with the repo.{RESET}"
        );
        return;
    }

    println!("{BOLD}FINDINGS{RESET}");
    for f in &r.findings {
        let c = kind_color(f.kind());
        match f {
            Finding::Conflict {
                topic,
                a,
                b,
                a_label,
                b_label,
            } => {
                let labels = if a_label.is_empty() {
                    String::new()
                } else {
                    format!(" ({a_label} vs {b_label})")
                };
                println!("  {c}CONFLICT{RESET}  {topic}{labels}");
                println!(
                    "          {}:{}  {DIM}\"{}\"{RESET}",
                    rel(display_root, &a.file),
                    a.line_idx + 1,
                    &a.line[..a.line.len().min(70)]
                );
                println!(
                    "          {}:{}  {DIM}\"{}\"{RESET}",
                    rel(display_root, &b.file),
                    b.line_idx + 1,
                    &b.line[..b.line.len().min(70)]
                );
            }
            Finding::DeadRef {
                reference,
                file,
                line,
            } => {
                println!("  {c}DEAD REF{RESET}  {reference}");
                println!(
                    "          {}:{}  {DIM}file does not exist{RESET}",
                    rel(display_root, file),
                    line
                );
            }
            Finding::DeadCmd {
                cmd,
                raw,
                file,
                line,
            } => {
                println!("  {c}DEAD CMD{RESET}  {raw}");
                println!(
                    "          {}:{}  {DIM}no \"{cmd}\" script in any package.json{RESET}",
                    rel(display_root, file),
                    line
                );
            }
            Finding::Duplicate { len, files } => {
                println!(
                    "  {c}DUPLICATE{RESET} ~{len}-char block in {} files",
                    files.len()
                );
                for p in files {
                    println!("          {DIM}{}{RESET}", rel(display_root, p));
                }
            }
            Finding::Missing { file, note } => {
                println!("  {c}MISSING{RESET}   {file}  {DIM}{note}{RESET}");
            }
            Finding::Bloat { file, note } => {
                println!(
                    "  {c}BLOAT{RESET}     {}  {DIM}{note}{RESET}",
                    rel(display_root, file)
                );
            }
            Finding::Stale {
                reference,
                file,
                line,
                note,
            } => {
                println!(
                    "  {c}STALE{RESET}     {reference} in {}:{}",
                    rel(display_root, file),
                    line
                );
                println!("          {DIM}{note}{RESET}");
            }
            Finding::DeadGlob {
                reference,
                file,
                line,
                note,
            } => {
                println!(
                    "  {c}DEAD GLOB{RESET} {reference} in {}:{}",
                    rel(display_root, file),
                    line
                );
                println!("          {DIM}{note}{RESET}");
            }
        }
    }
    println!("\n{DIM}fix the conflicts first. dead refs and dupes are why your agents disagree.{RESET}\n");
}

// ---------- github annotations ----------

fn esc_prop(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn esc_data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

pub fn github(display_root: &Path, r: &Report) -> String {
    let mut lines = Vec::new();
    for f in &r.findings {
        match f {
            Finding::Conflict { topic, a, b, .. } => {
                lines.push(format!(
                    "::error file={},line={},title=CONFLICT ({})::{}",
                    esc_prop(&rel(display_root, &a.file)),
                    a.line_idx + 1,
                    esc_prop(topic),
                    esc_data(&format!(
                        "Conflicts with {}:{} (\"{}\")",
                        rel(display_root, &b.file),
                        b.line_idx + 1,
                        &b.line[..b.line.len().min(70)]
                    ))
                ));
            }
            Finding::DeadRef {
                reference,
                file,
                line,
            }
            | Finding::Stale {
                reference,
                file,
                line,
                ..
            } => {
                let note = if let Finding::Stale { note, .. } = f {
                    format!(" - {note}")
                } else {
                    String::new()
                };
                lines.push(format!(
                    "::warning file={},line={},title={}::{}{}",
                    esc_prop(&rel(display_root, file)),
                    line,
                    esc_prop(f.kind()),
                    esc_data(reference),
                    esc_data(&note)
                ));
            }
            Finding::DeadCmd {
                cmd,
                raw,
                file,
                line,
            } => {
                let label = if raw.is_empty() {
                    format!("npm run {cmd}")
                } else {
                    raw.clone()
                };
                lines.push(format!(
                    "::warning file={},line={},title=DEAD CMD::{}",
                    esc_prop(&rel(display_root, file)),
                    line,
                    esc_data(&label)
                ));
            }
            Finding::DeadGlob {
                reference,
                file,
                line,
                note,
            } => {
                lines.push(format!(
                    "::warning file={},line={},title=DEAD GLOB::{}",
                    esc_prop(&rel(display_root, file)),
                    line,
                    esc_data(&format!("{reference} - {note}"))
                ));
            }
            Finding::Duplicate { len, files } => {
                lines.push(format!(
                    "::notice file={},title=DUPLICATE::{}",
                    esc_prop(&rel(display_root, &files[0])),
                    esc_data(&format!(
                        "~{len}-char block duplicated across {}",
                        files
                            .iter()
                            .map(|p| rel(display_root, p))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                ));
            }
            Finding::Missing { file, note } => {
                lines.push(format!(
                    "::notice file={},title=MISSING::{}",
                    esc_prop(file),
                    esc_data(note)
                ));
            }
            Finding::Bloat { file, note } => {
                lines.push(format!(
                    "::notice file={},title=BLOAT::{}",
                    esc_prop(&rel(display_root, file)),
                    esc_data(note)
                ));
            }
        }
    }
    lines.push(format!(
        "::notice title=ruleherd::grade {} ({}/100)",
        r.grade, r.score
    ));
    lines.join("\n")
}

// ---------- sarif ----------

pub fn sarif(display_root: &Path, r: &Report) -> String {
    let mut rules = serde_json::Map::new();
    let mut results = Vec::new();
    for f in &r.findings {
        let rule_id = f.kind().to_lowercase().replace(' ', "-");
        let level = match f.kind() {
            "CONFLICT" => "error",
            "DUPLICATE" | "MISSING" | "BLOAT" => "note",
            _ => "warning",
        };
        rules.entry(rule_id.clone()).or_insert_with(|| {
            serde_json::json!({
                "id": rule_id,
                "shortDescription": { "text": format!("ruleherd {}", f.kind()) },
                "defaultConfiguration": { "level": level }
            })
        });
        let (uri, start_line, msg) = match f {
            Finding::Conflict { topic, a, b, .. } => (
                rel(display_root, &a.file),
                a.line_idx + 1,
                format!(
                    "Conflict on {} between {}:{} and {}:{}",
                    topic,
                    rel(display_root, &a.file),
                    a.line_idx + 1,
                    rel(display_root, &b.file),
                    b.line_idx + 1
                ),
            ),
            Finding::Duplicate { len, files } => (
                rel(display_root, &files[0]),
                1,
                format!(
                    "~{len}-char block duplicated across {}",
                    files
                        .iter()
                        .map(|p| rel(display_root, p))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            Finding::Missing { file, note } => (file.clone(), 1, format!("{}: {note}", f.kind())),
            Finding::Bloat { file, note } => {
                (rel(display_root, file), 1, format!("{}: {note}", f.kind()))
            }
            Finding::DeadRef {
                reference,
                file,
                line,
            }
            | Finding::DeadGlob {
                reference,
                file,
                line,
                ..
            }
            | Finding::Stale {
                reference,
                file,
                line,
                ..
            } => {
                let note = match f {
                    Finding::Stale { note, .. } | Finding::DeadGlob { note, .. } => {
                        format!(" ({note})")
                    }
                    _ => String::new(),
                };
                (
                    rel(display_root, file),
                    *line,
                    format!("{}: {reference}{note}", f.kind()),
                )
            }
            Finding::DeadCmd {
                cmd,
                raw,
                file,
                line,
            } => {
                let label = if raw.is_empty() {
                    format!("npm run {cmd}")
                } else {
                    raw.clone()
                };
                (
                    rel(display_root, file),
                    *line,
                    format!("{}: {label}", f.kind()),
                )
            }
        };
        results.push(serde_json::json!({
            "ruleId": rule_id,
            "level": level,
            "message": { "text": msg },
            "locations": [{
                "physicalLocation": {
                    "artifactLocation": { "uri": uri },
                    "region": { "startLine": start_line.max(1) }
                }
            }]
        }));
    }
    serde_json::to_string_pretty(&serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "ruleherd",
                    "version": VERSION,
                    "informationUri": "https://github.com/princejain756/ruleherd-rs",
                    "rules": rules.values().collect::<Vec<_>>()
                }
            },
            "properties": {
                "root": display_root,
                "score": r.score,
                "grade": r.grade,
                "totalTokens": r.total_tokens
            },
            "results": results
        }]
    }))
    .unwrap()
}

// ---------- json ----------

pub fn json(display_root: &Path, r: &Report) -> String {
    let findings: Vec<serde_json::Value> = r
        .findings
        .iter()
        .map(|f| match f {
            Finding::Conflict {
                topic,
                a,
                b,
                a_label,
                b_label,
            } => serde_json::json!({
                "kind": "CONFLICT", "topic": topic,
                "aLabel": a_label, "bLabel": b_label,
                "a": { "file": rel(display_root, &a.file), "line": a.line_idx + 1, "text": a.line },
                "b": { "file": rel(display_root, &b.file), "line": b.line_idx + 1, "text": b.line }
            }),
            Finding::DeadRef {
                reference,
                file,
                line,
            } => serde_json::json!({
                "kind": "DEAD REF", "ref": reference,
                "file": rel(display_root, file), "line": line
            }),
            Finding::DeadCmd {
                cmd,
                raw,
                file,
                line,
            } => serde_json::json!({
                "kind": "DEAD CMD", "cmd": cmd, "raw": raw,
                "file": rel(display_root, file), "line": line
            }),
            Finding::Duplicate { len, files } => serde_json::json!({
                "kind": "DUPLICATE", "len": len,
                "files": files.iter().map(|p| rel(display_root, p)).collect::<Vec<_>>()
            }),
            Finding::Missing { file, note } => serde_json::json!({
                "kind": "MISSING", "file": file, "note": note
            }),
            Finding::Bloat { file, note } => serde_json::json!({
                "kind": "BLOAT", "file": rel(display_root, file), "note": note
            }),
            Finding::Stale {
                reference,
                file,
                line,
                note,
            } => serde_json::json!({
                "kind": "STALE", "ref": reference,
                "file": rel(display_root, file), "line": line, "note": note
            }),
            Finding::DeadGlob {
                reference,
                file,
                line,
                note,
            } => serde_json::json!({
                "kind": "DEAD GLOB", "ref": reference,
                "file": rel(display_root, file), "line": line, "note": note
            }),
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "root": display_root,
        "score": r.score,
        "grade": r.grade,
        "files": r.files.iter().map(|(f, _)| rel(display_root, &f.path)).collect::<Vec<_>>(),
        "totalTokens": r.total_tokens,
        "findings": findings
    }))
    .unwrap()
}

pub fn collect_bloat(files: &[(RuleFile, usize)], findings: &mut Vec<Finding>) {
    let mut seen = HashSet::new();
    for (f, tok) in files {
        if *tok > 4000 && seen.insert(f.path.clone()) {
            findings.push(Finding::Bloat {
                file: f.path.clone(),
                note: format!("~{tok} tokens load into every prompt"),
            });
        }
    }
}
