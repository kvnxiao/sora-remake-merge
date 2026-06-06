# Sora 1st Chapter — Xseed localization into EVO Voice mod

## Goal

Merge the **Xseed English localization** into the **EVO Voice mod** scripts.

The EVO Voice mod re-adds audio from the EVO edition of the original (non-remake) game, but ships with the **GungHo localization**, which is weaker than Xseed's. For every dialogue line that exists in both versions, force the EVO script to use Xseed's wording. Preserve EVO-only lines that exist to back the extra voiced audio.

## Directories

All three corpora live under `resources/` (read-only inputs). The merge tool writes its results to a separate `output/` tree, which is gitignored.

| Path | Role | Mutability |
| --- | --- | --- |
| `resources/xseed-restoration/` | Xseed English localization — **source of truth** for text | **Read-only.** |
| `resources/evo-voice-mod/` | EVO Voice mod scripts — merge input | **Read-only.** |
| `resources/original/` | As-shipped GungHo English decompile — baseline both other corpora mod from | **Read-only.** Verification reference only. |
| `output/` | Merged EVO scripts (EVO structure + Xseed text) | Generated; not checked in. |

Each `script_en/scena/X.dat` has a matching `script_en/scena/X.ing`. The merge operates on `.ing`. `.dat` is the compiled binary; `.ing` is the human-readable decompiled form.

`resources/original/` and `resources/xseed-restoration/` are structurally identical (same line counts, same function-block shapes) — Xseed is a pure text-overlay mod. `resources/evo-voice-mod/` adds calls-table metadata blocks, voice-ID args, and new `[5,6]` continuation calls on top of the same `original/` baseline. The merge tool reconstructs: EVO structure + Xseed text.

## File format (`.ing`)

Decompiler output from Ingert (`C:/Users/kvnxiao/github/Ingert/`). Two structural pieces matter:

### Function shape

```
fn FOO(args) calls { ...called table... } { ...code body... }
```

The first `{ }` after `calls` is **called-table metadata** — every external/system call this function makes, listed with its args. The second `{ }` is the **code body** that executes at runtime. Both blocks contain the same `system[5,*]` dialogue calls (metadata vs. control-flow expressions). **Both must be swapped to stay consistent** (the compiler warns on mismatch).

Simple functions may omit the `calls { }` block entirely, or use `dup` (called-table mirrors body verbatim).

### Dialogue calls

Three syscall opcodes carry dialogue text:

- `system[5,0](char_id, [voice_ids…], "<#E…>", ["<K>",] "text", 10, "text", …)` — message box
- `system[5,6](char_id, [voice_ids…], "<#E…>", ["<K>",] "text", …)` — voiced/continuation message (identical shape to `[5,0]`)
- `system[5,8](65535, [shape-specific prefix,] "text", …)` — narration; multiple shapes exist (parameter-only, plain, letter, voiced-letter)

A fourth call carries the on-screen zone label, emitted by the decompiler as a named prelude alias rather than a raw syscall:

- `ui_mapname_effect("text", x, y, scale)` (the alias for `system[22,38]`) — map-name label. The localized text is the single **leading** string; the trailing numeric coordinates are preserved. It has no `char_id`/portrait/voice key, so it is matched **positionally** within the function. Xseed v1.5 retitled several zones (e.g. "Sky Pirate Stronghold" → "Sky Bandit Stronghold", "Royal Capital Grancel" → "City of Grancel").

Conventions inside the arg list:

- Strings after the portrait tag (or after the shape-specific prefix for `[5,8]`) are the localized text. Integer `10` between strings is a literal newline.
- Tokens like `2489@` are line-number annotations from the original source, ignored by matching.
- The optional `"<K>"` / `"<k>"` string is a continuation marker — part of the text run, not metadata.

### Voice IDs

`[5,0]` and `[5,6]` calls often carry numeric args between `char_id` and the portrait tag, e.g. `system[5,0](134, 11, 33247, "<#E…>", …)`. The `11, <num>` pair is a **voice-line ID**. Two classes exist, both preserved verbatim:

- **Original audio cues** — e.g. Lugran `11, 33247`, Cassius `11, 34832-34844`. Exist in `resources/original/`, `resources/xseed-restoration/`, and `resources/evo-voice-mod/`.
- **EVO mod additions** — e.g. Joshua `11, 60589`, Estelle `11, 60593`. Exist only in `resources/evo-voice-mod/`.

**Preserve voice IDs verbatim regardless of class** — they are not text and are never the target of a swap.

## Matching anchor

For a `system[5,*]` call in EVO, find the matching Xseed call by:

1. **File path** (relative path under `script_en/scena/` is identical across all three corpora).
2. **Function name** (`fn FOO`).
3. **Opcode-specific key**:
   - `[5,0]` and `[5,6]`: `(char_id, portrait_tag)` — the `<#…>` portrait string. The face set is a letter (`E`, `L`, …), e.g. `<#E_2#M_2#B_0>` or `<#L_0#G[2]#M_2#B_0>`; matching only `<#E` would silently drop the others.
   - `[5,8]`: voice ID when present, else `(shape, position)` — see `docs/ARCHITECTURE.md` for details.
4. **Structural position** within the function — for tie-breaking among calls that share the same key (the same character speaks twice with the same portrait in the same function).

The match is **not** "same arg count" and **not** fuzzy string similarity. EVO may have extra voice-ID args; Xseed may have `<num>@` line annotations EVO lacks. Those are stripped from the anchor.

## Multiple EVO occurrences per Xseed line

A single Xseed dialogue line frequently maps to **multiple occurrences in the EVO file**. Two sources:

### 1. Called-table metadata + code body

The `calls { … } { … }` shape duplicates every call: once in metadata, once in body. Both must be swapped.

Example: in `resources/evo-voice-mod/.../mp1010_04.ing`, `fn EV_01_06_00() calls { … } { … }` runs from L1135 to L2106. Metadata ends at L1620 (`} {`); body runs L1621–L2106. The Lugran line at **L1212** lives in the metadata; **L1697** is the same line in the body. The corresponding Xseed function (`resources/xseed-restoration/.../mp1010_04.ing` L854) has no `calls` block — only one occurrence (L931).

### 2. First-visit vs. revisit gameplay branches

Some functions guard dialogue with `if !flag(N) { …first visit… } else { …revisit… }`. Both branches play to the player at different times, and both often carry the same dialogue. Both must be swapped.

Branches can show up inside the metadata block, the body block, or both — doubling the occurrence count again. Example: `TK_RUGLANG` in `mp1010_04.ing` has four near-identical Lugran intro blocks (two flag-gated branches × metadata + body).

### Rule: swap every matching occurrence in the file

Don't stop after the first hit. Walk the AST and apply the swap to **every** occurrence whose anchor matches.

## String-run replacement

EVO and Xseed differ in how they split a long line across string args with `10` (newline) separators. Examples:

- EVO: `"<P2>Yes, from Aina."` (1 string)
- Xseed: `"<P2>Yes, I received a call from Aina", 10, "not that long ago."` (2 strings + newline)

A naïve per-string replacement fails because the `10`-separated chunks don't align. Treat the **entire run of string args from just after the anchor element up to the closing `)`** as one unit, and replace the whole run.

Preserve everything outside the string run: `char_id`, voice IDs, portrait tag, shape-specific `[5,8]` prefix, line annotations on non-text args. Drop Xseed's `<num>@` annotations on the string args themselves.

## Examples

All three examples are in `script_en/scena/mp1010_04.ing` (Bose guild branch, Chapter 1).

### Lugran — "Yes, from Aina."

**EVO** (`fn EV_01_06_00`, occurrences at L1212 metadata, L1697 body):
```
system[5,0](134, 11, 33247, "<#E[11111110]#M_0#B_0>", "<P2>Yes, from Aina.");
```

**Xseed** (`fn EV_01_06_00`, single occurrence L931):
```
2490@system[5,0](134, 11, 33247, 2489@"<#E[11111110]#M_0#B_0>", "<P2>Yes, I received a call from Aina", 10, "not that long ago.");
```

**After swap** (both EVO occurrences):
```
system[5,0](134, 11, 33247, "<#E[11111110]#M_0#B_0>", "<P2>Yes, I received a call from Aina", 10, "not that long ago.");
```

Voice ID `11, 33247` (original audio cue) and portrait tag preserved; only the string run changes.

### Joshua — "In other words, this is a power struggle."

**EVO** (L1301 metadata, no voice ID; L1786 body, EVO-added voice ID `11, 60589`):
```
system[5,0](1, "<#E_2#M_2#B_0>", "<K>In other words, this is a power", 10, "struggle.");
system[5,0](1, 11, 60589, "<#E_2#M_2#B_0>", "<K>In other words, this is a power", 10, "struggle.");
```

**Xseed** (L1020):
```
2813@system[5,0](1, 2811@"<#E_2#M_2#B_0>", 2812@"<K>So, pretty much what you're saying", 10, "is that it's a bunch of jurisdictional", 10, "disputes, right?");
```

**After swap** (each EVO line keeps its own leading args, only the string run changes):
```
system[5,0](1, "<#E_2#M_2#B_0>", "<K>So, pretty much what you're saying", 10, "is that it's a bunch of jurisdictional", 10, "disputes, right?");
system[5,0](1, 11, 60589, "<#E_2#M_2#B_0>", "<K>So, pretty much what you're saying", 10, "is that it's a bunch of jurisdictional", 10, "disputes, right?");
```

### Estelle — "Who's this General Morgan guy?"

**EVO** (L1326 metadata, no voice ID; L1811 body, EVO-added voice ID `11, 60593`):
```
system[5,0](0, "<#E_E#M_2#B_0>", "<P1>Who's this General Morgan guy?");
system[5,0](0, 11, 60593, "<#E_E#M_2#B_0>", "<P1>Who's this General Morgan guy?");
```

**Xseed** (L1045):
```
2891@system[5,0](0, 2890@"<#E_E#M_2#B_0>", "<P1>General Morgan? Who's that?");
```

**After swap**:
```
system[5,0](0, "<#E_E#M_2#B_0>", "<P1>General Morgan? Who's that?");
system[5,0](0, 11, 60593, "<#E_E#M_2#B_0>", "<P1>General Morgan? Who's that?");
```

## Workflow

For each pair `resources/evo-voice-mod/.../X.ing` ↔ `resources/xseed-restoration/.../X.ing` (written to `output/.../X.ing`):

1. Parse both files. Walk EVO function-by-function. For each `system[5,*]` call, compute its anchor.
2. Look up the anchor in the Xseed index built for the same function. If absent, the line is EVO-only — leave it byte-identical.
3. If present and the text runs differ as `Vec<String>`, replace EVO's string run with Xseed's.
4. Apply the swap to every matching occurrence (calls-vs-body duplicates, flag-gated branch duplicates).
5. Touch only the text strings inside matched `system[5,*]` and `ui_mapname_effect` calls. Opcodes, control flow, char IDs, portrait tags, voice IDs, numeric args (including map-name coordinates), prelude declarations, and line annotations on non-text args are all off-limits.

`docs/ARCHITECTURE.md` covers the N-to-M overflow rule for cases where EVO has more occurrences than Xseed within the same anchor key.

## Tooling

- `scripts/dat2ing.py <path>` — wraps `ingert.exe --mode tree` to decompile `.dat` → `.ing`. Already run; `.ing` files exist for all three corpora under `resources/`.
- `scripts/prune.py` — drops files from a target corpus that have no Xseed counterpart. Dry-run by default; pass `--apply` to actually delete.
- `ingert.exe` lives at `C:/Users/kvnxiao/github/Ingert/target/release/ingert.exe`. Recompiling `.ing` → `.dat` is a separate Ingert step, not part of the swap workflow.
- The merge tool itself (`sora-remake-merge`) is the Rust binary built from this crate — see `docs/ARCHITECTURE.md`.

## Rules of thumb

- Xseed is canonical for any line present in both versions, even stylistically.
- EVO-only additions are preserved verbatim.
- Never modify anything under `resources/`. The merge writes to `output/`.
- Never edit `.dat` directly.
- Only the **text strings** inside `system[5,0]`, `[5,6]`, `[5,8]`, and `ui_mapname_effect` (`system[22,38]`) calls change. Everything else is untouchable.
- One Xseed line ↔ many EVO occurrences is the norm, not the exception. Always sweep the whole file.
- When in doubt about a match, leave the EVO line alone and surface it for review.
