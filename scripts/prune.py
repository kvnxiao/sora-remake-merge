"""Prune files from target folders that don't exist in xseed-restoration.

xseed-restoration is the source of truth. Any file under a target folder whose
relative path has no counterpart in xseed-restoration is considered extraneous
and deleted. Dry-run by default; pass --apply to actually remove anything.
"""

import argparse
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
RESOURCES = REPO_ROOT / "resources"
DEFAULT_SOURCE = RESOURCES / "xseed-restoration"
DEFAULT_TARGETS = [
    RESOURCES / "evo-voice-mod",
    RESOURCES / "original",
]
ALWAYS_KEEP = [Path("packed")]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        default=DEFAULT_SOURCE,
        help="Source-of-truth folder (default: ./resources/xseed-restoration).",
    )
    parser.add_argument(
        "--target",
        type=Path,
        action="append",
        default=None,
        help="Folder to prune (repeatable; default: ./resources/evo-voice-mod, ./resources/original).",
    )
    parser.add_argument(
        "--keep",
        action="append",
        default=[],
        metavar="REL_PATH",
        help="Relative path under target to keep regardless (repeatable). "
             "Matches the path itself and anything beneath it.",
    )
    parser.add_argument(
        "--prune-empty-dirs",
        action="store_true",
        help="After deleting files, remove directories left empty.",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Actually delete. Without this flag, prints what would happen.",
    )
    return parser.parse_args()


def is_kept(rel: Path, keeps: list[Path]) -> bool:
    for keep in keeps:
        try:
            rel.relative_to(keep)
            return True
        except ValueError:
            continue
    return False


def prune_target(
    source: Path, target: Path, keeps: list[Path], apply: bool, prune_empty: bool
) -> tuple[int, int]:
    print(f"\n== {target} ==")

    to_delete: list[Path] = []
    for path in target.rglob("*"):
        if not path.is_file():
            continue
        rel = path.relative_to(target)
        if is_kept(rel, keeps):
            continue
        if not (source / rel).exists():
            to_delete.append(path)

    action = "Deleting" if apply else "Would delete"
    for path in to_delete:
        print(f"{action}: {path.relative_to(target)}")
        if apply:
            path.unlink()

    pruned_dirs: list[Path] = []
    if prune_empty:
        # Walk deepest-first so parents become empty after children removed.
        dirs = sorted(
            (p for p in target.rglob("*") if p.is_dir()),
            key=lambda p: len(p.parts),
            reverse=True,
        )
        for d in dirs:
            rel = d.relative_to(target)
            if is_kept(rel, keeps):
                continue
            try:
                if not any(d.iterdir()):
                    pruned_dirs.append(d)
                    if apply:
                        d.rmdir()
            except OSError:
                pass
        dir_action = "Removing" if apply else "Would remove"
        for d in pruned_dirs:
            print(f"{dir_action} empty dir: {d.relative_to(target)}")

    return len(to_delete), len(pruned_dirs)


def main() -> int:
    args = parse_args()
    source: Path = args.source.resolve()
    targets: list[Path] = [t.resolve() for t in (args.target or DEFAULT_TARGETS)]

    if not source.is_dir():
        print(f"error: source folder not found: {source}", file=sys.stderr)
        return 1
    for target in targets:
        if not target.is_dir():
            print(f"error: target folder not found: {target}", file=sys.stderr)
            return 1

    keeps = ALWAYS_KEEP + [Path(k) for k in args.keep]

    total_files = 0
    total_dirs = 0
    for target in targets:
        n_files, n_dirs = prune_target(
            source, target, keeps, args.apply, args.prune_empty_dirs
        )
        total_files += n_files
        total_dirs += n_dirs

    summary = (
        f"\n{'Deleted' if args.apply else 'Would delete'} "
        f"{total_files} file(s) across {len(targets)} target(s)"
    )
    if args.prune_empty_dirs:
        summary += (
            f", {'removed' if args.apply else 'would remove'} "
            f"{total_dirs} empty dir(s)"
        )
    summary += "." if args.apply else " (dry run; pass --apply to execute)."
    print(summary)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
