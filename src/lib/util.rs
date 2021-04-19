#[inline(always)]
pub fn round_up(value: usize, to: usize) -> usize {
    ((value + to - 1) / to) * to
}

#[inline(always)]
pub fn round_down(value: usize, to: usize) -> usize {
    value & !(to - 1)
}

#[inline(always)]
pub fn range_in_range(base1: usize, size1: usize, base2: usize, size2: usize) -> bool {
    (base1 >= base2) && ((base1 + size1) <= (base2 + size2))
}

#[inline(always)]
pub fn in_range(addr: usize, base: usize, size: usize) {
    range_in_range(addr, 0, base, size);
}
