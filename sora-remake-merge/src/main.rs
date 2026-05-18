use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use sora_remake_merge::{SwapStats, parse_ing, print_ing, swap_scena};
use walkdir::WalkDir;

const DEFAULT_EVO: &str = "resources/evo-voice-mod";
const DEFAULT_XSEED: &str = "resources/xseed-restoration";
const DEFAULT_OUT: &str = "output";

#[derive(Parser, Debug)]
#[command(
    name = "sora-remake-merge",
    about = "Merge XSeed English text into EVO Voice mod .ing scripts"
)]
struct Cli {
    /// EVO source (file or directory). Default: `resources/evo-voice-mod`
    #[arg(long, default_value = DEFAULT_EVO)]
    evo: PathBuf,

    /// XSeed source (file or directory). Default: `resources/xseed-restoration`
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
        anyhow::bail!("XSeed path does not exist: {}", cli.xseed.display());
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
            .map_err(|e| anyhow!("strip_prefix({}, {}): {e}", entry.path().display(), evo_root.display()))?;
        let xseed_path = xseed_root.join(rel);
        if !xseed_path.exists() {
            files_missing_xseed += 1;
            continue;
        }
        let out_path = out_root.join(rel);
        let stats = run_file(entry.path(), &xseed_path, &out_path, dry_run)
            .with_context(|| format!("processing {}", rel.display()))?;
        if stats.swaps_applied > 0 {
            files_changed += 1;
        }
        merge(&mut total, &stats);
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
    }

    println!(
        "Processed {files_processed} files, {files_changed} changed, \
         {files_missing_xseed} XSeed-missing skipped"
    );
    println!(
        "Swaps: {applied} applied, {noops} no-ops, {unmatched} unmatched, {overflow} overflow",
        applied = total.swaps_applied,
        noops = total.no_ops_equal,
        unmatched = total.unmatched_evo_calls,
        overflow = total.overflow_reuses,
    );
    if dry_run {
        println!("(dry run; nothing written)");
    } else {
        println!("Output: {}", out_root.display());
    }
    Ok(())
}

fn run_file(evo: &Path, xseed: &Path, out: &Path, dry_run: bool) -> Result<SwapStats> {
    let evo_src = std::fs::read_to_string(evo)
        .with_context(|| format!("failed to read EVO file {}", evo.display()))?;
    let xseed_src = std::fs::read_to_string(xseed)
        .with_context(|| format!("failed to read XSeed file {}", xseed.display()))?;

    let mut evo_scena = parse_ing(&evo_src)
        .with_context(|| format!("failed to parse EVO file {}", evo.display()))?;
    let xseed_scena = parse_ing(&xseed_src)
        .with_context(|| format!("failed to parse XSeed file {}", xseed.display()))?;

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

fn merge(total: &mut SwapStats, add: &SwapStats) {
    total.swaps_applied += add.swaps_applied;
    total.no_ops_equal += add.no_ops_equal;
    total.unmatched_evo_calls += add.unmatched_evo_calls;
    total.overflow_reuses += add.overflow_reuses;
}
