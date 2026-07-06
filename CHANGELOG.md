# Changelog

All notable changes to **sora-remake-merge**. The mod lays [Xseed Restoration](https://www.nexusmods.com/trailsintheskyfirstchapter/mods/52) English text onto the [EVO Voice mod](https://www.nexusmods.com/trailsintheskyfirstchapter/mods/41) scripts, so each release is keyed to the Xseed Restoration version it brings in. The README's [Compatibility](README.md#compatibility) section lists the exact upstream versions a release targets.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions use [Semantic Versioning](https://semver.org/spec/v2.0.0.html). "In-game changes" are what a player sees differ in the game; "Tooling & internals" cover the merge tool itself.

## [1.4.0] - 2026-07-06 (targets Xseed Restoration v1.8.1)

Bug-fix release for the "Temp Librarian" quest scene in the Central Factory Archives. No Xseed version change from v1.3.0 — keep Xseed Restoration v1.8.1 installed.

### In-game changes

- **Fixed: talking to Constance in the Central Factory Archives ("Temp Librarian" quest) no longer freezes the game on an infinite loading screen.** The scene's completion cutscene (`mp3010_01.ing:QS300_01_00`) is the one function EVO ships as un-decompilable bytecode, and the special handling that lays Xseed's text over it emitted a scene script whose internal call table disagreed with its code — which the engine hangs on when loading the scene. The table is now rebuilt to match the code. This freeze affected the scene in every prior release.
- **The same cutscene regains its EVO voice acting.** The earlier handling cloned Xseed's voiceless body and dropped every EVO voice cue in the scene; the merge now recovers those cues from EVO's bytecode and re-injects them, so the cutscene plays with Xseed's wording *and* EVO's voice.

### Tooling & internals

- **`Body::Asm` substitution now adopts Xseed's called-table alongside its body.** The substitution replaced only the body, leaving EVO's asm-derived `Called::Raw` table in place. Ingert's `compile` writes a `Called::Raw` table to the `.dat` verbatim with no check against the code (the reconcile pass runs only at decompile time), so EVO's table paired with Xseed's substituted body diverged (e.g. `camera_lookat` arg counts) and produced a scene the engine hung on. The swap now takes Xseed's `Called::Merged` too (`adopt_xseed_body`), so ingert re-infers a table that matches the substituted body. `QS300_01_00` is the corpus's only `Body::Asm` function, so it is the only one affected.
- **`Body::Asm` substitution preserves EVO's body voice cues.** The gate previously read only the calls-table (`evo_calls_have_voice_ids`), which misses voice IDs EVO adds in the bytecode body but not the metadata — exactly `QS300_01_00`'s case. The swap now parses the asm to recover each dialogue call's `11, V` pair, clones Xseed's `Tree`, and re-injects the pairs at their original positions (gated on the dialogue-call counts matching so a cue can't be misplaced; it falls back to the calls-table gate if the asm can't be parsed). `body_substitutions.tsv` gains a `voice_ids_reinjected` column and the run summary reports the count. New unit and e2e tests cover recovery, re-injection, the called-table adoption, and the misalignment fallback; `verify-delta` still reports zero violations.
- **`just bundle` now runs the full pipeline before zipping.** It depends on `all` (`merge` → `ing2dat`), so the release zip can never ship a stale `.dat` — the earlier voice fix appeared not to work precisely because a plain `just bundle` re-zipped stale `.dat` files. `just bundle-only` re-zips the existing `output/**/*.dat` without rebuilding.

## [1.3.0] - 2026-07-03 (targets Xseed Restoration v1.8)

Updates the merge to Xseed Restoration **v1.8** (from v1.7). Install Xseed Restoration v1.8 with this build; pairing it with an older version leaves the new text desynced. v1.8 is the mod author's broadest re-translation pass yet, and the existing anchors absorb it with no merge-tool changes.

### In-game changes

- **Xseed v1.8's re-translation carries onto the EVO-voiced scripts** across the large majority of scene scripts. Wherever v1.8 reworded a line that the EVO scripts voice, the merge re-applies v1.8's wording (e.g. in `mp2000_ev`, "W-Wow...!" → "I-Incredible...!").
- **Dialogue-option / choice-menu text** now follows Xseed v1.8 (the `menu_additem` choice entries).
- **System text and portrait-less narration** — examine descriptions, `<C1>` story-recap screens, internal monologue, and records/encyclopedia entries — re-translated to v1.8's wording.
- **Character-name labels** updated to v1.8's spelling and punctuation, e.g. "Lonnie, Dino & Lyle" → "Lonnie, Dino, & Lyall" (`chr_set_display_name`).
- **Inline `<C2>…</C>` colour markup** fixes from v1.8 ride through the merged text intact.
- Xseed v1.8 also added dialogue **wait/pacing commands** to its patched lines. These are control flow, not text: the merge preserves EVO's own voice-driven pacing and carries only the wording, so v1.8's added waits are intentionally not applied.

### Tooling & internals

- No merge-tool changes. v1.8 is a text-refresh bump — regenerate the Xseed `.ing` (`just dat2ing`), re-run the merge, recompile (`just ing2dat`), and re-bundle (`just bundle`). `verify-delta` still reports zero violations across all three corpora, with only the documented exemptions (the `EV_01_61_00` Letter→Voiced upgrades and the `QS300_01_00` body substitution); `compare-xseed` reports no anchor-distribution drift.
- Test fixtures stay at Xseed v1.7. They exercise the merge logic, which is unchanged; the shipped scripts are built from the v1.8 corpus regardless.

## [1.2.0] - 2026-06-15 (targets Xseed Restoration v1.7)

Updates the merge to Xseed Restoration **v1.7** (from v1.5). Install Xseed Restoration v1.7 with this build; pairing it with v1.5 leaves the new text desynced.

### In-game changes

- **Zeiss orbal-records terminal headers** retitled to Xseed v1.7's wording: "[History]" → "[Establishment]", "[Orbment]" → "[Orbments]", "[Orbal Weapons]" → "[Orbal Weaponry]", "[All Orbal Technology]" → "[Universal Tech]", "[Other Information]" → "[Related Topics]", "[Internal Combustion Engine]" → "[Combustion Engine]", "[Orbal Automobile]" → "[Haulage Vehicle]" (the `menu_additem` topic menu in `mp3010_01`).
- **Records, encyclopedia, and on-screen narration** (`system[5,8]` signposts, device and terminal UIs, museum captions, the records terminal) now follow Xseed v1.7's re-translations, e.g. "becomes the first factory chief" → "…Factory Chief". Voiced device lines such as the Jenis Academy fortune-teller keep their EVO voice cue.
- **Narrator and system text without a portrait** (examine descriptions, `<C1>` story-recap screens, internal monologue) now uses Xseed's wording, e.g. in `mp1110`, "Men can be heard talking." → "The voices of some men can be heard."
- **Combined-party speaker labels** rephrased to Xseed's wording: "Lonnie, Dino, & Lyle" → "Lonnie, Dino & Lyle" and "Scherazard, Kloe, & Estelle" → "Scherazard, Kloe, and Estelle" (`chr_set_display_name`).
- **Xseed v1.7 dialogue edits** the earlier merge missed are now applied, including Lugran's `<#L` portrait line in `mp1010_04` ("N-Now hold on…") and Estelle's "That voice sounds suspiciously…".
- **Inline `<C2>…</C>` colour markup** introduced by Xseed v1.7 (e.g. the tutorial glossary lines in `mp0010_05`) now carries through to the merged text intact.

### Tooling & internals

- New `AnchorKey` shapes so the swap reaches the calls above: `Untagged` (portrait-less narrator / variable speaker, matched positionally per `char_id`), `Narration` (`[5,8]` integer-prefix narration, bucketed per prefix), `MenuItem` (`menu_additem` labels), and `DisplayName` (`chr_set_display_name`, keyed per `(function, char_id)`, integer `char_id` only).
- The `Portrait` classifier now handles a voice ID placed *after* the portrait tag (`(2, "<#E…>", 11, 34731, …)`), keeping it in the preserved prefix.
- `compare-original` and `compare-xseed` now classify and count the named prelude-alias calls (`MapName`, `MenuItem`, `DisplayName`) alongside the dialogue syscalls, so their anchor-distribution reports cover the full localizable surface.
- New `verify-delta` binary (`just verify-delta`) and `verify` module assert the **localization-delta invariant** across all three corpora: the merged output differs from EVO exactly where Xseed differs from `original/`, and carries Xseed's text wherever it differs. A clean run reports zero violations, with only the documented exemptions (the `EV_01_61_00` Letter→Voiced upgrades and the `QS300_01_00` body substitution). The test suite asserts the same invariant at fixture scale.
- Test fixtures for Xseed (`mp0010_05`, `mp3010_01`) bumped to v1.7.

## [1.1.0] - 2026-06-06 (targets Xseed Restoration v1.5)

First build to fully cover Xseed Restoration **v1.5**, adding the non-dialogue edits the initial release left untouched.

### In-game changes

- **On-screen zone labels** (`ui_mapname_effect`) now use Xseed v1.5's retitled place names: "Jade Tower" → "Esmelas Tower", "Amber Tower" → "Amberl Tower", "Sky Pirate Stronghold" → "Sky Bandit Stronghold", "Kaldia Limestone Cave" → "Limestone Cave", and "Royal Capital Grancel" → "City of Grancel". Only the on-screen label changes; a zone's old name may still appear in dialogue where Xseed left it unchanged.
- **Dialogue with non-`<#E` portraits** (e.g. Lugran's `<#L` face set) now localised; the earlier `<#E`-only anchor silently dropped these lines.

### Tooling & internals

- `MapName` anchor and the `ui_mapname_effect` (`system[22,38]`) prelude-alias path, matched positionally per function with the trailing coordinates preserved.
- Portrait anchoring widened to accept any uppercase face-set letter, not just `<#E`.
- Releases now ship **only the merged `script_en/**/*.dat` scripts**; the verbatim Xseed `table_en/t_name.tbl` is no longer redistributed (it loads from the player's Xseed install). The `resources/` corpora are untracked (local-only build inputs), and the test suite runs against committed `.dat` fixtures under `tests/fixtures/`.

## [1.0.0] - 2026-05-18

Initial release: the core EVO ↔ Xseed text merge.

### In-game changes

- Character dialogue (`system[5,0]` / `[5,6]` portrait message boxes) and `system[5,8]` narration (letter, plain, voiced) carry Xseed's wording on the EVO-voiced scripts, with every EVO voice cue preserved.

### Tooling & internals

- AST-based merge (parse, index, walk, print) over the `ingert-sora1` fork; `Portrait`, `Voiced`, `Letter`, and `Plain` anchors; whole-string-run replacement that preserves voice IDs, char IDs, and portrait tags.
- Handling for EVO's structural divergences from the GungHo baseline: voice-ID insertions, `[5,8]` Letter→Voiced and Plain→VoicedPlain anchor-shape upgrades (via positional fallback), and `Body::Asm` body substitution.
- Per-run audit logs (`unmatched.tsv`, `overflow.tsv`, `body_substitutions.tsv`) and the `compare-original` / `compare-xseed` analysis binaries.

[1.4.0]: https://github.com/kvnxiao/sora-remake-merge/releases/tag/v1.4.0
[1.3.0]: https://github.com/kvnxiao/sora-remake-merge/releases/tag/v1.3.0
[1.2.0]: https://github.com/kvnxiao/sora-remake-merge/releases/tag/v1.2.0
[1.1.0]: https://github.com/kvnxiao/sora-remake-merge/releases/tag/v1.1.0
[1.0.0]: https://github.com/kvnxiao/sora-remake-merge/releases/tag/v1.0.0
