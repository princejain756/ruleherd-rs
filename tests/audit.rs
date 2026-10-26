use ruleherd::checks::Finding;
use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixture_repo")
}

fn kinds(r: &ruleherd::report::Report) -> Vec<&'static str> {
    r.findings.iter().map(|f| f.kind()).collect()
}

#[test]
fn detects_conflicts_across_files() {
    let r = ruleherd::run(&fixture(), false);
    let conflicts: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| matches!(f, Finding::Conflict { .. }))
        .collect();
    // package manager: npm run build vs pnpm run build
    // indentation: tabs vs spaces
    // commit policy: never commit vs always commit
    let topics: Vec<String> = conflicts
        .iter()
        .map(|f| match f {
            Finding::Conflict { topic, .. } => topic.clone(),
            _ => unreachable!(),
        })
        .collect();
    assert!(
        topics.iter().any(|t| t.contains("package manager")),
        "{topics:?}"
    );
    assert!(
        topics.iter().any(|t| t.contains("indentation")),
        "{topics:?}"
    );
    assert!(topics.iter().any(|t| t.contains("commit")), "{topics:?}");
}

#[test]
fn detects_dead_refs_and_cmds() {
    let r = ruleherd::run(&fixture(), false);
    // docs/guide.md and tools/scripts/deploy.sh don't exist
    let dead_refs: Vec<&String> = r
        .findings
        .iter()
        .filter_map(|f| match f {
            Finding::DeadRef { reference, .. } => Some(reference),
            _ => None,
        })
        .collect();
    assert!(
        dead_refs.iter().any(|r| r.contains("docs/guide.md")),
        "{dead_refs:?}"
    );
    assert!(
        dead_refs
            .iter()
            .any(|r| r.contains("tools/scripts/deploy.sh")),
        "{dead_refs:?}"
    );
    // README.md exists so it must NOT be flagged
    assert!(
        !dead_refs.iter().any(|r| r.contains("README.md")),
        "{dead_refs:?}"
    );

    // `npm run e2e` has no e2e script; `npm run build`/`pnpm run lint` exist
    let dead_cmds: Vec<&String> = r
        .findings
        .iter()
        .filter_map(|f| match f {
            Finding::DeadCmd { cmd, .. } => Some(cmd),
            _ => None,
        })
        .collect();
    assert!(
        dead_cmds.iter().any(|c| c.as_str() == "e2e"),
        "{dead_cmds:?}"
    );
    assert!(
        !dead_cmds.iter().any(|c| c.as_str() == "build"),
        "{dead_cmds:?}"
    );
    assert!(
        !dead_cmds.iter().any(|c| c.as_str() == "lint"),
        "{dead_cmds:?}"
    );
}

#[test]
fn detects_stale_versions_and_dead_globs() {
    let r = ruleherd::run(&fixture(), false);
    // react 17 mentioned while package.json has react ^19
    let stale: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| matches!(f, Finding::Stale { .. }))
        .collect();
    assert!(
        stale.iter().any(|f| match f {
            Finding::Stale { reference, .. } => reference.contains("17"),
            _ => false,
        }),
        "{:?}",
        kinds(&r)
    );

    // cursor rule globs point at api/ and routes/ which don't exist
    let dead_globs: Vec<&Finding> = r
        .findings
        .iter()
        .filter(|f| matches!(f, Finding::DeadGlob { .. }))
        .collect();
    assert!(!dead_globs.is_empty(), "{:?}", kinds(&r));
}

#[test]
fn score_drops_with_findings() {
    let r = ruleherd::run(&fixture(), false);
    assert!(r.score < 100);
    assert!(r.findings.len() >= 4);
}

#[test]
fn clean_repo_grades_high() {
    let dir = std::env::temp_dir().join(format!("ruleherd-clean-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("AGENTS.md"),
        "# Rules\n\nRun `npm run test` to test. See `README.md`.\n",
    )
    .unwrap();
    std::fs::write(dir.join("README.md"), "# fixture\n").unwrap();
    std::fs::write(dir.join("package.json"), r#"{"scripts":{"test":"vitest"}}"#).unwrap();
    let r = ruleherd::run(&dir, false);
    assert_eq!(r.findings.len(), 0, "{:?}", kinds(&r));
    assert_eq!(r.grade, "A");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn strict_exit_code() {
    let bin = env!("CARGO_BIN_EXE_ruleherd");
    let out = Command::new(bin)
        .arg("--strict")
        .arg("--format")
        .arg("json")
        .arg(fixture())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("\"findings\""));

    let dir = std::env::temp_dir().join(format!("ruleherd-strict-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("AGENTS.md"), "# ok\n").unwrap();
    let out2 = Command::new(bin)
        .arg("--strict")
        .arg("--format")
        .arg("json")
        .arg(&dir)
        .output()
        .unwrap();
    assert_eq!(out2.status.code(), Some(0));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sarif_output_is_valid() {
    let bin = env!("CARGO_BIN_EXE_ruleherd");
    let out = Command::new(bin)
        .arg("--format")
        .arg("sarif")
        .arg(fixture())
        .output()
        .unwrap();
    let sarif: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sarif["version"], "2.1.0");
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert!(!results.is_empty());
}
