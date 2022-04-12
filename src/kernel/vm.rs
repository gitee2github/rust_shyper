use alloc::sync::Arc;
use alloc::vec::Vec;
use core::mem::size_of;

use spin::Mutex;

use crate::arch::{PAGE_SIZE, PTE_S2_FIELD_AP_RO, PTE_S2_NORMAL, PTE_S2_RO};
use crate::arch::{GICC_CTLR_EN_BIT, GICC_CTLR_EOIMODENS_BIT};
use crate::arch::PageTable;
use crate::arch::Vgic;
use crate::config::VmConfigEntry;
use crate::device::EmuDevs;
use crate::kernel::{get_share_mem, mem_page_alloc, VM_CONTEXT_RECEIVE, VM_CONTEXT_SEND};
use crate::lib::*;
use crate::mm::PageFrame;

use super::vcpu::Vcpu;

pub const DIRTY_MEM_THRESHOLD: usize = 0x200;
pub const VM_NUM_MAX: usize = 8;
pub static VM_IF_LIST: [Mutex<VmInterface>; VM_NUM_MAX] = [
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
    Mutex::new(VmInterface::default()),
];

pub fn vm_if_set_state(vm_id: usize, vm_state: VmState) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.state = vm_state;
}

pub fn vm_if_get_state(vm_id: usize) -> VmState {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.state
}

pub fn vm_if_set_type(vm_id: usize, vm_type: VmType) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.vm_type = vm_type;
}

pub fn vm_if_get_type(vm_id: usize) -> VmType {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.vm_type
}

pub fn vm_if_set_cpu_id(vm_id: usize, master_cpu_id: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.master_cpu_id = master_cpu_id;
    println!(
        "vm_if_list_set_cpu_id vm [{}] set master_cpu_id {}",
        vm_id, master_cpu_id
    );
}

pub fn vm_if_get_cpu_id(vm_id: usize) -> usize {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.master_cpu_id
}

pub fn vm_if_cmp_mac(vm_id: usize, frame: &[u8]) -> bool {
    let vm_if = VM_IF_LIST[vm_id].lock();
    for i in 0..6 {
        if vm_if.mac[i] != frame[i] {
            return false;
        }
    }
    true
}

pub fn vm_if_set_ivc_arg(vm_id: usize, ivc_arg: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.ivc_arg = ivc_arg;
}

pub fn vm_if_ivc_arg(vm_id: usize) -> usize {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.ivc_arg
}

pub fn vm_if_set_ivc_arg_ptr(vm_id: usize, ivc_arg_ptr: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.ivc_arg_ptr = ivc_arg_ptr;
}

pub fn vm_if_ivc_arg_ptr(vm_id: usize) -> usize {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.ivc_arg_ptr
}
// new if for vm migration
pub fn vm_if_init_mem_map(vm_id: usize, len: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map = Some(FlexBitmap::new(len));
}

pub fn vm_if_set_mem_map_cache(vm_id: usize, pf: PageFrame) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map_cache = Some(pf);
}

pub fn vm_if_mem_map_cache(vm_id: usize) -> Option<PageFrame> {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map_cache.clone()
}

pub fn vm_if_dirty_mem_map(vm_id: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map.as_mut().unwrap().init_dirty();
}

pub fn vm_if_set_mem_map(vm_id: usize, bit: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map.as_mut().unwrap().set(bit, true);
}

pub fn vm_if_clear_mem_map(vm_id: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map.as_mut().unwrap().clear();
}

pub fn vm_if_copy_mem_map(vm_id: usize) {
    let mut vm_if = VM_IF_LIST[vm_id].lock();
    let map = vm_if.mem_map.as_ref().unwrap();

    memcpy_safe(
        vm_if.mem_map_cache.as_ref().unwrap().pa() as *const u8,
        map.slice() as *const _ as *const u8,
        size_of::<u64>() * map.vec_len(),
    );
    // clear bitmap after copy
    vm_if.mem_map.as_mut().unwrap().clear();
}

pub fn vm_if_mem_map_page_num(vm_id: usize) -> usize {
    let vm_if = VM_IF_LIST[vm_id].lock();
    let map = vm_if.mem_map.as_ref().unwrap();
    8 * map.vec_len() / PAGE_SIZE
}

pub fn vm_if_mem_map_dirty_sum(vm_id: usize) -> usize {
    let vm_if = VM_IF_LIST[vm_id].lock();
    vm_if.mem_map.as_ref().unwrap().sum()
}
// End vm interface func implementation

#[derive(Clone, Copy)]
pub enum VmState {
    VmInv = 0,
    VmPending = 1,
    VmActive = 2,
}

#[derive(Clone, Copy, PartialEq)]
pub enum VmType {
    VmTOs = 0,
    VmTBma = 1,
}

impl VmType {
    pub fn from_usize(value: usize) -> VmType {
        match value {
            0 => VmType::VmTOs,
            1 => VmType::VmTBma,
            _ => panic!("Unknown VmType value: {}", value),
        }
    }
}

pub struct VmInterface {
    pub master_cpu_id: usize,
    pub state: VmState,
    pub vm_type: VmType,
    pub mac: [u8; 6],
    pub ivc_arg: usize,
    pub ivc_arg_ptr: usize,
    pub mem_map: Option<FlexBitmap>,
    pub mem_map_cache: Option<PageFrame>,
}

impl VmInterface {
    const fn default() -> VmInterface {
        VmInterface {
            master_cpu_id: 0,
            state: VmState::VmPending,
            vm_type: VmType::VmTBma,
            mac: [0; 6],
            ivc_arg: 0,
            ivc_arg_ptr: 0,
            mem_map: None,
            mem_map_cache: None,
        }
    }
}

#[derive(Clone)]
pub struct VmPa {
    pub pa_start: usize,
    pub pa_length: usize,
    pub offset: isize,
}

impl VmPa {
    pub fn default() -> VmPa {
        VmPa {
            pa_start: 0,
            pa_length: 0,
            offset: 0,
        }
    }
}

// #[repr(align(4096))]
#[derive(Clone)]
pub struct Vm {
    pub inner: Arc<Mutex<VmInner>>,
}

impl Vm {
    pub fn inner(&self) -> Arc<Mutex<VmInner>> {
        self.inner.clone()
    }

    pub fn default() -> Vm {
        Vm {
            inner: Arc::new(Mutex::new(VmInner::default())),
        }
    }

    pub fn new(id: usize) -> Vm {
        Vm {
            inner: Arc::new(Mutex::new(VmInner::new(id))),
        }
    }

    pub fn init_intc_mode(&self, emu: bool) {
        let vm_inner = self.inner.lock();
        for vcpu in &vm_inner.vcpu_list {
            println!(
                "vm {} vcpu {} set {} hcr",
                vm_inner.id,
                vcpu.id(),
                if emu { "emu" } else { "partial passthrough" }
            );
            if !emu {
                vcpu.set_gich_ctlr((GICC_CTLR_EN_BIT) as u32);
                vcpu.set_hcr(0x80080001); // HCR_EL2_GIC_PASSTHROUGH_VAL
            } else {
                vcpu.set_gich_ctlr((GICC_CTLR_EN_BIT | GICC_CTLR_EOIMODENS_BIT) as u32);
                vcpu.set_hcr(0x80080019);
            }
        }
    }

    pub fn med_blk_id(&self) -> usize {
        let vm_inner = self.inner.lock();
        match vm_inner.config.as_ref().unwrap().med_blk_idx {
            None => {
                panic!("vm {} do not have mediated blk", vm_inner.id);
            }
            Some(idx) => idx,
        }
    }

    pub fn dtb(&self) -> Option<*mut fdt::myctypes::c_void> {
        let vm_inner = self.inner.lock();
        vm_inner.dtb.map(|x| x as *mut fdt::myctypes::c_void)
    }

    pub fn set_dtb(&self, val: *mut fdt::myctypes::c_void) {
        let mut vm_inner = self.inner.lock();
        vm_inner.dtb = Some(val as usize);
    }

    pub fn vcpu(&self, index: usize) -> Option<Vcpu> {
        let vm_inner = self.inner.lock();
        if vm_inner.vcpu_list.len() > index {
            Some(vm_inner.vcpu_list[index].clone())
        } else {
            None
        }
    }

    pub fn push_vcpu(&self, vcpu: Vcpu) {
        let mut vm_inner = self.inner.lock();
        vm_inner.vcpu_list.push(vcpu);
    }

    pub fn set_has_master_cpu(&self, has_master: bool) {
        let mut vm_inner = self.inner.lock();
        vm_inner.has_master = has_master;
    }

    pub fn has_master_cpu(&self) -> bool {
        let vm_inner = self.inner.lock();
        vm_inner.has_master
    }

    pub fn set_ncpu(&self, ncpu: usize) {
        let mut vm_inner = self.inner.lock();
        vm_inner.ncpu = ncpu;
    }

    pub fn set_cpu_num(&self, cpu_num: usize) {
        let mut vm_inner = self.inner.lock();
        vm_inner.cpu_num = cpu_num;
    }

    pub fn set_entry_point(&self, entry_point: usize) {
        let mut vm_inner = self.inner.lock();
        vm_inner.entry_point = entry_point;
    }

    pub fn set_emu_devs(&self, idx: usize, emu: EmuDevs) {
        let mut vm_inner = self.inner.lock();
        if idx < vm_inner.emu_devs.len() {
            if let EmuDevs::None = vm_inner.emu_devs[idx] {
                println!("set_emu_devs: cover a None emu dev");
                vm_inner.emu_devs[idx] = emu;
                return;
            } else {
                panic!("set_emu_devs: set an exsit emu dev");
            }
        }
        while idx > vm_inner.emu_devs.len() {
            println!("set_emu_devs: push a None emu dev");
            vm_inner.emu_devs.push(EmuDevs::None);
        }
        vm_inner.emu_devs.push(emu);
    }

    pub fn set_intc_dev_id(&self, intc_dev_id: usize) {
        let mut vm_inner = self.inner.lock();
        vm_inner.intc_dev_id = intc_dev_id;
    }

    pub fn set_int_bit_map(&self, int_id: usize) {
        let mut vm_inner = self.inner.lock();
        vm_inner.int_bitmap.as_mut().unwrap().set(int_id);
    }

    pub fn set_config_entry(&self, config: Option<VmConfigEntry>) {
        let mut vm_inner = self.inner.lock();
        vm_inner.config = config;
    }

    pub fn pt_map_range(&self, ipa: usize, len: usize, pa: usize, pte: usize) {
        let vm_inner = self.inner.lock();
        match &vm_inner.pt {
            Some(pt) => pt.pt_map_range(ipa, len, pa, pte),
            None => {
                panic!("Vm::pt_map_range: vm{} pt is empty", vm_inner.id);
            }
        }
    }

    // ap: access permission
    pub fn pt_set_access_permission(&self, pa: usize, ap: usize) {
        let vm_inner = self.inner.lock();
        match &vm_inner.pt {
            Some(pt) => {
                for i in 0..vm_inner.mem_region_num {
                    let start = vm_inner.pa_region[i].pa_start;
                    let end = start + vm_inner.pa_region[i].pa_length;
                    if start >= pa && pa < end {
                        let ipa_start = pa + vm_inner.pa_region[i].offset as usize;
                        pt.access_permission(ipa_start, PAGE_SIZE, ap);
                    }
                }
            }
            None => {
                panic!("pt_set_access_permission: vm{} pt is empty", vm_inner.id);
            }
        }
    }

    pub fn pt_read_only(&self) {
        let vm_inner = self.inner.lock();
        match &vm_inner.pt {
            Some(pt) => {
                for i in 0..vm_inner.mem_region_num {
                    let ipa_start = vm_inner.pa_region[i].pa_start + vm_inner.pa_region[i].offset as usize;
                    let len = vm_inner.pa_region[i].pa_length;
                    pt.access_permission(ipa_start, len, PTE_S2_FIELD_AP_RO);
                }
            }
            None => {
                panic!("Vm::read_only: vm{} pt is empty", vm_inner.id);
            }
        }
    }

    pub fn set_pt(&self, pt_dir_frame: PageFrame) {
        let mut vm_inner = self.inner.lock();
        vm_inner.pt = Some(PageTable::new(pt_dir_frame))
    }

    pub fn pt_dir(&self) -> usize {
        let vm_inner = self.inner.lock();
        match &vm_inner.pt {
            Some(pt) => return pt.base_pa(),
            None => {
                panic!("Vm::pt_dir: vm{} pt is empty", vm_inner.id);
            }
        }
    }

    pub fn cpu_num(&self) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.cpu_num
    }

    pub fn id(&self) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.id
    }

    pub fn config(&self) -> VmConfigEntry {
        let vm_inner = self.inner.lock();
        vm_inner.config.as_ref().unwrap().clone()
    }

    pub fn add_region(&self, region: VmPa) {
        let mut vm_inner = self.inner.lock();
        vm_inner.pa_region.push(region);
    }

    pub fn region_num(&self) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.pa_region.len()
    }

    pub fn pa_start(&self, idx: usize) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.pa_region[idx].pa_start
    }

    pub fn pa_length(&self, idx: usize) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.pa_region[idx].pa_length
    }

    pub fn pa_offset(&self, idx: usize) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.pa_region[idx].offset as usize
    }

    pub fn set_mem_region_num(&self, mem_region_num: usize) {
        let mut vm_inner = self.inner.lock();
        vm_inner.mem_region_num = mem_region_num;
    }

    pub fn mem_region_num(&self) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.mem_region_num
    }

    pub fn vgic(&self) -> Arc<Vgic> {
        let vm_inner = self.inner.lock();
        match &vm_inner.emu_devs[vm_inner.intc_dev_id] {
            EmuDevs::Vgic(vgic) => {
                return vgic.clone();
            }
            _ => {
                panic!("vm{} cannot find vgic", vm_inner.id);
            }
        }
    }

    pub fn has_vgic(&self) -> bool {
        let vm_inner = self.inner.lock();
        if vm_inner.intc_dev_id >= vm_inner.emu_devs.len() {
            return false;
        }
        match &vm_inner.emu_devs[vm_inner.intc_dev_id] {
            EmuDevs::Vgic(_) => true,
            _ => false,
        }
    }

    pub fn emu_dev(&self, dev_id: usize) -> EmuDevs {
        let vm_inner = self.inner.lock();
        vm_inner.emu_devs[dev_id].clone()
    }

    pub fn emu_net_dev(&self, id: usize) -> EmuDevs {
        let vm_inner = self.inner.lock();
        let mut dev_num = 0;

        for i in 0..vm_inner.emu_devs.len() {
            match vm_inner.emu_devs[i] {
                EmuDevs::VirtioNet(_) => {
                    if dev_num == id {
                        return vm_inner.emu_devs[i].clone();
                    }
                    dev_num += 1;
                }
                _ => {}
            }
        }
        return EmuDevs::None;
    }

    pub fn emu_blk_dev(&self, id: usize) -> EmuDevs {
        let vm_inner = self.inner.lock();
        let mut dev_num = 0;

        for i in 0..vm_inner.emu_devs.len() {
            match vm_inner.emu_devs[i] {
                EmuDevs::VirtioBlk(_) => {
                    if dev_num == id {
                        return vm_inner.emu_devs[i].clone();
                    }
                    dev_num += 1;
                }
                _ => {}
            }
        }
        return EmuDevs::None;
    }

    // Get console dev by ipa.
    pub fn emu_console_dev(&self, ipa: u64) -> EmuDevs {
        let mut emu_dev_id = -1;
        for idx in 0..self.config().emulated_device_list().len() {
            if self.config().emulated_device_list()[idx].base_ipa == ipa as usize {
                emu_dev_id = idx as i32;
            }
        }
        if emu_dev_id > 0 {
            let vm_inner = self.inner.lock();
            return vm_inner.emu_devs[emu_dev_id as usize].clone();
        }
        return EmuDevs::None;
    }

    pub fn ncpu(&self) -> usize {
        let vm_inner = self.inner.lock();
        vm_inner.ncpu
    }

    pub fn has_interrupt(&self, int_id: usize) -> bool {
        let mut vm_inner = self.inner.lock();
        vm_inner.int_bitmap.as_mut().unwrap().get(int_id) != 0
    }

    pub fn emu_has_interrupt(&self, int_id: usize) -> bool {
        for emu_dev in self.config().emulated_device_list() {
            if int_id == emu_dev.irq_id {
                return true;
            }
        }
        false
    }

    pub fn vcpuid_to_pcpuid(&self, vcpuid: usize) -> Result<usize, ()> {
        // println!("vcpuid_to_pcpuid");
        let vm_inner = self.inner.lock();
        if vcpuid < vm_inner.cpu_num {
            let vcpu = vm_inner.vcpu_list[vcpuid].clone();
            drop(vm_inner);
            return Ok(vcpu.phys_id());
        } else {
            return Err(());
        }
    }

    pub fn pcpuid_to_vcpuid(&self, pcpuid: usize) -> Result<usize, ()> {
        let vm_inner = self.inner.lock();
        for vcpuid in 0..vm_inner.cpu_num {
            if vm_inner.vcpu_list[vcpuid].phys_id() == pcpuid {
                return Ok(vcpuid);
            }
        }
        return Err(());
    }

    pub fn vcpu_to_pcpu_mask(&self, mask: usize, len: usize) -> usize {
        let mut pmask = 0;
        for i in 0..len {
            let shift = self.vcpuid_to_pcpuid(i);
            if mask & (1 << i) != 0 && !shift.is_err() {
                pmask |= 1 << shift.unwrap();
            }
        }
        return pmask;
    }

    pub fn pcpu_to_vcpu_mask(&self, mask: usize, len: usize) -> usize {
        let mut pmask = 0;
        for i in 0..len {
            let shift = self.pcpuid_to_vcpuid(i);
            if mask & (1 << i) != 0 && !shift.is_err() {
                pmask |= 1 << shift.unwrap();
            }
        }
        return pmask;
    }

    pub fn show_pagetable(&self, ipa: usize) {
        let vm_inner = self.inner.lock();
        vm_inner.pt.as_ref().unwrap().show_pt(ipa);
    }

    pub fn ready(&self) -> bool {
        let vm_inner = self.inner.lock();
        vm_inner.ready
    }

    pub fn set_ready(&self, _ready: bool) {
        let mut vm_inner = self.inner.lock();
        vm_inner.ready = _ready;
    }

    // init for migrate restore
    pub fn context_vm_migrate_init(&self) {
        let mvm = vm(0).unwrap();
        // for i in 0..self.ncpu() {
        match mem_page_alloc() {
            Ok(pf) => {
                mvm.pt_map_range(get_share_mem(VM_CONTEXT_RECEIVE), PAGE_SIZE, pf.pa(), PTE_S2_NORMAL);
                let mut inner = self.inner.lock();
                inner.migrate_restore_pf.push(pf);
            }
            Err(_) => {
                panic!("context_vm_migrate_restore_init: mem_pages_alloc for vm context failed");
            }
        }
        // }
    }

    pub fn context_vm_migrate_save(&self) {
        // TODO: 仅支持单核VM，支持多核并不困难，遍历所有vcpu即可
        let vcpu = self.vcpu(0).unwrap();
        let mvm = vm(0).unwrap();
        // println!(
        //     "size of vm ctx {:x}, size of vcpu ctx {:x}",
        //     size_of::<VmContext>(),
        //     size_of::<Aarch64ContextFrame>()
        // );
        match mem_page_alloc() {
            Ok(pf) => {
                vcpu.migrate_vm_ctx_save(pf.pa());
                vcpu.migrate_vcpu_ctx_save(pf.pa() + PAGE_SIZE / 2);
                let base = get_share_mem(VM_CONTEXT_SEND);
                mvm.pt_map_range(base, PAGE_SIZE, pf.pa(), PTE_S2_RO);
                let mut inner = self.inner.lock();
                inner.migrate_save_pf.push(pf);
            }
            Err(_) => {
                panic!("context_vm_migrate_save: mem_pages_alloc for vm context failed");
            }
        }
    }

    pub fn context_vm_migrate_restore(&self) {
        let vcpu = self.vcpu(0).unwrap();
        let inner = self.inner.lock();
        let pa = inner.migrate_restore_pf[0].pa();
        drop(inner);
        vcpu.migrate_vm_ctx_restore(pa);
        vcpu.migrate_vcpu_ctx_restore(pa + PAGE_SIZE / 2);
    }

    pub fn share_mem_base(&self) -> usize {
        let inner = self.inner.lock();
        inner.share_mem_base
    }

    pub fn add_share_mem_base(&self, len: usize) {
        let mut inner = self.inner.lock();
        inner.share_mem_base += len;
    }
}

#[repr(align(4096))]
pub struct VmInner {
    pub id: usize,
    pub ready: bool,
    pub config: Option<VmConfigEntry>,
    pub dtb: Option<usize>,
    // memory config
    pub pt: Option<PageTable>,
    pub mem_region_num: usize,
    pub pa_region: Vec<VmPa>, // Option<[VmPa; VM_MEM_REGION_MAX]>,

    // image config
    pub entry_point: usize,

    // vcpu config
    pub has_master: bool,
    pub vcpu_list: Vec<Vcpu>,
    pub cpu_num: usize,
    pub ncpu: usize,

    // interrupt
    pub intc_dev_id: usize,
    pub int_bitmap: Option<BitMap<BitAlloc256>>,

    // migration
    pub share_mem_base: usize,
    pub migrate_save_pf: Vec<PageFrame>,
    pub migrate_restore_pf: Vec<PageFrame>,

    // emul devs
    pub emu_devs: Vec<EmuDevs>,
}

impl VmInner {
    pub const fn default() -> VmInner {
        VmInner {
            id: 0,
            ready: false,
            config: None,
            dtb: None,
            pt: None,
            mem_region_num: 0,
            pa_region: Vec::new(),
            entry_point: 0,

            has_master: false,
            vcpu_list: Vec::new(),
            cpu_num: 0,
            ncpu: 0,

            intc_dev_id: 0,
            int_bitmap: Some(BitAlloc4K::default()),
            share_mem_base: 0xd00000000, // hard code
            migrate_save_pf: vec![],
            migrate_restore_pf: vec![],
            emu_devs: Vec::new(),
        }
    }

    pub fn new(id: usize) -> VmInner {
        VmInner {
            id,
            ready: false,
            config: None,
            dtb: None,
            pt: None,
            mem_region_num: 0,
            pa_region: Vec::new(),
            entry_point: 0,

            has_master: false,
            vcpu_list: Vec::new(),
            cpu_num: 0,
            ncpu: 0,

            intc_dev_id: 0,
            int_bitmap: Some(BitAlloc4K::default()),
            share_mem_base: 0xd00000000, // hard code
            migrate_save_pf: vec![],
            migrate_restore_pf: vec![],
            emu_devs: Vec::new(),
        }
    }
}

// static VM_LIST: Mutex<[Vm; VM_NUM_MAX]> = Mutex::new([Vm::default(); VM_NUM_MAX]);
// lazy_static! {
//     pub static ref VM_LIST: Mutex<[Vm; VM_NUM_MAX]> = Mutex::new([Vm::default(); VM_NUM_MAX]);
// }
// pub static VM_LIST: Mutex<[Vm; VM_NUM_MAX]> = Mutex::new([
//     Vm::default(),
//     Vm::default(),
//     Vm::default(),
//     Vm::default(),
//     Vm::default(),
//     Vm::default(),
//     Vm::default(),
//     Vm::default(),
// ]);
pub static VM_LIST: Mutex<Vec<Vm>> = Mutex::new(Vec::new());

pub fn push_vm(id: usize) -> Result<(), ()> {
    let mut vm_list = VM_LIST.lock();
    if id < vm_list.len() {
        println!("push_vm: vm {} already exists", id);
        return Err(());
    }
    let vm = Vm::new(id);
    vm_list.push(vm);
    Ok(())
}

pub fn vm(id: usize) -> Option<Vm> {
    let vm_list = VM_LIST.lock();
    if vm_list.get(id).is_none() {
        return None;
    } else {
        return Some(vm_list[id].clone());
    }
}

pub fn vm_list_size() -> usize {
    let vm_list = VM_LIST.lock();
    vm_list.len()
}

pub fn vm_ipa2pa(vm: Vm, ipa: usize) -> usize {
    if ipa == 0 {
        return 0;
    }

    for i in 0..vm.mem_region_num() {
        if in_range(
            (ipa as isize - vm.pa_offset(i) as isize) as usize,
            vm.pa_start(i),
            vm.pa_length(i),
        ) {
            return (ipa as isize - vm.pa_offset(i) as isize) as usize;
        }
    }

    println!("vm_ipa2pa: VM {} access invalid ipa {:x}", vm.id(), ipa);
    return 0;
}
