# sora-remake-merge — common dev tasks.
# Run `just` (or `just --list`) to see all recipes.
#
# Requires `INGERT_EXE` to be set for any recipe that shells out to
# scripts/dat2ing.py or scripts/ing2dat.py (both invoke ingert.exe).

# Print recipe list (default)
default:
    @just --list

# === Pipeline ===

# Bootstrap: decompile .dat -> .ing for the committed test fixtures (always
# present) and, when present, the full corpora under resources/ (which are
# local-only build inputs — empty dirs are skipped without error).
dat2ing:
    python scripts/dat2ing.py sora-remake-merge/tests/fixtures
    python scripts/dat2ing.py resources/evo-voice-mod
    python scripts/dat2ing.py resources/xseed-restoration
    python scripts/dat2ing.py resources/original

# Decompile .dat -> .ing for a single path (file or directory)
dat2ing-path PATH:
    python scripts/dat2ing.py "{{PATH}}"

# Run the merge tool: EVO + Xseed -> output/ (forwards extra args)
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

# Compare EVO body vs Xseed body at AST level — confirms the merge has full
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

# Full pipeline: merge, then recompile (assumes .ing fixtures already exist)
all: merge ing2dat

# Bundle merged script_en/**/*.dat into dist/sora-remake-merge.zip (tables excluded; see README Compatibility)
bundle:
    python scripts/bundle_release.py

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
