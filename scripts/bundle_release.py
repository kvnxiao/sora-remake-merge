"""Bundle the merged release into a zip for distribution.

Packs `output/script_en/**/*.dat` and `output/table_en/**/*.tbl` into a single
archive, deliberately excluding the human-readable `.ing` decompiles and the
`_audit/` TSV logs. The resulting zip mirrors the loose-file layout that the
Sora1 loose-loader DLL reads, so end users can extract it straight into the
game's mod-staging directory.
"""

import argparse
import sys
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# (subdirectory under `output/`, allowed file extension)
BUNDLE_SPEC: list[tuple[str, str]] = [
    ("script_en", ".dat"),
    ("table_en", ".tbl"),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        default=REPO_ROOT / "output",
        help="Source root containing script_en/ and table_en/ (default: <repo>/output).",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=REPO_ROOT / "dist" / "sora-remake-merge.zip",
        help="Output zip path (default: <repo>/dist/sora-remake-merge.zip).",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    source: Path = args.source
    out: Path = args.out

    if not source.is_dir():
        print(f"error: source directory not found: {source}", file=sys.stderr)
        return 1

    files: list[tuple[Path, Path]] = []
    for subdir, ext in BUNDLE_SPEC:
        root = source / subdir
        if not root.is_dir():
            print(f"error: missing {root} — run `just all` first.", file=sys.stderr)
            return 1
        for path in sorted(root.rglob(f"*{ext}")):
            if path.is_file():
                files.append((path, path.relative_to(source)))

    if not files:
        print(f"error: no .dat or .tbl files found under {source}", file=sys.stderr)
        return 1

    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for abs_path, rel_path in files:
            zf.write(abs_path, arcname=rel_path.as_posix())

    size_mb = out.stat().st_size / (1024 * 1024)
    print(f"wrote {out.relative_to(REPO_ROOT) if out.is_relative_to(REPO_ROOT) else out} "
          f"({len(files)} files, {size_mb:.2f} MiB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
