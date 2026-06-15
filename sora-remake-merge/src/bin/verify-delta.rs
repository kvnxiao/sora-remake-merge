//! Full-corpus localization-delta invariant check.
//!
//! Runs the merge across every EVO/Xseed/`original` triple under `resources/`
//! and asserts the invariant proven by [`sora_remake_merge::verify`]: the
//! merged output differs from EVO exactly where Xseed differs from `original`,
//! and carries Xseed's text wherever it differs.
//!
//! Reports the documented exemptions (EVO anchor-shape upgrades and `Body::Asm`
//! substitutions) and exits non-zero if any real violation is found. The e2e
//! suite covers the same invariant on the committed fixtures; this binary is
//! the manual full-corpus counterpart (it needs the `resources/` corpora, which
//! are local-only build inputs).

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "this is a CLI analysis tool"
)]

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use sora_remake_merge::AnchorKey;
use sora_remake_merge::Site;
use sora_remake_merge::Violation;
use sora_remake_merge::parse_ing;
use sora_remake_merge::verify_scena;
use std::path::Path;
use std::process::ExitCode;
use walkdir::WalkDir;

const EVO_ROOT: &str = "resources/evo-voice-mod/script_en/scena";
const XSEED_ROOT: &str = "resources/xseed-restoration/script_en/scena";
const ORIG_ROOT: &str = "resources/original/script_en/scena";

fn fmt_site(site: Site) -> &'static str {
    match site {
        Site::Body => "body",
        Site::Called => "called",
    }
}

fn fmt_key(key: &AnchorKey) -> String {
    match key {
        AnchorKey::Portrait { char_id, tag } => format!("Portrait(char_id={char_id}, tag={tag})"),
        AnchorKey::Untagged { char_id } => format!("Untagged({char_id:?})"),
        AnchorKey::Voiced(v) => format!("Voiced({v})"),
        AnchorKey::Letter => "Letter".to_owned(),
        AnchorKey::Plain => "Plain".to_owned(),
        AnchorKey::Narration(prefix) => format!("Narration({prefix:?})"),
        AnchorKey::MapName => "MapName".to_owned(),
        AnchorKey::MenuItem => "MenuItem".to_owned(),
        AnchorKey::DisplayName { char_id } => format!("DisplayName(char_id={char_id})"),
    }
}

fn report_violation(file: &str, v: &Violation) {
    println!(
        "[violation:{kind:?}] {file}  {func}  {site}  {key}  occ={occ}",
        kind = v.kind,
        func = v.function,
        site = fmt_site(v.site),
        key = fmt_key(&v.key),
        occ = v.occurrence,
    );
}

fn run() -> Result<()> {
    let evo_root = Path::new(EVO_ROOT);
    let xseed_root = Path::new(XSEED_ROOT);
    let orig_root = Path::new(ORIG_ROOT);
    for (label, root) in [
        ("EVO", evo_root),
        ("Xseed", xseed_root),
        ("original", orig_root),
    ] {
        if !root.exists() {
            anyhow::bail!("{label} root does not exist: {}", root.display());
        }
    }

    let mut files_checked = 0_usize;
    let mut files_missing = 0_usize;
    let mut functions_checked = 0_usize;
    let mut occurrences_checked = 0_usize;
    let mut localized = 0_usize;
    let mut total_violations = 0_usize;
    let mut total_upgrades = 0_usize;
    let mut total_body_subs = 0_usize;
    let mut missing_original: Vec<String> = Vec::new();

    for entry in WalkDir::new(evo_root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().is_none_or(|e| e != "ing") {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(evo_root)
            .map_err(|e| anyhow!("{e}"))?;
        let xseed_path = xseed_root.join(rel);
        let orig_path = orig_root.join(rel);
        if !xseed_path.exists() || !orig_path.exists() {
            files_missing += 1;
            continue;
        }
        let rel_display = rel.to_string_lossy().replace('\\', "/");

        let evo_src = std::fs::read_to_string(entry.path())
            .with_context(|| format!("read {}", entry.path().display()))?;
        let xseed_src = std::fs::read_to_string(&xseed_path)
            .with_context(|| format!("read {}", xseed_path.display()))?;
        let orig_src = std::fs::read_to_string(&orig_path)
            .with_context(|| format!("read {}", orig_path.display()))?;
        let evo = parse_ing(&evo_src).with_context(|| format!("parse EVO {rel_display}"))?;
        let xseed = parse_ing(&xseed_src).with_context(|| format!("parse Xseed {rel_display}"))?;
        let orig = parse_ing(&orig_src).with_context(|| format!("parse original {rel_display}"))?;

        let report = verify_scena(&evo, &xseed, &orig);
        files_checked += 1;
        functions_checked += report.functions_checked;
        occurrences_checked += report.occurrences_checked;
        localized += report.localized;

        for v in &report.violations {
            report_violation(&rel_display, v);
        }
        total_violations += report.violations.len();
        for u in &report.upgrades {
            println!(
                "[upgrade] {rel_display}  {func}  {site}  {key}  (occ={occ})",
                func = u.function,
                site = fmt_site(u.site),
                key = fmt_key(&u.key),
                occ = u.occurrences,
            );
        }
        total_upgrades += report.upgrades.len();
        for b in &report.body_subs {
            println!(
                "[body-sub] {rel_display}  {func}  (evo body={kind})",
                func = b.function,
                kind = b.evo_body_kind,
            );
        }
        total_body_subs += report.body_subs.len();
        for f in report.missing_original {
            missing_original.push(format!("{rel_display}:{f}"));
        }
    }

    for f in &missing_original {
        println!("[missing-original] {f}");
    }

    println!("\n--- summary ---");
    println!("Files checked:             {files_checked}");
    println!("Files w/o triple:          {files_missing}");
    println!("Functions checked:         {functions_checked}");
    println!("Occurrences checked:       {occurrences_checked}");
    println!("Occurrences localized:     {localized}");
    println!("Anchor-shape upgrades:     {total_upgrades}");
    println!("Body substitutions:        {total_body_subs}");
    println!("Missing-original fns:      {}", missing_original.len());
    println!("Violations:                {total_violations}");

    if total_violations > 0 {
        anyhow::bail!("{total_violations} delta-invariant violation(s) found");
    }
    println!(
        "\nDelta invariant holds: merge changed text exactly where Xseed differs from original."
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
