use quickheap::simd::{SimdElem, push_position};

/// `pool` holds both extrema and a narrow band of values, so sorted draws
/// produce duplicates and equal-to-pivot probes at every lane boundary.
fn check<T, S>(pool: &[T])
where
    T: Copy + Ord + std::fmt::Debug,
    S: SimdElem<T>,
{
    let mut rng = fastrand::Rng::with_seed(0x514d44);
    for len in 0..=2 * 64 + S::L {
        for _ in 0..8 {
            let mut pivots: Vec<T> = (0..len).map(|_| pool[rng.usize(..pool.len())]).collect();
            pivots.sort_unstable_by(|a, b| b.cmp(a));
            // Exact-size allocation: any load past `len` leaves the allocation.
            let pivots = pivots.into_boxed_slice();
            for &t in pool {
                assert_eq!(
                    push_position::<T, S>(&pivots, t),
                    pivots.partition_point(|&p| t < p),
                    "len={len} t={t:?} pivots={pivots:?}"
                );
            }
        }
    }
}

macro_rules! widths {
    ($backend:ty) => {
        #[test]
        fn i32() {
            check::<i32, $backend>(&[i32::MIN, i32::MIN + 1, -2, -1, 0, 1, 2, i32::MAX - 1, i32::MAX]);
        }
        #[test]
        fn u32() {
            check::<u32, $backend>(&[0, 1, 2, 3, 1 << 31, u32::MAX - 1, u32::MAX]);
        }
        #[test]
        fn i64() {
            check::<i64, $backend>(&[i64::MIN, i64::MIN + 1, -2, -1, 0, 1, 2, i64::MAX - 1, i64::MAX]);
        }
        #[test]
        fn u64() {
            check::<u64, $backend>(&[0, 1, 2, 3, 1 << 63, u64::MAX - 1, u64::MAX]);
        }
    };
}

#[cfg(target_feature = "avx2")]
mod avx2 {
    use super::*;
    widths!(quickheap::Avx2);
}

#[cfg(target_feature = "avx512f")]
mod avx512 {
    use super::*;
    widths!(quickheap::Avx512);
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::*;
    widths!(quickheap::Neon);
}
