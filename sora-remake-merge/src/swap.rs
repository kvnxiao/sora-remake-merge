use crate::anchor::AnchorKey;
use crate::anchor::Classification;
use crate::anchor::classify_named_call_call;
use crate::anchor::classify_named_call_expr;
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
use ingert::scena::Expr;
use ingert::scena::Function;
use ingert::scena::Scena;
use ingert::scena::Stmt;
use ingert::scena::Value;
use ingert::scp::Call;
use ingert::scp::CallArg;
use ingert::scp::CallKind;
use ingert::scp::Op;
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
/// but Xseed's body is `Tree`. The swap layer replaces EVO's body with a
/// clone of Xseed's so the runtime executes the Xseed text rather than the
/// GungHo text embedded in EVO's asm bytecode. For an `Asm` body, any voice
/// IDs EVO added in the bytecode are recovered and re-injected into the
/// clone, so the substitution loses no audio; `voice_ids_reinjected` records
/// how many.
#[derive(Debug, Clone)]
pub struct BodySubstitutionEntry {
    pub function: String,
    pub evo_body_kind: &'static str,
    pub voice_ids_reinjected: usize,
}

#[derive(Debug, Default, Clone)]
pub struct SwapStats {
    pub swaps_applied: usize,
    pub no_ops_equal: usize,
    pub unmatched_evo_calls: usize,
    pub overflow_reuses: usize,
    pub voiced_to_letter_fallback: usize,
    pub body_substitutions: usize,
    pub voice_ids_reinjected: usize,
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
        self.voice_ids_reinjected += other.voice_ids_reinjected;
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
/// gate before substituting an EVO Asm/Flat body with a clone of Xseed's
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
            // Portrait+voice: the `11, V` marker may sit before the portrait
            // (`char_id, 11, V, "<#E…>"`) or after it (`char_id, "<#E…>", 11,
            // V`). Scan the whole preserved prefix.
            AnchorKey::Portrait { .. } => {
                (1..cls.prefix_len).any(|i| is_int_11(i) && next_is_int(i + 1))
            }
            // VoicedPlain: (65535, 11, V, "…", …). Classified as Plain with
            // prefix_len 3 (vs 1 for regular Plain).
            AnchorKey::Plain => cls.prefix_len == 3 && is_int_11(1),
            // Narration / Untagged: EVO may insert an `11, V` voice marker
            // anywhere in the integer prefix (e.g. narration `26, 13, 11, V` or
            // a variable-speaker line `var, 14, 15, 11, V`). Scan the preserved
            // prefix for it.
            AnchorKey::Narration(_) | AnchorKey::Untagged { .. } => {
                (1..cls.prefix_len).any(|i| is_int_11(i) && next_is_int(i + 1))
            }
            AnchorKey::Letter
            | AnchorKey::MapName
            | AnchorKey::MenuItem
            | AnchorKey::DisplayName { .. } => false,
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

pub(crate) type Index = HashMap<(Site, AnchorKey), Vec<TextRun>>;

pub(crate) fn build_index(f: &Function) -> Index {
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

fn index_run_expr(cls: Classification, args: &[ingert::scena::Expr], b: &mut IndexBuilder) {
    let run_end = match cls.run_len {
        Some(n) => cls.prefix_len + n,
        None => args.len(),
    };
    if let Some(rest) = args.get(cls.prefix_len..run_end)
        && let Some(run) = extract_run_expr(rest)
    {
        b.push(cls.key, run);
    }
}

fn collect_expr(expr: &ingert::scena::Expr, b: &mut IndexBuilder) {
    use ingert::scena::Expr;
    match expr {
        Expr::Syscall(_, a, bb, args) => {
            for arg in args {
                collect_expr(arg, b);
            }
            if let Some(cls) = classify_syscall_expr(*a, *bb, args) {
                index_run_expr(cls, args, b);
            }
        }
        Expr::Call(_, name, args) => {
            for arg in args {
                collect_expr(arg, b);
            }
            if let Some(cls) = classify_named_call_expr(name, args) {
                index_run_expr(cls, args, b);
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
        let cls = match &call.kind {
            CallKind::Syscall(..) => classify_syscall_call(&call.kind, &call.args),
            CallKind::Normal(name) => classify_named_call_call(name, &call.args),
            CallKind::Tailcall(_) => None,
        };
        let Some(cls) = cls else { continue };
        let run_end = match cls.run_len {
            Some(n) => cls.prefix_len + n,
            None => call.args.len(),
        };
        if let Some(rest) = call.args.get(cls.prefix_len..run_end)
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
        // multiple upgraded Voiceds advance positionally through Xseed's
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
    substitute_body(name, evo, xseed, &mut visitor.stats);
    if let Body::Tree(stmts) = &mut evo.body {
        rewrite_body(stmts, &mut visitor);
    }
    if let Called::Raw(calls) = &mut evo.called {
        rewrite_called(calls, &mut visitor);
    }
    visitor.stats
}

/// A voice-ID pair recovered from an EVO `Body::Asm` dialogue call, so it can
/// be re-injected into the cloned Xseed `Body::Tree` at the same position.
#[derive(Debug, Clone, Copy)]
struct VoiceInsert {
    /// Arg index where the `11` marker sits (the value follows at `index + 1`).
    index: usize,
    /// The voice-line value that follows the `11` marker.
    value: i32,
}

/// Body substitution for a non-`Tree` EVO body paired with a `Tree` Xseed body.
///
/// Ingert's tree-mode decompiler can't always lift EVO's bytecode to a `Tree`;
/// the body walker only rewrites `Tree`, so left alone such a function keeps
/// its GungHo text embedded in the bytecode. We clone Xseed's `Tree` in its
/// place so the runtime executes Xseed's text.
///
/// For an `Asm` body this must not lose EVO's voice cues: EVO inserts them as
/// `11, V` args in the bytecode, and Xseed's body has none. We recover the
/// per-call voice IDs from the asm and re-inject them into the clone at the
/// same positions. If the asm can't be parsed into literal args, we fall back
/// to the calls-table gate (`evo_calls_have_voice_ids`): substitute only when
/// EVO added no voice there either, otherwise leave the body untouched rather
/// than drop audio. `Flat` bodies (no instances in the current corpus) take
/// that same gated path.
fn substitute_body(name: &str, evo: &mut Function, xseed: &Function, stats: &mut SwapStats) {
    if !matches!(&xseed.body, Body::Tree(_)) {
        return;
    }
    let evo_kind = body_kind(&evo.body);
    let inserts = match &evo.body {
        Body::Asm(ops) => extract_body_voice_ids(ops),
        Body::Flat(_) => None,
        Body::Tree(_) => return,
    };
    let record = |stats: &mut SwapStats, reinjected: usize| {
        stats.body_substitutions += 1;
        stats.voice_ids_reinjected += reinjected;
        stats.body_subs.push(BodySubstitutionEntry {
            function: name.to_owned(),
            evo_body_kind: evo_kind,
            voice_ids_reinjected: reinjected,
        });
    };
    match inserts {
        // Asm parsed cleanly and carries voice cues: clone Xseed's body, then
        // re-inject the recovered voice IDs. Gate on the dialogue-call counts
        // matching so positional injection can't misplace a cue; if they don't,
        // leave EVO's body alone to preserve the audio.
        Some(inserts) if inserts.iter().any(Option::is_some) => {
            let Body::Tree(xseed_stmts) = &xseed.body else {
                return;
            };
            if count_dialogue_syscalls(xseed_stmts) != inserts.len() {
                return;
            }
            adopt_xseed_body(evo, xseed);
            let reinjected = if let Body::Tree(stmts) = &mut evo.body {
                inject_voice_ids(stmts, &inserts)
            } else {
                0
            };
            record(stats, reinjected);
        }
        // Asm parsed cleanly with no voice cues (or an empty body): a plain
        // voiceless clone is faithful — there is no audio to preserve.
        Some(_) => {
            adopt_xseed_body(evo, xseed);
            record(stats, 0);
        }
        // Couldn't parse the asm (or a Flat body): fall back to the calls-table
        // gate. Substitute only if EVO added no voice there.
        None => {
            if !evo_calls_have_voice_ids(&evo.called) {
                adopt_xseed_body(evo, xseed);
                record(stats, 0);
            }
        }
    }
}

/// Replace EVO's body *and* called-table with Xseed's, keeping the two from the
/// same source. Ingert's `compile` writes a `Called::Raw` table to the `.dat`
/// verbatim, with no check that it matches the code; pairing EVO's asm-derived
/// raw table with Xseed's substituted `Tree` body yields a `.dat` whose called
/// table disagrees with its body (e.g. `camera_lookat` arg counts differ),
/// which the engine mis-reads and hangs on. Adopting Xseed's `Called::Merged`
/// makes ingert re-infer the table from the (now Xseed) body at compile time,
/// so it is always consistent. Voice IDs re-injected afterward live only in the
/// body, mirroring how EVO ships voiced calls (voice in the body, not the
/// table).
fn adopt_xseed_body(evo: &mut Function, xseed: &Function) {
    evo.body = xseed.body.clone();
    evo.called = xseed.called.clone();
}

/// Recover per-dialogue-call voice IDs from an EVO `Body::Asm` op stream, in
/// body order. Returns one entry per `system[5,{0,6,8}]` call (`Some` if it
/// carries an `11, V` voice pair, `None` if unvoiced). Returns `None` for the
/// whole function if any dialogue call's args can't be reconstructed from
/// literal pushes, so the caller can fall back conservatively.
fn extract_body_voice_ids(ops: &[Op]) -> Option<Vec<Option<VoiceInsert>>> {
    let mut out = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let Op::CallSystem(a, b, argc) = op else {
            continue;
        };
        if *a != 5 || !matches!(*b, 0 | 6 | 8) {
            continue;
        }
        let call_args = reconstruct_call_args(ops, i, usize::from(*argc))?;
        out.push(find_voice_pair(*a, *b, &call_args));
    }
    Some(out)
}

/// Reconstruct a syscall's argument list, in source order, by walking backward
/// over the literal `Push` ops preceding the `CallSystem` at `call_idx`. Args
/// are pushed in reverse, so the push nearest the call is `arg[0]`. Returns
/// `None` if a non-literal operand (computed value, null) sits in the window —
/// dialogue calls use only literal args, so that signals "leave this alone".
fn reconstruct_call_args(ops: &[Op], call_idx: usize, argc: usize) -> Option<Vec<Value>> {
    let mut vals = Vec::with_capacity(argc);
    let mut i = call_idx;
    while vals.len() < argc {
        i = i.checked_sub(1)?;
        match ops.get(i)? {
            Op::Push(v) => vals.push(v.clone()),
            // Labels and source-line markers don't touch the operand stack.
            Op::Label(_) | Op::Line(_) => {}
            _ => return None,
        }
    }
    Some(vals)
}

/// Locate the `11, V` voice pair in a reconstructed dialogue-call arg list,
/// bounded to the classified prefix so an in-text or trailing literal `11`
/// can't be mistaken for a voice marker. Mirrors `evo_calls_have_voice_ids`.
fn find_voice_pair(a: u8, b: u8, args: &[Value]) -> Option<VoiceInsert> {
    let call_args: Vec<CallArg> = args.iter().cloned().map(CallArg::Value).collect();
    let cls = classify_syscall_call(&CallKind::Syscall(a, b), &call_args)?;
    for i in 1..cls.prefix_len {
        if matches!(args.get(i), Some(Value::Int(11)))
            && let Some(Value::Int(value)) = args.get(i + 1)
        {
            return Some(VoiceInsert {
                index: i,
                value: *value,
            });
        }
    }
    None
}

/// Count `system[5,{0,6,8}]` calls in a tree body, in the order
/// [`inject_voice_ids`] visits them.
fn count_dialogue_syscalls(stmts: &[Stmt]) -> usize {
    let mut n = 0;
    for stmt in stmts {
        walk_dialogue_stmt(stmt, &mut |_| n += 1);
    }
    n
}

/// Inject recovered voice IDs into a cloned Xseed tree body. Visits
/// `system[5,{0,6,8}]` calls in body order, matching `inserts` positionally,
/// and splices `11, V` into each voiced call's args at the recorded index.
/// Returns the number of calls that received a voice ID.
fn inject_voice_ids(stmts: &mut [Stmt], inserts: &[Option<VoiceInsert>]) -> usize {
    let mut idx = 0;
    let mut injected = 0;
    for stmt in stmts {
        walk_dialogue_stmt_mut(stmt, &mut |args| {
            let cur = idx;
            idx += 1;
            if let Some(vi) = inserts.get(cur).copied().flatten()
                && vi.index <= args.len()
            {
                args.insert(vi.index, Expr::Value(None, Value::Int(vi.value)));
                args.insert(vi.index, Expr::Value(None, Value::Int(11)));
                injected += 1;
            }
        });
    }
    injected
}

/// Visit the arg list of every `system[5,{0,6,8}]` call in a statement, in
/// source order (mirrors `walker::rewrite_stmt`). Immutable counting variant.
fn walk_dialogue_stmt(stmt: &Stmt, f: &mut impl FnMut(&[Expr])) {
    match stmt {
        Stmt::Expr(e) | Stmt::Set(_, _, e) => walk_dialogue_expr(e, f),
        Stmt::Return(_, e) | Stmt::PushVar(_, _, e) => {
            if let Some(e) = e {
                walk_dialogue_expr(e, f);
            }
        }
        Stmt::If(_, cond, then, els) => {
            walk_dialogue_expr(cond, f);
            for s in then {
                walk_dialogue_stmt(s, f);
            }
            if let Some(els) = els {
                for s in els {
                    walk_dialogue_stmt(s, f);
                }
            }
        }
        Stmt::While(_, cond, body) => {
            walk_dialogue_expr(cond, f);
            for s in body {
                walk_dialogue_stmt(s, f);
            }
        }
        Stmt::Switch(_, scrut, cases) => {
            walk_dialogue_expr(scrut, f);
            for arm in cases.values() {
                for s in arm {
                    walk_dialogue_stmt(s, f);
                }
            }
        }
        Stmt::Block(stmts) => {
            for s in stmts {
                walk_dialogue_stmt(s, f);
            }
        }
        Stmt::Debug(_, args) | Stmt::Tailcall(_, _, args) => {
            for a in args {
                walk_dialogue_expr(a, f);
            }
        }
        Stmt::Break | Stmt::Continue => {}
    }
}

fn walk_dialogue_expr(expr: &Expr, f: &mut impl FnMut(&[Expr])) {
    match expr {
        Expr::Syscall(_, a, b, args) => {
            for arg in args {
                walk_dialogue_expr(arg, f);
            }
            if *a == 5 && matches!(*b, 0 | 6 | 8) {
                f(args);
            }
        }
        Expr::Call(_, _, args) => {
            for arg in args {
                walk_dialogue_expr(arg, f);
            }
        }
        Expr::Unop(_, _, inner) => walk_dialogue_expr(inner, f),
        Expr::Binop(_, _, l, r) => {
            walk_dialogue_expr(l, f);
            walk_dialogue_expr(r, f);
        }
        Expr::Value(_, _) | Expr::Var(_, _) | Expr::Ref(_, _) => {}
    }
}

/// Mutable twin of [`walk_dialogue_stmt`]: hands each `system[5,{0,6,8}]`
/// call's arg list to `f` for in-place editing, in the same source order.
fn walk_dialogue_stmt_mut(stmt: &mut Stmt, f: &mut impl FnMut(&mut Vec<Expr>)) {
    match stmt {
        Stmt::Expr(e) | Stmt::Set(_, _, e) => walk_dialogue_expr_mut(e, f),
        Stmt::Return(_, e) | Stmt::PushVar(_, _, e) => {
            if let Some(e) = e {
                walk_dialogue_expr_mut(e, f);
            }
        }
        Stmt::If(_, cond, then, els) => {
            walk_dialogue_expr_mut(cond, f);
            for s in then.iter_mut() {
                walk_dialogue_stmt_mut(s, f);
            }
            if let Some(els) = els {
                for s in els.iter_mut() {
                    walk_dialogue_stmt_mut(s, f);
                }
            }
        }
        Stmt::While(_, cond, body) => {
            walk_dialogue_expr_mut(cond, f);
            for s in body.iter_mut() {
                walk_dialogue_stmt_mut(s, f);
            }
        }
        Stmt::Switch(_, scrut, cases) => {
            walk_dialogue_expr_mut(scrut, f);
            for arm in cases.values_mut() {
                for s in arm.iter_mut() {
                    walk_dialogue_stmt_mut(s, f);
                }
            }
        }
        Stmt::Block(stmts) => {
            for s in stmts.iter_mut() {
                walk_dialogue_stmt_mut(s, f);
            }
        }
        Stmt::Debug(_, args) | Stmt::Tailcall(_, _, args) => {
            for a in args.iter_mut() {
                walk_dialogue_expr_mut(a, f);
            }
        }
        Stmt::Break | Stmt::Continue => {}
    }
}

fn walk_dialogue_expr_mut(expr: &mut Expr, f: &mut impl FnMut(&mut Vec<Expr>)) {
    match expr {
        Expr::Syscall(_, a, b, args) => {
            for arg in args.iter_mut() {
                walk_dialogue_expr_mut(arg, f);
            }
            if *a == 5 && matches!(*b, 0 | 6 | 8) {
                f(args);
            }
        }
        Expr::Call(_, _, args) => {
            for arg in args.iter_mut() {
                walk_dialogue_expr_mut(arg, f);
            }
        }
        Expr::Unop(_, _, inner) => walk_dialogue_expr_mut(inner, f),
        Expr::Binop(_, _, l, r) => {
            walk_dialogue_expr_mut(l, f);
            walk_dialogue_expr_mut(r, f);
        }
        Expr::Value(_, _) | Expr::Var(_, _) | Expr::Ref(_, _) => {}
    }
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
    use ingert::scp::Op;

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
        // Reverse order in Xseed to prove the match is by voice ID, not position.
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

    fn narrator_call(text: &str) -> Expr {
        // system[5,6](65535, "text") — portrait-less narrator/system message.
        Expr::Syscall(None, 5, 6, vec![iv(65535), sv(text)])
    }
    fn portrait_call_voice_after(char_id: i32, tag: &str, voice: i32, text: &str) -> Expr {
        // (char_id, "<#E…>", 11, V, "text") — voice ID placed AFTER the portrait.
        Expr::Syscall(
            None,
            5,
            6,
            vec![iv(char_id), sv(tag), iv(11), iv(voice), sv(text)],
        )
    }
    fn var_speaker_voiced_call(voice: i32, text: &str) -> Expr {
        // (var, 14, 15, 11, V, "text") — dynamic speaker, no portrait. char_id is
        // a Var (as_int -> None) so it anchors as Untagged{None}; the voice ID
        // stays in the preserved prefix.
        Expr::Syscall(
            None,
            5,
            6,
            vec![
                Expr::Var(None, ingert::scena::Place::Var(ingert::scena::Var(0))),
                iv(14),
                iv(15),
                iv(11),
                iv(voice),
                sv(text),
            ],
        )
    }

    #[test]
    fn portrait_less_narrator_swapped_positionally() {
        // mp1110 EV_01_53_00 pattern: a 65535 narrator line with no portrait
        // tag. It carries no per-call key, so it matches positionally within the
        // Untagged{Some(65535)} bucket; the char_id is preserved.
        let evo_fn = make_fn(vec![Stmt::Expr(narrator_call("Men can be heard talking."))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(narrator_call(
            "The voices of some men can be heard.",
        ))]);
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
        // char_id 65535 preserved.
        assert!(matches!(&args[0], Expr::Value(_, Value::Int(65535))));
        let Expr::Value(_, Value::String(t)) = &args[1] else {
            unreachable!()
        };
        assert_eq!(t, "The voices of some men can be heard.");
    }

    #[test]
    fn voice_id_after_portrait_swapped_preserving_voice() {
        // mp1110 EV_01_60_00 Bose line: the voice ID sits AFTER the portrait tag
        // (2, "<#E…>", 11, 34731, "text"). The text run swaps; the voice ID
        // survives in the preserved prefix between the tag and the text.
        let evo_fn = make_fn(vec![Stmt::Expr(portrait_call_voice_after(
            2,
            "<#E_0#M_2#B_0>",
            34731,
            "For now, we should head back to the",
        ))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call_voice_after(
            2,
            "<#E_0#M_2#B_0>",
            34731,
            "In the meantime, let's get back to",
        ))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 1);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let Stmt::Expr(Expr::Syscall(_, _, _, args)) = &body[0] else {
            unreachable!()
        };
        // Voice ID after the portrait preserved.
        assert!(matches!(&args[2], Expr::Value(_, Value::Int(11))));
        assert!(matches!(&args[3], Expr::Value(_, Value::Int(34731))));
        let Expr::Value(_, Value::String(t)) = &args[4] else {
            unreachable!()
        };
        assert_eq!(t, "In the meantime, let's get back to");
    }

    #[test]
    fn variable_speaker_swapped_preserving_voice() {
        // Portrait-less variable speaker (internal monologue): the char_id is a
        // Var, so it anchors as Untagged{None} and matches positionally. The
        // voice ID in the prefix survives the swap.
        let evo_fn = make_fn(vec![Stmt::Expr(var_speaker_voiced_call(
            30546,
            "(EVO old monologue...)",
        ))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(var_speaker_voiced_call(
            30546,
            "(I'm not going to make it in time...)",
        ))]);
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
        // Var speaker and voice-ID prefix preserved.
        assert!(matches!(&args[0], Expr::Var(_, _)));
        assert!(matches!(&args[3], Expr::Value(_, Value::Int(11))));
        assert!(matches!(&args[4], Expr::Value(_, Value::Int(30546))));
        let Expr::Value(_, Value::String(t)) = &args[5] else {
            unreachable!()
        };
        assert_eq!(t, "(I'm not going to make it in time...)");
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
        // follow-ups in mp1010_04 EV_01_61_00). Xseed still has them as
        // Letters with re-translated text. The fallback should consume
        // Xseed's Letter runs in source order.
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
        // returns AnchorKey::Plain with prefix_len=3, matching Xseed's
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

    fn s58_narration(prefix: &[i32], text: &str) -> Expr {
        let mut args = vec![iv(65535)];
        args.extend(prefix.iter().map(|&n| iv(n)));
        args.push(sv(text));
        Expr::Syscall(None, 5, 8, args)
    }

    #[test]
    fn s58_narration_swapped_positionally() {
        // Two device/UI panels (26, 13). Xseed re-translates both; the merge
        // matches them positionally within the Narration([26, 13]) bucket.
        let evo_fn = make_fn(vec![
            Stmt::Expr(s58_narration(&[26, 13], "Orbal Fortune Machine")),
            Stmt::Expr(s58_narration(&[26, 13], "Would you like a reading?")),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(s58_narration(&[26, 13], "Orbal Compatibility Tester")),
            Stmt::Expr(s58_narration(&[26, 13], "Begin test?")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        assert_eq!(stats.unmatched_evo_calls, 0);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let texts: Vec<&str> = body
            .iter()
            .map(|s| {
                let Stmt::Expr(Expr::Syscall(_, _, _, args)) = s else {
                    unreachable!()
                };
                let Expr::Value(_, Value::String(t)) = &args[3] else {
                    unreachable!()
                };
                t.as_str()
            })
            .collect();
        assert_eq!(texts, vec!["Orbal Compatibility Tester", "Begin test?"]);
    }

    #[test]
    fn s58_narration_evo_voice_id_preserved() {
        // EVO inserts (11, V) on a device line: (65535, 26, 13, 11, V, "..."),
        // which Xseed has unvoiced as (65535, 26, 13, "..."). The text swaps;
        // the voice ID survives in the preserved prefix.
        let evo_fn = make_fn(vec![Stmt::Expr(s58_narration(
            &[26, 13, 11, 97148],
            "Orbal Fortune Machine",
        ))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(s58_narration(
            &[26, 13],
            "Orbal Compatibility Tester",
        ))]);
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
        assert!(matches!(&args[3], Expr::Value(_, Value::Int(11))));
        assert!(matches!(&args[4], Expr::Value(_, Value::Int(97148))));
        let Expr::Value(_, Value::String(t)) = &args[5] else {
            unreachable!()
        };
        assert_eq!(t, "Orbal Compatibility Tester");
    }

    #[test]
    fn s58_plain_trailing_terminator_preserved_on_swap() {
        // Museum/exhibit entry ending in a `13` record terminator. The text
        // swaps; the trailing 13 survives untouched.
        let evo_fn = make_fn(vec![Stmt::Expr(Expr::Syscall(
            None,
            5,
            8,
            vec![iv(65535), sv("Outer Wall of the Tetracyclic Tower"), iv(13)],
        ))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(Expr::Syscall(
            None,
            5,
            8,
            vec![
                iv(65535),
                sv("Tetracyclic Tower Outer Wall Segment"),
                iv(13),
            ],
        ))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 1);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let Stmt::Expr(Expr::Syscall(_, _, _, args)) = &body[0] else {
            unreachable!()
        };
        let Expr::Value(_, Value::String(t)) = &args[1] else {
            unreachable!()
        };
        assert_eq!(t, "Tetracyclic Tower Outer Wall Segment");
        assert_eq!(args.len(), 3, "trailing terminator preserved");
        assert!(matches!(&args[2], Expr::Value(_, Value::Int(13))));
    }

    #[test]
    fn s58_parameterized_message_left_untouched() {
        // (65535, 16, "Received ", 17, n, ".") — text split around a runtime
        // value. Not localizable as a single run, so it is left byte-identical
        // even when Xseed's wording differs.
        let evo_fn = make_fn(vec![Stmt::Expr(Expr::Syscall(
            None,
            5,
            8,
            vec![iv(65535), iv(16), sv("Received "), iv(17), iv(208), sv(".")],
        ))]);
        let evo_orig = evo_fn.clone();
        let xseed_fn = make_fn(vec![Stmt::Expr(Expr::Syscall(
            None,
            5,
            8,
            vec![iv(65535), iv(16), sv("Got "), iv(17), iv(208), sv(".")],
        ))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 0);
        assert_eq!(stats.unmatched_evo_calls, 0, "skipped, not unmatched");
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let Body::Tree(orig) = &evo_orig.body else {
            unreachable!()
        };
        assert_eq!(body, orig);
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
        // EVO body is Asm (ingert couldn't decompile to Tree) but Xseed body
        // is Tree, and the asm carries no voice cues. The swap layer clones
        // Xseed's body into EVO so the runtime executes Xseed text rather than
        // the GungHo text embedded in EVO's asm bytecode. Nothing to re-inject.
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
        assert_eq!(stats.voice_ids_reinjected, 0);
        assert_eq!(stats.body_subs.len(), 1);
        assert_eq!(stats.body_subs[0].function, "F");
        assert_eq!(stats.body_subs[0].evo_body_kind, "asm");
        assert_eq!(stats.body_subs[0].voice_ids_reinjected, 0);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!("body should have been substituted to Tree")
        };
        assert_eq!(body, &xseed_body);
        // The called-table is adopted from Xseed too, so ingert re-infers it
        // from the substituted body instead of keeping EVO's asm-derived table
        // (which would disagree with the body and hang the engine).
        assert_eq!(evo.functions["F"].called, Called::Merged(false));
    }

    #[test]
    fn asm_body_voice_ids_reinjected_into_substituted_tree() {
        // mp3010_01 QS300_01_00 case: EVO's Asm body carries voice cues that
        // exist only in the bytecode (not the calls-table). Cloning Xseed's
        // voiceless Tree must not drop them — the swap recovers each `11, V`
        // pair from the asm and re-injects it into the clone at the same
        // position, yielding Xseed text with EVO voice.
        //
        // Asm for `system[5,0](0, 11, 60589, "<#E_0>", "GungHo")`: args are
        // pushed in reverse, char_id last, right before the CallSystem.
        let asm = vec![
            Op::Push(Value::String("GungHo".into())),
            Op::Push(Value::String("<#E_0>".into())),
            Op::Push(Value::Int(60589)),
            Op::Push(Value::Int(11)),
            Op::Push(Value::Int(0)),
            Op::CallSystem(5, 0, 5),
            Op::Pop(5),
        ];
        let evo_fn = Function {
            args: Vec::new(),
            called: Called::Raw(Vec::new()),
            is_prelude: false,
            body: Body::Asm(asm),
        };
        let xseed_fn = make_fn(vec![Stmt::Expr(portrait_call(0, "<#E_0>", "XSEED"))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.body_substitutions, 1);
        assert_eq!(stats.voice_ids_reinjected, 1);
        assert_eq!(stats.body_subs[0].voice_ids_reinjected, 1);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!("body should have been substituted to Tree")
        };
        // Xseed text ("XSEED") with EVO voice (11, 60589) restored in place.
        assert_eq!(
            body,
            &vec![Stmt::Expr(portrait_call_voiced(
                0, 60589, "<#E_0>", "XSEED"
            ))]
        );
        // Called-table adopted from Xseed; voice lives only in the body, so the
        // re-inferred table stays consistent with the code (see adopt_xseed_body).
        assert_eq!(evo.functions["F"].called, Called::Merged(false));
    }

    #[test]
    fn asm_body_left_alone_when_voice_present_but_calls_misalign() {
        // Safety fallback: the asm carries a voice cue but the dialogue-call
        // counts between EVO's asm and Xseed's tree don't match, so positional
        // injection could misplace it. Leave EVO's asm body untouched rather
        // than drop or misassign audio.
        let asm = vec![
            Op::Push(Value::String("GungHo".into())),
            Op::Push(Value::String("<#E_0>".into())),
            Op::Push(Value::Int(60589)),
            Op::Push(Value::Int(11)),
            Op::Push(Value::Int(0)),
            Op::CallSystem(5, 0, 5),
            Op::Pop(5),
        ];
        let evo_fn = Function {
            args: Vec::new(),
            called: Called::Raw(Vec::new()),
            is_prelude: false,
            body: Body::Asm(asm),
        };
        // Xseed tree has two dialogue calls vs the asm's one — a count mismatch.
        let xseed_fn = make_fn(vec![
            Stmt::Expr(portrait_call(0, "<#E_0>", "XSEED one")),
            Stmt::Expr(portrait_call(0, "<#E_0>", "XSEED two")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.body_substitutions, 0);
        assert_eq!(stats.voice_ids_reinjected, 0);
        assert!(matches!(&evo.functions["F"].body, Body::Asm(_)));
        // No substitution, so EVO's called-table is kept as-is — it stays
        // consistent with EVO's own asm body that we left in place.
        assert!(matches!(&evo.functions["F"].called, Called::Raw(_)));
    }

    fn mapname_call(name: &str) -> Expr {
        // ui_mapname_effect("Name", 110, 505, 4). Int coords stand in for the
        // real floats; the swap preserves the trailing args verbatim either way.
        Expr::Syscall(None, 22, 38, vec![sv(name), iv(110), iv(505), iv(4)])
    }

    #[test]
    fn mapname_swapped_positionally_preserving_coords() {
        // Two identical map-name calls (mp1110 ships "Sky Pirate Stronghold"
        // twice). Both swap to Xseed's v1.5 retitle; the numeric coords after
        // the string survive untouched.
        let evo_fn = make_fn(vec![
            Stmt::Expr(mapname_call("Sky Pirate Stronghold")),
            Stmt::Expr(mapname_call("Sky Pirate Stronghold")),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(mapname_call("Sky Bandit Stronghold")),
            Stmt::Expr(mapname_call("Sky Bandit Stronghold")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        for stmt in body {
            let Stmt::Expr(Expr::Syscall(_, 22, 38, args)) = stmt else {
                unreachable!()
            };
            let Expr::Value(_, Value::String(name)) = &args[0] else {
                unreachable!()
            };
            assert_eq!(name, "Sky Bandit Stronghold");
            assert_eq!(args.len(), 4, "coords must be preserved");
            assert!(matches!(&args[1], Expr::Value(_, Value::Int(110))));
            assert!(matches!(&args[2], Expr::Value(_, Value::Int(505))));
            assert!(matches!(&args[3], Expr::Value(_, Value::Int(4))));
        }
    }

    #[test]
    fn mapname_unchanged_label_is_noop() {
        let evo_fn = make_fn(vec![Stmt::Expr(mapname_call("City of Rolent"))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(mapname_call("City of Rolent"))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 0);
        assert_eq!(stats.no_ops_equal, 1);
    }

    fn menuitem_call(char_id: i32, label: &str, index: i32) -> Expr {
        // menu_additem(char_id, "label", index) — a named prelude-alias call
        // (like ui_mapname_effect), not a raw syscall. Drives the
        // records-terminal topic headers.
        Expr::Call(
            None,
            ingert::scp::Name::local("menu_additem".into()),
            vec![iv(char_id), sv(label), iv(index)],
        )
    }

    #[test]
    fn menuitem_swapped_positionally_preserving_index() {
        // mp3010_01 LP_Capel records headers: positional within the function,
        // and the trailing menu-index arg survives the label swap.
        let evo_fn = make_fn(vec![
            Stmt::Expr(menuitem_call(1, "<c930>[History]", 0)),
            Stmt::Expr(menuitem_call(2, "<c930>[Orbment]", 0)),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(menuitem_call(1, "<c930>[Establishment]", 0)),
            Stmt::Expr(menuitem_call(2, "<c930>[Orbments]", 0)),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        let Body::Tree(body) = &evo.functions["F"].body else {
            unreachable!()
        };
        let labels: Vec<&str> = body
            .iter()
            .map(|s| {
                let Stmt::Expr(Expr::Call(_, _, args)) = s else {
                    unreachable!()
                };
                let Expr::Value(_, Value::String(t)) = &args[1] else {
                    unreachable!()
                };
                // char_id and trailing index both preserved.
                assert!(matches!(&args[0], Expr::Value(_, Value::Int(_))));
                assert!(matches!(&args[2], Expr::Value(_, Value::Int(0))));
                t.as_str()
            })
            .collect();
        assert_eq!(labels, vec!["<c930>[Establishment]", "<c930>[Orbments]"]);
    }

    #[test]
    fn menuitem_unchanged_label_is_noop() {
        let evo_fn = make_fn(vec![Stmt::Expr(menuitem_call(2, "<c930>[Quartz]", 1))]);
        let xseed_fn = make_fn(vec![Stmt::Expr(menuitem_call(2, "<c930>[Quartz]", 1))]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 0);
        assert_eq!(stats.no_ops_equal, 1);
    }

    fn display_name_call(char_id: i32, name: &str) -> Expr {
        // chr_set_display_name(char_id, "name") — a named prelude-alias call.
        Expr::Call(
            None,
            ingert::scp::Name::local("chr_set_display_name".into()),
            vec![iv(char_id), sv(name)],
        )
    }

    fn display_names(scena: &Scena, function: &str) -> Vec<(i32, String)> {
        let Body::Tree(body) = &scena.functions[function].body else {
            unreachable!()
        };
        body.iter()
            .map(|s| {
                let Stmt::Expr(Expr::Call(_, _, args)) = s else {
                    unreachable!()
                };
                let Expr::Value(_, Value::Int(c)) = &args[0] else {
                    unreachable!()
                };
                let Expr::Value(_, Value::String(n)) = &args[1] else {
                    unreachable!()
                };
                (*c, n.clone())
            })
            .collect()
    }

    #[test]
    fn display_name_group_label_swapped_preserving_char_id() {
        // mp0000_ev / mp4000_ev group labels: each name swaps to Xseed's wording
        // and the leading char_id is preserved.
        let evo_fn = make_fn(vec![
            Stmt::Expr(display_name_call(0, "Scherazard, Kloe, & Estelle")),
            Stmt::Expr(display_name_call(10066, "Lonnie, Dino, & Lyle")),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(display_name_call(0, "Scherazard, Kloe, and Estelle")),
            Stmt::Expr(display_name_call(10066, "Lonnie, Dino & Lyle")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        assert_eq!(
            display_names(&evo, "F"),
            vec![
                (0, "Scherazard, Kloe, and Estelle".to_owned()),
                (10066, "Lonnie, Dino & Lyle".to_owned()),
            ]
        );
    }

    #[test]
    fn display_name_keyed_by_char_id_not_position() {
        // Xseed lists the two characters in the OPPOSITE order. Because the key
        // is the char_id, each EVO name still matches its own character's slot —
        // not whatever sits at the same position.
        let evo_fn = make_fn(vec![
            Stmt::Expr(display_name_call(5, "EVO five")),
            Stmt::Expr(display_name_call(9, "EVO nine")),
        ]);
        let xseed_fn = make_fn(vec![
            Stmt::Expr(display_name_call(9, "Xseed nine")),
            Stmt::Expr(display_name_call(5, "Xseed five")),
        ]);
        let mut evo = make_scena("F", evo_fn);
        let xseed = make_scena("F", xseed_fn);
        let stats = swap_scena(&mut evo, &xseed);
        assert_eq!(stats.swaps_applied, 2);
        assert_eq!(
            display_names(&evo, "F"),
            vec![(5, "Xseed five".to_owned()), (9, "Xseed nine".to_owned())]
        );
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
