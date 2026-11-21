use std::ffi::{OsStr, OsString};

use sysinfo::{Pid, Process, ProcessStatus, ProcessesToUpdate, Signal, System};

#[derive(Clone, Debug)]
pub struct ProcessInfo {
    pub pid: Pid,
    pub name: String,
    pub command: String,
    pub memory_bytes: u64,
}

impl ProcessInfo {
    fn from_process(pid: Pid, process: &Process) -> Self {
        let command = match process.exe() {
            Some(exe) => {
                let mut parts = vec![text(exe.as_os_str())];
                parts.extend(process.cmd().iter().skip(1).map(text_os));
                parts.join(" ")
            }
            None => process
                .cmd()
                .iter()
                .map(text_os)
                .collect::<Vec<_>>()
                .join(" "),
        };

        Self {
            pid,
            name: text(process.name()),
            command,
            memory_bytes: process.memory(),
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        query.is_empty() || self.name.eq_ignore_ascii_case(query)
    }
}

pub fn collect(system: &mut System) -> Vec<ProcessInfo> {
    system.refresh_processes(ProcessesToUpdate::All);

    let mut processes: Vec<_> = system
        .processes()
        .iter()
        .map(|(&pid, process)| ProcessInfo::from_process(pid, process))
        .collect();

    processes.sort_by_key(|p| std::cmp::Reverse(p.memory_bytes));
    processes
}

pub fn is_protected(pid: Pid, own_pid: Pid) -> bool {
    pid.as_u32() <= 1 || pid == own_pid
}

pub fn signal(system: &mut System, pid: Pid, signal: Signal, own_pid: Pid) -> Result<(), String> {
    if is_protected(pid, own_pid) {
        return Err("processo protegido".into());
    }

    system.refresh_processes(ProcessesToUpdate::All);

    let Some(process) = system.process(pid) else {
        return Err("processo não existe mais".into());
    };

    if process.status() == ProcessStatus::Zombie {
        return Err("processo já encerrado".into());
    }

    match process.kill_with(signal) {
        Some(true) => Ok(()),
        Some(false) => Err("sem permissão".into()),
        None => Err("sinal não suportado".into()),
    }
}

pub fn is_alive(system: &mut System, pid: Pid) -> bool {
    system.refresh_processes(ProcessesToUpdate::All);
    system
        .process(pid)
        .is_some_and(|process| process.status() != ProcessStatus::Zombie)
}

fn text(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

fn text_os(value: &OsString) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ProcessInfo {
        ProcessInfo {
            pid: Pid::from_u32(1234),
            name: "vesktop".into(),
            command: "/usr/bin/vesktop --enable-features=X".into(),
            memory_bytes: 2048,
        }
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(sample().matches(""));
        assert!(sample().matches("   "));
    }

    #[test]
    fn query_matches_process_name_exactly() {
        let process = sample();
        assert!(process.matches("vesktop"));
        assert!(process.matches("VESKTOP"));
        assert!(!process.matches("ves"));
        assert!(!process.matches("1234"));
        assert!(!process.matches("vesktop enable"));
        assert!(!process.matches("discord"));
    }
}
