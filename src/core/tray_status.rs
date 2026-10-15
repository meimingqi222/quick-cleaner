//! 托盘轻量采样；不枚举进程、端口、应用或用户目录。
use sysinfo::System;

#[derive(Clone, Debug, Default)]
pub struct TraySnapshot {
    pub cpu_pct: u32,
    pub memory_used: u64,
    pub memory_total: u64,
    pub swap_used: u64,
    pub temp_c: Option<f32>,
}

pub struct TraySampler {
    system: System,
    temp_c: Option<f32>,
}

impl Default for TraySampler {
    fn default() -> Self {
        let mut system = System::new();
        system.refresh_cpu_usage();
        Self {
            system,
            temp_c: None,
        }
    }
}

impl TraySampler {
    /// CPU 百分比需要先建立基线，调用方应至少等待 MINIMUM_CPU_UPDATE_INTERVAL。
    pub fn sample(&mut self, refresh_thermal: bool) -> TraySnapshot {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        if refresh_thermal {
            self.temp_c = crate::platform::read_thermal().cpu_temp;
        }
        TraySnapshot {
            cpu_pct: self.system.global_cpu_usage().round() as u32,
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            swap_used: self.system.used_swap(),
            temp_c: self.temp_c,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_sampler_reads_memory_without_building_a_process_table() {
        let start = std::time::Instant::now();
        let mut sampler = TraySampler::default();
        let snapshot = sampler.sample(true);
        let cold = start.elapsed();
        let start = std::time::Instant::now();
        let second = sampler.sample(false);
        eprintln!(
            "tray cold sample: {cold:?}; warm sample: {:?}",
            start.elapsed()
        );
        assert!(snapshot.memory_total > 0);
        assert!(second.memory_used <= second.memory_total);
        assert!(
            sampler.system.processes().is_empty(),
            "popover must not enumerate processes"
        );
    }
}
