use crate::anchor::AnchorKey;
use crate::anchor::classify_syscall_call;
use crate::anchor::classify_syscall_expr;
use crate::text_run::TextRun;
use crate::text_run::extract_run_call;
use crate::text_run::extract_run_expr;
use crate::walker::Site;
use crate::walker::Visitor;
use crate::walker::rewrite_body;
use crate::walker::rewrite_called;
use ingert::scena::Body;
use ingert::scena::Called;
use ingert::scena::Function;
use ingert::scena::Scena;
use ingert::scp::Call;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct UnmatchedEntry {
    pub function: String,
    pub site: Site,
    pub line: Option<u16>,
    pub key: AnchorKey,
    pub evo_run: TextRun,
}

#[derive(Debug, Clone)]
pub struct OverflowEntry {
    pub function: String,
    pub site: Site,
    pub line: Option<u16>,
    pub key: AnchorKey,
    pub evo_run: TextRun,
    pub reused_run: TextRun,
}

/// Recorded when EVO's body is `Asm`/`Flat` (couldn't decompile to `Tree`)
/// but XSeed's body is `Tree`, and EVO's calls-table has no voice IDs that
/// would be lost by substitution. The swap layer replaces EVO's body with
/// a clone of XSeed's so the runtime executes the XSeed text rather than
/// the GungHo text embedded in EVO's asm bytecode.
#[derive(Debug, Clone)]
pub struct BodySubstitutionEntry {
    pub function: String,
    pub evo_body_kind: &'static str,
}

#[derive(Debug, Default, Clone)]
pub struct SwapStats {
    pub swaps_applied: usize,
    pub no_ops_equal: usize,
    pub unmatched_evo_calls: usize,
    pub overflow_reuses: usize,
    pub voiced_to_letter_fallback: usize,
    pub body_substitutions: usize,
    pub unmatched: Vec<UnmatchedEntry>,
    pub overflows: Vec<OverflowEntry>,
    pub body_subs: Vec<BodySubstitutionEntry>,
}

impl SwapStats {
    pub fn merge(&mut self, other: SwapStats) {
        self.swaps_applied += other.swaps_applied;
        self.no_ops_equal += other.no_ops_equal;
        self.unmatched_evo_calls += other.unmatched_evo_calls;
        self.overflow_reuses += other.overflow_reuses;
        self.voiced_to_letter_fallback += other.voiced_to_letter_fallback;
        self.body_substitutions += other.body_substitutions;
        self.unmatched.extend(other.unmatched);
        self.overflows.extend(other.overflows);
        self.body_subs.extend(other.body_subs);
    }
}

pub fn swap_scena(evo: &mut Scena, xseed: &Scena) -> SwapStats {
    let mut stats = SwapStats::default();
    for (name, evo_fn) in &mut evo.functions {
        let Some(xseed_fn) = xseed.functions.get(name) else {
            continue;
        };
        let index = build_index(xseed_fn);
        let fn_stats = swap_function(name, evo_fn, xseed_fn, &index);
        stats.merge(fn_stats);
    }
    stats
}

/// Returns true if EVO's calls-table contains any syscall whose argument
/// list carries an explicit `11, V` voice-ID marker. Used as the safety
/// gate before substituting an EVO Asm/Flat body with a clone of XSeed's
/// Tree body — we only substitute when EVO has added nothing voice-related
/// to this function.
///
/// `prefix_len > N` alone is insufficient: some `[5,0]` calls carry other
/// integer params between `char_id` and the portrait tag (e.g.
/// `system[5,0](11510, 25, "<#E…>", …)`) that are not voice IDs. We check
/// for the literal `11` marker that always precedes a voice ID.
fn evo_calls_have_voice_ids(called: &Called) -> bool {
    let Called::Raw(calls) = called else {
        return false;
    };
    calls.iter().any(|call| {
        let Some(cls) = classify_syscall_call(&call.kind, &call.args) else {
            return false;
        };
        let is_int_11 = |idx: usize| {
            matches!(
                call.args.get(idx),
                Some(ingert::scp::CallArg::Value(ingert::scp::Value::Int(11)))
            )
        };
        let next_is_int = |idx: usize| {
            matches!(
                call.args.get(idx),
                Some(ingert::scp::CallArg::Value(ingert::scp::Value::Int(_)))
            )
        };
        match cls.key {
            AnchorKey::Voiced(_) => true,
            // Portrait+voice: (char_id, 11, V, "<#E…>", …).
            AnchorKey::Portrait { .. } => is_int_11(1) && next_is_int(2),
            // VoicedPlain: (65535, 11, V, "…", …). Classified as Plain with
            // prefix_len 3 (vs 1 for regular Plain).
            AnchorKey::Plain => cls.prefix_len == 3 && is_int_11(1),
            AnchorKey::Letter => false,
        }
    })
}

fn body_kind(body: &Body) -> &'static str {
    match body {
        Body::Tree(_) => "tree",
        Body::Flat(_) => "flat",
        Body::Asm(_) => "asm",
    }
}

type Index = HashMap<(Site, AnchorKey), Vec<TextRun>>;

fn build_index(f: &Function) -> Index {
    let mut idx: Index = HashMap::new();
    if let Body::Tree(stmts) = &f.body {
        let mut collector = IndexBuilder {
            idx: &mut idx,
            site: Site::Body,
        };
        collect_body(stmts, &mut collector);
    }
    if let Called::Raw(calls) = &f.called {
        let mut collector = IndexBuilder {
            idx: &mut idx,
            site: Site::Called,
        };
        collect_called(calls, &mut collector);
    }
    idx
}

struct IndexBuilder<'a> {
    idx: &'a mut Index,
    site: Site,
}

impl IndexBuilder<'_> {
    fn push(&mut self, key: AnchorKey, run: TextRun) {
        self.idx.entry((self.site, key)).or_default().push(run);
    }
}

fn collect_body(stmts: &[ingert::scena::Stmt], b: &mut IndexBuilder) {
    use ingert::scena::Stmt;
    for stmt in stmts {
        match stmt {
            Stmt::Expr(e) | Stmt::Set(_, _, e) => collect_expr(e, b),
            Stmt::Return(_, e) | Stmt::PushVar(_, _, e) => {
                if let Some(e) = e {
                    collect_expr(e, b);
                }
            }
            Stmt::If(_, cond, then, els) => {
                collect_expr(cond, b);
                collect_body(then, b);
                if let Some(els) = els {
                    collect_body(els, b);
                }
            }
            Stmt::While(_, cond, body) => {
                collect_expr(cond, b);
                collect_body(body, b);
            }
            Stmt::Switch(_, scrut, cases) => {
                collect_expr(scrut, b);
                for arm in cases.values() {
                    collect_body(arm, b);
                }
            }
            Stmt::Block(stmts) => collect_body(stmts, b),
            Stmt::Debug(_, args) | Stmt::Tailcall(_, _, args) => {
                for a in args {
                    collect_expr(a, b);
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
}

fn collect_expr(expr: &ingert::scena::Expr, b: &mut IndexBuilder) {
    use ingert::scena::Expr;
    match expr {
        Expr::Syscall(_, a, bb, args) => {
            for arg in args {
                collect_expr(arg, b);
            }
            if let Some(cls) = classify_syscall_expr(*a, *bb, args)
                && let Some(rest) = args.get(cls.prefix_len..)
                && let Some(run) = extract_run_expr(rest)
            {
                b.push(cls.key, run);
            }
        }
        Expr::Call(_, _, args) => {
            for arg in args {
                collect_expr(arg, b);
            }
        }
        Expr::Unop(_, _, inner) => collect_expr(inner, b),
        Expr::Binop(_, _, l, r) => {
            collect_expr(l, b);
            collect_expr(r, b);
        }
        Expr::Value(_, _) | Expr::Var(_, _) | Expr::Ref(_, _) => {}
    }
}

fn collect_called(calls: &[Call], b: &mut IndexBuilder) {
    for call in calls {
        if let Some(cls) = classify_syscall_call(&call.kind, &call.args)
            && let Some(rest) = call.args.get(cls.prefix_len..)
            && let Some(run) = extract_run_call(rest)
        {
            b.push(cls.key, run);
        }
    }
}

struct SwapVisitor<'a> {
    function: &'a str,
    index: &'a Index,
    counters: HashMap<(Site, AnchorKey), usize>,
    stats: SwapStats,
}

impl Visitor for SwapVisitor<'_> {
    fn on_syscall(
        &mut self,
        site: Site,
        line: Option<u16>,
        key: &AnchorKey,
        evo_run: &TextRun,
    ) -> Option<TextRun> {
        // Lookup order: (site, key) → (Body, key) when site is Called →
        // (site, Letter) when key is Voiced(_) [EVO Letter→Voiced upgrade].
        // The last fallback shares the counter with regular Letter calls so
        // multiple upgraded Voiceds advance positionally through XSeed's
        // Letter runs in the same source order.
        let direct = self
            .index
            .get(&(site, key.clone()))
            .filter(|r| !r.is_empty());
        let called_fallback = direct.or_else(|| {
            if site == Site::Called {
                self.index
                    .get(&(Site::Body, key.clone()))
                    .filter(|r| !r.is_empty())
            } else {
                None
            }
        });
        let (runs, counter_key) = if let Some(runs) = called_fallback {
            (runs, (site, key.clone()))
        } else if matches!(key, AnchorKey::Voiced(_))
            && let Some(runs) = self
                .index
                .get(&(site, AnchorKey::Letter))
                .filter(|r| !r.is_empty())
        {
            self.stats.voiced_to_letter_fallback += 1;
            (runs, (site, AnchorKey::Letter))
        } else {
            self.stats.unmatched_evo_calls += 1;
            self.stats.unmatched.push(UnmatchedEntry {
                function: self.function.to_owned(),
                site,
                line,
                key: key.clone(),
                evo_run: evo_run.clone(),
            });
            return None;
        };
        let key_owned = counter_key;
        let n = *self.counters.get(&key_owned).unwrap_or(&0);
        let (run, overflow) = match runs.get(n) {
            Some(r) => (r.clone(), false),
            None => (runs.last()?.clone(), true),
        };
        self.counters.insert(key_owned, n + 1);
        if overflow {
            self.stats.overflow_reuses += 1;
            self.stats.overflows.push(OverflowEntry {
                function: self.function.to_owned(),
                site,
                line,
                key: key.clone(),
                evo_run: evo_run.clone(),
                reused_run: run.clone(),
            });
        }
        if &run == evo_run {
            self.stats.no_ops_equal += 1;
            None
        } else {
            self.stats.swaps_applied += 1;
            Some(run)
        }
    }
}

fn swap_function(name: &str, evo: &mut Function, xseed: &Function, index: &Index) -> SwapStats {
    let mut visitor = SwapVisitor {
        function: name,
        index,
        counters: HashMap::new(),
        stats: SwapStats::default(),
    };
    // Asm/Flat body substitution: when EVO's body couldn't be decompiled to
    // Tree but XSeed's body could, and EVO has added no voice IDs in this
    // function, clone XSeed's body into EVO. Without this, EVO retains
    // GungHo text embedded in its asm bytecode (since the body walker only
    // touches Tree bodies). The called-table swap alone doesn't reach the
    // runtime since that block is metadata.
    let needs_substitution = matches!(&evo.body, Body::Asm(_) | Body::Flat(_))
        && matches!(&xseed.body, Body::Tree(_))
        && !evo_calls_have_voice_ids(&evo.called);
    if needs_substitution {
        let evo_kind = body_kind(&evo.body);
        evo.body = xseed.body.clone();
        visitor.stats.body_substitutions += 1;
        visitor.stats.body_subs.push(BodySubstitutionEntry {
            function: name.to_owned(),
            evo_body_kind: evo_kind,
        });
    }
    if let Body::Tree(stmts) = &mut evo.body {
        rewrite_body(stmts, &mut visitor);
    }
    if let Called::Raw(calls) = &mut evo.called {
        rewrite_called(calls, &mut visitor);
    }
    visitor.stats
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::panic,
        clippy::indexing_slicing,
        clippy::unreachable,
        reason = "tests panic on assertion failure by design"
    )]

    use super::*;
    use indexmap::IndexMap;
    use ingert::scena::Arg;
    use ingert::scena::ArgType;
    use ingert::scena::Body;
    use ingert::scena::Called;
    use ingert::scena::Expr;
    use ingert::scena::Function;
    use ingert::scena::Scena;
    use ingert::scena::Stmt;
    use ingert::scena::Value;

    fn iv(n: i32) -> Expr {
        Expr::Value(None, Value::Int(n))
    }
    fn sv(s: &str) -> Expr {
        Expr::Value(None, Value::String(s.to_string()))
    }
    fn portrait_call(char_id: i32, tag: &str, text: &str) -> Expr {
        Expr::Syscall(None, 5, 0, vec![iv(char_id), sv(tag), sv(text)])
    }
    fn portrait_call_voiced(char_id: i32, voice: i32, tag: &str, text: &str) -> Expr {
        Expr::Syscall(
            None,
            5,
            0,
            vec![iv(char_id), iv(11), iv(voice), sv(tag), sv(text)],
        )
    }
    fn s58_voiced(v: i32, text: &str) -> Expr {
        Expr::Syscall(
            None,
            5,
            8,
            vec![iv(65535), iv(19), iv(13), iv(11), iv(v), sv(text)],
        )
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
    fn evo_only_call_left_untouched() {
        let evo_fn = make_fn(vec![Stmt::Expr(portrait_call(7, "<#E_7>", "EVO-only"))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "different"))]);
        let mut evo = make_scena("F", evo_fn.clone());
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 0);
        assert_eq!(stats.unmatched_evo_calls, 1);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let Body::Tree(orig) = &evo_fn.body else {
            unreachable!()
        };
        assert_eq!(body, orig);
    }

    #[test]
    fn n_to_m_overflow_reuses_last() {
        let evo_fn = make_fn(vec![
            Stmt::Expr(portrait_call(0, "<#E_0>", "evo1")),
            Stmt::Expr(portrait_call(0, "<#E_0>", "evo2")),
            Stmt::Expr(portrait_call(0, "<#E_0>", "evo3")),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(portrait_call(0, "<#E_0>", "run_a")),
            Stmt::Expr(portrait_call(0, "<#E_0>", "run_b")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 3);
        assert_eq!(stats.overflow_reuses, 1);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let texts: Vec<&str> = body
            .iter()
            .map(|s| {
                let Stmt::Expr(Expr::Syscall(_, _, _, args)) = s else {
                    unreachable!()
                };
                let Expr::Value(_, Value::String(t)) = &args[2] else {
                    unreachable!()
                };
                t.as_str()
            })
            .collect();
        assert_eq!(texts, vec!["run_a", "run_b", "run_b"]);
    }

    #[test]
    fn idempotent_second_run_is_noop() {
        let evo_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "EVO"))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "XSEED"))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);

        let _ = swap_scena(&mut evo, &xseed);
        let snapshot1 = evo.clone();
        let stats2 = swap_scena(&mut evo, &xseed);
        assert_eq!(stats2.swaps_applied, 0);
        assert_eq!(stats2.no_ops_equal, 1);
        assert_eq!(evo, snapshot1);
    }

    #[test]
    fn no_op_when_evo_already_matches() {
        let evo_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "same"))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "same"))]);
        let evo_orig = evo_fn.clone();
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 0);
        assert_eq!(stats.no_ops_equal, 1);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let Body::Tree(orig) = &evo_orig.body else {
            unreachable!()
        };
        assert_eq!(body, orig);
    }

    #[test]
    fn s58_voiced_anchors_on_voice_id() {
        let evo_fn = make_fn(vec![
            Stmt::Expr(s58_voiced(34832, "evo-a")),
            Stmt::Expr(s58_voiced(34833, "evo-b")),
        ]);
        // Reverse order in XSeed to prove the match is by voice ID, not position.
        let xseed_fn = make_fn(vec![
            Stmt::Expr(s58_voiced(34833, "x-b")),
            Stmt::Expr(s58_voiced(34832, "x-a")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let texts: Vec<&str> = body
            .iter()
            .map(|s| {
                let Stmt::Expr(Expr::Syscall(_, _, _, args)) = s else {
                    unreachable!()
                };
                let Expr::Value(_, Value::String(t)) = &args[5] else {
                    unreachable!()
                };
                t.as_str()
            })
            .collect();
        assert_eq!(texts, vec!["x-a", "x-b"]);
    }

    #[test]
    fn called_table_swapped_with_same_index_as_body() {
        use ingert::scp::Call;
        use ingert::scp::CallArg;
        use ingert::scp::CallKind;
        use ingert::scp::Value as ScpValue;

        let body = vec![Stmt::Expr(portrait_call_voiced(
            134, 33247, "<#E_0>", "EVO body",
        ))];
        let called = vec![Call {
            kind: CallKind::Syscall(5, 0),
            args: vec![
                CallArg::Value(ScpValue::Int(134)),
                CallArg::Value(ScpValue::String("<#E_0>".into())),
                CallArg::Value(ScpValue::String("EVO meta".into())),
            ],
        }];
        let evo_fn = Function {
            args: Vec::new(),
            called: Called::Raw(called),
            is_prelude: false,
            body: Body::Tree(body),
        };
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(134, "<#E_0>", "XSEED"))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let _ = swap_scena(&mut evo, &xseed);
        let f = &evo.functions["F"];
        let Body::Tree(body) = &f.body else {
            unreachable!()
        };
        let Stmt::Expr(Expr::Syscall(_, _, _, body_args)) = &body[0] else {
            unreachable!()
        };
        let Expr::Value(_, Value::String(body_text)) = &body_args[4] else {
            unreachable!()
        };
        assert_eq!(body_text, "XSEED");
        let Called::Raw(calls) = &f.called else {
            unreachable!()
        };
        match &calls[0].args[2] {
            ingert::scp::CallArg::Value(ingert::scp::Value::String(s)) => {
                assert_eq!(s, "XSEED");
            }
            _ => panic!("expected string"),
        }
    }

    fn s58_letter(text: &str) -> Expr {
        Expr::Syscall(None, 5, 8, vec![iv(65535), iv(19), iv(13), sv(text)])
    }
    fn s58_plain(text: &str) -> Expr {
        Expr::Syscall(None, 5, 8, vec![iv(65535), sv(text)])
    }
    fn s58_voiced_plain(v: i32, text: &str) -> Expr {
        // EVO upgrade shape: (65535, 11, V, "text"). Classifies as
        // AnchorKey::Plain with prefix_len=3.
        Expr::Syscall(None, 5, 8, vec![iv(65535), iv(11), iv(v), sv(text)])
    }

    #[test]
    fn voiced_to_letter_fallback_matches_positionally() {
        // EVO upgraded 2 Letter calls to Voiced (e.g. Cassius letter
        // follow-ups in mp1010_04 EV_01_61_00). XSeed still has them as
        // Letters with re-translated text. The fallback should consume
        // XSeed's Letter runs in source order.
        let evo_fn = make_fn(vec![
            Stmt::Expr(s58_voiced(97068, "EVO old text A")),
            Stmt::Expr(s58_voiced(97069, "EVO old text B")),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(s58_letter("XSEED translated A")),
            Stmt::Expr(s58_letter("XSEED translated B")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        assert_eq!(stats.voiced_to_letter_fallback, 2);
        assert_eq!(stats.unmatched_evo_calls, 0);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        // EVO body retains Voiced shape: (65535, 19, 13, 11, V, text).
        // Only args[5] (the string) gets swapped; voice ID args[3..5]
        // survive untouched.
        let texts: Vec<&str> = body
            .iter()
            .map(|s| {
                let Stmt::Expr(Expr::Syscall(_, _, _, args)) = s else {
                    unreachable!()
                };
                // Confirm voice marker preserved.
                let Expr::Value(_, Value::Int(11)) = &args[3] else {
                    unreachable!()
                };
                let Expr::Value(_, Value::String(t)) = &args[5] else {
                    unreachable!()
                };
                t.as_str()
            })
            .collect();
        assert_eq!(texts, vec!["XSEED translated A", "XSEED translated B"]);
    }

    #[test]
    fn voiced_plain_evo_upgrade_matches_xseed_plain() {
        // mp3010_01 QS308_01_00 song-lyric pattern: EVO upgraded Plain to
        // VoicedPlain shape (65535, 11, V, "text"). The classifier now
        // returns AnchorKey::Plain with prefix_len=3, matching XSeed's
        // regular Plain run positionally. Voice ID at args[1..3] survives.
        let evo_fn = make_fn(vec![Stmt::Expr(s58_voiced_plain(97064, "EVO old lyric"))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(s58_plain("XSEED translated lyric"))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 1);
        assert_eq!(stats.unmatched_evo_calls, 0);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let Stmt::Expr(Expr::Syscall(_, _, _, args)) = &body[0] else {
            unreachable!()
        };
        // Voice ID args preserved.
        assert!(matches!(&args[1], Expr::Value(_, Value::Int(11))));
        assert!(matches!(&args[2], Expr::Value(_, Value::Int(97064))));
        let Expr::Value(_, Value::String(t)) = &args[3] else {
            unreachable!()
        };
        assert_eq!(t, "XSEED translated lyric");
    }

    #[test]
    fn evo_calls_voice_id_helper_distinguishes_non_voice_int_args() {
        use ingert::scp::Call;
        use ingert::scp::CallArg;
        use ingert::scp::CallKind;
        use ingert::scp::Value as ScpValue;

        // Real-world false-positive that motivated this check:
        // system[5,0](11510, 25, "<#E…>", "…") — the `25` is a game param
        // (not a voice ID). Without the explicit `11` check, prefix_len > 2
        // would flag this as voiced.
        let calls = vec![Call {
            kind: CallKind::Syscall(5, 0),
            args: vec![
                CallArg::Value(ScpValue::Int(11510)),
                CallArg::Value(ScpValue::Int(25)),
                CallArg::Value(ScpValue::String("<#E_0>".into())),
                CallArg::Value(ScpValue::String("text".into())),
            ],
        }];
        assert!(!evo_calls_have_voice_ids(&Called::Raw(calls)));

        // Genuine voice-ID upgrade: (char_id, 11, V, "<#E…>", "…").
        let calls_voice = vec![Call {
            kind: CallKind::Syscall(5, 0),
            args: vec![
                CallArg::Value(ScpValue::Int(0)),
                CallArg::Value(ScpValue::Int(11)),
                CallArg::Value(ScpValue::Int(60589)),
                CallArg::Value(ScpValue::String("<#E_0>".into())),
                CallArg::Value(ScpValue::String("text".into())),
            ],
        }];
        assert!(evo_calls_have_voice_ids(&Called::Raw(calls_voice)));

        // VoicedPlain: (65535, 11, V, "text").
        let calls_vp = vec![Call {
            kind: CallKind::Syscall(5, 8),
            args: vec![
                CallArg::Value(ScpValue::Int(65535)),
                CallArg::Value(ScpValue::Int(11)),
                CallArg::Value(ScpValue::Int(97064)),
                CallArg::Value(ScpValue::String("lyric".into())),
            ],
        }];
        assert!(evo_calls_have_voice_ids(&Called::Raw(calls_vp)));

        // Regular Plain (no voice).
        let calls_plain = vec![Call {
            kind: CallKind::Syscall(5, 8),
            args: vec![
                CallArg::Value(ScpValue::Int(65535)),
                CallArg::Value(ScpValue::String("text".into())),
            ],
        }];
        assert!(!evo_calls_have_voice_ids(&Called::Raw(calls_plain)));
    }

    #[test]
    fn asm_body_substituted_when_xseed_is_tree_and_no_voice_ids() {
        // mp3010_01 QS300_01_00 case: EVO body is Asm (ingert couldn't
        // decompile to Tree) but XSeed body is Tree and EVO's calls-table
        // has no voice IDs. The swap layer should clone XSeed's body into
        // EVO so the runtime executes XSeed text rather than GungHo text
        // embedded in EVO's asm bytecode.
        let evo_fn = Function {
            args: Vec::new(),
            called: Called::Raw(Vec::new()),
            is_prelude: false,
            body: Body::Asm(Vec::new()),
        };
        let xseed_body = vec![Stmt::Expr(s58_plain("XSEED text"))];
        let xseed_fn = make_fn(xseed_body.clone());
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.body_substitutions, 1);
        assert_eq!(stats.body_subs.len(), 1);
        assert_eq!(stats.body_subs[0].function, "F");
        assert_eq!(stats.body_subs[0].evo_body_kind, "asm");
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!("body should have been substituted to Tree")
        };
        assert_eq!(body, &xseed_body);
    }

    #[test]
    fn asm_body_not_substituted_when_evo_has_voice_ids() {
        // If EVO's calls-table reveals any voice-ID upgrade in this function,
        // substituting XSeed's body would silently drop EVO's contribution.
        // Skip the substitution and leave EVO's asm body alone.
        use ingert::scp::Call;
        use ingert::scp::CallArg;
        use ingert::scp::CallKind;
        use ingert::scp::Value as ScpValue;
        let evo_fn = Function {
            args: Vec::new(),
            called: Called::Raw(vec![Call {
                kind: CallKind::Syscall(5, 0),
                args: vec![
                    CallArg::Value(ScpValue::Int(0)),
                    CallArg::Value(ScpValue::Int(11)),
                    CallArg::Value(ScpValue::Int(60589)),
                    CallArg::Value(ScpValue::String("<#E_0>".into())),
                    CallArg::Value(ScpValue::String("EVO voiced".into())),
                ],
            }]),
            is_prelude: false,
            body: Body::Asm(Vec::new()),
        };
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "XSEED"))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.body_substitutions, 0);
        assert!(matches!(&evo.functions["F"].body, Body::Asm(_)));
    }

    #[test]
    fn _silence_unused_warnings_for_args_arg() {
        let _ = Arg {
            ty: ArgType::Number,
            default: None,
            line: None,
        };
        let _ = IndexMap::<String, i32>::new();
    }
}
