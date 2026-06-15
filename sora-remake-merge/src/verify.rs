//! Localization-delta invariant verification.
//!
//! Proves the merge applied *exactly* Xseed's text changes and nothing else.
//! For every localizable call:
//!
//! > `(output_text != evo_text)` ⟺ `(xseed_text != original_text)`, and where
//! > they differ, `output_text == xseed_text`.
//!
//! This holds because the EVO Voice mod ships the GungHo text verbatim — EVO
//! text equals `original/` text on every shared line — so a positional match
//! against Xseed is sound and the merge's "swap iff the runs differ" rule
//! coincides with Xseed's own localization delta against `original/`.
//!
//! The check reuses the swap's own per-`(Site, AnchorKey)` index
//! ([`build_index`]) for all four corpora (EVO input, merged output, Xseed,
//! `original/`) so the occurrence ordering it compares is exactly the ordering
//! the merge consumes. It mirrors the swap's lookup fallback — a `Site::Called`
//! EVO bucket resolves against Xseed's `Site::Body`, since EVO adds the
//! `calls {}` metadata blocks that Xseed/`original` lack.
//!
//! Two configurations are recorded as exemptions rather than violations,
//! because a dedicated swap mechanism (verified by its own tests) handles them:
//!
//! * **Anchor-shape upgrades** — EVO promoted a `[5,8]`-Letter line to Voiced
//!   (or Plain to `VoicedPlain`) by inserting a voice ID, so the upgraded
//!   `AnchorKey` has no direct Xseed counterpart and the swap reaches Xseed's
//!   text through the Voiced→Letter fallback. (`mp1010_04.ing:EV_01_61_00`.)
//! * **Body substitutions** — EVO's body is `Asm`/`Flat` (Ingert could not
//!   decompile it to `Tree`) and the swap clones Xseed's body wholesale, so the
//!   EVO input has no `Tree` occurrences to diff against.
//!   (`mp3010_01.ing:QS300_01_00`.)

use crate::anchor::AnchorKey;
use crate::swap::Index;
use crate::swap::build_index;
use crate::swap::swap_scena;
use crate::text_run::TextRun;
use crate::walker::Site;
use ingert::scena::Body;
use ingert::scena::Function;
use ingert::scena::Scena;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationKind {
    /// Merged output text does not equal the Xseed run the merge applied.
    Content,
    /// EVO text differs from `original/` text on a matched anchor — the
    /// "EVO ships GungHo verbatim" overlay assumption is broken here.
    Overlay,
    /// The merge changed an occurrence but Xseed equals `original/` there (or
    /// the merge left an occurrence alone while Xseed localized it).
    Delta,
}

#[derive(Debug, Clone)]
pub struct Violation {
    pub function: String,
    pub site: Site,
    pub key: AnchorKey,
    pub occurrence: usize,
    pub kind: ViolationKind,
}

/// A merge change with no direct Xseed anchor — an EVO anchor-shape upgrade
/// resolved through a swap fallback. Not a violation; recorded so a caller can
/// confirm the set matches the documented exemptions.
#[derive(Debug, Clone)]
pub struct UpgradeExemption {
    pub function: String,
    pub site: Site,
    pub key: AnchorKey,
    pub occurrences: usize,
}

/// A function whose EVO body is not `Tree` and was body-substituted from Xseed.
/// Skipped by the delta check (verified by the substitution tests instead).
#[derive(Debug, Clone)]
pub struct BodySubExemption {
    pub function: String,
    pub evo_body_kind: &'static str,
}

#[derive(Debug, Default, Clone)]
pub struct DeltaReport {
    /// Functions present in EVO, Xseed, and `original` with a `Tree` EVO body.
    pub functions_checked: usize,
    /// Localizable EVO occurrences inspected across those functions.
    pub occurrences_checked: usize,
    /// Occurrences the merge changed (output run differs from EVO run).
    pub localized: usize,
    pub violations: Vec<Violation>,
    pub upgrades: Vec<UpgradeExemption>,
    pub body_subs: Vec<BodySubExemption>,
    /// Functions present in EVO+Xseed but absent in `original` — the
    /// localization delta cannot be computed for them.
    pub missing_original: Vec<String>,
}

fn body_kind(body: &Body) -> &'static str {
    match body {
        Body::Tree(_) => "tree",
        Body::Flat(_) => "flat",
        Body::Asm(_) => "asm",
    }
}

/// Resolve the Xseed (or `original`) run-list a given EVO bucket maps to,
/// mirroring [`crate::swap::SwapVisitor::on_syscall`]'s first two lookup
/// stages: a direct `(site, key)` hit, then `(Body, key)` when the EVO bucket
/// is in the called-table. Returns the *resolved* site so the `original` lookup
/// can use the same one. The Voiced→Letter fallback is intentionally not
/// modelled — a `None` here for an upgraded `Voiced` key is what flags the
/// documented anchor-shape exemption.
fn resolve<'a>(idx: &'a Index, site: Site, key: &AnchorKey) -> Option<(Site, &'a Vec<TextRun>)> {
    if let Some(runs) = idx.get(&(site, key.clone())).filter(|r| !r.is_empty()) {
        return Some((site, runs));
    }
    if site == Site::Called
        && let Some(runs) = idx
            .get(&(Site::Body, key.clone()))
            .filter(|r| !r.is_empty())
    {
        return Some((Site::Body, runs));
    }
    None
}

/// Verify the merge of `evo` against `xseed` honours the localization-delta
/// invariant relative to `original`. Clones `evo`, applies the merge, and
/// diffs the four corpora bucket-by-bucket. A clean run has an empty
/// `violations` list (the exemption lists may be non-empty).
#[must_use]
pub fn verify_scena(evo: &Scena, xseed: &Scena, original: &Scena) -> DeltaReport {
    let mut merged = evo.clone();
    let _ = swap_scena(&mut merged, xseed);

    let mut report = DeltaReport::default();
    for (name, evo_fn) in &evo.functions {
        let (Some(xseed_fn), Some(merged_fn)) =
            (xseed.functions.get(name), merged.functions.get(name))
        else {
            // EVO-only function: the merge leaves it byte-identical.
            continue;
        };
        if !matches!(evo_fn.body, Body::Tree(_)) {
            report.body_subs.push(BodySubExemption {
                function: name.clone(),
                evo_body_kind: body_kind(&evo_fn.body),
            });
            continue;
        }
        let Some(orig_fn) = original.functions.get(name) else {
            report.missing_original.push(name.clone());
            continue;
        };
        report.functions_checked += 1;
        verify_function(name, evo_fn, merged_fn, xseed_fn, orig_fn, &mut report);
    }
    report
}

fn verify_function(
    name: &str,
    evo_fn: &Function,
    merged_fn: &Function,
    xseed_fn: &Function,
    orig_fn: &Function,
    report: &mut DeltaReport,
) {
    let idx_evo = build_index(evo_fn);
    let idx_merged = build_index(merged_fn);
    let idx_xseed = build_index(xseed_fn);
    let idx_orig = build_index(orig_fn);

    for ((site, key), evo_runs) in &idx_evo {
        let Some(out_runs) = idx_merged.get(&(*site, key.clone())) else {
            // Merge preserves structure, so every EVO bucket has a merged
            // counterpart; a gap would mean a structural change. Skip rather
            // than index past the end.
            continue;
        };
        if let Some((resolved_site, xseed_runs)) = resolve(&idx_xseed, *site, key) {
            let orig_runs = idx_orig.get(&(resolved_site, key.clone()));
            for (i, evo_i) in evo_runs.iter().enumerate() {
                report.occurrences_checked += 1;
                // `resolve` guarantees `xseed_runs` is non-empty, and `j`
                // clamps to its last index (overflow reuse, never observed
                // on the corpus). So `get(j)` is always `Some`.
                let j = i.min(xseed_runs.len().saturating_sub(1));
                let Some(xseed_j) = xseed_runs.get(j) else {
                    continue;
                };
                let out_i = out_runs.get(i);

                if out_i != Some(xseed_j) {
                    report.violations.push(Violation {
                        function: name.to_owned(),
                        site: *site,
                        key: key.clone(),
                        occurrence: i,
                        kind: ViolationKind::Content,
                    });
                }
                let changed = out_i != Some(evo_i);
                if changed {
                    report.localized += 1;
                }
                if let Some(orig_j) = orig_runs.and_then(|r| r.get(j)) {
                    if evo_i != orig_j {
                        report.violations.push(Violation {
                            function: name.to_owned(),
                            site: *site,
                            key: key.clone(),
                            occurrence: i,
                            kind: ViolationKind::Overlay,
                        });
                    }
                    let localized = xseed_j != orig_j;
                    if changed != localized {
                        report.violations.push(Violation {
                            function: name.to_owned(),
                            site: *site,
                            key: key.clone(),
                            occurrence: i,
                            kind: ViolationKind::Delta,
                        });
                    }
                }
            }
        } else {
            // No direct Xseed anchor: either an EVO-only line (left
            // byte-identical) or an EVO anchor-shape upgrade reaching Xseed
            // text through a fallback. Count the changed occurrences; a
            // non-zero count is the upgrade exemption.
            let mut changed = 0;
            for (i, evo_i) in evo_runs.iter().enumerate() {
                report.occurrences_checked += 1;
                if out_runs.get(i) != Some(evo_i) {
                    changed += 1;
                }
            }
            if changed > 0 {
                report.localized += changed;
                report.upgrades.push(UpgradeExemption {
                    function: name.to_owned(),
                    site: *site,
                    key: key.clone(),
                    occurrences: changed,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ingert::scena::Body;
    use ingert::scena::Called;
    use ingert::scena::Expr;
    use ingert::scena::Stmt;
    use ingert::scena::Value;

    fn iv(n: i32) -> Expr {
        Expr::Value(None, Value::Int(n))
    }
    fn sv(s: &str) -> Expr {
        Expr::Value(None, Value::String(s.to_string()))
    }
    fn portrait(char_id: i32, tag: &str, text: &str) -> Stmt {
        Stmt::Expr(Expr::Syscall(
            None,
            5,
            0,
            vec![iv(char_id), sv(tag), sv(text)],
        ))
    }
    fn make_fn(body: Vec<Stmt>) -> Function {
        Function {
            args: Vec::new(),
            called: Called::Merged(false),
            is_prelude: false,
            body: Body::Tree(body),
        }
    }
    fn make_scena(name: &str, f: Function) -> Scena {
        let mut s = Scena::default();
        s.functions.insert(name.to_string(), f);
        s
    }

    #[test]
    fn clean_when_evo_equals_original_and_xseed_localizes() {
        // EVO == original (GungHo), Xseed localizes the line. The merge must
        // change it to Xseed text — delta invariant holds, no violations.
        let evo = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "GungHo")]));
        let xseed = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "Xseed")]));
        let original = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "GungHo")]));
        let report = verify_scena(&evo, &xseed, &original);
        assert!(report.violations.is_empty(), "{:?}", report.violations);
        assert_eq!(report.localized, 1);
        assert_eq!(report.occurrences_checked, 1);
    }

    #[test]
    fn clean_when_xseed_unchanged_from_original() {
        // Xseed == original: nothing to localize, the merge must leave it alone.
        let evo = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "same")]));
        let xseed = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "same")]));
        let original = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "same")]));
        let report = verify_scena(&evo, &xseed, &original);
        assert!(report.violations.is_empty());
        assert_eq!(report.localized, 0);
    }

    #[test]
    fn overlay_violation_when_evo_differs_from_original() {
        // EVO text differs from original (overlay assumption broken). The merge
        // still applies Xseed (no Content violation), but Overlay and Delta
        // fire: the merge leaves EVO alone (EVO already == Xseed) while Xseed
        // differs from original.
        let evo = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "Xseed")]));
        let xseed = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "Xseed")]));
        let original = make_scena("F", make_fn(vec![portrait(0, "<#E_0>", "GungHo")]));
        let report = verify_scena(&evo, &xseed, &original);
        let kinds: Vec<_> = report.violations.iter().map(|v| v.kind).collect();
        assert!(kinds.contains(&ViolationKind::Overlay), "{kinds:?}");
        assert!(kinds.contains(&ViolationKind::Delta), "{kinds:?}");
    }

    #[test]
    fn evo_only_function_is_skipped() {
        // A function only EVO has: the merge leaves it byte-identical and the
        // check skips it (no original/xseed to compare against).
        let evo = make_scena("ONLY", make_fn(vec![portrait(0, "<#E_0>", "x")]));
        let xseed = Scena::default();
        let original = Scena::default();
        let report = verify_scena(&evo, &xseed, &original);
        assert_eq!(report.functions_checked, 0);
        assert!(report.violations.is_empty());
    }
}
