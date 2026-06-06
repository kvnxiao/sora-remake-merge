use ingert::scena::Expr;
use ingert::scena::Value;
use ingert::scp::CallArg;
use ingert::scp::CallKind;
use ingert::scp::Name;
use ingert::scp::Value as ScpValue;

/// The prelude name ingert assigns to `system[22,38]`. Unlike the dialogue
/// opcodes (emitted as raw `system[5,*]`), the map-name syscall is decompiled
/// as this named alias, so its call sites are `Expr::Call` (body) and
/// `CallKind::Normal` (metadata) rather than raw syscalls — matched by name.
pub const MAPNAME_FN: &str = "ui_mapname_effect";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnchorKey {
    Portrait {
        char_id: i32,
        tag: String,
    },
    Voiced(i32),
    Letter,
    Plain,
    /// `ui_mapname_effect` (`system[22,38]`) on-screen zone label. It carries
    /// no per-call key (no `char_id`, portrait, or voice ID), so it is matched
    /// positionally within a function — the Nth EVO map-name call maps to the
    /// Nth Xseed one.
    MapName,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    pub key: AnchorKey,
    /// Number of leading args before the localized text run begins.
    pub prefix_len: usize,
    /// Length of the text run, in args. `None` means the run extends to the
    /// end of the arg list — the case for every dialogue opcode, where the
    /// localized text is the suffix. `Some(n)` means the run is exactly `n`
    /// args and everything after it is preserved verbatim — the case for
    /// `ui_mapname_effect`, whose single string is followed by numeric
    /// coordinates.
    pub run_len: Option<usize>,
}

fn as_int_expr(e: &Expr) -> Option<i32> {
    match e {
        Expr::Value(_, Value::Int(n)) => Some(*n),
        _ => None,
    }
}

fn as_string_expr(e: &Expr) -> Option<&str> {
    match e {
        Expr::Value(_, Value::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn as_int_call(a: &CallArg) -> Option<i32> {
    match a {
        CallArg::Value(ScpValue::Int(n)) => Some(*n),
        _ => None,
    }
}

fn as_string_call(a: &CallArg) -> Option<&str> {
    match a {
        CallArg::Value(ScpValue::String(s)) => Some(s.as_str()),
        _ => None,
    }
}

#[must_use]
pub fn is_localized_opcode(a: u8, b: u8) -> bool {
    matches!((a, b), (5, 0 | 6 | 8) | (22, 38))
}

#[must_use]
pub fn classify_syscall_expr(a: u8, b: u8, args: &[Expr]) -> Option<Classification> {
    classify_generic(a, b, args, as_int_expr, as_string_expr, |arg| {
        matches!(arg, Expr::Value(_, Value::String(_)))
    })
}

#[must_use]
pub fn classify_syscall_call(kind: &CallKind, args: &[CallArg]) -> Option<Classification> {
    let (a, b) = match kind {
        CallKind::Syscall(a, b) => (*a, *b),
        _ => return None,
    };
    classify_generic(a, b, args, as_int_call, as_string_call, |arg| {
        matches!(arg, CallArg::Value(ScpValue::String(_)))
    })
}

fn classify_generic<T>(
    a: u8,
    b: u8,
    args: &[T],
    as_int: impl Fn(&T) -> Option<i32>,
    as_string: impl Fn(&T) -> Option<&str>,
    is_string: impl Fn(&T) -> bool,
) -> Option<Classification> {
    if !is_localized_opcode(a, b) {
        return None;
    }
    match (a, b) {
        (5, 0 | 6) => {
            let char_id = as_int(args.first()?)?;
            for (i, arg) in args.iter().enumerate().skip(1) {
                if let Some(s) = as_string(arg)
                    && s.starts_with("<#E")
                {
                    return Some(Classification {
                        key: AnchorKey::Portrait {
                            char_id,
                            tag: s.to_owned(),
                        },
                        prefix_len: i + 1,
                        run_len: None,
                    });
                }
            }
            None
        }
        (5, 8) => classify_s58(args, &as_int, &is_string),
        (22, 38) => classify_mapname(args, &is_string),
        _ => None,
    }
}

fn classify_mapname<T>(args: &[T], is_string: &impl Fn(&T) -> bool) -> Option<Classification> {
    // ui_mapname_effect(str, num, num, num): the on-screen zone label is the
    // single leading string. The trailing numeric coordinates are not text,
    // so the run is exactly one arg and the coords are preserved verbatim.
    if !is_string(args.first()?) {
        return None;
    }
    Some(Classification {
        key: AnchorKey::MapName,
        prefix_len: 0,
        run_len: Some(1),
    })
}

/// Classify a named `Expr::Call` callee — currently only the
/// `ui_mapname_effect` prelude alias for `system[22,38]`.
#[must_use]
pub fn classify_named_call_expr(name: &Name, args: &[Expr]) -> Option<Classification> {
    classify_named(name, args, |a| {
        matches!(a, Expr::Value(_, Value::String(_)))
    })
}

/// Classify a named `CallKind::Normal` call in a called-table — the metadata
/// counterpart of [`classify_named_call_expr`].
#[must_use]
pub fn classify_named_call_call(name: &Name, args: &[CallArg]) -> Option<Classification> {
    classify_named(name, args, |a| {
        matches!(a, CallArg::Value(ScpValue::String(_)))
    })
}

fn classify_named<T>(
    name: &Name,
    args: &[T],
    is_string: impl Fn(&T) -> bool,
) -> Option<Classification> {
    if name.as_local().is_none_or(|n| n.as_str() != MAPNAME_FN) {
        return None;
    }
    classify_mapname(args, &is_string)
}

fn classify_s58<T>(
    args: &[T],
    as_int: &impl Fn(&T) -> Option<i32>,
    is_string: &impl Fn(&T) -> bool,
) -> Option<Classification> {
    let _ = as_int(args.first()?)?;
    if !args.iter().skip(1).any(is_string) {
        return None;
    }
    if let (Some(a1), Some(a2), Some(a3), Some(a4), Some(a5)) = (
        args.get(1),
        args.get(2),
        args.get(3),
        args.get(4),
        args.get(5),
    ) && as_int(a1) == Some(19)
        && as_int(a2) == Some(13)
        && as_int(a3) == Some(11)
        && let Some(v) = as_int(a4)
        && is_string(a5)
    {
        return Some(Classification {
            key: AnchorKey::Voiced(v),
            prefix_len: 5,
            run_len: None,
        });
    }
    if let (Some(a1), Some(a2), Some(a3)) = (args.get(1), args.get(2), args.get(3))
        && as_int(a1) == Some(19)
        && as_int(a2) == Some(13)
        && is_string(a3)
    {
        return Some(Classification {
            key: AnchorKey::Letter,
            prefix_len: 3,
            run_len: None,
        });
    }
    // [5,8]-voiced-plain: (65535, 11, V, strings...) — EVO upgrade of a Plain
    // line with a voice ID. Same anchor as regular Plain (positional match
    // against Xseed's Plain runs); prefix_len skips past `11, V` so the voice
    // ID survives the swap.
    if let (Some(a1), Some(a2), Some(a3)) = (args.get(1), args.get(2), args.get(3))
        && as_int(a1) == Some(11)
        && as_int(a2).is_some()
        && is_string(a3)
    {
        return Some(Classification {
            key: AnchorKey::Plain,
            prefix_len: 3,
            run_len: None,
        });
    }
    if let Some(a1) = args.get(1)
        && is_string(a1)
    {
        return Some(Classification {
            key: AnchorKey::Plain,
            prefix_len: 1,
            run_len: None,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "tests panic on assertion failure by design"
    )]

    use super::*;
    use ingert::scena::Expr;
    use ingert::scena::Value;

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
    fn portrait_no_voice_id() {
        let args = vec![iv(134), sv("<#E_0#M_0#B_0>"), sv("text")];
        let got = classify_syscall_expr(5, 0, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 134,
                tag: "<#E_0#M_0#B_0>".into()
            }
        );
        assert_eq!(got.prefix_len, 2);
    }

    #[test]
    fn portrait_with_voice_id() {
        let args = vec![iv(134), iv(11), iv(33247), sv("<#E_0#M_0#B_0>"), sv("text")];
        let got = classify_syscall_expr(5, 0, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 134,
                tag: "<#E_0#M_0#B_0>".into()
            }
        );
        assert_eq!(got.prefix_len, 4);
    }

    #[test]
    fn portrait_with_evo_voice_id() {
        let args = vec![iv(1), iv(11), iv(60589), sv("<#E_2#M_2#B_0>"), sv("text")];
        let got = classify_syscall_expr(5, 0, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 1,
                tag: "<#E_2#M_2#B_0>".into()
            }
        );
        assert_eq!(got.prefix_len, 4);
    }

    #[test]
    fn portrait_56_with_continuation_marker() {
        let args = vec![iv(2), sv("<#E_8#M_0#B_0>"), sv("<K>"), sv("text")];
        let got = classify_syscall_expr(5, 6, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 2,
                tag: "<#E_8#M_0#B_0>".into()
            }
        );
        assert_eq!(got.prefix_len, 2);
    }

    #[test]
    fn portrait_ignores_line_annotation() {
        let args = vec![
            iv_line(134, 2489),
            sv_line("<#E_0#M_0#B_0>", 2490),
            sv("text"),
        ];
        let got = classify_syscall_expr(5, 0, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 134,
                tag: "<#E_0#M_0#B_0>".into()
            }
        );
    }

    #[test]
    fn s58_params_skipped() {
        let args = vec![iv(65535), iv(16), iv(0), iv(17), iv(0), iv(0)];
        assert!(classify_syscall_expr(5, 8, &args).is_none());
    }

    #[test]
    fn s58_plain() {
        let args = vec![iv(65535), sv("<C1>text")];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Plain);
        assert_eq!(got.prefix_len, 1);
    }

    #[test]
    fn s58_letter() {
        let args = vec![iv(65535), iv(19), iv(13), sv("text")];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Letter);
        assert_eq!(got.prefix_len, 3);
    }

    #[test]
    fn s58_voiced() {
        let args = vec![iv(65535), iv(19), iv(13), iv(11), iv(34832), sv("text")];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Voiced(34832));
        assert_eq!(got.prefix_len, 5);
    }

    #[test]
    fn s58_voiced_plain_evo_upgrade() {
        // EVO upgrade of a Plain song-lyric line: (65535, 11, V, string...).
        // Anchors as Plain (positional) so it matches Xseed's regular Plain
        // run at the same position; prefix_len=3 preserves the `11, V` voice
        // ID args during the swap.
        let args = vec![iv(65535), iv(11), iv(97064), sv("<C1>lyric line")];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Plain);
        assert_eq!(got.prefix_len, 3);
    }

    #[test]
    fn mapname_single_string_with_coords() {
        // ui_mapname_effect("City of Grancel", 110.0, 600.0, 6.0). The coords
        // are floats in real data; Ints stand in here since the classifier
        // only requires args[0] to be a string and ignores the rest.
        let args = vec![sv("City of Grancel"), iv(110), iv(600), iv(6)];
        let got = classify_syscall_expr(22, 38, &args).unwrap();
        assert_eq!(got.key, AnchorKey::MapName);
        assert_eq!(got.prefix_len, 0);
        assert_eq!(got.run_len, Some(1));
    }

    #[test]
    fn mapname_requires_leading_string() {
        let args = vec![iv(0), iv(110)];
        assert!(classify_syscall_expr(22, 38, &args).is_none());
    }

    #[test]
    fn named_mapname_call_classified() {
        let args = vec![sv("Esmelas Tower"), iv(110), iv(505), iv(4)];
        let got =
            classify_named_call_expr(&Name::local("ui_mapname_effect".into()), &args).unwrap();
        assert_eq!(got.key, AnchorKey::MapName);
        assert_eq!(got.prefix_len, 0);
        assert_eq!(got.run_len, Some(1));
    }

    #[test]
    fn named_non_mapname_call_ignored() {
        let args = vec![sv("x")];
        assert!(
            classify_named_call_expr(&Name::local("camera_set_calc_mode".into()), &args).is_none()
        );
    }

    #[test]
    fn unsupported_opcode_returns_none() {
        let args = vec![iv(0)];
        assert!(classify_syscall_expr(4, 2, &args).is_none());
    }
}
