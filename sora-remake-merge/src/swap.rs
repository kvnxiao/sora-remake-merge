use std::collections::HashMap;

use ingert::scena::{Body, Called, Function, Scena};
use ingert::scp::Call;

use crate::anchor::{
    classify_syscall_call, classify_syscall_expr, AnchorKey,
};
use crate::text_run::{extract_run_call, extract_run_expr, TextRun};
use crate::walker::{rewrite_body, rewrite_called, Site, Visitor};

#[derive(Debug, Default, Clone)]
pub struct SwapStats {
    pub swaps_applied: usize,
    pub no_ops_equal: usize,
    pub unmatched_evo_calls: usize,
    pub overflow_reuses: usize,
}

impl SwapStats {
    fn merge(&mut self, other: &SwapStats) {
        self.swaps_applied += other.swaps_applied;
        self.no_ops_equal += other.no_ops_equal;
        self.unmatched_evo_calls += other.unmatched_evo_calls;
        self.overflow_reuses += other.overflow_reuses;
    }
}

pub fn swap_scena(evo: &mut Scena, xseed: &Scena) -> SwapStats {
    let mut stats = SwapStats::default();
    for (name, evo_fn) in &mut evo.functions {
        let Some(xseed_fn) = xseed.functions.get(name) else {
            continue;
        };
        let index = build_index(xseed_fn);
        let fn_stats = swap_function(evo_fn, &index);
        stats.merge(&fn_stats);
    }
    stats
}

type Index = HashMap<AnchorKey, Vec<TextRun>>;

fn build_index(f: &Function) -> Index {
    let mut idx: Index = HashMap::new();
    let mut collector = IndexBuilder { idx: &mut idx };
    match &f.body {
        Body::Tree(stmts) => collect_body(stmts, &mut collector),
        Body::Flat(_) | Body::Asm(_) => {}
    }
    if let Called::Raw(calls) = &f.called
        && matches!(f.body, Body::Flat(_) | Body::Asm(_))
    {
        collect_called(calls, &mut collector);
    }
    idx
}

struct IndexBuilder<'a> {
    idx: &'a mut Index,
}

impl IndexBuilder<'_> {
    fn push(&mut self, key: AnchorKey, run: TextRun) {
        self.idx.entry(key).or_default().push(run);
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
    index: &'a Index,
    counters: HashMap<(Site, AnchorKey), usize>,
    stats: SwapStats,
}

impl Visitor for SwapVisitor<'_> {
    fn on_syscall(
        &mut self,
        site: Site,
        key: &AnchorKey,
        evo_run: &TextRun,
    ) -> Option<TextRun> {
        let Some(runs) = self.index.get(key) else {
            self.stats.unmatched_evo_calls += 1;
            return None;
        };
        if runs.is_empty() {
            self.stats.unmatched_evo_calls += 1;
            return None;
        }
        let key_owned = (site, key.clone());
        let n = *self.counters.get(&key_owned).unwrap_or(&0);
        let (run, overflow) = match runs.get(n) {
            Some(r) => (r.clone(), false),
            None => (runs.last()?.clone(), true),
        };
        self.counters.insert(key_owned, n + 1);
        if overflow {
            self.stats.overflow_reuses += 1;
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

fn swap_function(evo: &mut Function, index: &Index) -> SwapStats {
    let mut visitor = SwapVisitor {
        index,
        counters: HashMap::new(),
        stats: SwapStats::default(),
    };
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
    use ingert::scena::{Arg, ArgType, Body, Called, Expr, Function, Scena, Stmt, Value};

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
        let Body::Tree(orig) = &evo_fn.body else { unreachable!() };
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
        use ingert::scp::{Call, CallArg, CallKind, Value as ScpValue};

        let body = vec![Stmt::Expr(portrait_call_voiced(
            134,
            33247,
            "<#E_0>",
            "EVO body",
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
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(
            134,
            "<#E_0>",
            "XSEED",
        ))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let _ = swap_scena(&mut evo, &xseed);
        let f = &evo.functions["F"];
        let Body::Tree(body) = &f.body else { unreachable!() };
        let Stmt::Expr(Expr::Syscall(_, _, _, body_args)) = &body[0] else {
            unreachable!()
        };
        let Expr::Value(_, Value::String(body_text)) = &body_args[4] else {
            unreachable!()
        };
        assert_eq!(body_text, "XSEED");
        let Called::Raw(calls) = &f.called else { unreachable!() };
        match &calls[0].args[2] {
            ingert::scp::CallArg::Value(ingert::scp::Value::String(s)) => {
                assert_eq!(s, "XSEED");
            }
            _ => panic!("expected string"),
        }
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
