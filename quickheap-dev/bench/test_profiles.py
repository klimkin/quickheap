import contextlib
import csv
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("profiles", Path(__file__).with_name("profiles.py"))
profiles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(profiles)


class ProfilesTest(unittest.TestCase):
    def parse(self, *args):
        return profiles.parser().parse_args(args)

    def test_original_command_keeps_native_runner_defaults(self):
        args = self.parse("--profile", "original-rust")
        self.assertEqual(profiles.command(args),
                         ["cargo", "run", "--release", "--locked", "--package", "bench",
                          "--example", "bench", "--"])
        self.assertRegex(profiles.original_sources(), r"^fnv1a64:[0-9a-f]{16}$")

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
        with patch.object(profiles.json, "loads", return_value=manifest):
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
                                    "compiler;source=fnv1a64:1234")
        output.seek(0)
        rows = list(csv.DictReader(output))
        self.assertEqual(rows[0]["nanos"], "123")
        self.assertEqual(rows[0]["reservation"], "reserve:1048576")
        self.assertEqual(rows[1]["reservation"], "reserve:268435456")
        for row in rows:
            self.assertEqual(row["fixture_digest"], "")
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
                                        io.StringIO(), self.parse("--profile", "original-rust"), "build")

    def test_original_build_records_merged_cargo_configuration(self):
        values = {"build.rustflags": ["-C", "force-frame-pointers=yes", "-C", "target-cpu=native"],
                  "build.target": "test-target", "profile.release": {"lto": "thin"}}
        with patch.object(profiles.subprocess, "check_output", side_effect=["rustc test", "host: host-target\n"]):
            with patch.object(profiles, "cargo_config", side_effect=lambda key, default: values[key]):
                with patch.dict(profiles.os.environ, {}, clear=True):
                    build = profiles.original_build(self.parse("--profile", "original-rust"), "source")
        self.assertIn("target=test-target", build)
        self.assertIn("flags=-C force-frame-pointers=yes -C target-cpu=native", build)
        self.assertIn("lto=thin;codegen-units=1", build)

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
                code = profiles.main(["--profile", "original-rust", "--dry-run"])
        self.assertEqual(code, 0)
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
