import contextlib
import csv
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("profiles", Path(__file__).with_name("profiles.py"))
profiles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profiles)
NO_REPAIRS = {"reference": "pinned", "repairs": []}


class ProfilesTest(unittest.TestCase):
    def parse(self, *args):
        return profiles.parser().parse_args(args)

    def test_original_command_keeps_native_runner_defaults(self):
        args = self.parse("--profile", "original-rust")
        self.assertEqual(profiles.command(args),
                         ["cargo", "run", "--release", "--locked", "--package", "bench",
                          "--example", "bench", "--"])
        self.assertRegex(profiles.original_sources().digest, r"^fnv1a64:[0-9a-f]{16}$")

    def test_original_rejects_controlled_contract(self):
        for extra in (("--fixture", "sample.qh"), ("--repeat", "3"),
                      ("--warmup", "0"), ("--heap", "binary_heap"),
                      ("--verify-only",), ("--comparisons",), ("--memory",),
                      ("--ablation",), ("--i32", "--i64"), ("--table", "--i32"),
                      ("--features", "perf"), ("--min", "26")):
            with self.subTest(extra=extra), self.assertRaises(ValueError):
                profiles.command(self.parse("--profile", "original-rust", *extra))

    def test_controlled_forwards_fixture_options(self):
        args = self.parse("--profile", "controlled-operations", "--fixture", "sample.qh",
                          "--heap", "binary_heap", "--repeat", "5", "--offline",
                          "--features", "quickheap-avx2")
        cmd = profiles.command(args)
        self.assertIn("--manifest-path", cmd)
        self.assertIn("--offline", cmd)
        self.assertEqual(cmd[cmd.index("--") + 1:],
                         ["--profile", "controlled-operations", "--fixture",
                          str(Path("sample.qh").resolve()), "--heap", "binary_heap",
                          "--repeat", "5"])

    def test_original_source_drift_blocks_launch(self):
        manifest = {"revision": "pinned", "files": {"Cargo.toml": "0" * 64}}
        with patch.object(profiles, "manifests", return_value=(manifest, NO_REPAIRS)):
            with self.assertRaisesRegex(ValueError, "source drift.*Cargo.toml"):
                profiles.original_sources()

    def test_original_attribution_preserves_metrics(self):
        raw = io.StringIO("elem,heap,n,workload,ops,repeat,nanos\n"
                          "i32,alloc::collections::binary_heap::BinaryHeap,8,"
                          "bench::workloads::HeapSort,8,2,123\n"
                          "i64,bench::original_quickheap::OriginalQuickHeapI64,8,"
                          "bench::workloads::MonotoneConstantSize,80,0,456\n")
        output = io.StringIO()
        profiles.attribute_original(raw, output,
                                    self.parse("--profile", "original-rust", "--table"),
                                    "compiler;source=fnv1a64:1234", "pinned+fix@commit")
        output.seek(0)
        rows = list(csv.DictReader(output))
        self.assertEqual(rows[0]["nanos"], "123")
        self.assertEqual(rows[0]["reservation"], "reserve:1048576")
        self.assertEqual(rows[1]["reservation"], "reserve:268435456")
        for row in rows:
            self.assertEqual(row["fixture_digest"], "")
            self.assertEqual(row["source_revision"], "pinned+fix@commit")
            self.assertEqual(row["input_policy"], "fresh-random-original")
            self.assertEqual(row["timer"], "workload-input-queue-destruction")
            self.assertEqual(row["cutoff"], "128")
            self.assertNotIn("::", row["workload"])

    def test_original_backend_identity(self):
        self.assertEqual(profiles.backend("quickheap::SimdQuickHeap<i32, quickheap::simd::Avx2>"), "avx2")
        self.assertEqual(profiles.backend("bench::simd_quickheap::DevSimdQuickHeap<i64, quickheap::simd::Avx512>"), "avx512")
        self.assertEqual(profiles.backend("bench::scalar_quickheap::ScalarQuickHeap"), "scalar")
        self.assertEqual(profiles.backend("bench::s3q::S3qHeapI32"), "ffi")

    def test_header_only_original_output_is_not_a_result(self):
        with self.assertRaisesRegex(ValueError, "no timing rows"):
            profiles.attribute_original(io.StringIO("elem,heap,n,workload,ops,repeat,nanos\n"),
                                        io.StringIO(), self.parse("--profile", "original-rust"), "build", "pinned")

    def test_original_build_records_merged_cargo_configuration(self):
        values = {"build.rustflags": ["-C", "force-frame-pointers=yes", "-C", "target-cpu=native"],
                  "build.target": "test-target", "profile.release": {"lto": "thin"}}
        with patch.object(profiles.subprocess, "check_output", side_effect=["rustc test", "host: host-target\n"]):
            with patch.object(profiles, "cargo_config", side_effect=lambda key, default: values.get(key, default)):
                with patch.dict(profiles.os.environ, {}, clear=True):
                    build = profiles.original_build(self.parse("--profile", "original-rust"), "source")
        self.assertIn("target=test-target", build)
        self.assertIn("flags=-C force-frame-pointers=yes -C target-cpu=native", build)
        self.assertIn("lto=thin;codegen-units=1", build)

    def test_original_flags_follow_cargo_precedence(self):
        values = {"build.rustflags": ["-C", "target-cpu=native"],
                  "target.host-target.rustflags": ["-C", "target-cpu=x86-64"]}
        cases = [({}, "-C target-cpu=x86-64"),
                 ({"RUSTFLAGS": "-C target-cpu=skylake"},
                  "-C target-cpu=skylake"),
                 ({"RUSTFLAGS": "ignored", "CARGO_ENCODED_RUSTFLAGS": "-C\x1ftarget-cpu=znver3"},
                  "-C target-cpu=znver3")]
        for environment, expected in cases:
            with self.subTest(environment=environment), patch.dict(profiles.os.environ, environment, clear=True):
                with patch.object(profiles, "cargo_config", side_effect=lambda key, default: values.get(key, default)):
                    self.assertEqual(profiles.original_rustflags("host-target"), expected)

    def test_original_target_flags_change_build_identity(self):
        values = {"build.rustflags": ["-C", "target-cpu=native"]}
        identities = []
        for cpu in ("x86-64", "native"):
            values["target.host-target.rustflags"] = ["-C", f"target-cpu={cpu}"]
            with patch.dict(profiles.os.environ,
                            {"CARGO_TARGET_HOST_TARGET_RUSTFLAGS": f"-C target-cpu={cpu}"}, clear=True):
                with patch.object(profiles, "cargo_config", side_effect=lambda key, default: values.get(key, default)):
                    with patch.object(profiles.subprocess, "check_output", side_effect=["rustc test", "host: host-target\n"]):
                        identities.append(profiles.original_build(self.parse("--profile", "original-rust"), "source"))
        self.assertNotEqual(*identities)
        self.assertIn("flags=-C target-cpu=x86-64", identities[0])

    def test_original_target_flags_merge_config_and_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            (home / "config.toml").write_text('[target.x86_64-unknown-linux-gnu]\n'
                                            'rustflags = ["-C", "force-frame-pointers=yes"]\n')
            with patch.dict(profiles.os.environ,
                            {"CARGO_HOME": directory,
                             "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS": "-C target-cpu=x86-64"},
                            clear=True):
                flags = profiles.original_rustflags("x86_64-unknown-linux-gnu")
            self.assertEqual(flags, "-C force-frame-pointers=yes -C target-cpu=x86-64")

    def test_original_conditional_target_flags_fail_without_explicit_override(self):
        values = {"target": {'cfg(target_arch = "x86_64")': {"rustflags": ["-C", "target-cpu=native"]}}}
        with patch.dict(profiles.os.environ, {}, clear=True):
            with patch.object(profiles, "cargo_config", side_effect=lambda key, default: values.get(key, default)):
                with self.assertRaisesRegex(ValueError, "conditional target rustflags"):
                    profiles.original_rustflags("host-target")

    def test_original_ffi_requires_initialized_dependency(self):
        manifest = {"revision": "pinned", "files": {},
                    "submodules": {"missing-submodule": {"feature": "ffi", "revision": "pinned"}}}
        with patch.object(profiles, "manifests", return_value=(manifest, NO_REPAIRS)):
            with self.assertRaisesRegex(ValueError, "FFI dependency uninitialized"):
                profiles.original_sources({"ffi"})
            self.assertRegex(profiles.original_sources().digest, r"^fnv1a64:")

    def test_original_ffi_verifies_revision_and_actual_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bench = root / "quickheap-dev/bench"
            (bench / "src").mkdir(parents=True)
            (bench / "src/fixture.rs").write_text("")
            (bench / "profiles.py").write_text("")
            dependency = root / "dependency"
            dependency.mkdir()
            subprocess.run(["git", "init", "-q", str(dependency)], check=True)
            header = dependency / "queue.hpp"
            header.write_text("original\n")
            subprocess.run(["git", "add", "queue.hpp"], cwd=dependency, check=True)
            subprocess.run(["git", "-c", "user.name=Test", "-c", "user.email=test@example.invalid",
                            "commit", "-qm", "test fixture"], cwd=dependency, check=True)
            revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=dependency, text=True).strip()
            manifest = {"revision": "pinned", "files": {},
                        "submodules": {"dependency": {"feature": "ffi", "revision": revision}}}
            manifest_path = bench / "original-sources.json"
            manifest_path.write_text(json.dumps(manifest))
            (bench / "original-repairs.json").write_text(json.dumps(NO_REPAIRS))
            with patch.object(profiles, "ROOT", root), patch.object(profiles, "BENCH", bench):
                self.assertRegex(profiles.original_sources({"ffi"}).digest, r"^fnv1a64:")
                header.write_text("changed\n")
                with self.assertRaisesRegex(ValueError, "FFI source drift.*queue.hpp"):
                    profiles.original_sources({"ffi"})
                header.write_text("original\n")
                manifest["submodules"]["dependency"]["revision"] = "0" * 40
                manifest_path.write_text(json.dumps(manifest))
                with self.assertRaisesRegex(ValueError, "FFI revision drift"):
                    profiles.original_sources({"ffi"})

    def test_original_repair_is_attributed_only_when_complete(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bench = root / "quickheap-dev/bench"
            (bench / "src").mkdir(parents=True)
            (bench / "src/fixture.rs").write_text("")
            (bench / "profiles.py").write_text("")
            sha = lambda text: hashlib.sha256(text.encode()).hexdigest()
            manifest = {"revision": "pinned", "files": {"a.rs": sha("a"), "b.rs": sha("b")}}
            repairs = {"reference": "pinned", "repairs": [
                {"id": "fix", "commit": "c0ffee", "files": {"a.rs": sha("a2"), "b.rs": sha("b2")}}]}
            (bench / "original-sources.json").write_text(json.dumps(manifest))
            (bench / "original-repairs.json").write_text(json.dumps(repairs))
            def sources(a, b):
                (root / "a.rs").write_text(a)
                (root / "b.rs").write_text(b)
                return profiles.original_sources()
            with patch.object(profiles, "ROOT", root), patch.object(profiles, "BENCH", bench):
                reference = sources("a", "b")
                repaired = sources("a2", "b2")
                self.assertEqual(reference.revision, "pinned")
                self.assertEqual(repaired.revision, "pinned+fix@c0ffee")
                self.assertNotEqual(reference.digest, repaired.digest)
                with self.assertRaisesRegex(ValueError, "partial original repair fix: b.rs"):
                    sources("a2", "b")
                with self.assertRaisesRegex(ValueError, "source drift.*a.rs"):
                    sources("a3", "b2")

    def test_child_failure_preserves_existing_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "result.csv"
            output.write_text("previous result\n")
            def failed_run(cmd, cwd, stdout):
                stdout.write("partial result\n")
                return profiles.subprocess.CompletedProcess(cmd, 7)
            with patch.object(profiles.subprocess, "run", side_effect=failed_run):
                code = profiles.main(["--profile", "controlled-operations", "--fixture", "any.qh",
                                      "--output", str(output)])
            self.assertEqual(code, 7)
            self.assertEqual(output.read_text(), "previous result\n")

    def test_dry_run_never_launches_cargo(self):
        with patch.object(profiles.subprocess, "run") as run:
            with contextlib.redirect_stdout(io.StringIO()):
                code = profiles.main(["--profile", "original-rust", "--no-default-features", "--dry-run"])
        self.assertEqual(code, 0)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
