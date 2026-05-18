//! Per-function body syscall comparison between EVO and XSeed.
//!
//! The companion to `compare-original`. Where that binary verifies EVO does
//! not add new lines vs the GungHo baseline, this one verifies the merge can
//! actually reach every EVO line — flagging any function that would be
//! silently skipped by `swap_scena` because its XSeed counterpart is missing
//! or its body cannot be walked (`Body::Asm` / `Body::Flat`).
//!
//! Reports:
//!   * Functions in EVO with no XSeed counterpart (merge leaves byte-identical)
//!   * Functions whose body-syscall count differs between EVO and XSeed
//!   * Functions whose anchor distribution differs (Letter→Voiced, etc.)
//!   * `Body::Asm` / `Body::Flat` bodies in either side
//!
//! 0 net diffs on a clean run means the merge has full coverage.

#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::too_many_lines,
    reason = "this is a CLI analysis tool"
)]

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use ingert::scena::Body;
use ingert::scena::Expr;
use ingert::scena::Function;
use ingert::scena::Stmt;
use sora_remake_merge::AnchorKey;
use sora_remake_merge::classify_syscall_expr;
use sora_remake_merge::parse_ing;
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;
use walkdir::WalkDir;

const EVO_ROOT: &str = "resources/evo-voice-mod/script_en/scena";
const XSEED_ROOT: &str = "resources/xseed-restoration/script_en/scena";

#[derive(Default, Debug, Clone)]
struct FnCounts {
    by_anchor: HashMap<AnchorKind, usize>,
    total: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum AnchorKind {
    Portrait,
    Voiced,
    Letter,
    Plain,
}

impl AnchorKind {
    fn from(key: &AnchorKey) -> Self {
        match key {
            AnchorKey::Portrait { .. } => Self::Portrait,
            AnchorKey::Voiced(_) => Self::Voiced,
            AnchorKey::Letter => Self::Letter,
            AnchorKey::Plain => Self::Plain,
        }
    }
}

fn walk_body(stmts: &[Stmt], out: &mut FnCounts) {
    for stmt in stmts {
        walk_stmt(stmt, out);
    }
}

fn walk_stmt(stmt: &Stmt, out: &mut FnCounts) {
    match stmt {
        Stmt::Expr(e) | Stmt::Set(_, _, e) => walk_expr(e, out),
        Stmt::Return(_, e) | Stmt::PushVar(_, _, e) => {
            if let Some(e) = e {
                walk_expr(e, out);
            }
        }
        Stmt::If(_, cond, then, els) => {
            walk_expr(cond, out);
            walk_body(then, out);
            if let Some(els) = els {
                walk_body(els, out);
            }
        }
        Stmt::While(_, cond, body) => {
            walk_expr(cond, out);
            walk_body(body, out);
        }
        Stmt::Switch(_, scrut, cases) => {
            walk_expr(scrut, out);
            for arm in cases.values() {
                walk_body(arm, out);
            }
        }
        Stmt::Block(stmts) => walk_body(stmts, out),
        Stmt::Debug(_, args) | Stmt::Tailcall(_, _, args) => {
            for arg in args {
                walk_expr(arg, out);
            }
        }
        Stmt::Break | Stmt::Continue => {}
    }
}

fn walk_expr(expr: &Expr, out: &mut FnCounts) {
    match expr {
        Expr::Syscall(_, a, b, args) => {
            for arg in args {
                walk_expr(arg, out);
            }
            if let Some(cls) = classify_syscall_expr(*a, *b, args) {
                *out.by_anchor.entry(AnchorKind::from(&cls.key)).or_default() += 1;
                out.total += 1;
            }
        }
        Expr::Call(_, _, args) => {
            for arg in args {
                walk_expr(arg, out);
            }
        }
        Expr::Unop(_, _, inner) => walk_expr(inner, out),
        Expr::Binop(_, _, l, r) => {
            walk_expr(l, out);
            walk_expr(r, out);
        }
        Expr::Value(_, _) | Expr::Var(_, _) | Expr::Ref(_, _) => {}
    }
}

fn body_kind(f: &Function) -> &'static str {
    match &f.body {
        Body::Tree(_) => "tree",
        Body::Flat(_) => "flat",
        Body::Asm(_) => "asm",
    }
}

fn count_fn(f: &Function) -> Option<FnCounts> {
    if let Body::Tree(stmts) = &f.body {
        let mut out = FnCounts::default();
        walk_body(stmts, &mut out);
        Some(out)
    } else {
        None
    }
}

fn fmt_anchor_dist(counts: &FnCounts) -> String {
    let p = counts
        .by_anchor
        .get(&AnchorKind::Portrait)
        .copied()
        .unwrap_or(0);
    let v = counts
        .by_anchor
        .get(&AnchorKind::Voiced)
        .copied()
        .unwrap_or(0);
    let l = counts
        .by_anchor
        .get(&AnchorKind::Letter)
        .copied()
        .unwrap_or(0);
    let pl = counts
        .by_anchor
        .get(&AnchorKind::Plain)
        .copied()
        .unwrap_or(0);
    format!("Portrait={p}, Voiced={v}, Letter={l}, Plain={pl}")
}

fn run() -> Result<()> {
    let evo_root = Path::new(EVO_ROOT);
    let xseed_root = Path::new(XSEED_ROOT);
    if !evo_root.exists() {
        anyhow::bail!("EVO root does not exist: {}", evo_root.display());
    }
    if !xseed_root.exists() {
        anyhow::bail!("XSeed root does not exist: {}", xseed_root.display());
    }

    let mut files_processed = 0_usize;
    let mut missing_xseed_files = 0_usize;
    let mut evo_only_fns = 0_usize;
    let mut count_diff_fns = 0_usize;
    let mut anchor_diff_fns = 0_usize;
    let mut non_tree_evo = 0_usize;
    let mut non_tree_xseed = 0_usize;
    let mut total_evo_lines = 0_usize;
    let mut total_xseed_lines = 0_usize;

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
        if !xseed_path.exists() {
            missing_xseed_files += 1;
            continue;
        }
        files_processed += 1;
        let evo_src = std::fs::read_to_string(entry.path())
            .with_context(|| format!("read {}", entry.path().display()))?;
        let xseed_src = std::fs::read_to_string(&xseed_path)
            .with_context(|| format!("read {}", xseed_path.display()))?;
        let evo_scena =
            parse_ing(&evo_src).with_context(|| format!("parse EVO {}", entry.path().display()))?;
        let xseed_scena = parse_ing(&xseed_src)
            .with_context(|| format!("parse XSeed {}", xseed_path.display()))?;

        for (name, evo_fn) in &evo_scena.functions {
            let Some(xseed_fn) = xseed_scena.functions.get(name) else {
                let evo_total = count_fn(evo_fn).map_or(0, |c| c.total);
                if evo_total > 0 {
                    evo_only_fns += 1;
                    println!(
                        "[evo-only fn] {rel}  {name}  (evo total={evo_total})",
                        rel = rel.display(),
                    );
                }
                continue;
            };
            let evo_kind = body_kind(evo_fn);
            let xseed_kind = body_kind(xseed_fn);
            if evo_kind != "tree" {
                non_tree_evo += 1;
                println!(
                    "[evo {evo_kind}] {rel}  {name}  (xseed={xseed_kind})",
                    rel = rel.display(),
                );
            }
            if xseed_kind != "tree" {
                non_tree_xseed += 1;
                println!(
                    "[xseed {xseed_kind}] {rel}  {name}  (evo={evo_kind})",
                    rel = rel.display(),
                );
            }
            let (Some(evo_c), Some(xseed_c)) = (count_fn(evo_fn), count_fn(xseed_fn)) else {
                continue;
            };
            total_evo_lines += evo_c.total;
            total_xseed_lines += xseed_c.total;
            let count_differs = evo_c.total != xseed_c.total;
            let anchors_differ = evo_c.by_anchor != xseed_c.by_anchor;
            if count_differs {
                count_diff_fns += 1;
                println!(
                    "[count diff] {rel}  {name}\n  evo:   total={} ({})\n  xseed: total={} ({})",
                    evo_c.total,
                    fmt_anchor_dist(&evo_c),
                    xseed_c.total,
                    fmt_anchor_dist(&xseed_c),
                    rel = rel.display(),
                );
            } else if anchors_differ {
                anchor_diff_fns += 1;
                println!(
                    "[anchor diff] {rel}  {name}  (total={})\n  evo:   {}\n  xseed: {}",
                    evo_c.total,
                    fmt_anchor_dist(&evo_c),
                    fmt_anchor_dist(&xseed_c),
                    rel = rel.display(),
                );
            }
        }
    }

    println!("\n--- summary ---");
    println!("Files processed:           {files_processed}");
    println!("Files w/o XSeed:           {missing_xseed_files}");
    println!("EVO-only fns (>0 lines):   {evo_only_fns}");
    println!("Functions w/ count diff:   {count_diff_fns}");
    println!("Functions w/ anchor diff:  {anchor_diff_fns}");
    println!("EVO non-tree bodies:       {non_tree_evo}");
    println!("XSeed non-tree bodies:     {non_tree_xseed}");
    println!("Total EVO body syscalls:   {total_evo_lines}");
    println!("Total XSeed body syscalls: {total_xseed_lines}");
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
