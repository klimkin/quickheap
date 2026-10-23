#[path = "../../src/fixture.rs"]
pub mod fixture;
mod scalar;

use fixture::{Element, FixtureKey, InputFixture, Inputs, TypedFixture, Workload};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt::Debug;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

trait Key: FixtureKey + Ord + Debug + Default {
    fn bits(self) -> u64;
    fn offset(self, offset: u64) -> Self;
    fn checked_offset(self, offset: u64) -> Option<Self>;
}

macro_rules! impl_key {
    ($($key:ty => $unsigned:ty),*) => {$(
        impl Key for $key {
            fn bits(self) -> u64 { self as $unsigned as u64 }
            fn offset(self, offset: u64) -> Self {
                (self as $unsigned).wrapping_add(offset as $unsigned) as Self
            }
            fn checked_offset(self, offset: u64) -> Option<Self> {
                let value = self as i128 + offset as i128;
                if value < 0 || value > Self::MAX as i128 { None } else { Some(value as Self) }
            }
        }
    )*};
}
impl_key!(i32 => u32, i64 => u64, u32 => u32, u64 => u64);

trait Queue<T: Key>: Default {
    fn push(&mut self, key: T);
    fn pop(&mut self) -> T;
    fn len(&self) -> usize;
    #[inline(always)]
    fn offset(&mut self, base: T, offset: u64) -> T {
        base.offset(offset)
    }
}

struct Binary<T: Key>(BinaryHeap<Reverse<T>>);
impl<T: Key> Default for Binary<T> {
    fn default() -> Self {
        Self(BinaryHeap::new())
    }
}
impl<T: Key> Queue<T> for Binary<T> {
    #[inline(always)]
    fn push(&mut self, key: T) {
        self.0.push(Reverse(key));
    }
    #[inline(always)]
    fn pop(&mut self) -> T {
        self.0.pop().unwrap().0
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

#[cfg(feature = "quickheap-avx2")]
struct Quick<T: Key>(quickheap::SimdQuickHeap<T, quickheap::Avx2>)
where
    quickheap::Avx2: quickheap::SimdElem<T>;
#[cfg(feature = "quickheap-avx2")]
impl<T: Key> Default for Quick<T>
where
    quickheap::Avx2: quickheap::SimdElem<T>,
{
    fn default() -> Self {
        Self(quickheap::SimdQuickHeap::default())
    }
}
#[cfg(feature = "quickheap-avx2")]
impl<T: Key> Queue<T> for Quick<T>
where
    quickheap::Avx2: quickheap::SimdElem<T>,
{
    #[inline(always)]
    fn push(&mut self, key: T) {
        self.0.push(key);
    }
    #[inline(always)]
    fn pop(&mut self) -> T {
        self.0.pop().unwrap()
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Clone, Copy)]
struct InputView<'a, T> {
    keys: &'a [T],
    offsets: &'a [u64],
}
impl<'a, T> From<&'a Inputs<T>> for InputView<'a, T> {
    fn from(inputs: &'a Inputs<T>) -> Self {
        match inputs {
            Inputs::Keys(keys) => Self { keys, offsets: &[] },
            Inputs::Offsets(offsets) => Self { keys: &[], offsets },
        }
    }
}

fn setup<T: Key, Q: Queue<T>>(q: &mut Q, workload: Workload, n: usize, inputs: InputView<T>) {
    match workload {
        Workload::MonotoneConstantSize => {
            let mut last = T::default();
            for i in 0..n + 50 {
                let key = q.offset(last, inputs.offsets[2 * i]);
                q.push(key);
                last = q.pop();
                let key = q.offset(last, inputs.offsets[2 * i + 1]);
                q.push(key);
            }
        }
        Workload::RandomConstantSize => {
            for i in 0..n + 50 {
                q.push(inputs.keys[2 * i]);
                q.pop();
                q.push(inputs.keys[2 * i + 1]);
            }
        }
        _ => {}
    }
}

type Loop<T, Q> = fn(&mut Q, usize, InputView<T>);
fn workload_loop<T: Key, Q: Queue<T>>(workload: Workload) -> Loop<T, Q> {
    match workload {
        Workload::HeapSort => heap_sort::<T, Q>,
        Workload::MonotoneConstantSize => monotone_constant::<T, Q>,
        Workload::RandomConstantSize => random_constant::<T, Q>,
        Workload::MonotoneWiggle | Workload::GeometricMonotoneWiggle => monotone_wiggle::<T, Q>,
        Workload::RandomWiggle => random_wiggle::<T, Q>,
        Workload::WorstCaseDescending => worst_case::<T, Q>,
        Workload::MostlyPushDecreasing => mostly_push::<T, Q>,
    }
}

fn heap_sort<T: Key, Q: Queue<T>>(q: &mut Q, n: usize, inputs: InputView<T>) {
    for &key in inputs.keys {
        q.push(key);
    }
    for _ in 0..n {
        q.pop();
    }
}
fn monotone_constant<T: Key, Q: Queue<T>>(q: &mut Q, _: usize, inputs: InputView<T>) {
    for &offset in inputs.offsets {
        let last = q.pop();
        let key = q.offset(last, offset);
        q.push(key);
    }
}
fn random_constant<T: Key, Q: Queue<T>>(q: &mut Q, _: usize, inputs: InputView<T>) {
    for &key in inputs.keys {
        q.pop();
        q.push(key);
    }
}
fn monotone_wiggle<T: Key, Q: Queue<T>>(q: &mut Q, n: usize, inputs: InputView<T>) {
    let mut last = T::default();
    for i in 0..n {
        let key = q.offset(last, inputs.offsets[2 * i]);
        q.push(key);
        last = q.pop();
        let key = q.offset(last, inputs.offsets[2 * i + 1]);
        q.push(key);
    }
    for i in 0..n {
        last = q.pop();
        let key = q.offset(last, inputs.offsets[2 * n + i]);
        q.push(key);
        q.pop();
    }
}
fn random_wiggle<T: Key, Q: Queue<T>>(q: &mut Q, n: usize, inputs: InputView<T>) {
    for i in 0..n {
        q.push(inputs.keys[2 * i]);
        q.pop();
        q.push(inputs.keys[2 * i + 1]);
    }
    for i in 0..n {
        q.pop();
        q.push(inputs.keys[2 * n + i]);
        q.pop();
    }
}
fn worst_case<T: Key, Q: Queue<T>>(q: &mut Q, n: usize, inputs: InputView<T>) {
    for group in inputs.keys.as_chunks::<9>().0 {
        for &key in group {
            q.push(key);
        }
        q.pop();
    }
    for _ in 0..8 * n {
        q.pop();
    }
}
fn mostly_push<T: Key, Q: Queue<T>>(q: &mut Q, n: usize, inputs: InputView<T>) {
    for group in inputs.keys.as_chunks::<10>().0 {
        for &key in &group[..4] {
            q.push(key);
        }
        q.pop();
        for &key in &group[4..] {
            q.push(key);
        }
        q.pop();
    }
    for _ in 0..8 * n {
        q.pop();
    }
}

struct Verified<T: Key, Q: Queue<T>> {
    actual: Q,
    reference: Binary<T>,
    trace: Option<Vec<String>>,
    error: Option<String>,
    calls: usize,
}
impl<T: Key, Q: Queue<T>> Default for Verified<T, Q> {
    fn default() -> Self {
        Self {
            actual: Q::default(),
            reference: Binary::default(),
            trace: None,
            error: None,
            calls: 0,
        }
    }
}
impl<T: Key, Q: Queue<T>> Queue<T> for Verified<T, Q> {
    fn push(&mut self, key: T) {
        self.actual.push(key);
        self.reference.push(key);
        self.calls += 1;
        if let Some(trace) = &mut self.trace {
            trace.push(format!("push,{}", key.bits()));
        }
    }
    fn pop(&mut self) -> T {
        let result = self.actual.pop();
        let expected = self.reference.pop();
        if result != expected && self.error.is_none() {
            self.error = Some(format!("pop returned {result:?}, expected {expected:?}"));
        }
        self.calls += 1;
        if let Some(trace) = &mut self.trace {
            trace.push(format!("pop,{}", result.bits()));
        }
        result
    }
    fn len(&self) -> usize {
        self.actual.len()
    }
    fn offset(&mut self, base: T, offset: u64) -> T {
        if base.checked_offset(offset).is_none() && self.error.is_none() {
            self.error = Some(format!(
                "monotone addition out of range for {}: {base:?} + {offset}",
                T::ELEMENT.name()
            ));
        }
        base.offset(offset)
    }
}

fn verify<T: Key, Q: Queue<T>>(
    fixture: &TypedFixture<T>,
    trace: bool,
) -> Result<Vec<String>, String> {
    let mut q = Verified::<T, Q>::default();
    if trace {
        q.trace = Some(Vec::new());
    }
    setup(&mut q, fixture.workload, fixture.n, (&fixture.setup).into());
    let setup_calls = q.calls;
    workload_loop::<T, Verified<T, Q>>(fixture.workload)(
        &mut q,
        fixture.n,
        (&fixture.timed).into(),
    );
    let expected_calls = 2 * fixture.workload.factor() * fixture.n;
    if q.calls - setup_calls != expected_calls {
        return Err(format!(
            "timed queue calls {}, expected {expected_calls}",
            q.calls - setup_calls
        ));
    }
    let expected_size = match fixture.workload {
        Workload::MonotoneConstantSize | Workload::RandomConstantSize => fixture.n + 50,
        _ => 0,
    };
    if q.len() != expected_size || q.reference.len() != expected_size {
        return Err(format!(
            "final queue size {}, expected {expected_size}",
            q.len()
        ));
    }
    let final_size = q.len();
    if let Some(trace) = &mut q.trace {
        trace.push(format!("final,{final_size}"));
    }
    while q.reference.len() > 0 {
        let actual = q.actual.pop();
        let expected = q.reference.pop();
        if actual != expected {
            return Err(format!("drain returned {actual:?}, expected {expected:?}"));
        }
        if let Some(trace) = &mut q.trace {
            trace.push(format!("drain,{}", actual.bits()));
        }
    }
    if let Some(error) = q.error {
        return Err(error);
    }
    Ok(q.trace.unwrap_or_default())
}

trait Clock {
    type Stamp;
    fn start(&mut self) -> Self::Stamp;
    fn stop(&mut self, start: Self::Stamp) -> u128;
}
struct WallClock;
impl Clock for WallClock {
    type Stamp = Instant;
    fn start(&mut self) -> Instant {
        Instant::now()
    }
    fn stop(&mut self, start: Instant) -> u128 {
        start.elapsed().as_nanos()
    }
}
fn measure<T: Key, Q: Queue<T>, C: Clock>(
    q: &mut Q,
    n: usize,
    inputs: InputView<T>,
    run: Loop<T, Q>,
    clock: &mut C,
) -> u128 {
    let start = clock.start();
    run(q, n, inputs);
    let elapsed = clock.stop(start);
    black_box(q);
    elapsed
}

struct Options {
    fixture: PathBuf,
    heap: String,
    repeats: usize,
    warmup: usize,
    verify_only: bool,
    trace: bool,
    scalar_trace: bool,
    comparisons: bool,
    samples: Vec<u64>,
}
fn options() -> Result<Options, String> {
    let mut fixture = None;
    let mut heap = "binary_heap".to_owned();
    let mut repeats = 3;
    let mut warmup = 1;
    let mut verify_only = false;
    let mut trace = false;
    let mut scalar_trace = false;
    let mut comparisons = false;
    let mut samples = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture" => {
                fixture = Some(PathBuf::from(args.next().ok_or("missing fixture path")?))
            }
            "--heap" => heap = args.next().ok_or("missing heap name")?,
            "--repeat" => {
                repeats = args
                    .next()
                    .ok_or("missing repeat count")?
                    .parse()
                    .map_err(|_| "invalid repeat count")?
            }
            "--warmup" => {
                warmup = args
                    .next()
                    .ok_or("missing warmup count")?
                    .parse()
                    .map_err(|_| "invalid warmup count")?
            }
            "--verify-only" => verify_only = true,
            "--trace" => trace = true,
            "--scalar-trace" => scalar_trace = true,
            "--comparisons" => comparisons = true,
            "--samples" => {
                samples = args
                    .next()
                    .ok_or("missing pivot samples")?
                    .split(',')
                    .map(|value| value.parse().map_err(|_| "invalid pivot sample".to_owned()))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    if repeats == 0 {
        return Err("repeat count must be positive".into());
    }
    if !matches!(
        heap.as_str(),
        "binary_heap"
            | "quickheap"
            | "scalar_median1_linear"
            | "scalar_median3_linear"
            | "scalar_perfect_linear"
            | "scalar_median1_binary"
            | "scalar_median3_binary"
            | "scalar_perfect_binary"
    ) {
        return Err(format!("unknown heap {heap}"));
    }
    if !heap.starts_with("scalar_") && (comparisons || scalar_trace || !samples.is_empty()) {
        return Err("comparison counts and pivot traces require a scalar heap".into());
    }
    if !scalar_trace && !samples.is_empty() {
        return Err("scripted samples require --scalar-trace".into());
    }
    Ok(Options {
        fixture: fixture.ok_or("--fixture is required")?,
        heap,
        repeats,
        warmup,
        verify_only,
        trace,
        scalar_trace,
        comparisons,
        samples,
    })
}

fn csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn run<T: Key, Q: Queue<T>>(
    fixture: &InputFixture,
    options: &Options,
    heap: &str,
    backend: &str,
    comparisons: Option<(u64, u64)>,
) -> Result<(), String> {
    let fixture = fixture.decode::<T>().map_err(|error| error.to_string())?;
    let trace = verify::<T, Q>(&fixture, options.trace || options.verify_only)?;
    if options.trace || options.verify_only {
        for line in trace {
            println!("{line}");
        }
    }
    if options.verify_only {
        return Ok(());
    }
    let timed: InputView<T> = (&fixture.timed).into();
    let setup_inputs: InputView<T> = (&fixture.setup).into();
    let loop_fn = workload_loop::<T, Q>(fixture.workload);
    println!(
        "elem,heap,n,workload,ops,repeat,nanos,protocol,build,backend,fixture_digest,reservation,timer,repeats,warmup,push_comparisons,pop_comparisons"
    );
    for repeat in 0..options.warmup + options.repeats {
        let mut q = Q::default();
        setup(&mut q, fixture.workload, fixture.n, setup_inputs);
        let nanos = if comparisons.is_some() {
            0
        } else {
            measure(&mut q, fixture.n, timed, loop_fn, &mut WallClock)
        };
        if repeat >= options.warmup {
            println!(
                "{},{heap},{},{},{},{},{nanos},{},{},{backend},{},backend-default,workload-only-raii,{},{},{},{}",
                T::ELEMENT.name(),
                fixture.n,
                fixture.workload.name(),
                fixture.workload.factor() * fixture.n,
                repeat - options.warmup,
                fixture::PROTOCOL,
                csv(env!("PAIRED_BUILD")),
                fixture.digest,
                options.repeats,
                options.warmup,
                comparisons
                    .map(|counts| counts.0.to_string())
                    .unwrap_or_default(),
                comparisons
                    .map(|counts| counts.1.to_string())
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn keys<T: Key>(values: &[T]) -> String {
    values
        .iter()
        .map(|value| value.bits().to_string())
        .collect::<Vec<_>>()
        .join(":")
}
struct ScalarTrace<T: Key, const S: usize, const P: bool, const L: bool> {
    actual: scalar::Scalar<T, S, P, L, true>,
    reference: Binary<T>,
    lines: Vec<String>,
    error: Option<String>,
}
impl<T: Key, const S: usize, const P: bool, const L: bool> Default for ScalarTrace<T, S, P, L> {
    fn default() -> Self {
        Self {
            actual: scalar::Scalar::default(),
            reference: Binary::default(),
            lines: Vec::new(),
            error: None,
        }
    }
}
impl<T: Key, const S: usize, const P: bool, const L: bool> ScalarTrace<T, S, P, L> {
    fn snapshot(&mut self, operation: &str, value: T) {
        let counts = self.actual.counters();
        self.lines.push(format!(
            "{operation},{},{},{},{},{}",
            value.bits(),
            counts.push,
            counts.pop,
            keys(self.actual.pivots()),
            self.actual
                .buckets()
                .iter()
                .map(|bucket| keys(bucket))
                .collect::<Vec<_>>()
                .join(";")
        ));
    }
}
impl<T: Key, const S: usize, const P: bool, const L: bool> Queue<T> for ScalarTrace<T, S, P, L> {
    fn push(&mut self, key: T) {
        self.actual.push(key);
        self.reference.push(key);
        self.snapshot("push", key);
    }
    fn pop(&mut self) -> T {
        let lines = &mut self.lines;
        let value = self.actual.pop_observed(|event| {
            lines.push(format!(
                "partition,{},{},{},{},{},{},{}",
                event.pivot.bits(),
                event.pivot_index,
                u8::from(event.retry),
                event.push_comparisons,
                event.pop_comparisons,
                keys(event.upper),
                keys(event.lower)
            ));
        });
        let expected = self.reference.pop();
        if expected != value && self.error.is_none() {
            self.error = Some(format!(
                "scalar pop returned {value:?}, expected {expected:?}"
            ));
        }
        self.snapshot("pop", value);
        value
    }
    fn len(&self) -> usize {
        self.actual.len()
    }
}
fn run_scalar<T: Key, const S: usize, const P: bool, const L: bool>(
    fixture: &InputFixture,
    options: &Options,
) -> Result<(), String> {
    let typed = fixture.decode::<T>().map_err(|error| error.to_string())?;
    if options.scalar_trace {
        verify::<T, scalar::Scalar<T, S, P, L, false>>(&typed, false)?;
        let mut queue = ScalarTrace::<T, S, P, L> {
            actual: scalar::Scalar::new(&options.samples),
            ..Default::default()
        };
        setup(&mut queue, typed.workload, typed.n, (&typed.setup).into());
        queue.actual.reset_counters();
        queue.lines.push("reset,0,0".to_owned());
        workload_loop::<T, ScalarTrace<T, S, P, L>>(typed.workload)(
            &mut queue,
            typed.n,
            (&typed.timed).into(),
        );
        let counts = queue.actual.counters();
        queue.lines.push(format!(
            "final,{},{},{}",
            queue.len(),
            counts.push,
            counts.pop
        ));
        while queue.len() > 0 {
            queue.pop();
        }
        if let Some(error) = queue.error {
            return Err(error);
        }
        for line in queue.lines {
            println!("{line}");
        }
        return Ok(());
    }
    let comparisons = if options.comparisons {
        let mut queue = scalar::Scalar::<T, S, P, L, true>::default();
        setup(&mut queue, typed.workload, typed.n, (&typed.setup).into());
        queue.reset_counters();
        workload_loop::<T, scalar::Scalar<T, S, P, L, true>>(typed.workload)(
            &mut queue,
            typed.n,
            (&typed.timed).into(),
        );
        let counts = queue.counters();
        Some((counts.push, counts.pop))
    } else {
        None
    };
    run::<T, scalar::Scalar<T, S, P, L, false>>(
        fixture,
        options,
        &options.heap,
        "paired-scalar-cpp-v1",
        comparisons,
    )
}

fn main_result() -> Result<(), String> {
    let options = options()?;
    let fixture = InputFixture::read(&options.fixture).map_err(|error| error.to_string())?;
    macro_rules! dispatch {
        ($key:ty) => {{
            match options.heap.as_str() {
                "binary_heap" => run::<$key, Binary<$key>>(
                    &fixture,
                    &options,
                    "binary_heap",
                    "rust-std-substitute",
                    None,
                ),
                "quickheap" => {
                    #[cfg(feature = "quickheap-avx2")]
                    {
                        if !std::is_x86_feature_detected!("avx2") {
                            return Err("quickheap requires AVX2".into());
                        }
                        run::<$key, Quick<$key>>(
                            &fixture,
                            &options,
                            "quickheap_avx2",
                            "rust-production-avx2",
                            None,
                        )
                    }
                    #[cfg(not(feature = "quickheap-avx2"))]
                    {
                        Err("quickheap requires cargo feature quickheap-avx2".into())
                    }
                }
                "scalar_median1_linear" => run_scalar::<$key, 1, false, true>(&fixture, &options),
                "scalar_median3_linear" => run_scalar::<$key, 3, false, true>(&fixture, &options),
                "scalar_perfect_linear" => run_scalar::<$key, 1, true, true>(&fixture, &options),
                "scalar_median1_binary" => run_scalar::<$key, 1, false, false>(&fixture, &options),
                "scalar_median3_binary" => run_scalar::<$key, 3, false, false>(&fixture, &options),
                "scalar_perfect_binary" => run_scalar::<$key, 1, true, false>(&fixture, &options),
                _ => unreachable!(),
            }
        }};
    }
    match fixture.elem {
        Element::I32 => dispatch!(i32),
        Element::I64 => dispatch!(i64),
        Element::U32 => dispatch!(u32),
        Element::U64 => dispatch!(u64),
    }
}
fn main() {
    if let Err(error) = main_result() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Controlled {
        popped: u32,
        pushed: Vec<u32>,
    }
    impl Queue<u32> for Controlled {
        fn push(&mut self, key: u32) {
            self.pushed.push(key);
        }
        fn pop(&mut self) -> u32 {
            self.popped
        }
        fn len(&self) -> usize {
            0
        }
    }
    #[test]
    fn tested_pop_controls_next_monotone_push() {
        for popped in [10, 37] {
            let mut q = Controlled {
                popped,
                ..Controlled::default()
            };
            monotone_constant(
                &mut q,
                1,
                InputView {
                    keys: &[],
                    offsets: &[6],
                },
            );
            assert_eq!(q.pushed, [popped + 6]);
        }
    }

    thread_local! { static EVENTS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) }; }
    struct LifetimeQueue {
        _marker: (),
    }
    impl Default for LifetimeQueue {
        fn default() -> Self {
            EVENTS.with(|events| events.borrow_mut().push("construct"));
            Self { _marker: () }
        }
    }
    impl Drop for LifetimeQueue {
        fn drop(&mut self) {
            EVENTS.with(|events| events.borrow_mut().push("drop"));
        }
    }
    impl Queue<u32> for LifetimeQueue {
        fn push(&mut self, _: u32) {
            EVENTS.with(|events| events.borrow_mut().push("push"));
        }
        fn pop(&mut self) -> u32 {
            EVENTS.with(|events| events.borrow_mut().push("pop"));
            0
        }
        fn len(&self) -> usize {
            0
        }
    }
    struct EventClock;
    impl Clock for EventClock {
        type Stamp = ();
        fn start(&mut self) {
            EVENTS.with(|events| events.borrow_mut().push("start"));
        }
        fn stop(&mut self, _: ()) -> u128 {
            EVENTS.with(|events| events.borrow_mut().push("stop"));
            0
        }
    }
    #[test]
    fn construction_setup_and_destruction_stay_outside_timer() {
        EVENTS.with(|events| events.borrow_mut().clear());
        {
            let mut q = LifetimeQueue::default();
            q.push(1);
            measure(
                &mut q,
                1,
                InputView {
                    keys: &[2],
                    offsets: &[],
                },
                heap_sort,
                &mut EventClock,
            );
        }
        EVENTS.with(|events| {
            assert_eq!(
                *events.borrow(),
                ["construct", "push", "start", "push", "pop", "stop", "drop"]
            )
        });
    }
    #[test]
    fn validation_rejects_monotone_overflow() {
        let fixture = TypedFixture {
            workload: Workload::MonotoneWiggle,
            n: 1,
            digest: String::new(),
            setup: Inputs::Offsets(vec![]),
            timed: Inputs::Offsets(vec![u32::MAX as u64, 1, 0]),
        };
        assert!(
            verify::<u32, Binary<u32>>(&fixture, false)
                .unwrap_err()
                .contains("out of range")
        );
    }

    #[test]
    fn every_workload_preserves_reference_results_and_call_counts() {
        let workloads = [
            Workload::HeapSort,
            Workload::MonotoneConstantSize,
            Workload::RandomConstantSize,
            Workload::MonotoneWiggle,
            Workload::GeometricMonotoneWiggle,
            Workload::RandomWiggle,
            Workload::WorstCaseDescending,
            Workload::MostlyPushDecreasing,
        ];
        for workload in workloads {
            for elem in [Element::I32, Element::I64, Element::U32, Element::U64] {
                let n = 8;
                let mut text = format!("QHB1 {} {} {n}\n", elem.name(), workload.name());
                for (name, count) in [
                    ("setup", workload.setup_count(n)),
                    ("timed", workload.timed_count(n)),
                ] {
                    text.push_str(&format!("{name} {count}\n"));
                    for index in 0..count {
                        text.push_str(&format!("{}\n", 1 + index % 13));
                    }
                }
                let fixture = InputFixture::parse(text.as_bytes()).unwrap();
                macro_rules! check {
                    ($key:ty) => {{
                        let typed = fixture.decode().unwrap();
                        verify::<$key, Binary<$key>>(&typed, false).unwrap();
                        verify::<$key, scalar::Scalar<$key, 1, false, true, false>>(&typed, false)
                            .unwrap();
                        verify::<$key, scalar::Scalar<$key, 3, false, true, false>>(&typed, false)
                            .unwrap();
                        verify::<$key, scalar::Scalar<$key, 1, true, true, false>>(&typed, false)
                            .unwrap();
                        verify::<$key, scalar::Scalar<$key, 1, false, false, false>>(&typed, false)
                            .unwrap();
                        verify::<$key, scalar::Scalar<$key, 3, false, false, false>>(&typed, false)
                            .unwrap();
                        verify::<$key, scalar::Scalar<$key, 1, true, false, false>>(&typed, false)
                            .unwrap();
                    }};
                }
                match elem {
                    Element::I32 => check!(i32),
                    Element::I64 => check!(i64),
                    Element::U32 => check!(u32),
                    Element::U64 => check!(u64),
                };
            }
        }
    }
}
