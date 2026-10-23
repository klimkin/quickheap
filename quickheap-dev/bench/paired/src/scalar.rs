use super::{Key, Queue};
use std::cmp::Ordering;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counters {
    pub push: u64,
    pub pop: u64,
    pub partitions: u64,
    pub elements: u64,
    pub retries: u64,
    pub samples: u64,
}

pub struct PartitionEvent<'a, T> {
    pub pivot: T,
    pub pivot_index: usize,
    pub retry: bool,
    pub upper: &'a [T],
    pub lower: &'a [T],
    pub push_comparisons: u64,
    pub pop_comparisons: u64,
}

pub struct Scalar<
    T,
    const SAMPLE: usize,
    const PERFECT: bool,
    const LINEAR: bool,
    const COUNT: bool,
> {
    pivots: Vec<T>,
    buckets: Vec<Vec<T>>,
    size: usize,
    random: u64,
    scripted: Vec<u64>,
    next_script: usize,
    counters: Counters,
}

impl<T: Key, const S: usize, const P: bool, const L: bool, const C: bool> Default
    for Scalar<T, S, P, L, C>
{
    fn default() -> Self {
        Self::new(&[])
    }
}

impl<T: Key, const S: usize, const P: bool, const L: bool, const C: bool> Scalar<T, S, P, L, C> {
    pub fn new(scripted: &[u64]) -> Self {
        let mut buckets = Vec::with_capacity(128);
        buckets.push(Vec::new());
        Self {
            pivots: Vec::with_capacity(128),
            buckets,
            size: 0,
            random: 0x243f_6a88_85a3_08d3,
            scripted: scripted.to_vec(),
            next_script: 0,
            counters: Counters::default(),
        }
    }
    pub fn pivots(&self) -> &[T] {
        &self.pivots
    }
    pub fn buckets(&self) -> &[Vec<T>] {
        &self.buckets[..self.pivots.len() + 1]
    }
    pub fn counters(&self) -> Counters {
        self.counters
    }
    pub fn reset_counters(&mut self) {
        self.counters = Counters::default();
    }
    fn random_word(&mut self) -> u64 {
        self.random = self.random.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut word = self.random;
        word = (word ^ (word >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        word = (word ^ (word >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        word ^ (word >> 31)
    }
    fn sample_index(&mut self, size: usize) -> usize {
        if C {
            self.counters.samples += 1;
        }
        if let Some(&index) = self.scripted.get(self.next_script) {
            self.next_script += 1;
            assert!(index < size as u64, "scripted pivot sample out of range");
            return index as usize;
        }
        let bound = size as u64;
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let word = self.random_word();
            if word >= threshold {
                return (word % bound) as usize;
            }
        }
    }
    fn pivot_index(&mut self, bucket: &mut [T]) -> usize {
        if P {
            bucket.sort();
            return (bucket.len() - 1) / 2;
        }
        let mut samples = [(T::default(), 0usize); S];
        for sample in &mut samples {
            let index = self.sample_index(bucket.len());
            *sample = (bucket[index], index);
        }
        for index in 1..S {
            let value = samples[index];
            let mut position = index;
            while position > 0 {
                if C {
                    self.counters.pop += 1;
                }
                let less = match value.0.cmp(&samples[position - 1].0) {
                    Ordering::Less => true,
                    Ordering::Equal => value.1 < samples[position - 1].1,
                    Ordering::Greater => false,
                };
                if !less {
                    break;
                }
                samples[position] = samples[position - 1];
                position -= 1;
            }
            samples[position] = value;
        }
        samples[(S - 1) / 2].1
    }
    pub fn pop_observed(&mut self, mut observe: impl FnMut(PartitionEvent<'_, T>)) -> T {
        assert!(self.size > 0, "pop from empty scalar queue");
        loop {
            let last = self.pivots.len();
            if self.buckets[last].len() == 1 {
                let key = self.buckets[last].pop().unwrap();
                if last > 0 {
                    self.pivots.pop();
                }
                self.size -= 1;
                return key;
            }
            if last + 1 == self.buckets.len() {
                self.buckets.push(Vec::new());
            }
            let mut bucket = std::mem::take(&mut self.buckets[last]);
            let pivot_index = self.pivot_index(&mut bucket);
            let pivot = bucket[pivot_index];
            self.buckets[last] = bucket;
            let (active, spare) = self.buckets.split_at_mut(last + 1);
            let upper = &mut active[last];
            let lower = &mut spare[0];
            lower.clear();
            let size = upper.len();
            lower.reserve(size);
            if C {
                self.counters.partitions += 1;
                self.counters.elements += size as u64;
                self.counters.pop += size as u64;
            }
            let mut index = 0;
            upper.retain(|&key| {
                let goes_lower = if index <= pivot_index {
                    key <= pivot
                } else {
                    key < pivot
                };
                index += 1;
                if goes_lower {
                    lower.push(key);
                }
                !goes_lower
            });
            let retry = upper.is_empty();
            if C && retry {
                self.counters.retries += 1;
            }
            observe(PartitionEvent {
                pivot,
                pivot_index,
                retry,
                upper,
                lower,
                push_comparisons: self.counters.push,
                pop_comparisons: self.counters.pop,
            });
            if retry {
                std::mem::swap(upper, lower);
            } else {
                self.pivots.push(pivot);
            }
        }
    }
}

impl<T: Key, const S: usize, const P: bool, const L: bool, const C: bool> Queue<T>
    for Scalar<T, S, P, L, C>
{
    #[inline(always)]
    fn push(&mut self, key: T) {
        let mut first = 0;
        if L {
            while first < self.pivots.len() {
                if C {
                    self.counters.push += 1;
                }
                if self.pivots[first] < key {
                    break;
                }
                first += 1;
            }
        } else {
            let mut count = self.pivots.len();
            while count > 0 {
                let half = count / 2;
                let middle = first + half;
                if C {
                    self.counters.push += 1;
                }
                if self.pivots[middle] < key {
                    count = half;
                } else {
                    first = middle + 1;
                    count -= half + 1;
                }
            }
        }
        self.buckets[first].push(key);
        self.size += 1;
    }
    #[inline(always)]
    fn pop(&mut self) -> T {
        self.pop_observed(|_| {})
    }
    fn len(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_keep_bucket_order_and_consume_next_sample() {
        let mut queue = Scalar::<u32, 1, false, true, true>::new(&[3, 0]);
        for key in [1, 1, 2, 3] {
            queue.push(key);
        }
        let mut events = Vec::new();
        assert_eq!(
            queue.pop_observed(|event| {
                events.push((
                    event.pivot_index,
                    event.retry,
                    event.lower.to_vec(),
                    event.upper.to_vec(),
                ));
            }),
            1
        );
        assert_eq!(events[0], (3, true, vec![1, 1, 2, 3], vec![]));
        assert_eq!(events[1], (0, false, vec![1], vec![1, 2, 3]));
        assert_eq!(queue.counters().pop, 8);
        assert_eq!(queue.counters().retries, 1);
        assert_eq!(queue.counters().samples, 2);
    }

    #[test]
    fn equals_route_to_lower_bucket() {
        for linear in [true, false] {
            macro_rules! check {
                ($linear:literal) => {{
                    let mut queue = Scalar::<u32, 1, false, $linear, true>::new(&[1]);
                    for key in [1, 2, 3] {
                        queue.push(key);
                    }
                    assert_eq!(queue.pop(), 1);
                    queue.reset_counters();
                    queue.push(2);
                    assert_eq!(queue.buckets().last().unwrap(), &[2, 2]);
                    assert_eq!(queue.counters().push, 1);
                    assert_eq!(queue.pop(), 2);
                    assert_eq!(queue.pop(), 2);
                    assert_eq!(queue.pop(), 3);
                }};
            }
            if linear {
                check!(true);
            } else {
                check!(false);
            }
        }
    }

    #[test]
    fn all_configs_sort_duplicate_inputs() {
        macro_rules! check {
            ($sample:literal, $perfect:literal, $linear:literal) => {{
                let mut queue = Scalar::<u32, $sample, $perfect, $linear, true>::default();
                for key in [5, 1, 5, 3, 1, 0, 5, 0] {
                    queue.push(key);
                }
                let output: Vec<_> = (0..8).map(|_| queue.pop()).collect();
                assert_eq!(output, [0, 0, 1, 1, 3, 5, 5, 5]);
                assert_eq!(queue.len(), 0);
            }};
        }
        check!(1, false, true);
        check!(3, false, true);
        check!(1, true, true);
        check!(1, false, false);
        check!(3, false, false);
        check!(1, true, false);
    }
}
