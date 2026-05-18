use ingert::scena::Expr;
use ingert::scena::Value;
use ingert::scp::CallArg;
use ingert::scp::Value as ScpValue;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TextChunk {
    Str(String),
    Newline,
}

pub type TextRun = Vec<TextChunk>;

#[must_use]
pub fn extract_run_expr(args: &[Expr]) -> Option<TextRun> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            Expr::Value(_, Value::String(s)) => out.push(TextChunk::Str(s.clone())),
            Expr::Value(_, Value::Int(10)) => out.push(TextChunk::Newline),
            _ => return None,
        }
    }
    Some(out)
}

#[must_use]
pub fn extract_run_call(args: &[CallArg]) -> Option<TextRun> {
    let mut out = Vec::with_capacity(args.len());
    for arg in args {
        match arg {
            CallArg::Value(ScpValue::String(s)) => out.push(TextChunk::Str(s.clone())),
            CallArg::Value(ScpValue::Int(10)) => out.push(TextChunk::Newline),
            _ => return None,
        }
    }
    Some(out)
}

#[must_use]
pub fn build_run_expr(run: &TextRun) -> Vec<Expr> {
    run.iter()
        .map(|chunk| match chunk {
            TextChunk::Str(s) => Expr::Value(None, Value::String(s.clone())),
            TextChunk::Newline => Expr::Value(None, Value::Int(10)),
        })
        .collect()
}

#[must_use]
pub fn build_run_call(run: &TextRun) -> Vec<CallArg> {
    run.iter()
        .map(|chunk| match chunk {
            TextChunk::Str(s) => CallArg::Value(ScpValue::String(s.clone())),
            TextChunk::Newline => CallArg::Value(ScpValue::Int(10)),
        })
        .collect()
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
    fn s(t: &str) -> TextChunk {
        TextChunk::Str(t.to_string())
    }
    const NL: TextChunk = TextChunk::Newline;

    #[test]
    fn extract_single_string() {
        let args = vec![sv("hello")];
        assert_eq!(extract_run_expr(&args), Some(vec![s("hello")]));
    }

    #[test]
    fn extract_multi_string() {
        let args = vec![sv("hello"), iv(10), sv("world")];
        assert_eq!(
            extract_run_expr(&args),
            Some(vec![s("hello"), NL, s("world")])
        );
    }

    #[test]
    fn extract_adjacent_strings_no_newline() {
        // Xseed sometimes emits "a", "b" with no Int(10) between — auto-wrap, no forced
        // newline.
        let args = vec![sv("hello"), sv("world")];
        assert_eq!(extract_run_expr(&args), Some(vec![s("hello"), s("world")]));
    }

    #[test]
    fn extract_mixed_three_strings_one_newline() {
        // "a", 10, "b", "c" — newline between a and b, no newline between b and c.
        let args = vec![sv("a"), iv(10), sv("b"), sv("c")];
        assert_eq!(
            extract_run_expr(&args),
            Some(vec![s("a"), NL, s("b"), s("c")])
        );
    }

    #[test]
    fn extract_drops_line_annotations() {
        let args = vec![sv_line("hello", 100), iv(10), sv_line("world", 101)];
        assert_eq!(
            extract_run_expr(&args),
            Some(vec![s("hello"), NL, s("world")])
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
    fn build_round_trips_adjacent_strings() {
        let run = vec![s("a"), s("b"), NL, s("c")];
        let built = build_run_expr(&run);
        assert_eq!(extract_run_expr(&built), Some(run));
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
        let new_run = vec![s("NEW1"), NL, s("NEW2")];
        replace_run_expr(&mut args, 4, &new_run);
        assert_eq!(&args[..4], &prefix_clone[..]);
        assert_eq!(args.len(), 4 + 3);
        match &args[4] {
            Expr::Value(line, Value::String(text)) => {
                assert_eq!(line, &None);
                assert_eq!(text, "NEW1");
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
            Expr::Value(line, Value::String(text)) => {
                assert_eq!(line, &None);
                assert_eq!(text, "NEW2");
            }
            _ => panic!("expected string"),
        }
    }

    #[test]
    fn build_run_no_line_annotations() {
        let run = vec![s("a"), NL, s("b")];
        let built = build_run_expr(&run);
        for e in &built {
            match e {
                Expr::Value(line, _) => assert_eq!(line, &None),
                _ => panic!("expected value"),
            }
        }
    }
}
