#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    reason = "tests panic on assertion failure by design"
)]

use sora_remake_merge::parse_ing;
use sora_remake_merge::print_ing;
use sora_remake_merge::swap_scena;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

const EVO_MP1010_04: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../resources/evo-voice-mod/script_en/scena/mp1010_04.ing"
);
const XSEED_MP1010_04: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../resources/xseed-restoration/script_en/scena/mp1010_04.ing"
);
const EVO_MP0010_05: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../resources/evo-voice-mod/script_en/scena/mp0010_05.ing"
);
const XSEED_MP0010_05: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../resources/xseed-restoration/script_en/scena/mp0010_05.ing"
);
const ORIGINAL_MP1010_04: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../resources/original/script_en/scena/mp1010_04.ing"
);
const INGERT_ENV: &str = "INGERT_EXE";

fn ingert_exe() -> PathBuf {
    let path = std::env::var(INGERT_ENV).unwrap_or_else(|_| {
        panic!(
            "{INGERT_ENV} is not set. Point it at the ingert.exe built from \
             https://github.com/kvnxiao/ingert-sora1 (e.g. \
             INGERT_EXE=C:/path/to/Ingert/target/release/ingert.exe)."
        )
    });
    PathBuf::from(path)
}

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "read {path}: {e}\n\
             hint: .ing fixtures are gitignored. Regenerate from .dat with:\n  \
             python scripts/dat2ing.py resources/evo-voice-mod\n  \
             python scripts/dat2ing.py resources/xseed-restoration\n  \
             python scripts/dat2ing.py resources/original"
        )
    })
}

fn roundtrip_stable(path: &str) {
    let src = read(path);
    let scena1 = parse_ing(&src).expect("parse 1");
    let out1 = print_ing(&scena1);
    let scena2 = parse_ing(&out1).expect("parse 2");
    let out2 = print_ing(&scena2);
    assert_eq!(out1, out2, "{path}: print(parse(...)) is not stable");
}

#[test]
fn evo_mp1010_04_roundtrip_stable() {
    roundtrip_stable(EVO_MP1010_04);
}

#[test]
fn xseed_mp1010_04_roundtrip_stable() {
    roundtrip_stable(XSEED_MP1010_04);
}

#[test]
fn evo_mp0010_05_roundtrip_stable() {
    roundtrip_stable(EVO_MP0010_05);
}

#[test]
fn xseed_mp0010_05_roundtrip_stable() {
    roundtrip_stable(XSEED_MP0010_05);
}

fn apply_swap(evo_path: &str, xseed_path: &str) -> String {
    let evo_src = read(evo_path);
    let xseed_src = read(xseed_path);
    let mut evo = parse_ing(&evo_src).expect("evo parse");
    let xseed = parse_ing(&xseed_src).expect("xseed parse");
    let stats = swap_scena(&mut evo, &xseed);
    assert!(
        stats.swaps_applied > 0,
        "expected at least one swap in {evo_path}, got {stats:?}"
    );
    print_ing(&evo)
}

fn apply_swap_mp1010_04() -> String {
    apply_swap(EVO_MP1010_04, XSEED_MP1010_04)
}

#[test]
fn lugran_yes_from_aina_swapped_both_occurrences() {
    let out = apply_swap_mp1010_04();
    assert!(
        out.contains("<P2>Yes, I received a call from Aina"),
        "missing XSeed Lugran wording"
    );
    assert!(
        out.contains("not that long ago."),
        "missing XSeed Lugran continuation"
    );
    assert!(
        !out.contains("<P2>Yes, from Aina."),
        "old EVO wording still present"
    );
    assert!(out.contains("11, 33247"), "voice id 11, 33247 must survive");
    let count = out.matches("<P2>Yes, I received a call from Aina").count();
    assert!(count >= 2, "expected >=2 occurrences, got {count}");
}

#[test]
fn joshua_jurisdictional_disputes_swapped() {
    let out = apply_swap_mp1010_04();
    assert!(out.contains("jurisdictional"), "missing XSeed wording");
    assert!(
        !out.contains("In other words, this is a power"),
        "old EVO Joshua wording still present"
    );
    assert!(
        out.contains("11, 60589"),
        "EVO voice id 11, 60589 must survive"
    );
}

#[test]
fn estelle_general_morgan_swapped() {
    let out = apply_swap_mp1010_04();
    assert!(
        out.contains("<P1>General Morgan? Who's that?"),
        "missing XSeed Estelle wording"
    );
    assert!(
        !out.contains("<P1>Who's this General Morgan guy?"),
        "old EVO Estelle wording still present"
    );
    assert!(
        out.contains("11, 60593"),
        "EVO voice id 11, 60593 must survive"
    );
}

#[test]
fn cassius_letter_voiced_swapped() {
    let out = apply_swap_mp1010_04();
    assert!(
        out.contains("'Dear Estelle and Joshua,'"),
        "missing XSeed Cassius letter quoting style"
    );
    for v in 34832..=34844 {
        let needle = format!(", {v},");
        assert!(out.contains(&needle), "voice id {v} must survive in output");
    }
}

fn assert_idempotent(evo_path: &str, xseed_path: &str) {
    let evo_src = read(evo_path);
    let xseed_src = read(xseed_path);
    let mut evo = parse_ing(&evo_src).expect("evo parse");
    let xseed = parse_ing(&xseed_src).expect("xseed parse");

    let _ = swap_scena(&mut evo, &xseed);
    let first_print = print_ing(&evo);

    let mut evo2 = parse_ing(&first_print).expect("reparse");
    let stats2 = swap_scena(&mut evo2, &xseed);
    let second_print = print_ing(&evo2);

    assert_eq!(
        stats2.swaps_applied, 0,
        "{evo_path}: second run should be no-op"
    );
    assert_eq!(
        first_print, second_print,
        "{evo_path}: second run must be byte-identical"
    );
}

#[test]
fn idempotent_mp1010_04() {
    assert_idempotent(EVO_MP1010_04, XSEED_MP1010_04);
}

#[test]
fn idempotent_mp0010_05() {
    assert_idempotent(EVO_MP0010_05, XSEED_MP0010_05);
}

#[test]
fn xseed_corpus_untouched_by_swap() {
    let xseed_before = read(XSEED_MP1010_04);
    let _ = apply_swap_mp1010_04();
    let xseed_after = read(XSEED_MP1010_04);
    assert_eq!(xseed_before, xseed_after);
}

#[test]
fn original_corpus_untouched_by_swap() {
    if !Path::new(ORIGINAL_MP1010_04).exists() {
        return;
    }
    let before = read(ORIGINAL_MP1010_04);
    let _ = apply_swap_mp1010_04();
    let after = read(ORIGINAL_MP1010_04);
    assert_eq!(before, after);
}

#[test]
fn mp0010_05_swap_applies() {
    let out = apply_swap(EVO_MP0010_05, XSEED_MP0010_05);
    assert!(!out.is_empty());
    let scena = parse_ing(&out).expect("output reparses");
    assert!(!scena.functions.is_empty(), "expected functions");
}

fn ingert_recompile(ing_path: &Path) -> bool {
    let exe = ingert_exe();
    assert!(
        exe.is_file(),
        "{INGERT_ENV} points to {} which does not exist",
        exe.display()
    );
    let dat_path = ing_path.with_extension("dat");
    let _ = fs::remove_file(&dat_path);
    let status = Command::new(&exe)
        .args(["-o", dat_path.to_str().unwrap(), ing_path.to_str().unwrap()])
        .status()
        .expect("spawn ingert.exe");
    status.success() && dat_path.exists()
}

fn write_tmp_ing(name: &str, contents: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("sora-remake-merge-{name}.ing"));
    fs::write(&tmp, contents).expect("write tmp ing");
    tmp
}

#[test]
fn mp0010_05_output_recompiles_via_ingert() {
    // This file was the motivation for forking Ingert. If the fork fix regresses,
    // either the parser, the printer, or the compiler will reject the output.
    let out = apply_swap(EVO_MP0010_05, XSEED_MP0010_05);
    let tmp = write_tmp_ing("mp0010_05", &out);
    let ok = ingert_recompile(&tmp);
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(tmp.with_extension("dat"));
    assert!(ok, "ingert.exe failed to recompile mp0010_05 output");
}

#[test]
fn mp1010_04_output_recompiles_via_ingert() {
    let out = apply_swap_mp1010_04();
    let tmp = write_tmp_ing("mp1010_04", &out);
    let ok = ingert_recompile(&tmp);
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(tmp.with_extension("dat"));
    assert!(ok, "ingert.exe failed to recompile mp1010_04 output");
}
