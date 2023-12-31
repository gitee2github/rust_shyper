// Copyright (c) 2023 Beihang University, Huawei Technologies Co.,Ltd. All rights reserved.
// Rust-Shyper is licensed under Mulan PSL v2.
// You can use this software according to the terms and conditions of the Mulan PSL v2.
// You may obtain a copy of Mulan PSL v2 at:
//          http://license.coscl.org.cn/MulanPSL2
// THIS SOFTWARE IS PROVIDED ON AN "AS IS" BASIS, WITHOUT WARRANTIES OF ANY KIND,
// EITHER EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO NON-INFRINGEMENT,
// MERCHANTABILITY OR FIT FOR A PARTICULAR PURPOSE.
// See the Mulan PSL v2 for more details.

use spin::Mutex;
use tock_registers::interfaces::*;

const CTL_IMASK: usize = 1 << 1;

pub static TIMER_FREQ: Mutex<usize> = Mutex::new(0);
pub static TIMER_SLICE: Mutex<usize> = Mutex::new(0); // ms

pub fn timer_arch_set(num: usize) {
    let slice_lock = TIMER_SLICE.lock();
    let val = *slice_lock * num;
    drop(slice_lock);
    msr!(CNTHP_TVAL_EL2, val);
}

pub fn timer_arch_enable_irq() {
    let val = 1;
    msr!(CNTHP_CTL_EL2, val, "x");
}

pub fn timer_arch_disable_irq() {
    let val = 2;
    msr!(CNTHP_CTL_EL2, val, "x");
}

pub fn timer_arch_get_counter() -> usize {
    cortex_a::registers::CNTPCT_EL0.get() as usize
}

pub fn timer_arch_get_frequency() -> usize {
    cortex_a::registers::CNTFRQ_EL0.get() as usize
}

pub fn timer_arch_init() {
    let mut freq_lock = TIMER_FREQ.lock();
    let mut slice_lock = TIMER_SLICE.lock();
    *freq_lock = timer_arch_get_frequency();
    *slice_lock = (*freq_lock) / 1000; // ms

    let ctl = 0x3 & (1 | !CTL_IMASK);
    let tval = *slice_lock * 10;
    msr!(CNTHP_CTL_EL2, ctl);
    msr!(CNTHP_TVAL_EL2, tval);
}
