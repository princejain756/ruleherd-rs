use ruleherd::report::VERSION;
use std::path::PathBuf;
use std::process::exit;

fn usage() -> ! {
    println!(
        "ruleherd {VERSION} - audit agent instruction files for drift

usage:
  ruleherd [dir]             audit a repo (default: cwd)
  ruleherd --user            audit your global config files (~/.claude, ~/.cursor, ...)
  ruleherd --json            machine-readable JSON output
  ruleherd --format <fmt>    output format: text (default), json, github, sarif
  ruleherd --strict          exit 1 if any finding (for CI)

finds: contradictions between files, dead file refs, commands that don't
exist in package.json, stale version mentions, dead rule globs, blocks
copy-pasted across files, missing AGENTS.md."
    );
    exit(0);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        usage();
    }
    let get_val = |flag: &str| -> Option<String> {
        args.iter()
            .find_map(|a| a.strip_prefix(&format!("{flag}=")).map(String::from))
            .or_else(|| {
                args.iter()
                    .position(|a| a == flag)
                    .and_then(|i| args.get(i + 1).cloned())
            })
    };
    let format = get_val("--format").unwrap_or_else(|| {
        if args.iter().any(|a| a == "--json") {
            "json".to_string()
        } else {
            "text".to_string()
        }
    });
    if !["text", "json", "github", "sarif"].contains(&format.as_str()) {
        eprintln!("ruleherd: --format must be text, json, github, or sarif");
        exit(2);
    }
    let user_mode = args.iter().any(|a| a == "--user" || a == "-u");
    let strict = args.iter().any(|a| a == "--strict");

    let fmt_idx = args.iter().position(|a| a == "--format");
    let dir_arg = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with('-') && (fmt_idx.is_none() || *i != fmt_idx.unwrap() + 1));
    let root: PathBuf = dir_arg
        .map(|(_, a)| PathBuf::from(a))
        .unwrap_or_else(|| PathBuf::from("."));
    let root = root.canonicalize().unwrap_or(root);

    let display_root = if user_mode {
        ruleherd::discover::home()
    } else {
        root.clone()
    };
    let r = ruleherd::run(&root, user_mode);

    match format.as_str() {
        "json" => println!("{}", ruleherd::report::json(&display_root, &r)),
        "github" => println!("{}", ruleherd::report::github(&display_root, &r)),
        "sarif" => println!("{}", ruleherd::report::sarif(&display_root, &r)),
        _ => ruleherd::report::print_text(&display_root, &r),
    }
    exit(if strict && !r.findings.is_empty() {
        1
    } else {
        0
    });
}
