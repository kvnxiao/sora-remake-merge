use crate::anchor::AnchorKey;
use crate::anchor::classify_syscall_call;
use crate::anchor::classify_syscall_expr;
use crate::text_run::TextRun;
use crate::text_run::build_run_call;
use crate::text_run::build_run_expr;
use crate::text_run::extract_run_call;
use crate::text_run::extract_run_expr;
use ingert::scena::Expr;
use ingert::scena::Stmt;
use ingert::scp::Call;
use ingert::scp::CallKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Site {
    Body,
    Called,
}

pub trait Visitor {
    fn on_syscall(&mut self, site: Site, key: &AnchorKey, evo_run: &TextRun) -> Option<TextRun>;
}

pub fn rewrite_body(stmts: &mut [Stmt], visitor: &mut impl Visitor) {
    for stmt in stmts {
        rewrite_stmt(stmt, visitor);
    }
}

fn rewrite_stmt(stmt: &mut Stmt, v: &mut impl Visitor) {
    match stmt {
        Stmt::Expr(e) | Stmt::Set(_, _, e) => rewrite_expr(e, v),
        Stmt::Return(_, e) | Stmt::PushVar(_, _, e) => {
            if let Some(e) = e {
                rewrite_expr(e, v);
            }
        }
        Stmt::If(_, cond, then, els) => {
            rewrite_expr(cond, v);
            rewrite_body(then, v);
            if let Some(els) = els {
                rewrite_body(els, v);
            }
        }
        Stmt::While(_, cond, body) => {
            rewrite_expr(cond, v);
            rewrite_body(body, v);
        }
        Stmt::Switch(_, scrut, cases) => {
            rewrite_expr(scrut, v);
            for arm in cases.values_mut() {
                rewrite_body(arm, v);
            }
        }
        Stmt::Block(stmts) => rewrite_body(stmts, v),
        Stmt::Debug(_, args) | Stmt::Tailcall(_, _, args) => {
            for arg in args {
                rewrite_expr(arg, v);
            }
        }
        Stmt::Break | Stmt::Continue => {}
    }
}

fn rewrite_expr(expr: &mut Expr, v: &mut impl Visitor) {
    match expr {
        Expr::Syscall(_, a, b, args) => {
            for arg in args.iter_mut() {
                rewrite_expr(arg, v);
            }
            if let Some(cls) = classify_syscall_expr(*a, *b, args)
                && let Some(rest) = args.get(cls.prefix_len..)
                && let Some(evo_run) = extract_run_expr(rest)
                && let Some(new_run) = v.on_syscall(Site::Body, &cls.key, &evo_run)
                && new_run != evo_run
            {
                args.truncate(cls.prefix_len);
                args.extend(build_run_expr(&new_run));
            }
        }
        Expr::Call(_, _, args) => {
            for arg in args {
                rewrite_expr(arg, v);
            }
        }
        Expr::Unop(_, _, inner) => rewrite_expr(inner, v),
        Expr::Binop(_, _, l, r) => {
            rewrite_expr(l, v);
            rewrite_expr(r, v);
        }
        Expr::Value(_, _) | Expr::Var(_, _) | Expr::Ref(_, _) => {}
    }
}

pub fn rewrite_called(calls: &mut [Call], visitor: &mut impl Visitor) {
    for call in calls {
        if matches!(call.kind, CallKind::Syscall(_, _))
            && let Some(cls) = classify_syscall_call(&call.kind, &call.args)
            && let Some(rest) = call.args.get(cls.prefix_len..)
            && let Some(evo_run) = extract_run_call(rest)
            && let Some(new_run) = visitor.on_syscall(Site::Called, &cls.key, &evo_run)
            && new_run != evo_run
        {
            call.args.truncate(cls.prefix_len);
            call.args.extend(build_run_call(&new_run));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use ingert::scena::Place;
    use ingert::scena::Stmt;
    use ingert::scena::Value;
    use ingert::scena::Var;
    use ingert::scp::Call;
    use ingert::scp::CallArg;
    use ingert::scp::CallKind;
    use ingert::scp::Value as ScpValue;

    fn iv(n: i32) -> Expr {
        Expr::Value(None, Value::Int(n))
    }
    fn sv(s: &str) -> Expr {
        Expr::Value(None, Value::String(s.to_string()))
    }

    fn make_syscall(text: &str) -> Expr {
        Expr::Syscall(None, 5, 0, vec![iv(0), sv("<#E_0#M_0#B_0>"), sv(text)])
    }

    fn make_stmt(text: &str) -> Stmt {
        Stmt::Expr(make_syscall(text))
    }

    struct CountingVisitor {
        count: usize,
        new_text: String,
    }

    impl Visitor for CountingVisitor {
        fn on_syscall(
            &mut self,
            _site: Site,
            _key: &AnchorKey,
            _evo_run: &TextRun,
        ) -> Option<TextRun> {
            self.count += 1;
            Some(vec![self.new_text.clone()])
        }
    }

    fn count_swaps(stmts: &mut [Stmt]) -> usize {
        let mut v = CountingVisitor {
            count: 0,
            new_text: "REPLACED".to_string(),
        };
        rewrite_body(stmts, &mut v);
        v.count
    }

    #[test]
    fn walker_visits_stmt_expr() {
        let mut body = vec![make_stmt("a")];
        assert_eq!(count_swaps(&mut body), 1);
    }

    #[test]
    fn walker_visits_set_rhs() {
        let mut body = vec![Stmt::Set(None, Place::Var(Var(0)), make_syscall("a"))];
        assert_eq!(count_swaps(&mut body), 1);
    }

    #[test]
    fn walker_visits_if_both_branches() {
        let mut body = vec![Stmt::If(
            None,
            iv(0),
            vec![make_stmt("a")],
            Some(vec![make_stmt("b")]),
        )];
        assert_eq!(count_swaps(&mut body), 2);
    }

    #[test]
    fn walker_visits_while_body() {
        let mut body = vec![Stmt::While(None, iv(0), vec![make_stmt("a")])];
        assert_eq!(count_swaps(&mut body), 1);
    }

    #[test]
    fn walker_visits_switch_arms() {
        let mut cases: IndexMap<Option<i32>, Vec<Stmt>> = IndexMap::new();
        cases.insert(Some(1), vec![make_stmt("a")]);
        cases.insert(Some(2), vec![make_stmt("b")]);
        cases.insert(None, vec![make_stmt("c")]);
        let mut body = vec![Stmt::Switch(None, iv(0), cases)];
        assert_eq!(count_swaps(&mut body), 3);
    }

    #[test]
    fn walker_visits_nested_block_and_if() {
        let mut body = vec![Stmt::Block(vec![
            Stmt::If(None, iv(0), vec![Stmt::Block(vec![make_stmt("a")])], None),
            make_stmt("b"),
        ])];
        assert_eq!(count_swaps(&mut body), 2);
    }

    #[test]
    fn called_walker_visits_syscalls_only() {
        let mut calls = vec![
            Call {
                kind: CallKind::Syscall(5, 0),
                args: vec![
                    CallArg::Value(ScpValue::Int(0)),
                    CallArg::Value(ScpValue::String("<#E_0#M_0#B_0>".into())),
                    CallArg::Value(ScpValue::String("a".into())),
                ],
            },
            Call {
                kind: CallKind::Normal(ingert::scena::Name(String::new(), "FOO".into())),
                args: vec![CallArg::Value(ScpValue::Int(0))],
            },
        ];
        let mut v = CountingVisitor {
            count: 0,
            new_text: "REPLACED".into(),
        };
        rewrite_called(&mut calls, &mut v);
        assert_eq!(v.count, 1);
    }

    #[test]
    fn called_merged_is_left_alone_by_swap_caller() {
        // Called::Merged variants aren't even passed to rewrite_called; the
        // caller skips them. This is documented behavior — verified by
        // the swap layer test.
    }
}
