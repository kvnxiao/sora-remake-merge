#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::let_underscore_must_use,
    clippy::string_slice,
    reason = "tests panic on assertion failure by design"
)]

use sora_remake_merge::AnchorKey;
use sora_remake_merge::parse_ing;
use sora_remake_merge::print_ing;
use sora_remake_merge::swap_scena;
use sora_remake_merge::verify_scena;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

// Fixtures are committed `.dat` under tests/fixtures/, decompiled to `.ing` by
// `scripts/dat2ing.py` (the `.ing` are gitignored, like the resource corpora).
// They are copies of the EVO/Xseed/original corpora for the specific files the
// tests exercise, so the suite runs without the (untracked) resources/ tree.
const EVO_MP1010_04: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp1010_04.ing"
);
const XSEED_MP1010_04: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp1010_04.ing"
);
const EVO_MP0010_05: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp0010_05.ing"
);
const XSEED_MP0010_05: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp0010_05.ing"
);
const ORIGINAL_MP1010_04: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/original/mp1010_04.ing"
);
const EVO_MP3010_01: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp3010_01.ing"
);
const XSEED_MP3010_01: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp3010_01.ing"
);
const EVO_MP3030: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp3030.ing"
);
const XSEED_MP3030: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp3030.ing"
);
const EVO_MP0000_EV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp0000_ev.ing"
);
const XSEED_MP0000_EV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp0000_ev.ing"
);
const EVO_MP1000_EV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp1000_ev.ing"
);
const XSEED_MP1000_EV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp1000_ev.ing"
);
const EVO_MP1110: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp1110.ing"
);
const XSEED_MP1110: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp1110.ing"
);
const EVO_MP4000_EV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/evo-voice-mod/mp4000_ev.ing"
);
const XSEED_MP4000_EV: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/xseed-restoration/mp4000_ev.ing"
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
             hint: .ing fixtures are gitignored. Regenerate from the committed \
             .dat with:\n  \
             python scripts/dat2ing.py sora-remake-merge/tests/fixtures\n  \
             (or `just dat2ing`)"
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
        "missing Xseed Lugran wording"
    );
    assert!(
        out.contains("not that long ago."),
        "missing Xseed Lugran continuation"
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
    assert!(out.contains("jurisdictional"), "missing Xseed wording");
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
        "missing Xseed Estelle wording"
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
        "missing Xseed Cassius letter quoting style"
    );
    for v in 34832..=34844 {
        let needle = format!(", {v},");
        assert!(out.contains(&needle), "voice id {v} must survive in output");
    }
}

// === mp1010_04: Xseed v1.7 dialogue edits ===
//
// Xseed re-translated four Lugran/Estelle lines (the wording here is v1.7's).
// Three use `<#E` portraits; Lugran's 34793 uses a `<#L` portrait, which only
// merges once the classifier recognises non-`<#E` face sets.
#[test]
fn mp1010_04_v17_dialogue_swapped() {
    let out = apply_swap_mp1010_04();
    // Lugran 34793 ("<#L_0#G[2]#M_2#B_0>"): EVO "Wait..." -> Xseed v1.7 wording.
    assert!(
        out.contains("N-Now hold on... Maybe I'm not in"),
        "missing v1.7 Lugran wording (non-<#E portrait not merged?)"
    );
    assert!(out.contains("11, 34793"), "voice id 34793 must survive");
    // Estelle 34871 ("<#E_E#M_2#B_0>").
    assert!(
        out.contains("That voice sounds suspiciously"),
        "missing v1.7 Estelle wording"
    );
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

// Xseed v1.7 introduced inline `<C2>...</C>` colour markup in some dialogue
// (e.g. the tutorial NPC's glossary lines). The markup is opaque string
// content, so it must flow through the swap verbatim as part of the text run.
#[test]
fn mp0010_05_v17_color_markup_survives() {
    let out = apply_swap(EVO_MP0010_05, XSEED_MP0010_05);
    assert!(
        out.contains("<C2>'orbal energy.'</C>"),
        "missing Xseed v1.7 <C2> colour markup in merged dialogue"
    );
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

// === mp1010_04 EV_01_61_00: Letter→Voiced fallback ===
//
// EVO upgraded 2 Letter syscalls (Cassius letter follow-ups) to Voiced by
// inserting voice IDs 97068/97069. Xseed re-translated the text with the
// single-quote letter style. The merge should apply Xseed text via the
// Voiced→Letter fallback while preserving EVO's voice IDs.

#[test]
fn evo_letter_to_voiced_upgrade_swapped_via_fallback() {
    let evo_src = read(EVO_MP1010_04);
    let xseed_src = read(XSEED_MP1010_04);
    let mut evo = parse_ing(&evo_src).expect("evo parse");
    let xseed = parse_ing(&xseed_src).expect("xseed parse");
    let stats = swap_scena(&mut evo, &xseed);
    assert!(
        stats.voiced_to_letter_fallback >= 2,
        "expected >=2 Voiced→Letter fallbacks, got {}",
        stats.voiced_to_letter_fallback
    );
    let out = print_ing(&evo);
    assert!(
        out.contains("11, 97068"),
        "EVO voice id 97068 must survive in output"
    );
    assert!(
        out.contains("11, 97069"),
        "EVO voice id 97069 must survive in output"
    );
    assert!(
        out.contains("'I was able to secure the item the"),
        "missing Xseed letter-style translation for 97068"
    );
    assert!(
        out.contains("'Please ask Professor R to do an"),
        "missing Xseed letter-style translation for 97069"
    );
    assert!(
        !out.contains("\"I retrieved this item from that group.\""),
        "old EVO GungHo text still present at voiced line"
    );
}

// === mp3010_01: VoicedPlain song lyric + Body::Asm substitution ===
//
// QS308_01_00 has a song lyric where EVO upgraded Plain to VoicedPlain shape
// (65535, 11, V, "text"). The merge must apply Xseed's re-translated lyric
// while preserving voice ID 97064.
//
// QS300_01_00 has Body::Asm in EVO (ingert couldn't decompile it to Tree)
// but Body::Tree in Xseed. The merge substitutes Xseed's Tree body so the
// runtime executes Xseed text rather than GungHo text from EVO's asm
// bytecode. EVO added voice cues inside that asm body (not the calls-table),
// so the merge recovers each `11, V` pair from the bytecode and re-injects it
// into the clone — the substituted scene keeps Xseed's text AND EVO's voice.

#[test]
fn mp3010_01_voiced_plain_song_lyric_swapped() {
    let evo_src = read(EVO_MP3010_01);
    let xseed_src = read(XSEED_MP3010_01);
    let mut evo = parse_ing(&evo_src).expect("evo parse");
    let xseed = parse_ing(&xseed_src).expect("xseed parse");
    let _ = swap_scena(&mut evo, &xseed);
    let out = print_ing(&evo);
    assert!(
        out.contains("11, 97064,"),
        "EVO voice id 97064 must survive in VoicedPlain output"
    );
    // Xseed's re-translation contains "Ah, you" and "(3)1 cypress trees".
    assert!(
        out.contains("'Ah, you (3)1 cypress trees"),
        "missing Xseed translation for QS308_01_00 song lyric"
    );
    assert!(
        !out.contains("Atop the hill are 31 cypress trees. 3"),
        "old EVO GungHo lyric still present"
    );
}

#[test]
fn mp3010_01_asm_body_substituted() {
    let evo_src = read(EVO_MP3010_01);
    let xseed_src = read(XSEED_MP3010_01);
    let mut evo = parse_ing(&evo_src).expect("evo parse");
    let xseed = parse_ing(&xseed_src).expect("xseed parse");
    let stats = swap_scena(&mut evo, &xseed);
    assert_eq!(
        stats.body_substitutions, 1,
        "expected exactly 1 body substitution (QS300_01_00 asm→tree)"
    );
    let sub = stats
        .body_subs
        .iter()
        .find(|e| e.function == "QS300_01_00")
        .expect("QS300_01_00 should be in body_subs");
    assert_eq!(sub.evo_body_kind, "asm");
    // EVO voiced this cutscene inside the asm body; every cue must be recovered
    // and re-injected, not dropped.
    assert!(
        sub.voice_ids_reinjected > 0,
        "expected EVO voice cues to be re-injected into the substituted body"
    );
    assert_eq!(
        stats.voice_ids_reinjected, sub.voice_ids_reinjected,
        "the only body substitution accounts for all re-injected voice IDs"
    );
    // After substitution, the body should print without asm syntax, carry
    // Xseed's text, and keep EVO's voice cues.
    let out = print_ing(&evo);
    let fn_start = out
        .find("fn QS300_01_00")
        .expect("function should exist in output");
    let fn_end = out[fn_start..]
        .find("\nfn ")
        .map_or(out.len(), |o| fn_start + o);
    let fn_body = &out[fn_start..fn_end];
    assert!(
        !fn_body.contains(" asm {"),
        "asm body should have been replaced with tree body"
    );
    // Xseed text (the completion line) and an EVO voice cue both present.
    assert!(
        fn_body.contains("All of the books have been returned!"),
        "Xseed completion text should be present in the substituted body"
    );
    assert!(
        fn_body.contains("11, 78898,"),
        "EVO voice cue 78898 should be re-injected into the substituted body"
    );
}

// === mp3010_01: [5,8] narration (records/encyclopedia) ===
//
// The Zeiss orbal-records terminal stores entries via [5,8] narration shapes
// the classifier previously skipped — (65535, 26, 22, ...) and
// (65535, 16, 26, 22, ...). They carry no portrait or voice key, so they match
// positionally within their Narration prefix bucket. Xseed v1.7 re-translated
// several entries (e.g. "factory chief" → "Factory Chief"); the merge must now
// apply that text rather than leaving EVO's GungHo wording.
#[test]
fn mp3010_01_narration_records_swapped() {
    let out = apply_swap(EVO_MP3010_01, XSEED_MP3010_01);
    assert!(
        out.contains("becomes the first Factory Chief"),
        "missing Xseed v1.7 records wording (narration not merged?)"
    );
    assert!(
        !out.contains("becomes the first factory chief"),
        "old EVO GungHo records wording still present"
    );
}

// === mp3010_01 LP_Capel: menu_additem records-terminal topic headers ===
//
// The Zeiss orbal-records terminal builds its topic menu with `menu_additem`
// (a named prelude-alias call, like ui_mapname_effect — not a raw syscall).
// Xseed v1.7 retitled several headers: "[History]" -> "[Establishment]",
// "[Orbment]" -> "[Orbments]", "[Orbal Weapons]" -> "[Orbal Weaponry]", etc.
// All 11 differing headers live in fn LP_Capel, which aligns 1:1 between EVO
// and Xseed, so they match positionally; the trailing menu-index arg survives.
#[test]
fn mp3010_01_menu_records_headers_swapped() {
    let out = apply_swap(EVO_MP3010_01, XSEED_MP3010_01);
    for new in [
        "<c930>[Establishment]",
        "<c930>[Universal Tech]",
        "<c930>[Related Topics]",
        "<c930>[Orbments]",
        "<c930>[Orbal Weaponry]",
        "<c930>[Combustion Engine]",
        "<c930>[Haulage Vehicle]",
    ] {
        assert!(
            out.contains(new),
            "missing Xseed v1.7 records header {new:?}"
        );
    }
    for old in [
        "<c930>[History]",
        "<c930>[All Orbal Technology]",
        "<c930>[Other Information]",
        "<c930>[Orbment]",
        "<c930>[Orbal Weapons]",
        "<c930>[Internal Combustion Engine]",
        "<c930>[Orbal Automobile]",
    ] {
        assert!(
            !out.contains(old),
            "old EVO records header {old:?} still present"
        );
    }
    // The char_id and trailing menu-index arg survive the label swap.
    assert!(
        out.contains("menu_additem(1, \"<c930>[Establishment]\", 0)"),
        "menu_additem char_id/index not preserved through the swap"
    );
}

#[test]
fn idempotent_mp3010_01() {
    assert_idempotent(EVO_MP3010_01, XSEED_MP3010_01);
}

#[test]
fn mp3010_01_output_recompiles_via_ingert() {
    let out = apply_swap(EVO_MP3010_01, XSEED_MP3010_01);
    let tmp = write_tmp_ing("mp3010_01", &out);
    let ok = ingert_recompile(&tmp);
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(tmp.with_extension("dat"));
    assert!(
        ok,
        "ingert.exe failed to recompile mp3010_01 output (asm→tree substitution may have broken)"
    );
}

// === mp3030: ui_mapname_effect (system[22,38]) zone-label merge ===
//
// Xseed v1.5 retitled "Kaldia Limestone Cave" to "Limestone Cave". The
// on-screen zone label is a named prelude-alias call (`ui_mapname_effect`), not
// a raw syscall, and its string is followed by numeric coordinates that must
// survive the swap. mp3030 carries the call in both the called-table metadata
// and the body, so both occurrences must swap.

#[test]
fn mp3030_mapname_zone_retitle_swapped() {
    let out = apply_swap(EVO_MP3030, XSEED_MP3030);
    assert!(
        out.contains("ui_mapname_effect(\"Limestone Cave\""),
        "missing Xseed v1.5 zone retitle"
    );
    // Only the map *label* is renamed; "Kaldia Limestone Cave" legitimately
    // survives in unchanged dialogue ("So... the Kaldia Limestone Cave."), so
    // assert specifically that no map-name call keeps the old label.
    assert!(
        !out.contains("ui_mapname_effect(\"Kaldia Limestone Cave\""),
        "old zone label still present on a ui_mapname_effect call"
    );
    assert!(
        out.contains("ui_mapname_effect(\"Limestone Cave\", 110.0, 505.0, 5.0)"),
        "map-name coordinates were not preserved through the swap"
    );
    let count = out.matches("ui_mapname_effect(\"Limestone Cave\"").count();
    assert!(
        count >= 2,
        "expected >=2 swapped occurrences (metadata + body), got {count}"
    );
}

#[test]
fn idempotent_mp3030() {
    assert_idempotent(EVO_MP3030, XSEED_MP3030);
}

#[test]
fn mp3030_output_recompiles_via_ingert() {
    let out = apply_swap(EVO_MP3030, XSEED_MP3030);
    let tmp = write_tmp_ing("mp3030", &out);
    let ok = ingert_recompile(&tmp);
    let _ = fs::remove_file(&tmp);
    let _ = fs::remove_file(tmp.with_extension("dat"));
    assert!(ok, "ingert.exe failed to recompile mp3030 output");
}

// === Xseed v1.5 zone retitles across the remaining affected files ===
//
// Each of these files changed only a ui_mapname_effect label in v1.5. Assert
// the new label is on the merged map-name call and the old one is gone from it.
fn assert_mapname_retitle(evo_path: &str, xseed_path: &str, old_label: &str, new_label: &str) {
    let out = apply_swap(evo_path, xseed_path);
    let new_call = format!("ui_mapname_effect(\"{new_label}\"");
    let old_call = format!("ui_mapname_effect(\"{old_label}\"");
    assert!(
        out.contains(&new_call),
        "{evo_path}: missing v1.5 zone label {new_label:?}"
    );
    assert!(
        !out.contains(&old_call),
        "{evo_path}: old zone label {old_label:?} still on a map-name call"
    );
}

#[test]
fn mp0000_ev_mapname_retitle() {
    assert_mapname_retitle(
        EVO_MP0000_EV,
        XSEED_MP0000_EV,
        "Jade Tower",
        "Esmelas Tower",
    );
}

#[test]
fn mp1000_ev_mapname_retitle() {
    assert_mapname_retitle(
        EVO_MP1000_EV,
        XSEED_MP1000_EV,
        "Amber Tower",
        "Amberl Tower",
    );
}

#[test]
fn mp1110_mapname_retitle() {
    assert_mapname_retitle(
        EVO_MP1110,
        XSEED_MP1110,
        "Sky Pirate Stronghold",
        "Sky Bandit Stronghold",
    );
}

#[test]
fn mp4000_ev_mapname_retitle() {
    assert_mapname_retitle(
        EVO_MP4000_EV,
        XSEED_MP4000_EV,
        "Royal Capital Grancel",
        "City of Grancel",
    );
}

// === chr_set_display_name: combined-party speaker labels ===
//
// chr_set_display_name(char_id, "name") is a named prelude alias that sets the
// dialogue-box speaker label. It is matched positionally within the
// (function, char_id) bucket, and only for concrete-int char_ids (a Var slot is
// left alone). Xseed rephrased two combined-party labels: mp0000_ev's
// "Lonnie, Dino, & Lyle" (dropping the serial comma) on char_ids 10066/10068,
// and mp4000_ev's "Scherazard, Kloe, & Estelle" ("&" → "and") on char_id 0. The
// char_id is preserved through the swap.
#[test]
fn mp0000_ev_display_name_group_label_swapped() {
    let out = apply_swap(EVO_MP0000_EV, XSEED_MP0000_EV);
    assert!(
        out.contains("chr_set_display_name(10066, \"Lonnie, Dino & Lyle\")"),
        "missing Xseed group label on char 10066"
    );
    assert!(
        out.contains("chr_set_display_name(10068, \"Lonnie, Dino & Lyle\")"),
        "missing Xseed group label on char 10068"
    );
    assert!(
        !out.contains("Lonnie, Dino, & Lyle"),
        "old EVO serial-comma label still present"
    );
}

#[test]
fn mp4000_ev_display_name_group_label_swapped() {
    let out = apply_swap(EVO_MP4000_EV, XSEED_MP4000_EV);
    assert!(
        out.contains("chr_set_display_name(0, \"Scherazard, Kloe, and Estelle\")"),
        "missing Xseed group label on char 0"
    );
    assert!(
        !out.contains("Scherazard, Kloe, & Estelle"),
        "old EVO ampersand label still present"
    );
}

// === mp1110: portrait-less narrator (Untagged) + voice-ID-after-portrait ===
//
// Two `[5,0]`/`[5,6]` coverage paths that earlier passed through unmerged:
//
//  1. Portrait-less narrator — `system[5,6](65535, "<C1>...")` system text with
//     no `<#…>` portrait tag. With no per-call key it matches positionally
//     within the Untagged{Some(65535)} bucket, per function. EV_01_53_00,
//     EV_01_55_00, EV_01_56_00, and SB_01_01_00 each re-translate one such line
//     (EVO repeats each across calls-table/body, so all copies must swap).
//
//  2. Voice-ID-after-portrait — EV_01_60_00's Bose line places the `11, 34731`
//     voice ID *after* the portrait tag (`(2, "<#E…>", 11, 34731, "text")`)
//     rather than before it. The text run must still swap and the voice ID must
//     survive in the preserved prefix.
#[test]
fn mp1110_portrait_less_narrator_swapped() {
    let out = apply_swap(EVO_MP1110, XSEED_MP1110);
    assert!(
        out.contains("<C1>The voices of some men can be heard."),
        "missing Xseed portrait-less narrator wording (men talking)"
    );
    assert!(
        out.contains("<C1>A familiar voice can be heard."),
        "missing Xseed portrait-less narrator wording (familiar voice)"
    );
    assert!(
        out.contains("<C1>There is a rock wall at the end of the passage."),
        "missing Xseed portrait-less narrator wording (rock wall)"
    );
    // Every EVO copy of each line must swap (none left behind).
    assert!(
        !out.contains("Men can be heard talking."),
        "old EVO narrator wording still present"
    );
    assert!(
        !out.contains("Familiar voices can be heard from the room."),
        "old EVO narrator wording still present"
    );
    assert!(
        !out.contains("The passage ends in a wall of rock."),
        "old EVO narrator wording still present"
    );
}

#[test]
fn mp1110_voice_id_after_portrait_swapped() {
    let out = apply_swap(EVO_MP1110, XSEED_MP1110);
    assert!(
        out.contains("In the meantime, let's get back to"),
        "missing Xseed wording for the voice-after-portrait Bose line"
    );
    assert!(
        !out.contains("For now, we should head back to the"),
        "old EVO wording still present at the voice-after-portrait line"
    );
    // The voice ID 34731 follows the portrait tag and must stay adjacent to it.
    assert!(
        out.contains("\"<#E_0#M_2#B_0>\", 11, 34731,"),
        "voice id 34731 must stay in the preserved prefix after the portrait"
    );
}

// === Localization-delta invariant (mp1010_04, all three corpora) ===
//
// Proves the merge applied *exactly* Xseed's text changes and nothing else: for
// every localizable call, the merged output differs from EVO iff Xseed differs
// from `original/`, and where they differ the output carries Xseed's text. This
// holds because EVO ships the GungHo text verbatim (EVO text == `original/`
// text on every shared line). See `sora_remake_merge::verify`.
//
// mp1010_04 is the only fixture present in all three corpora; it exercises the
// Portrait/Untagged/Voiced/Letter/Plain anchors plus the EV_01_61_00
// Letter→Voiced upgrade — the documented anchor-shape exemption, reported as an
// upgrade rather than a violation. The `Body::Asm` substitution
// (mp3010_01:QS300_01_00) and the duplicate-Portrait authoring artefact
// (mp2000_ev:EV_03_00_00) live in files with no `original/` fixture; they are
// covered by their dedicated tests and the full-corpus `verify-delta` binary.
#[test]
fn mp1010_04_delta_invariant_holds() {
    let evo = parse_ing(&read(EVO_MP1010_04)).expect("evo parse");
    let xseed = parse_ing(&read(XSEED_MP1010_04)).expect("xseed parse");
    let original = parse_ing(&read(ORIGINAL_MP1010_04)).expect("original parse");
    let report = verify_scena(&evo, &xseed, &original);

    // The headline invariant: no call changed where Xseed didn't localize, none
    // left stale where it did, and every change carries Xseed's exact text.
    assert!(
        report.violations.is_empty(),
        "delta-invariant violations: {:#?}",
        report.violations
    );
    // Confirm the merge actually did substantial work on this file (otherwise an
    // empty/no-op run would pass the invariant vacuously).
    assert!(report.functions_checked > 0, "no functions checked");
    assert!(report.occurrences_checked > 0, "no occurrences checked");
    assert!(report.localized > 0, "merge localized nothing");
    assert!(
        report.missing_original.is_empty(),
        "functions missing from original/: {:?}",
        report.missing_original
    );
    // This file has no Asm/Flat EVO body, so nothing should be body-substituted.
    assert!(
        report.body_subs.is_empty(),
        "unexpected body substitutions: {:?}",
        report.body_subs
    );

    // The only anchor-shape exemption here is the EV_01_61_00 Letter→Voiced
    // upgrade: EVO inserted voice IDs 97068/97069, so those occurrences have no
    // direct Xseed `Voiced` anchor and reach Xseed's Letter text through the
    // swap's Voiced→Letter fallback (verified by
    // `evo_letter_to_voiced_upgrade_swapped_via_fallback`).
    for u in &report.upgrades {
        assert_eq!(u.function, "EV_01_61_00", "unexpected upgrade fn: {u:?}");
        assert!(
            matches!(u.key, AnchorKey::Voiced(97068 | 97069)),
            "unexpected upgrade key: {u:?}"
        );
    }
    let voiced: std::collections::BTreeSet<i32> = report
        .upgrades
        .iter()
        .filter_map(|u| match u.key {
            AnchorKey::Voiced(v) => Some(v),
            _ => None,
        })
        .collect();
    assert_eq!(
        voiced,
        std::collections::BTreeSet::from([97068, 97069]),
        "expected exactly the 97068/97069 Letter→Voiced upgrades, got {:?}",
        report.upgrades
    );
}
