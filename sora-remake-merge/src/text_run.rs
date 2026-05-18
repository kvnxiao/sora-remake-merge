use ingert::scena::Expr;
use ingert::scena::Value;
use ingert::scp::CallArg;
use ingert::scp::Value as ScpValue;

pub type TextRun = Vec<String>;

#[must_use]
pub fn extract_run_expr(args: &[Expr]) -> Option<TextRun> {
    let mut out = Vec::new();
    let mut expect_string = true;
    for arg in args {
        match arg {
            Expr::Value(_, Value::String(s)) => {
                if !expect_string {
                    return None;
                }
                out.push(s.clone());
                expect_string = false;
            }
            Expr::Value(_, Value::Int(10)) => {
                if expect_string {
                    return None;
                }
                expect_string = true;
            }
            _ => return None,
        }
    }
    Some(out)
}

#[must_use]
pub fn extract_run_call(args: &[CallArg]) -> Option<TextRun> {
    let mut out = Vec::new();
    let mut expect_string = true;
    for arg in args {
        match arg {
            CallArg::Value(ScpValue::String(s)) => {
                if !expect_string {
                    return None;
                }
                out.push(s.clone());
                expect_string = false;
            }
            CallArg::Value(ScpValue::Int(10)) => {
                if expect_string {
                    return None;
                }
                expect_string = true;
            }
            _ => return None,
        }
    }
    Some(out)
}

#[must_use]
pub fn build_run_expr(run: &TextRun) -> Vec<Expr> {
    let mut out = Vec::with_capacity(run.len() * 2);
    for (i, s) in run.iter().enumerate() {
        if i > 0 {
            out.push(Expr::Value(None, Value::Int(10)));
        }
        out.push(Expr::Value(None, Value::String(s.clone())));
    }
    out
}

#[must_use]
pub fn build_run_call(run: &TextRun) -> Vec<CallArg> {
    let mut out = Vec::with_capacity(run.len() * 2);
    for (i, s) in run.iter().enumerate() {
        if i > 0 {
            out.push(CallArg::Value(ScpValue::Int(10)));
        }
        out.push(CallArg::Value(ScpValue::String(s.clone())));
    }
    out
}

pub fn replace_run_expr(args: &mut Vec<Expr>, prefix_len: usize, new_run: &TextRun) {
    args.truncate(prefix_len);
    args.extend(build_run_expr(new_run));
}

pub fn replace_run_call(args: &mut Vec<CallArg>, prefix_len: usize, new_run: &TextRun) {
    args.truncate(prefix_len);
    args.extend(build_run_call(new_run));
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::panic,
        clippy::indexing_slicing,
        reason = "tests panic on assertion failure by design"
    )]

    use super::*;

    fn iv(n: i32) -> Expr {
        Expr::Value(None, Value::Int(n))
    }
    fn sv(s: &str) -> Expr {
        Expr::Value(None, Value::String(s.to_string()))
    }
    fn iv_line(n: i32, line: u16) -> Expr {
        Expr::Value(Some(line), Value::Int(n))
    }
    fn sv_line(s: &str, line: u16) -> Expr {
        Expr::Value(Some(line), Value::String(s.to_string()))
    }

    #[test]
    fn extract_single_string() {
        let args = vec![sv("hello")];
        assert_eq!(extract_run_expr(&args), Some(vec!["hello".to_string()]));
    }

    #[test]
    fn extract_multi_string() {
        let args = vec![sv("hello"), iv(10), sv("world")];
        assert_eq!(
            extract_run_expr(&args),
            Some(vec!["hello".to_string(), "world".to_string()])
        );
    }

    #[test]
    fn extract_drops_line_annotations() {
        let args = vec![sv_line("hello", 100), iv(10), sv_line("world", 101)];
        assert_eq!(
            extract_run_expr(&args),
            Some(vec!["hello".to_string(), "world".to_string()])
        );
    }

    #[test]
    fn extract_empty() {
        let args: Vec<Expr> = vec![];
        assert_eq!(extract_run_expr(&args), Some(vec![]));
    }

    #[test]
    fn extract_rejects_non_string_non_10() {
        let args = vec![sv("hello"), iv(11)];
        assert_eq!(extract_run_expr(&args), None);
    }

    #[test]
    fn replace_preserves_prefix_byte_identical() {
        let mut args = vec![
            iv_line(134, 2489),
            iv(11),
            iv(33247),
            sv_line("<#E_0#M_0#B_0>", 2490),
            sv("OLD"),
        ];
        let prefix_clone: Vec<Expr> = args[..4].to_vec();
        let new_run = vec!["NEW1".to_string(), "NEW2".to_string()];
        replace_run_expr(&mut args, 4, &new_run);
        assert_eq!(&args[..4], &prefix_clone[..]);
        assert_eq!(args.len(), 4 + 3);
        match &args[4] {
            Expr::Value(line, Value::String(s)) => {
                assert_eq!(line, &None);
                assert_eq!(s, "NEW1");
            }
            _ => panic!("expected string"),
        }
        match &args[5] {
            Expr::Value(line, Value::Int(n)) => {
                assert_eq!(line, &None);
                assert_eq!(*n, 10);
            }
            _ => panic!("expected int 10"),
        }
        match &args[6] {
            Expr::Value(line, Value::String(s)) => {
                assert_eq!(line, &None);
                assert_eq!(s, "NEW2");
            }
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn build_run_no_line_annotations() {
        let run = vec!["a".to_string(), "b".to_string()];
        let built = build_run_expr(&run);
        for e in &built {
            match e {
                Expr::Value(line, _) => assert_eq!(line, &None),
                _ => panic!("expected value"),
            }
        }
    }
}
