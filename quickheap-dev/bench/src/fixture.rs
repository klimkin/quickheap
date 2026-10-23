use std::fmt;
use std::path::Path;

pub const PROTOCOL: &str = "quickheap-cpp-v1";
pub const MAX_N: usize = 1 << 25;
pub const INSPECT_HEADER: &str = "protocol,elem,workload,n,fixture_digest,setup_count,timed_count";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Element {
    I32,
    I64,
    U32,
    U64,
}

impl Element {
    pub fn name(self) -> &'static str {
        match self {
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::U32 => "u32",
            Self::U64 => "u64",
        }
    }

    fn parse(value: &str) -> Result<Self, FixtureError> {
        match value {
            "i32" => Ok(Self::I32),
            "i64" => Ok(Self::I64),
            "u32" => Ok(Self::U32),
            "u64" => Ok(Self::U64),
            _ => Err(FixtureError(format!("unknown element {value}"))),
        }
    }

    fn max_bits(self) -> u64 {
        match self {
            Self::I32 | Self::U32 => u32::MAX as u64,
            Self::I64 | Self::U64 => u64::MAX,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Workload {
    HeapSort,
    MonotoneConstantSize,
    RandomConstantSize,
    MonotoneWiggle,
    GeometricMonotoneWiggle,
    RandomWiggle,
    WorstCaseDescending,
    MostlyPushDecreasing,
}

impl Workload {
    pub fn name(self) -> &'static str {
        match self {
            Self::HeapSort => "HeapSort",
            Self::MonotoneConstantSize => "MonotoneConstantSize",
            Self::RandomConstantSize => "RandomConstantSize",
            Self::MonotoneWiggle => "MonotoneWiggle",
            Self::GeometricMonotoneWiggle => "GeometricMonotoneWiggle",
            Self::RandomWiggle => "RandomWiggle",
            Self::WorstCaseDescending => "WorstCaseDescending",
            Self::MostlyPushDecreasing => "MostlyPushDecreasing",
        }
    }

    fn parse(value: &str) -> Result<Self, FixtureError> {
        match value {
            "HeapSort" => Ok(Self::HeapSort),
            "MonotoneConstantSize" => Ok(Self::MonotoneConstantSize),
            "RandomConstantSize" => Ok(Self::RandomConstantSize),
            "MonotoneWiggle" => Ok(Self::MonotoneWiggle),
            "GeometricMonotoneWiggle" => Ok(Self::GeometricMonotoneWiggle),
            "RandomWiggle" => Ok(Self::RandomWiggle),
            "WorstCaseDescending" => Ok(Self::WorstCaseDescending),
            "MostlyPushDecreasing" => Ok(Self::MostlyPushDecreasing),
            _ => Err(FixtureError(format!("unknown workload {value}"))),
        }
    }

    pub fn is_monotone(self) -> bool {
        matches!(
            self,
            Self::MonotoneConstantSize | Self::MonotoneWiggle | Self::GeometricMonotoneWiggle
        )
    }

    pub fn setup_count(self, n: usize) -> usize {
        match self {
            Self::MonotoneConstantSize | Self::RandomConstantSize => 2 * (n + 50),
            _ => 0,
        }
    }

    pub fn timed_count(self, n: usize) -> usize {
        self.factor() * n
    }

    pub fn factor(self) -> usize {
        match self {
            Self::HeapSort => 1,
            Self::MonotoneConstantSize | Self::RandomConstantSize | Self::MostlyPushDecreasing => {
                10
            }
            Self::MonotoneWiggle | Self::GeometricMonotoneWiggle | Self::RandomWiggle => 3,
            Self::WorstCaseDescending => 9,
        }
    }
}

#[derive(Debug)]
pub struct FixtureError(String);

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FixtureError {}

#[derive(Clone, Debug)]
pub struct InputFixture {
    pub elem: Element,
    pub workload: Workload,
    pub n: usize,
    pub digest: String,
    pub setup: Vec<u64>,
    pub timed: Vec<u64>,
}

#[derive(Clone, Debug)]
pub enum Inputs<T> {
    Keys(Vec<T>),
    Offsets(Vec<u64>),
}

#[derive(Clone, Debug)]
pub struct TypedFixture<T> {
    pub workload: Workload,
    pub n: usize,
    pub digest: String,
    pub setup: Inputs<T>,
    pub timed: Inputs<T>,
}

pub trait FixtureKey: Copy {
    const ELEMENT: Element;
    fn from_bits(bits: u64) -> Self;
}

macro_rules! impl_fixture_key {
    ($key:ty, $element:ident) => {
        impl FixtureKey for $key {
            const ELEMENT: Element = Element::$element;

            fn from_bits(bits: u64) -> Self {
                bits as Self
            }
        }
    };
}

impl_fixture_key!(i32, I32);
impl_fixture_key!(i64, I64);
impl_fixture_key!(u32, U32);
impl_fixture_key!(u64, U64);

impl InputFixture {
    pub fn read(path: impl AsRef<Path>) -> Result<Self, FixtureError> {
        let bytes = std::fs::read(path.as_ref()).map_err(|err| {
            FixtureError(format!("cannot read {}: {err}", path.as_ref().display()))
        })?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, FixtureError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| FixtureError("fixture must contain UTF-8 text".into()))?;
        let mut tokens = text.split_ascii_whitespace();
        expect(&mut tokens, "QHB1")?;
        let elem = Element::parse(next(&mut tokens, "element")?)?;
        let workload = Workload::parse(next(&mut tokens, "workload")?)?;
        let raw_n = decimal(next(&mut tokens, "n")?)?;
        if raw_n == 0 || raw_n > MAX_N as u64 {
            return Err(FixtureError(format!("n must be in 1..={MAX_N}")));
        }
        let n = raw_n as usize;
        let setup = parse_inputs(
            &mut tokens,
            "setup",
            workload.setup_count(n),
            elem,
            workload.is_monotone(),
        )?;
        let timed = parse_inputs(
            &mut tokens,
            "timed",
            workload.timed_count(n),
            elem,
            workload.is_monotone(),
        )?;
        if tokens.next().is_some() {
            return Err(FixtureError("trailing fixture fields".into()));
        }
        Ok(Self {
            elem,
            workload,
            n,
            digest: fixture_digest(bytes),
            setup,
            timed,
        })
    }

    pub fn decode<T: FixtureKey>(&self) -> Result<TypedFixture<T>, FixtureError> {
        if self.elem != T::ELEMENT {
            return Err(FixtureError(format!(
                "fixture element {} does not match {}",
                self.elem.name(),
                T::ELEMENT.name()
            )));
        }
        let decode = |values: &[u64]| {
            if self.workload.is_monotone() {
                Inputs::Offsets(values.to_vec())
            } else {
                Inputs::Keys(values.iter().copied().map(T::from_bits).collect())
            }
        };
        Ok(TypedFixture {
            workload: self.workload,
            n: self.n,
            digest: self.digest.clone(),
            setup: decode(&self.setup),
            timed: decode(&self.timed),
        })
    }

    pub fn inspect_row(&self) -> String {
        format!(
            "{PROTOCOL},{},{},{},{},{},{}",
            self.elem.name(),
            self.workload.name(),
            self.n,
            self.digest,
            self.setup.len(),
            self.timed.len()
        )
    }
}

pub fn fixture_digest(bytes: &[u8]) -> String {
    let digest = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ *byte as u64).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{digest:016x}")
}

fn next<'a>(
    tokens: &mut std::str::SplitAsciiWhitespace<'a>,
    field: &str,
) -> Result<&'a str, FixtureError> {
    tokens
        .next()
        .ok_or_else(|| FixtureError(format!("missing {field}")))
}

fn expect(
    tokens: &mut std::str::SplitAsciiWhitespace<'_>,
    expected: &str,
) -> Result<(), FixtureError> {
    let value = next(tokens, expected)?;
    if value != expected {
        return Err(FixtureError(format!("expected {expected}, found {value}")));
    }
    Ok(())
}

fn decimal(value: &str) -> Result<u64, FixtureError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FixtureError(format!("invalid unsigned decimal {value}")));
    }
    value
        .parse()
        .map_err(|_| FixtureError(format!("unsigned decimal out of range {value}")))
}

fn parse_inputs(
    tokens: &mut std::str::SplitAsciiWhitespace<'_>,
    name: &str,
    expected_count: usize,
    elem: Element,
    offsets: bool,
) -> Result<Vec<u64>, FixtureError> {
    expect(tokens, name)?;
    let count = decimal(next(tokens, "input count")?)?;
    if count != expected_count as u64 {
        return Err(FixtureError(format!(
            "{name} count {count} does not match expected {expected_count}"
        )));
    }
    let mut values = Vec::new();
    for _ in 0..expected_count {
        let value = decimal(next(tokens, name)?)?;
        if !offsets && value > elem.max_bits() {
            return Err(FixtureError(format!(
                "{name} value {value} exceeds {} width",
                elem.name()
            )));
        }
        values.push(value);
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(elem: Element, workload: Workload, n: usize, value: u64) -> Vec<u8> {
        let mut text = format!("QHB1 {} {} {n}\n", elem.name(), workload.name());
        for (label, count) in [
            ("setup", workload.setup_count(n)),
            ("timed", workload.timed_count(n)),
        ] {
            text.push_str(&format!("{label} {count}\n"));
            for _ in 0..count {
                text.push_str(&format!("{value}\n"));
            }
        }
        text.into_bytes()
    }

    #[test]
    fn accepts_every_workload_and_element() {
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
        for elem in [Element::I32, Element::I64, Element::U32, Element::U64] {
            for workload in workloads {
                let bytes = fixture(elem, workload, 2, elem.max_bits());
                let input = InputFixture::parse(&bytes).unwrap();
                assert_eq!(input.elem, elem);
                assert_eq!(input.workload, workload);
                assert_eq!(input.setup.len(), workload.setup_count(2));
                assert_eq!(input.timed.len(), workload.timed_count(2));
                assert!(input.timed.iter().all(|value| *value == elem.max_bits()));
            }
        }
    }

    #[test]
    fn decodes_signed_keys_and_unsigned_offsets() {
        let input = InputFixture::parse(&fixture(Element::I32, Workload::HeapSort, 1, 0xffff_ffff))
            .unwrap();
        let typed = input.decode::<i32>().unwrap();
        assert!(matches!(typed.timed, Inputs::Keys(values) if values == [-1]));
        assert!(input.decode::<u32>().is_err());

        let input = InputFixture::parse(&fixture(
            Element::I64,
            Workload::MonotoneConstantSize,
            1,
            u64::MAX,
        ))
        .unwrap();
        let typed = input.decode::<i64>().unwrap();
        assert!(matches!(typed.setup, Inputs::Offsets(values) if values == vec![u64::MAX; 102]));
        assert!(matches!(typed.timed, Inputs::Offsets(values) if values == vec![u64::MAX; 10]));

        let input = InputFixture::parse(&fixture(
            Element::I32,
            Workload::MonotoneWiggle,
            1,
            u64::MAX,
        ))
        .unwrap();
        let typed = input.decode::<i32>().unwrap();
        assert!(matches!(typed.timed, Inputs::Offsets(values) if values == vec![u64::MAX; 3]));

        let input = InputFixture::parse(&fixture(
            Element::U32,
            Workload::MonotoneWiggle,
            1,
            u64::MAX,
        ))
        .unwrap();
        let typed = input.decode::<u32>().unwrap();
        assert!(matches!(typed.timed, Inputs::Offsets(values) if values == vec![u64::MAX; 3]));
    }

    #[test]
    fn rejects_invalid_headers_counts_and_values() {
        let invalid = [
            "QHB2 u32 HeapSort 1 setup 0 timed 1 0",
            "QHB1 f32 HeapSort 1 setup 0 timed 1 0",
            "QHB1 u32 Unknown 1 setup 0 timed 1 0",
            "QHB1 u32 HeapSort 0 setup 0 timed 0",
            "QHB1 u32 HeapSort 33554433 setup 0 timed 1 0",
            "QHB1 u32 HeapSort -1 setup 0 timed 1 0",
            "QHB1 u32 HeapSort +1 setup 0 timed 1 0",
            "QHB1 u32 HeapSort 1 start 0 timed 1 0",
            "QHB1 u32 HeapSort 1 setup 1 0 timed 1 0",
            "QHB1 u32 HeapSort 1 setup 0 timed 2 0 0",
            "QHB1 u32 HeapSort 1 setup 0 timed 18446744073709551615",
            "QHB1 u32 HeapSort 1 setup 0 timed 1 4294967296",
            "QHB1 u32 HeapSort 1 setup 0 timed 1 -1",
            "QHB1 u64 HeapSort 1 setup 0 timed 1 18446744073709551616",
            "QHB1 u32 HeapSort 1 setup 0 timed 1",
            "QHB1 u32 HeapSort 1 setup 0 timed 1 0 trailing",
        ];
        for text in invalid {
            assert!(InputFixture::parse(text.as_bytes()).is_err(), "{text}");
        }
    }

    #[test]
    fn digest_attributes_exact_bytes() {
        assert_eq!(fixture_digest(b"hello"), "fnv1a64:a430d84680aabd0b");
        let bytes = fixture(Element::U32, Workload::HeapSort, 1, 7);
        let input = InputFixture::parse(&bytes).unwrap();
        let mut changed = bytes.clone();
        changed.push(b'\n');
        let other = InputFixture::parse(&changed).unwrap();
        assert_ne!(input.digest, other.digest);
        assert_eq!(input.timed, other.timed);
    }
}
