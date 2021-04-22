use super::GICD;
use super::GIC_SGIS_NUM;

pub const INTERRUPT_IRQ_HYPERVISOR_TIMER: usize = 26;
pub const INTERRUPT_IRQ_IPI: usize = 1;
use crate::board::platform_cpuid_to_cpuif;

pub fn interrupt_arch_init() {
    use crate::arch::{gic_cpu_init, gic_glb_init, gic_maintenance_handler};
    use crate::kernel::{interrupt_reserve_int, InterruptHandler};

    crate::lib::barrier();

    if crate::kernel::cpu_id() == 0 {
        gic_glb_init();
    }

    gic_cpu_init();

    use crate::board::PLAT_DESC;

    let int_id = PLAT_DESC.arch_desc.gic_desc.maintenance_int_id;
    interrupt_reserve_int(
        int_id,
        InterruptHandler::GicMaintenanceHandler(gic_maintenance_handler),
    );
    interrupt_arch_enable(int_id, true);
}

pub fn interrupt_arch_enable(int_id: usize, en: bool) {
    let cpu_id = crate::kernel::cpu_id();
    GICD.set_enable(int_id, en);
    GICD.set_prio(int_id, 0x7f);
    GICD.set_trgt(int_id, 1 << platform_cpuid_to_cpuif(cpu_id));
}

pub fn interrupt_arch_ipi_send(cpu_id: usize, ipi_id: usize) {
    if ipi_id< GIC_SGIS_NUM {
        GICD.send_sgi(platform_cpuid_to_cpuif(cpu_id), ipi_id);
    }
}