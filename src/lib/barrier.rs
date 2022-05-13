use core::arch::global_asm;

use volatile::Volatile;

use crate::board::PLAT_DESC;
use crate::lib::round_up;

global_asm!(include_str!("../arch/aarch64/barrier.S"));

#[repr(C)]
struct CpuSyncToken {
    lock: u32,
    n: usize,
    count: usize,
    ready: bool,
}

static mut CPU_GLB_SYNC: CpuSyncToken = CpuSyncToken {
    lock: 0,
    n: PLAT_DESC.cpu_desc.num,
    count: 0,
    ready: true,
};

extern "C" {
    pub fn spin_lock(lock: usize);
    pub fn spin_unlock(lock: usize);
}

#[inline(never)]
pub fn barrier() {
    unsafe {
        let lock_addr = &CPU_GLB_SYNC.lock as *const _ as usize;
        spin_lock(lock_addr);
        let mut count = Volatile::new(&mut CPU_GLB_SYNC.count);
        count.update(|count| *count += 1);
        let next_count = round_up(count.read(), CPU_GLB_SYNC.n);
        spin_unlock(lock_addr);
        while count.read() < next_count {}
    }
}

#[inline(never)]
pub fn add_barrier_count() {
    unsafe {
        let lock_addr = &CPU_GLB_SYNC.lock as *const _ as usize;
        spin_lock(lock_addr);
        let mut count = Volatile::new(&mut CPU_GLB_SYNC.count);
        count.update(|count| *count += 1);
        spin_unlock(lock_addr);
    }
}
