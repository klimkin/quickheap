# Benchmark profiles

`profiles.py` selects two timing contracts without changing historical runner,
workload, or heap sources.

| Profile | Runner | Protocol | Inputs | Timer |
|---|---|---|---|---|
| `original-rust` | Original `bench` example | `quickheap-original-rust-a9f2518-v1` | Fresh original random inputs each trial | Workload loops, consumed input storage, and returned queue destruction |
| `controlled-operations` | Independent `paired` crate | `quickheap-cpp-v1` | Shared fixture reused across trials | Workload loops only; queue construction, setup, and destruction stay outside |

Compare C++ and Rust results within one profile. Historical random trials share
an input distribution, not identical bytes. Controlled trials share exact fixture
bytes and retain their digest. Heap and backend identities still need matching;
profile selection does not establish algorithm parity.

## Original Rust options

```sh
python3 quickheap-dev/bench/profiles.py --profile original-rust \
  --output original.csv --raw-output original-raw.csv
```

Launcher invokes `cargo run --release --locked --package bench --example bench` from Rust
repository root. Default Cargo features remain `avx2,ffi`. `--offline`,
`--no-default-features`, and `--features` pass to Cargo. Timing permits `avx2`,
`avx512`, `ffi`, and `boost`; instrumentation features require another protocol.

`--min` and `--max` select size exponents within 1 through 25. Defaults remain 10
and 25. `--i32` or `--i64` selects one width; omit both for original width order.
Both width flags together fail because original runner emits no rows for that
combination. `--table` preserves original i64 SIMD registrations and exponent
step 5. Width filters with table mode fail.

Original runner executes one warmup and three measured trials. Plot cutoff is
32 ns per `ops * log2(n)`; table cutoff is 128. A cutoff stops larger sizes for
one implementation and workload. Standard binary heap reserves 1,048,576
elements before timing. FFI original quickheap reserves 268,435,456 elements.
Other queues retain backend defaults.

Fixture, heap, repeat, warmup, correctness trace, comparison, memory, and ablation
options fail in this profile. Original active registrations remain authoritative.

`original-sources.json` records SHA-256 hashes from reference revision
`a9f2518d9ed452130361cfe1681c68625a68ab35`. Launcher verifies compiled Rust sources,
FFI wrappers, manifests, locks, and build configuration before Cargo runs.
Enabled `ffi` also requires all six third-party submodules at manifest revisions.
Launcher checks their tracked file contents and rejects worktree changes.
Uninitialized dependencies fail before Cargo runs and identify the missing path.
`--no-default-features` skips dependencies that selected features do not compile.
Existing `pub mod fixture;` export in benchmark library root is the only allowed
source extension. Provenance hashes actual bytes, including fixture module,
launcher, and source manifest. Cargo lock stays frozen; launcher checks sources
again before attributing successful results.

Current pinned development crate fails to compile on installed nightly. Unsafe
`simd::push_position` call at `quickheap-dev/src/lib.rs:331` produces E0133 even
with `--no-default-features`. Launcher returns Cargo failure, preserves any
existing attributed output, and writes no replacement result. No safety patch or
fallback runner changes original reference behavior.

## Controlled operation options

```sh
python3 quickheap-dev/bench/profiles.py --profile controlled-operations \
  --fixture input.qh --heap binary_heap --repeat 5 --warmup 1 \
  --offline --output controlled.csv
```

Fixture selects width, size, and workload. `--heap`, `--repeat`, `--warmup`,
`--verify-only`, `--trace`, `--scalar-trace`, `--samples`, and `--comparisons`
pass to paired runner. Scalar diagnostic options require scalar heaps.
Controlled runner validates full output against reference heap before timing.
Cutoffs remain disabled. Standard binary heap grows naturally.

`--features quickheap-avx2` enables production AVX2 queue. Independent paired
workspace does not inherit original fat LTO and single codegen unit settings.
Build provenance records compiler version, Cargo target, optimization level,
enabled features, and `CARGO_ENCODED_RUSTFLAGS` supplied to the build script.
Paired metadata omits LTO and codegen unit settings because Cargo does not expose
their effective values to build scripts. Archive the Cargo command and verbose
build log when overriding those settings, including overrides through `--config`.

Direct paired CLI accepts `--profile controlled-operations`. Default retains
that profile for existing callers. `--profile original-rust` fails and directs
caller to launcher.

## Result attribution

Original raw CSV stays unchanged. `--raw-output` archives that stream. Launcher
adds attribution after child exits successfully, using temporary disk storage
instead of buffering result set in memory. `--output` replaces result file only
after successful attribution; omitted output sends result to stdout.

Timing rows retain `protocol`, `build`, `backend`, `fixture_digest`, `reservation`,
`timer`, `repeats`, `warmup`, `profile`, `input_policy`, and `cutoff`.
Original fixture digest stays empty and input policy is `fresh-random-original`.
Controlled input policy is `shared-fixture` and cutoff is `disabled`.
Original attribution preserves metrics and full Rust heap type names;
workload names drop Rust module prefixes.

Original attribution resolves `RUSTC` or `build.rustc` and records release profile
settings from merged Cargo configuration and environment overrides.
Rust flags follow Cargo precedence. Encoded flags override `RUSTFLAGS`, followed
by target-specific flags, then `build.rustflags`. Target environment flags include
`CARGO_TARGET_<TRIPLE>_RUSTFLAGS`. Conditional `cfg(...)` rustflags fail attribution
unless `RUSTFLAGS` or encoded flags provide an explicit override.

`--dry-run` validates profile options and source pin, then prints Cargo command
without executing it. `--help` lists launcher options.

Verification commands follow.

```sh
python3 -m unittest discover -s quickheap-dev/bench -p test_profiles.py -v
cargo test --manifest-path quickheap-dev/bench/paired/Cargo.toml --offline
```
