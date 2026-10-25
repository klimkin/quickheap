pub struct MedianOfM<const M: usize>;
impl<const M: usize> MedianOfM<M> {
    #[inline(always)]
    fn random(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = *state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    #[inline(always)]
    fn sample<T: Copy>(layer: &[T], state: &mut u64) -> (T, usize) {
        let pos = ((Self::random(state) as u128 * layer.len() as u128) >> 64) as usize;
        (layer[pos], pos)
    }

    #[inline(always)]
    pub fn pick<T: Copy + Ord>(layer: &[T], state: &mut u64) -> (T, usize) {
        if M == 3 {
            let (a, b, c) = (Self::sample(layer, state), Self::sample(layer, state), Self::sample(layer, state));
            return a.min(b).max(a.max(b).min(c));
        }
        let k = M / 2;
        let mut pivots: [(T, usize); M] = std::array::from_fn(|_| Self::sample(layer, state));
        pivots.select_nth_unstable(k);
        pivots[k]
    }
}
