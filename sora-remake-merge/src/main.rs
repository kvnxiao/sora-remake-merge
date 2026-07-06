use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use sora_remake_merge::AnchorKey;
use sora_remake_merge::BodySubstitutionEntry;
use sora_remake_merge::OverflowEntry;
use sora_remake_merge::Site;
use sora_remake_merge::SwapStats;
use sora_remake_merge::TextChunk;
use sora_remake_merge::UnmatchedEntry;
use sora_remake_merge::parse_ing;
use sora_remake_merge::print_ing;
use sora_remake_merge::swap_scena;
use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use walkdir::WalkDir;

const DEFAULT_EVO: &str = "resources/evo-voice-mod";
const DEFAULT_XSEED: &str = "resources/xseed-restoration";
const DEFAULT_OUT: &str = "output";

#[derive(Parser, Debug)]
#[command(
    name = "sora-remake-merge",
    about = "Merge Xseed English text into EVO Voice mod .ing scripts"
)]
struct Cli {
    /// EVO source (file or directory). Default: `resources/evo-voice-mod`
    #[arg(long, default_value = DEFAULT_EVO)]
    evo: PathBuf,

    /// Xseed source (file or directory). Default: `resources/xseed-restoration`
    #[arg(long, default_value = DEFAULT_XSEED)]
    xseed: PathBuf,

    /// Output destination. Default: `output/`
    ///
    /// If EVO is a directory, merged files are written under here mirroring
    /// the relative tree. If EVO is a single file, this is the output file
    /// path.
    #[arg(long, short, default_value = DEFAULT_OUT)]
    out: PathBuf,

    /// Parse and compute changes, do not write
    #[arg(long)]
    dry_run: bool,

    /// Log per-file swap counts to stderr
    #[arg(long, short)]
    verbose: bool,
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    if !cli.evo.exists() {
        anyhow::bail!("EVO path does not exist: {}", cli.evo.display());
    }
    if !cli.xseed.exists() {
        anyhow::bail!("Xseed path does not exist: {}", cli.xseed.display());
    }
    if cli.evo.is_dir() {
        run_dir(&cli.evo, &cli.xseed, &cli.out, cli.dry_run, cli.verbose)
    } else {
        let stats = run_file(&cli.evo, &cli.xseed, &cli.out, cli.dry_run)?;
        report_file(&cli.evo, &stats);
        Ok(())
    }
}

fn run_dir(
    evo_root: &Path,
    xseed_root: &Path,
    out_root: &Path,
    dry_run: bool,
    verbose: bool,
) -> Result<()> {
    let mut total = SwapStats::default();
    let mut files_processed = 0_usize;
    let mut files_changed = 0_usize;
    let mut files_missing_xseed = 0_usize;
    let mut audit_unmatched: Vec<(String, UnmatchedEntry)> = Vec::new();
    let mut audit_overflows: Vec<(String, OverflowEntry)> = Vec::new();
    let mut audit_body_subs: Vec<(String, BodySubstitutionEntry)> = Vec::new();

    for entry in WalkDir::new(evo_root).into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path().extension().is_none_or(|e| e != "ing") {
            continue;
        }
        let rel = entry.path().strip_prefix(evo_root).map_err(|e| {
            anyhow!(
                "strip_prefix({}, {}): {e}",
                entry.path().display(),
                evo_root.display()
            )
        })?;
        let xseed_path = xseed_root.join(rel);
        if !xseed_path.exists() {
            files_missing_xseed += 1;
            continue;
        }
        let out_path = out_root.join(rel);
        let mut stats = run_file(entry.path(), &xseed_path, &out_path, dry_run)
            .with_context(|| format!("processing {}", rel.display()))?;
        if stats.swaps_applied > 0 {
            files_changed += 1;
        }
        let rel_display = rel.to_string_lossy().replace('\\', "/");
        for e in stats.unmatched.drain(..) {
            audit_unmatched.push((rel_display.clone(), e));
        }
        for e in stats.overflows.drain(..) {
            audit_overflows.push((rel_display.clone(), e));
        }
        for e in stats.body_subs.drain(..) {
            audit_body_subs.push((rel_display.clone(), e));
        }
        files_processed += 1;
        if verbose {
            eprintln!(
                "{rel}: {applied} swaps, {unmatched} unmatched, {overflow} overflow",
                rel = rel.display(),
                applied = stats.swaps_applied,
                unmatched = stats.unmatched_evo_calls,
                overflow = stats.overflow_reuses,
            );
        }
        total.merge(stats);
    }

    println!(
        "Processed {files_processed} files, {files_changed} changed, \
         {files_missing_xseed} Xseed-missing skipped"
    );
    println!(
        "Swaps: {applied} applied, {noops} no-ops, {unmatched} unmatched, \
         {overflow} overflow, {fallback} voiced→letter fallbacks, \
         {subs} body substitutions ({reinjected} voice IDs re-injected)",
        applied = total.swaps_applied,
        noops = total.no_ops_equal,
        unmatched = total.unmatched_evo_calls,
        overflow = total.overflow_reuses,
        fallback = total.voiced_to_letter_fallback,
        subs = total.body_substitutions,
        reinjected = total.voice_ids_reinjected,
    );
    if dry_run {
        println!("(dry run; nothing written)");
    } else {
        write_audit(
            out_root,
            &audit_unmatched,
            &audit_overflows,
            &audit_body_subs,
        )?;
        println!("Output: {}", out_root.display());
    }
    Ok(())
}

fn write_audit(
    out_root: &Path,
    unmatched: &[(String, UnmatchedEntry)],
    overflows: &[(String, OverflowEntry)],
    body_subs: &[(String, BodySubstitutionEntry)],
) -> Result<()> {
    let audit_dir = out_root.join("_audit");
    std::fs::create_dir_all(&audit_dir)
        .with_context(|| format!("failed to create audit dir {}", audit_dir.display()))?;

    let mut unmatched_tsv = String::from("file\tfunction\tsite\tline\tkey\tevo_text\n");
    for (file, e) in unmatched {
        writeln!(
            unmatched_tsv,
            "{file}\t{fn_name}\t{site}\t{line}\t{key}\t{text}",
            fn_name = tsv_escape(&e.function),
            site = fmt_site(e.site),
            line = fmt_line(e.line),
            key = fmt_key(&e.key),
            text = fmt_run(&e.evo_run),
        )
        .context("write to in-memory String")?;
    }
    let unmatched_path = audit_dir.join("unmatched.tsv");
    std::fs::write(&unmatched_path, unmatched_tsv)
        .with_context(|| format!("failed to write {}", unmatched_path.display()))?;

    let mut overflow_tsv =
        String::from("file\tfunction\tsite\tline\tkey\tevo_text\treused_xseed_text\n");
    for (file, e) in overflows {
        writeln!(
            overflow_tsv,
            "{file}\t{fn_name}\t{site}\t{line}\t{key}\t{evo}\t{reused}",
            fn_name = tsv_escape(&e.function),
            site = fmt_site(e.site),
            line = fmt_line(e.line),
            key = fmt_key(&e.key),
            evo = fmt_run(&e.evo_run),
            reused = fmt_run(&e.reused_run),
        )
        .context("write to in-memory String")?;
    }
    let overflow_path = audit_dir.join("overflow.tsv");
    std::fs::write(&overflow_path, overflow_tsv)
        .with_context(|| format!("failed to write {}", overflow_path.display()))?;

    let mut body_subs_tsv = String::from("file\tfunction\tevo_body_kind\tvoice_ids_reinjected\n");
    for (file, e) in body_subs {
        writeln!(
            body_subs_tsv,
            "{file}\t{fn_name}\t{kind}\t{reinjected}",
            fn_name = tsv_escape(&e.function),
            kind = e.evo_body_kind,
            reinjected = e.voice_ids_reinjected,
        )
        .context("write to in-memory String")?;
    }
    let body_subs_path = audit_dir.join("body_substitutions.tsv");
    std::fs::write(&body_subs_path, body_subs_tsv)
        .with_context(|| format!("failed to write {}", body_subs_path.display()))?;

    println!(
        "Audit: {u} unmatched, {o} overflow, {s} body subs → {dir}",
        u = unmatched.len(),
        o = overflows.len(),
        s = body_subs.len(),
        dir = audit_dir.display(),
    );
    Ok(())
}

fn fmt_site(site: Site) -> &'static str {
    match site {
        Site::Body => "body",
        Site::Called => "called",
    }
}

fn fmt_line(line: Option<u16>) -> String {
    line.map_or_else(String::new, |n| n.to_string())
}

fn fmt_key(key: &AnchorKey) -> String {
    match key {
        AnchorKey::Portrait { char_id, tag } => {
            format!("Portrait(char_id={char_id}, tag={})", tsv_escape(tag))
        }
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

fn fmt_run(run: &[TextChunk]) -> String {
    let mut out = String::new();
    for chunk in run {
        match chunk {
            TextChunk::Str(s) => out.push_str(&tsv_escape(s)),
            TextChunk::Newline => out.push_str("\\n"),
        }
    }
    out
}

fn tsv_escape(s: &str) -> String {
    s.replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn run_file(evo: &Path, xseed: &Path, out: &Path, dry_run: bool) -> Result<SwapStats> {
    let evo_src = std::fs::read_to_string(evo)
        .with_context(|| format!("failed to read EVO file {}", evo.display()))?;
    let xseed_src = std::fs::read_to_string(xseed)
        .with_context(|| format!("failed to read Xseed file {}", xseed.display()))?;

    let mut evo_scena = parse_ing(&evo_src)
        .with_context(|| format!("failed to parse EVO file {}", evo.display()))?;
    let xseed_scena = parse_ing(&xseed_src)
        .with_context(|| format!("failed to parse Xseed file {}", xseed.display()))?;

    let stats = swap_scena(&mut evo_scena, &xseed_scena);

    if !dry_run {
        if let Some(parent) = out.parent()
            && !parent.as_os_str().is_empty()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create output dir {}", parent.display()))?;
        }
        let printed = print_ing(&evo_scena);
        std::fs::write(out, printed)
            .with_context(|| format!("failed to write output {}", out.display()))?;
    }

    Ok(stats)
}

fn report_file(path: &Path, stats: &SwapStats) {
    println!(
        "{path}: {applied} swaps, {noops} no-ops, {unmatched} unmatched, {overflow} overflow",
        path = path.display(),
        applied = stats.swaps_applied,
        noops = stats.no_ops_equal,
        unmatched = stats.unmatched_evo_calls,
        overflow = stats.overflow_reuses,
    );
}
