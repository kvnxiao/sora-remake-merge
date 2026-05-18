# sora-remake-merge — common dev tasks.
# Run `just` (or `just --list`) to see all recipes.
#
# Requires `INGERT_EXE` to be set for any recipe that shells out to
# scripts/dat2ing.py or scripts/ing2dat.py (both invoke ingert.exe).

# Print recipe list (default)
default:
    @just --list

# === Pipeline ===

# Bootstrap: decompile .dat -> .ing for all three corpora under resources/
dat2ing:
    python scripts/dat2ing.py resources/evo-voice-mod
    python scripts/dat2ing.py resources/xseed-restoration
    python scripts/dat2ing.py resources/original

# Decompile .dat -> .ing for a single path (file or directory)
dat2ing-path PATH:
    python scripts/dat2ing.py "{{PATH}}"

# Run the merge tool: EVO + XSeed -> output/ (forwards extra args)
merge *ARGS:
    cargo run --release --bin sora-remake-merge -- {{ARGS}}

# Dry-run: parse and compute changes without writing
merge-dry-run:
    cargo run --release --bin sora-remake-merge -- --dry-run --verbose

# Compare EVO body vs original body at AST level — surfaces anchor/count
# differences the merge tool's audit may not flag (Letter→Voiced upgrades,
# Plain→VoicedPlain, unsupported shapes, etc.).
compare-original:
    cargo run --release --bin compare-original

# Compare EVO body vs XSeed body at AST level — confirms the merge has full
# coverage (no EVO-only functions silently skipped, no body-kind mismatches
# missed, no anchor-distribution drift between the two corpora).
compare-xseed:
    cargo run --release --bin compare-xseed

# Recompile merged .ing files in output/ back to .dat
ing2dat:
    python scripts/ing2dat.py output

# Recompile a specific .ing path (file or directory) back to .dat
ing2dat-path PATH:
    python scripts/ing2dat.py "{{PATH}}"

# EVO ships t_name.tbl byte-identically to original/, so XSeed's copy is the merged result
# by definition (verified via KuroTools tbl2json/json2tbl round-trip — see README §"Auxiliary tables").
# Copy XSeed's auxiliary tables verbatim into output/
copy-aux:
    python scripts/copy_aux.py

# Full pipeline: merge, copy aux tables, then recompile (assumes .ing fixtures already exist)
all: merge copy-aux ing2dat

# === Dev ===

# Format the workspace with nightly rustfmt
fmt:
    cargo fmt --all

# Verify formatting without writing (matches CI)
fmt-check:
    cargo fmt --all -- --check

# Lint with workspace clippy config
lint:
    cargo clippy --all-targets --all-features

# Run all tests (requires INGERT_EXE and decompiled .ing fixtures)
test:
    cargo test

# Build the release binary
build:
    cargo build --release
