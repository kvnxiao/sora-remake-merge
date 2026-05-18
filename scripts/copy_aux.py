"""Copy auxiliary tables that don't require merging into output/.

Currently:
- table_en/t_name.tbl: EVO ships this byte-identically to resources/original/,
  so Xseed's copy is the merged result by definition. See README §"Auxiliary
  tables" for the byte-level verification.
"""

import argparse
import shutil
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

AUX_FILES: list[tuple[str, str]] = [
    ("resources/xseed-restoration/table_en/t_name.tbl", "table_en/t_name.tbl"),
]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Stage auxiliary (non-script) localisation files into output/. "
            "Run after `sora-remake-merge` and before `scripts/ing2dat.py`."
        ),
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=REPO_ROOT / "output",
        help="Output root directory (default: <repo>/output).",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    out_root: Path = args.out

    for src_rel, dst_rel in AUX_FILES:
        src = REPO_ROOT / src_rel
        dst = out_root / dst_rel
        if not src.is_file():
            print(f"error: source not found: {src}", file=sys.stderr)
            return 1
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(src, dst)
        print(f"copied {src_rel} -> {dst.relative_to(REPO_ROOT) if dst.is_relative_to(REPO_ROOT) else dst}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
