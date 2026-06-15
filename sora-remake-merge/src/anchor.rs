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

/// The prelude name ingert assigns to the menu-item-add syscall. Like
/// `ui_mapname_effect`, it is decompiled as a named alias rather than a raw
/// syscall, so its call sites are matched by name. Drives the in-game menus,
/// including the Zeiss orbal-records terminal's topic headers (`<c930>[…]`).
pub const MENU_ADDITEM_FN: &str = "menu_additem";

/// The prelude name ingert assigns to the set-display-name syscall — another
/// named alias. Sets the speaker label shown in a dialogue box (mostly a
/// character's own name, but occasionally a combined-party label such as
/// "Scherazard, Kloe, and Estelle").
pub const CHR_SET_DISPLAY_NAME_FN: &str = "chr_set_display_name";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnchorKey {
    Portrait {
        char_id: i32,
        tag: String,
    },
    /// A `[5,0]`/`[5,6]` message that carries **no** `<#…>` portrait tag —
    /// narrator/system text (`char_id` 65535, e.g. examine descriptions and
    /// `<C1>` story-recap screens) and variable-speaker lines (a `Var`
    /// `char_id`, e.g. internal monologue). With no portrait there is no
    /// per-call key, so these are matched positionally, bucketed by
    /// `char_id`: `Some(n)` for an integer channel, `None` for a variable
    /// speaker. Any voice-ID prefix sits inside the preserved prefix and is
    /// left untouched.
    Untagged {
        char_id: Option<i32>,
    },
    Voiced(i32),
    Letter,
    Plain,
    /// `[5,8]` narration whose argument list is an integer prefix followed by a
    /// text run, but which is none of the shapes above: signposts
    /// (`65535, 13, …`), device/UI panels (`65535, 26, 13, …`), and
    /// records/encyclopedia entries (`65535, 26, 22, …` / `65535, 16, 26, 22,
    /// …`). The wrapped `Vec<i32>` is the integer prefix between the `65535`
    /// channel and the first string; matching is positional within that prefix
    /// bucket, so each distinct shape advances on its own counter and never
    /// borrows another shape's runs.
    Narration(Vec<i32>),
    /// `ui_mapname_effect` (`system[22,38]`) on-screen zone label. It carries
    /// no per-call key (no `char_id`, portrait, or voice ID), so it is matched
    /// positionally within a function — the Nth EVO map-name call maps to the
    /// Nth Xseed one.
    MapName,
    /// `menu_additem` menu-entry label (e.g. the Zeiss orbal-records terminal's
    /// `<c930>[…]` topic headers). Like `MapName` it carries no per-call key,
    /// so it is matched positionally within a function. EVO's
    /// calls-table/body duplication is absorbed by the same `Site`
    /// partition and overflow rule as the dialogue opcodes.
    MenuItem,
    /// `chr_set_display_name` speaker label. Unlike the other named aliases it
    /// *does* carry a per-call key — the integer `char_id` whose slot label is
    /// being set — so matching is positional within the `(function, char_id)`
    /// bucket: a name only ever swaps against the same character's slot. Only
    /// concrete-int `char_id`s are anchored; a `Var` slot (dynamic speaker) is
    /// left untouched, since none of those differ from Xseed and pooling them
    /// would risk cross-matching distinct speakers.
    DisplayName {
        char_id: i32,
    },
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

/// A `[5,0]`/`[5,6]` portrait tag, e.g. `<#E_2#M_2#B_0>` or
/// `<#L_0#G[2]#M_2#B_0>`. The leading letter is the face set (`E`, `L`, …); it
/// is what distinguishes a portrait arg from an in-text control code like
/// `<#123I>` (digit) or a text marker like `<K>`. Anchoring on only `<#E` would
/// silently drop every line with another face set (e.g. Lugran's `<#L` lines).
fn is_portrait_tag(s: &str) -> bool {
    s.strip_prefix("<#")
        .and_then(|rest| rest.chars().next())
        .is_some_and(|c| c.is_ascii_uppercase())
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
            let char_id = as_int(args.first()?);
            // Portrait dialogue: an integer char_id followed by a `<#…>` tag.
            if let Some(char_id) = char_id {
                for (i, arg) in args.iter().enumerate().skip(1) {
                    if let Some(s) = as_string(arg)
                        && is_portrait_tag(s)
                    {
                        // The text run begins at the first string after the
                        // portrait tag. Usually that is the next arg, but some
                        // lines place the `11, V` voice ID *after* the portrait
                        // (e.g. `(2, "<#E…>", 11, 34731, "text")`); skip those
                        // ints so the voice ID stays in the preserved prefix
                        // rather than breaking the run.
                        let prefix_len = args
                            .iter()
                            .skip(i + 1)
                            .position(&is_string)
                            .map_or(i + 1, |off| i + 1 + off);
                        return Some(Classification {
                            key: AnchorKey::Portrait {
                                char_id,
                                tag: s.to_owned(),
                            },
                            prefix_len,
                            run_len: None,
                        });
                    }
                }
            }
            // Portrait-less message: narrator/system text (`char_id` 65535) or a
            // variable speaker (`char_id` is a `Var`, so `as_int` is `None`).
            // No portrait key exists, so match positionally bucketed by char_id;
            // the text run is the trailing strings, with any voice-ID prefix
            // preserved. A variable char_id never carries a portrait tag in this
            // corpus, so the first string is always the localized text.
            let first_str = args.iter().position(&is_string)?;
            Some(Classification {
                key: AnchorKey::Untagged { char_id },
                prefix_len: first_str,
                run_len: None,
            })
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

/// Classify a named `Expr::Call` callee — the `ui_mapname_effect`,
/// `menu_additem`, and `chr_set_display_name` prelude aliases.
#[must_use]
pub fn classify_named_call_expr(name: &Name, args: &[Expr]) -> Option<Classification> {
    classify_named(name, args, as_int_expr, |a| {
        matches!(a, Expr::Value(_, Value::String(_)))
    })
}

/// Classify a named `CallKind::Normal` call in a called-table — the metadata
/// counterpart of [`classify_named_call_expr`].
#[must_use]
pub fn classify_named_call_call(name: &Name, args: &[CallArg]) -> Option<Classification> {
    classify_named(name, args, as_int_call, |a| {
        matches!(a, CallArg::Value(ScpValue::String(_)))
    })
}

fn classify_named<T>(
    name: &Name,
    args: &[T],
    as_int: impl Fn(&T) -> Option<i32>,
    is_string: impl Fn(&T) -> bool,
) -> Option<Classification> {
    let local = name.as_local()?;
    match local.as_str() {
        MAPNAME_FN => classify_mapname(args, &is_string),
        MENU_ADDITEM_FN => classify_menuitem(args, &is_string),
        CHR_SET_DISPLAY_NAME_FN => classify_displayname(args, &as_int, &is_string),
        _ => None,
    }
}

fn classify_displayname<T>(
    args: &[T],
    as_int: &impl Fn(&T) -> Option<i32>,
    is_string: &impl Fn(&T) -> bool,
) -> Option<Classification> {
    // chr_set_display_name(char_id, "name"): the label is the string at arg 1.
    // Anchored only when char_id is a concrete int, so a name swaps only against
    // the same character's slot (positional within the (fn, char_id) bucket). A
    // `Var` slot (dynamic speaker) yields `None` here and is left untouched —
    // none of those differ from Xseed, and pooling them would risk
    // cross-matching distinct speakers.
    if !args.get(1).is_some_and(is_string) {
        return None;
    }
    let char_id = as_int(args.first()?)?;
    Some(Classification {
        key: AnchorKey::DisplayName { char_id },
        prefix_len: 1,
        run_len: Some(1),
    })
}

fn classify_menuitem<T>(args: &[T], is_string: &impl Fn(&T) -> bool) -> Option<Classification> {
    // menu_additem(char_id, "text", index): the localized label is the single
    // string at arg 1 (the leading arg is an int channel/menu id). The trailing
    // index is not text, so the run is exactly one arg and the index — along
    // with any other non-text tail — is preserved verbatim.
    if !args.get(1).is_some_and(is_string) {
        return None;
    }
    Some(Classification {
        key: AnchorKey::MenuItem,
        prefix_len: 1,
        run_len: Some(1),
    })
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
    let (key, prefix_len) = classify_s58_key(args, as_int, is_string)?;
    // The run is the contiguous string/newline span from `prefix_len`; any
    // trailing non-text args (e.g. a `13` record terminator) are preserved. A
    // string appearing *after* that span means the call is a parameterised
    // message whose text is split around a value placeholder — `s58_run_len`
    // returns `None` for it and we leave the whole call alone.
    let run_len = s58_run_len(args, prefix_len, as_int, is_string)?;
    Some(Classification {
        key,
        prefix_len,
        run_len: Some(run_len),
    })
}

/// Resolve a `[5,8]` argument list to its anchor key and prefix length (the
/// index of the first string). Returns `None` only when the integer prefix
/// preceding the first string contains a non-integer argument — an unexpected
/// shape we decline to touch.
fn classify_s58_key<T>(
    args: &[T],
    as_int: &impl Fn(&T) -> Option<i32>,
    is_string: &impl Fn(&T) -> bool,
) -> Option<(AnchorKey, usize)> {
    // [5,8]-voiced: (65535, 19, 13, 11, V, strings...).
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
        return Some((AnchorKey::Voiced(v), 5));
    }
    // [5,8]-letter: (65535, 19, 13, strings...).
    if let (Some(a1), Some(a2), Some(a3)) = (args.get(1), args.get(2), args.get(3))
        && as_int(a1) == Some(19)
        && as_int(a2) == Some(13)
        && is_string(a3)
    {
        return Some((AnchorKey::Letter, 3));
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
        return Some((AnchorKey::Plain, 3));
    }
    // [5,8]-plain: (65535, strings...).
    if let Some(a1) = args.get(1)
        && is_string(a1)
    {
        return Some((AnchorKey::Plain, 1));
    }
    // [5,8]-narration: an integer prefix (anything other than the shapes above)
    // followed by a text run — signposts, device/UI panels, and
    // records/encyclopedia entries. Bucket by the integer prefix so each shape
    // matches positionally against its own kind, but exclude any EVO-added
    // `11, V` voice marker from the bucket key so EVO's voiced device lines
    // (e.g. `26, 13, 11, V`) match Xseed's unvoiced shape (`26, 13`). The voice
    // marker still sits inside the preserved prefix (`prefix_len` points past
    // it), so the swap leaves it untouched.
    let first_str = args.iter().position(is_string)?;
    let mut sig = Vec::new();
    let mut i = 1;
    while i < first_str {
        let value = as_int(args.get(i)?)?;
        if value == 11 && i + 1 < first_str {
            i += 2;
        } else {
            sig.push(value);
            i += 1;
        }
    }
    Some((AnchorKey::Narration(sig), first_str))
}

/// Length of the contiguous string/`Int(10)` text run beginning at
/// `prefix_len`. Trailing non-text arguments are excluded from the run so the
/// swap preserves them verbatim (e.g. the `13` terminator on record entries).
/// Returns `None` when a string appears *after* the contiguous run — the
/// signature of a parameterised message such as `(65535, 16, "Received ", 17,
/// n, ".")`, whose text is split around a value placeholder and so cannot be
/// localised as a single trailing run.
fn s58_run_len<T>(
    args: &[T],
    prefix_len: usize,
    as_int: &impl Fn(&T) -> Option<i32>,
    is_string: &impl Fn(&T) -> bool,
) -> Option<usize> {
    let mut end = prefix_len;
    while let Some(arg) = args.get(end) {
        if is_string(arg) || as_int(arg) == Some(10) {
            end += 1;
        } else {
            break;
        }
    }
    if args.get(end..)?.iter().any(is_string) {
        return None;
    }
    Some(end - prefix_len)
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
    fn portrait_non_e_face_set() {
        // Lugran's "<#L_0#G[2]#M_2#B_0>" — a non-`<#E` portrait the old
        // `<#E`-only check silently dropped (so its text never merged).
        let args = vec![
            iv(134),
            iv(11),
            iv(34793),
            sv("<#L_0#G[2]#M_2#B_0>"),
            sv("text"),
        ];
        let got = classify_syscall_expr(5, 6, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 134,
                tag: "<#L_0#G[2]#M_2#B_0>".into()
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
    fn portrait_less_narrator() {
        // system[5,6](65535, "<C1>...") — narrator/system text, no portrait.
        let args = vec![iv(65535), sv("<C1>A card hangs from the door.")];
        let got = classify_syscall_expr(5, 6, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Untagged {
                char_id: Some(65535)
            }
        );
        assert_eq!(got.prefix_len, 1);
    }

    #[test]
    fn portrait_less_variable_speaker_with_voice() {
        // system[5,6](var, 14, 15, 11, V, "...") — dynamic speaker, no portrait.
        // char_id is a Var (as_int -> None); the voice ID stays in the prefix.
        let args = vec![
            Expr::Var(None, ingert::scena::Place::Var(ingert::scena::Var(0))),
            iv(14),
            iv(15),
            iv(11),
            iv(30546),
            sv("(I'm not going to make it in time...)"),
        ];
        let got = classify_syscall_expr(5, 6, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Untagged { char_id: None });
        assert_eq!(got.prefix_len, 5, "voice ID preserved in prefix");
    }

    #[test]
    fn voice_id_after_portrait() {
        // (char_id, "<#E…>", 11, V, "text") — voice ID placed AFTER the portrait
        // tag. prefix_len must skip past it so the run is just the text.
        let args = vec![
            iv(2),
            sv("<#E_0#M_2#B_0>"),
            iv(11),
            iv(34731),
            sv("In the meantime, let's get back to"),
        ];
        let got = classify_syscall_expr(5, 6, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Portrait {
                char_id: 2,
                tag: "<#E_0#M_2#B_0>".into()
            }
        );
        assert_eq!(
            got.prefix_len, 4,
            "prefix skips the 11, V after the portrait"
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
    fn s58_plain_trailing_terminator_preserved() {
        // Museum/exhibit entries end in a `13` record terminator after the
        // text. It must stay out of the run (preserved verbatim by the swap),
        // or the run is non-text and the whole entry goes unlocalized.
        let args = vec![iv(65535), sv("[Exhibit]"), iv(10), sv("Desc."), iv(13)];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Plain);
        assert_eq!(got.prefix_len, 1);
        assert_eq!(got.run_len, Some(3), "trailing 13 excluded from the run");
    }

    #[test]
    fn s58_narration_signpost() {
        // Direction signpost: (65535, 13, "<C1>West: ...").
        let args = vec![iv(65535), iv(13), sv("<C1>West: Bright Family House")];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Narration(vec![13]));
        assert_eq!(got.prefix_len, 2);
        assert_eq!(got.run_len, Some(1));
    }

    #[test]
    fn s58_narration_device_panel() {
        // Device/UI panel: (65535, 26, 13, "<c393>...", 10, ...).
        let args = vec![
            iv(65535),
            iv(26),
            iv(13),
            sv("<c393>Orbal Compatibility Tester"),
        ];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(got.key, AnchorKey::Narration(vec![26, 13]));
        assert_eq!(got.prefix_len, 3);
    }

    #[test]
    fn s58_narration_records_two_shapes_distinct_buckets() {
        // Records/encyclopedia entries appear in two shapes that must NOT share
        // a positional counter: (65535, 26, 22, ...) and (65535, 16, 26, 22,
        // ...).
        let a = vec![iv(65535), iv(26), iv(22), sv("<c930>Entry: ...")];
        let got_a = classify_syscall_expr(5, 8, &a).unwrap();
        assert_eq!(got_a.key, AnchorKey::Narration(vec![26, 22]));
        assert_eq!(got_a.prefix_len, 3);

        let b = vec![iv(65535), iv(16), iv(26), iv(22), sv("<c930>Entry: ...")];
        let got_b = classify_syscall_expr(5, 8, &b).unwrap();
        assert_eq!(got_b.key, AnchorKey::Narration(vec![16, 26, 22]));
        assert_eq!(got_b.prefix_len, 4);
    }

    #[test]
    fn s58_narration_strips_evo_voice_id_from_bucket() {
        // EVO inserts (11, V) into a device line: (65535, 26, 13, 11, V, ...).
        // The voice marker is excluded from the bucket key so it matches
        // Xseed's unvoiced (26, 13) shape, but prefix_len keeps it in the args.
        let args = vec![
            iv(65535),
            iv(26),
            iv(13),
            iv(11),
            iv(97148),
            sv("<c393>Orbal Compatibility Tester"),
        ];
        let got = classify_syscall_expr(5, 8, &args).unwrap();
        assert_eq!(
            got.key,
            AnchorKey::Narration(vec![26, 13]),
            "voice id excluded from the bucket key"
        );
        assert_eq!(got.prefix_len, 5, "voice id preserved in the prefix");
    }

    #[test]
    fn s58_parameterized_message_skipped() {
        // (65535, 16, "Received ", 17, n, ".") — text split around a runtime
        // value placeholder. The string after the `17, n` gap means the run
        // can't be localized as a single trailing run, so the call is skipped.
        let args = vec![iv(65535), iv(16), sv("Received "), iv(17), iv(208), sv(".")];
        assert!(classify_syscall_expr(5, 8, &args).is_none());
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
    fn menu_additem_classified() {
        // menu_additem(2, "<c930>[Orbment]", 0): label is the string at arg 1;
        // the trailing index is preserved (run is exactly one arg).
        let args = vec![iv(2), sv("<c930>[Orbment]"), iv(0)];
        let got = classify_named_call_expr(&Name::local("menu_additem".into()), &args).unwrap();
        assert_eq!(got.key, AnchorKey::MenuItem);
        assert_eq!(got.prefix_len, 1);
        assert_eq!(got.run_len, Some(1));
    }

    #[test]
    fn menu_additem_non_string_label_ignored() {
        // A menu entry whose arg 1 is not a string carries no localizable text.
        let args = vec![iv(0), iv(1)];
        assert!(classify_named_call_expr(&Name::local("menu_additem".into()), &args).is_none());
    }

    #[test]
    fn chr_set_display_name_classified() {
        // chr_set_display_name(char_id, "name"): label at arg 1, keyed by the
        // integer char_id.
        let args = vec![iv(0), sv("Scherazard, Kloe, and Estelle")];
        let got =
            classify_named_call_expr(&Name::local("chr_set_display_name".into()), &args).unwrap();
        assert_eq!(got.key, AnchorKey::DisplayName { char_id: 0 });
        assert_eq!(got.prefix_len, 1);
        assert_eq!(got.run_len, Some(1));
    }

    #[test]
    fn chr_set_display_name_var_char_id_ignored() {
        // A dynamic-speaker slot (Var char_id) is left untouched — pooling Vars
        // would risk cross-matching distinct speakers, and none of them differ
        // from Xseed anyway.
        let args = vec![
            Expr::Var(None, ingert::scena::Place::Var(ingert::scena::Var(0))),
            sv("Man in Black"),
        ];
        assert!(
            classify_named_call_expr(&Name::local("chr_set_display_name".into()), &args).is_none()
        );
    }

    #[test]
    fn unsupported_opcode_returns_none() {
        let args = vec![iv(0)];
        assert!(classify_syscall_expr(4, 2, &args).is_none());
    }
}
