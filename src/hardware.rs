use std::{collections::VecDeque, time::Duration};

use sysinfo::{Components, Disks, Networks, System};

pub const HISTORY_LEN: usize = 96;
pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub cpu_usage: f32,
    pub cpu_cores: Vec<f32>,
    pub cpu_history: VecDeque<f32>,
    pub memory_total: u64,
    pub memory_used: u64,
    pub memory_history: VecDeque<f32>,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskInfo>,
    pub interfaces: Vec<InterfaceInfo>,
    pub rx_history: VecDeque<f32>,
    pub tx_history: VecDeque<f32>,
    pub sensors: Vec<SensorInfo>,
    pub uptime: u64,
    pub load: [f64; 3],
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub hostname: String,
    pub os: String,
    pub kernel: String,
}

#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Clone, Debug)]
pub struct InterfaceInfo {
    pub name: String,
    pub rx_rate: f32,
    pub tx_rate: f32,
    pub total_received: u64,
    pub total_transmitted: u64,
}

#[derive(Clone, Debug)]
pub struct SensorInfo {
    pub label: String,
    pub temperature: f32,
    pub max: f32,
    pub critical: Option<f32>,
}

pub struct HardwareMonitor {
    system: System,
    components: Components,
    disks: Disks,
    networks: Networks,
    last_sample: Option<std::time::Instant>,
    pub snapshot: Snapshot,
}

impl HardwareMonitor {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            components: Components::new(),
            disks: Disks::new(),
            networks: Networks::new(),
            last_sample: None,
            snapshot: Snapshot::default(),
        }
    }

    pub fn due(&self) -> bool {
        self.last_sample
            .is_none_or(|last| last.elapsed() >= SAMPLE_INTERVAL)
    }

    pub fn sample(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = self
            .last_sample
            .map(|last| now.duration_since(last).as_secs_f32())
            .unwrap_or(SAMPLE_INTERVAL.as_secs_f32())
            .max(0.001);

        self.last_sample = Some(now);
        self.system.refresh_cpu_all();
        self.system.refresh_memory();
        self.components.refresh();
        self.disks.refresh();
        self.networks.refresh();

        let snapshot = &mut self.snapshot;
        snapshot.cpu_usage = self.system.global_cpu_usage();
        snapshot.cpu_cores = self
            .system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage())
            .collect();
        snapshot.memory_total = self.system.total_memory();
        snapshot.memory_used = self.system.used_memory();
        snapshot.swap_total = self.system.total_swap();
        snapshot.swap_used = self.system.used_swap();
        snapshot.uptime = System::uptime();
        let load = System::load_average();
        snapshot.load = [load.one, load.five, load.fifteen];
        snapshot.physical_cores = self.system.physical_core_count();
        snapshot.logical_cores = self.system.cpus().len();
        snapshot.hostname = System::host_name().unwrap_or_else(|| "—".into());
        snapshot.os = System::long_os_version().unwrap_or_else(|| "—".into());
        snapshot.kernel = System::kernel_version().unwrap_or_else(|| "—".into());

        let memory_fraction = fraction(snapshot.memory_used, snapshot.memory_total);
        push(&mut snapshot.cpu_history, snapshot.cpu_usage);
        push(&mut snapshot.memory_history, memory_fraction * 100.0);

        snapshot.disks = self
            .disks
            .list()
            .iter()
            .filter(|disk| disk.total_space() > 0)
            .map(|disk| DiskInfo {
                name: disk.name().to_string_lossy().into_owned(),
                mount: disk.mount_point().to_string_lossy().into_owned(),
                total: disk.total_space(),
                available: disk.available_space(),
            })
            .collect();
        snapshot
            .disks
            .sort_by_key(|disk| std::cmp::Reverse(disk.total));

        let mut rx_total = 0.0;
        let mut tx_total = 0.0;
        snapshot.interfaces = self
            .networks
            .list()
            .iter()
            .filter(|(_, data)| data.total_received() > 0 || data.total_transmitted() > 0)
            .map(|(name, data)| {
                let rx_rate = data.received() as f32 / elapsed;
                let tx_rate = data.transmitted() as f32 / elapsed;
                rx_total += rx_rate;
                tx_total += tx_rate;
                InterfaceInfo {
                    name: name.clone(),
                    rx_rate,
                    tx_rate,
                    total_received: data.total_received(),
                    total_transmitted: data.total_transmitted(),
                }
            })
            .collect();
        snapshot.interfaces.sort_by(|a, b| b.name.cmp(&a.name));
        push(&mut snapshot.rx_history, rx_total);
        push(&mut snapshot.tx_history, tx_total);

        snapshot.sensors = self
            .components
            .list()
            .iter()
            .map(|component| SensorInfo {
                label: component.label().to_owned(),
                temperature: component.temperature(),
                max: component.max(),
                critical: component.critical(),
            })
            .collect();
    }
}

fn push(history: &mut VecDeque<f32>, value: f32) {
    history.push_back(value);
    if history.len() > HISTORY_LEN {
        history.pop_front();
    }
}

pub fn fraction(used: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64).clamp(0.0, 1.0) as f32
    }
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn format_rate(bytes_per_second: f32) -> String {
    if bytes_per_second >= 1024.0 * 1024.0 {
        format!("{:.1} MiB/s", bytes_per_second / 1048576.0)
    } else if bytes_per_second >= 1024.0 {
        format!("{:.1} KiB/s", bytes_per_second / 1024.0)
    } else {
        format!("{bytes_per_second:.0} B/s")
    }
}

pub fn format_uptime(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_only_the_newest_samples() {
        let mut history = VecDeque::new();
        for value in 0..HISTORY_LEN + 5 {
            push(&mut history, value as f32);
        }

        assert_eq!(history.len(), HISTORY_LEN);
        assert_eq!(history.front(), Some(&5.0));
        assert_eq!(history.back(), Some(&((HISTORY_LEN + 4) as f32)));
    }

    #[test]
    fn fraction_handles_empty_totals_and_clamps() {
        assert_eq!(fraction(10, 0), 0.0);
        assert_eq!(fraction(1, 4), 0.25);
        assert_eq!(fraction(8, 4), 1.0);
    }

    #[test]
    fn formats_sizes_rates_and_uptime() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KiB");
        assert_eq!(format_bytes(5 * 1048576), "5.0 MiB");
        assert_eq!(format_rate(1536.0), "1.5 KiB/s");
        assert_eq!(format_rate(2.0 * 1048576.0), "2.0 MiB/s");
        assert_eq!(format_uptime(90_061), "1d 1h 1m");
    }
}
