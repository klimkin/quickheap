#!/usr/bin/env python3
"""Select historical Rust timing or shared-fixture queue timing."""

import argparse
import csv
from enum import Enum
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[2]
BENCH = ROOT / "quickheap-dev/bench"
ORIGINAL_PROTOCOL = "quickheap-original-rust-a9f2518-v1"


class Profile(str, Enum):
    ORIGINAL = "original-rust"
    CONTROLLED = "controlled-operations"


def parser():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--profile", choices=[p.value for p in Profile], required=True)
    cli.add_argument("--output", type=Path, help="attributed CSV; default stdout")
    cli.add_argument("--raw-output", type=Path, help="preserve original runner CSV")
    cli.add_argument("--offline", action="store_true")
    cli.add_argument("--no-default-features", action="store_true")
    cli.add_argument("--features", help="Cargo features for selected runner")
    cli.add_argument("--dry-run", action="store_true", help="validate and print Cargo command")
    cli.add_argument("--min", type=int, help="original size exponent, default 10")
    cli.add_argument("--max", type=int, help="original size exponent, default 25")
    cli.add_argument("--i32", action="store_true")
    cli.add_argument("--i64", action="store_true")
    cli.add_argument("--table", action="store_true", help="original i64 table registrations")
    cli.add_argument("--fixture", type=Path, help="controlled shared input fixture")
    cli.add_argument("--heap", help="controlled queue implementation")
    cli.add_argument("--repeat", type=int)
    cli.add_argument("--warmup", type=int)
    cli.add_argument("--verify-only", action="store_true")
    cli.add_argument("--trace", action="store_true")
    cli.add_argument("--scalar-trace", action="store_true")
    cli.add_argument("--samples")
    cli.add_argument("--comparisons", action="store_true")
    cli.add_argument("--memory", action="store_true")
    cli.add_argument("--ablation", action="store_true")
    return cli


def command(args):
    profile = Profile(args.profile)
    original = profile is Profile.ORIGINAL
    if original:
        controlled = ("fixture", "heap", "repeat", "warmup", "verify_only", "trace",
                      "scalar_trace", "samples", "comparisons", "memory", "ablation")
        if any(getattr(args, name) is not None and getattr(args, name) is not False
               for name in controlled):
            raise ValueError("original-rust preserves original timing; fixture, filters, "
                             "repeat/warmup overrides and diagnostics are unsupported")
        if args.i32 and args.i64:
            raise ValueError("original --i32 plus --i64 emits no rows; select one or neither")
        if args.table and (args.i32 or args.i64):
            raise ValueError("original --table always measures i64; omit width filters")
        minimum = args.min if args.min is not None else 10
        maximum = args.max if args.max is not None else 25
        if not 1 <= minimum <= maximum <= 25:
            raise ValueError("original size exponents require 1 <= min <= max <= 25")
        features = set((args.features or "").replace(",", " ").split())
        if features - {"avx2", "avx512", "ffi", "boost"}:
            raise ValueError("original timing permits avx2,avx512,ffi,boost features only")
        cmd = ["cargo", "run", "--release", "--locked", "--package", "bench", "--example", "bench"]
        run = []
        for name in ("min", "max"):
            if getattr(args, name) is not None:
                run.extend([f"--{name}", str(getattr(args, name))])
        run.extend(f"--{name}" for name in ("i32", "i64", "table") if getattr(args, name))
    else:
        if args.min is not None or args.max is not None or args.i32 or args.i64 or args.table:
            raise ValueError("controlled-operations derives size, width and workload from fixture")
        if args.memory or args.ablation or args.raw_output:
            raise ValueError("memory, ablation and raw-output require a different runner")
        if args.fixture is None:
            raise ValueError("controlled-operations requires --fixture")
        if args.repeat is not None and args.repeat <= 0:
            raise ValueError("repeat count must be positive")
        if args.warmup is not None and args.warmup < 0:
            raise ValueError("warmup count must be nonnegative")
        cmd = ["cargo", "run", "--release", "--locked", "--manifest-path",
               str(BENCH / "paired/Cargo.toml"), "--bin", "quickheap-paired"]
        run = ["--profile", profile.value, "--fixture", str(args.fixture.resolve())]
        for name in ("heap", "repeat", "warmup", "samples"):
            value = getattr(args, name)
            if value is not None:
                run.extend([f"--{name}", str(value)])
        run.extend(f"--{name.replace('_', '-')}" for name in
                   ("verify_only", "trace", "scalar_trace", "comparisons") if getattr(args, name))
    if args.offline:
        cmd.append("--offline")
    if args.no_default_features:
        cmd.append("--no-default-features")
    if args.features:
        cmd.extend(["--features", args.features])
    return cmd + ["--"] + run


def original_features(args):
    features = set() if args.no_default_features else {"avx2", "ffi"}
    features.update((args.features or "").replace(",", " ").split())
    if "avx512" in features:
        features.add("avx2")
    return features


def original_sources(features=()):
    manifest = json.loads((BENCH / "original-sources.json").read_text())
    digest = 0xCBF29CE484222325
    for name, expected in sorted(manifest["files"].items()):
        path = ROOT / name
        data = path.read_bytes()
        checked = data
        if name == "quickheap-dev/bench/src/lib.rs":
            checked = checked.replace(b"pub mod fixture;\n", b"", 1)
        if hashlib.sha256(checked).hexdigest() != expected:
            raise ValueError(f"original source drift from {manifest['revision']}: {name}")
        for byte in name.encode() + data:
            digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    for name, dependency in sorted(manifest.get("submodules", {}).items()):
        if dependency["feature"] not in features:
            continue
        path = ROOT / name
        if not (path / ".git").exists():
            raise ValueError(f"original FFI dependency uninitialized: {name}; "
                             "run git submodule update --init --recursive")
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=path, text=True).strip()
        if head != dependency["revision"]:
            raise ValueError(f"original FFI revision drift: {name}; expected {dependency['revision']}")
        tree = subprocess.check_output(["git", "ls-tree", "-r", "-z", "HEAD"], cwd=path)
        for entry in tree.split(b"\0"):
            if not entry:
                continue
            metadata, filename = entry.split(b"\t", 1)
            mode, kind, expected = metadata.split()
            if kind != b"blob":
                raise ValueError(f"original FFI nested dependency unsupported: {name}")
            file = path / os.fsdecode(filename)
            data = os.fsencode(os.readlink(file)) if mode == b"120000" else file.read_bytes()
            blob = b"blob " + str(len(data)).encode() + b"\0" + data
            if hashlib.sha1(blob).hexdigest().encode() != expected:
                raise ValueError(f"original FFI source drift: {name}/{os.fsdecode(filename)}")
            for byte in name.encode() + b"/" + filename + data:
                digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
        status = subprocess.check_output(["git", "status", "--porcelain", "--untracked-files=all"],
                                         cwd=path, text=True)
        if status:
            raise ValueError(f"original FFI worktree drift: {name}")
    for name in ("quickheap-dev/bench/src/fixture.rs", "quickheap-dev/bench/profiles.py",
                 "quickheap-dev/bench/original-sources.json"):
        for byte in name.encode() + (ROOT / name).read_bytes():
            digest = ((digest ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"fnv1a64:{digest:016x}"


def original_build(args, source):
    rustc = os.environ.get("RUSTC") or cargo_config("build.rustc", "rustc")
    version = subprocess.check_output([rustc, "--version"], cwd=ROOT, text=True).strip()
    verbose = subprocess.check_output([rustc, "-vV"], cwd=ROOT, text=True)
    host = next(line.removeprefix("host: ") for line in verbose.splitlines()
                if line.startswith("host: "))
    features = original_features(args)
    target = os.environ.get("CARGO_BUILD_TARGET", cargo_config("build.target", host))
    if isinstance(target, list):
        if len(target) != 1:
            raise ValueError("original build attribution requires one Cargo target")
        target = target[0]
    flags = original_rustflags(target)
    configured = cargo_config("profile.release", {})
    settings = [("opt", os.environ.get("CARGO_PROFILE_RELEASE_OPT_LEVEL",
                                       str(configured.get("opt-level", 3)))),
                ("lto", os.environ.get("CARGO_PROFILE_RELEASE_LTO",
                                       str(configured.get("lto", "fat")))),
                ("codegen-units", os.environ.get("CARGO_PROFILE_RELEASE_CODEGEN_UNITS",
                                                 str(configured.get("codegen-units", 1))))]
    return ";".join([version, f"target={target}",
                     *(f"{key}={value}" for key, value in settings),
                     f"features={','.join(sorted(features)) or 'none'}", f"flags={flags}",
                     f"source={source}", "reference=a9f2518d9ed452130361cfe1681c68625a68ab35"])


def original_rustflags(target):
    if "CARGO_ENCODED_RUSTFLAGS" in os.environ:
        return os.environ["CARGO_ENCODED_RUSTFLAGS"].replace("\x1f", " ")
    if "RUSTFLAGS" in os.environ:
        return os.environ["RUSTFLAGS"]
    configured = cargo_config("target", {})
    if any(key.startswith("cfg(") and value.get("rustflags")
           for key, value in configured.items()):
        raise ValueError("cannot attribute conditional target rustflags; use RUSTFLAGS "
                         "or CARGO_ENCODED_RUSTFLAGS")
    flags = cargo_config(f"target.{target}.rustflags", None)
    if flags is None:
        flags = cargo_config("build.rustflags", [])
    return " ".join(flags) if isinstance(flags, list) else flags


def cargo_config(key, default):
    result = subprocess.run(["cargo", "-Z", "unstable-options", "config", "get", key,
                             "--format", "json"], cwd=ROOT, capture_output=True, text=True)
    if result.returncode:
        if "is not set" in result.stderr:
            return default
        raise ValueError(f"cannot attribute Cargo configuration {key}: {result.stderr.strip()}")
    value = json.loads(result.stdout)
    for component in key.split("."):
        value = value[component]
    return value


def backend(heap):
    if "Avx512" in heap:
        return "avx512"
    if "Avx2" in heap:
        return "avx2"
    if any(name in heap for name in ("sequence_heap::", "s3q::", "original_quickheap::", "radix_heap::RadixHeapI")):
        return "ffi"
    return "scalar"


def attribute_original(raw, output, args, build):
    rows = csv.DictReader(raw)
    required = {"elem", "heap", "n", "workload", "ops", "repeat", "nanos"}
    if not rows.fieldnames or not required.issubset(rows.fieldnames):
        raise ValueError("original runner emitted no timing CSV header")
    added = ["protocol", "build", "backend", "fixture_digest", "reservation", "timer",
             "repeats", "warmup", "profile", "input_policy", "cutoff"]
    writer = csv.DictWriter(output, fieldnames=rows.fieldnames + added)
    writer.writeheader()
    count = 0
    for row in rows:
        count += 1
        heap = row["heap"]
        reservation = "backend-default"
        if "collections::binary_heap::BinaryHeap" in heap:
            reservation = "reserve:1048576"
        elif "original_quickheap::OriginalQuickHeap" in heap:
            reservation = "reserve:268435456"
        row.update(protocol=ORIGINAL_PROTOCOL, build=build, backend=backend(heap),
                   fixture_digest="", reservation=reservation,
                   timer="workload-input-queue-destruction", repeats="3", warmup="1",
                   profile=Profile.ORIGINAL.value, input_policy="fresh-random-original",
                   cutoff="128" if args.table else "32")
        row["workload"] = row["workload"].rsplit("::", 1)[-1]
        writer.writerow(row)
    if not count:
        raise ValueError("original runner emitted no timing rows; check enabled registrations")


def main(argv=None):
    cli = parser()
    args = cli.parse_args(argv)
    try:
        cmd = command(args)
        features = original_features(args) if args.profile == Profile.ORIGINAL.value else set()
        source = original_sources(features) if args.profile == Profile.ORIGINAL.value else None
        if args.output and args.raw_output and args.output.resolve() == args.raw_output.resolve():
            raise ValueError("output and raw-output must use different paths")
        if args.dry_run:
            print(json.dumps({"profile": args.profile, "cwd": str(ROOT), "command": cmd,
                              "source": source}, indent=2))
            return 0
        build = original_build(args, source) if source else None
        with tempfile.TemporaryFile(mode="w+", encoding="utf-8", newline="") as raw:
            result = subprocess.run(cmd, cwd=ROOT, stdout=raw)
            raw.seek(0)
            if args.raw_output:
                with args.raw_output.open("w", encoding="utf-8", newline="") as saved:
                    shutil.copyfileobj(raw, saved)
                raw.seek(0)
            if result.returncode:
                return result.returncode
            if source and original_sources(features) != source:
                raise ValueError("original source changed during benchmark; attributed output refused")
            if args.output:
                with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="",
                                                 dir=args.output.parent, delete=False) as output:
                    temporary = Path(output.name)
                    try:
                        if build:
                            attribute_original(raw, output, args, build)
                        else:
                            shutil.copyfileobj(raw, output)
                    except BaseException:
                        temporary.unlink()
                        raise
                temporary.replace(args.output)
            elif build:
                attribute_original(raw, sys.stdout, args, build)
            else:
                shutil.copyfileobj(raw, sys.stdout)
        return 0
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        cli.error(str(error))


if __name__ == "__main__":
    sys.exit(main())
