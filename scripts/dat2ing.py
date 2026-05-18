"""Batch wrapper around ingert.exe to decompile .dat scripts into .ing files."""

import argparse
import os
import subprocess
import sys
from pathlib import Path

INGERT_ENV = "INGERT_EXE"


def resolve_ingert(cli_override: Path | None) -> Path:
    if cli_override is not None:
        return cli_override
    env = os.environ.get(INGERT_ENV)
    if not env:
        sys.exit(
            f"error: {INGERT_ENV} is not set. "
            f"Set it to the path of ingert.exe, or pass --ingert <path>."
        )
    return Path(env)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Decompile ED9 .dat script files to .ing using ingert.",
    )
    parser.add_argument(
        "path",
        type=Path,
        help="A .dat file, or a directory to walk recursively for .dat files.",
    )
    parser.add_argument(
        "--ingert",
        type=Path,
        default=None,
        help=f"Path to ingert.exe. Defaults to ${INGERT_ENV} env var.",
    )
    parser.add_argument(
        "--show-warnings",
        action="store_true",
        help="Always print ingert's stderr (warnings + tracing). "
        "By default, stderr is only shown for files that fail.",
    )
    return parser.parse_args()


def collect_files(path: Path) -> list[Path]:
    if path.is_file():
        if path.suffix.lower() != ".dat":
            sys.exit(f"error: {path} is not a .dat file")
        return [path]
    if path.is_dir():
        return sorted(p for p in path.rglob("*.dat") if p.is_file())
    sys.exit(f"error: {path} does not exist")


def decompile_one(ingert: Path, src: Path, show_warnings: bool) -> tuple[bool, str]:
    dst = src.with_suffix(".ing")
    result = subprocess.run(
        [str(ingert), "--mode", "tree", "-o", str(dst), str(src)],
        capture_output=True,
        text=True,
    )
    ok = result.returncode == 0
    if show_warnings and result.stderr:
        sys.stderr.write(result.stderr)
    return ok, result.stderr


def main() -> int:
    args = parse_args()
    ingert = resolve_ingert(args.ingert)

    if not ingert.is_file():
        sys.exit(f"error: ingert binary not found at {ingert}")

    files = collect_files(args.path)
    if not files:
        print(f"no .dat files found under {args.path}")
        return 0

    failures: list[tuple[Path, str]] = []
    for i, src in enumerate(files, 1):
        prefix = f"[{i}/{len(files)}]"
        ok, stderr = decompile_one(ingert, src, args.show_warnings)
        if ok:
            print(f"{prefix} OK   {src} -> {src.with_suffix('.ing')}")
        else:
            print(f"{prefix} FAIL {src}", file=sys.stderr)
            if not args.show_warnings and stderr:
                sys.stderr.write(stderr)
            failures.append((src, stderr))

    total = len(files)
    print(f"\nDone. {total - len(failures)}/{total} succeeded.")
    if failures:
        print("Failed files:")
        for src, _ in failures:
            print(f"  {src}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
