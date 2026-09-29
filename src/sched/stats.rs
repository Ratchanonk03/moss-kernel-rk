use crate::drivers::timer::Instant;
use crate::kernel::cpu_id::CpuId;
use core::sync::atomic::Ordering;
use super::NUM_CONTEXT_SWITCHES;
use log::info;


pub struct SchedulerStats {
    pub completed_tasks: u64,
    pub total_wait_ns: u128,
    pub total_completion_ns: u128,
}

impl SchedulerStats {
    pub const fn new() -> Self {
        Self {
            completed_tasks: 0,
            total_wait_ns: 0,
            total_completion_ns: 0,
        }
    }

    pub fn task_selected(&mut self, now: Instant, ready_since: Instant) {
        self.total_wait_ns += (now - ready_since).as_nanos();
    }

    pub fn task_finished(&mut self, now: Instant, created_at: Instant) {
        self.total_completion_ns += (now - created_at).as_nanos();
        self.completed_tasks += 1;
    }

    pub fn log_stats(&self, policy: &str, quantum_ms: Option<u128>) {
        let completed_tasks = self.completed_tasks;
        let total_wait_ns = self.total_wait_ns;
        let total_completion_ns = self.total_completion_ns;

        if completed_tasks > 0 {
            let avg_wait_ns = total_wait_ns / completed_tasks as u128;
            let avg_completion_ns = total_completion_ns / completed_tasks as u128;
            let context_switches = NUM_CONTEXT_SWITCHES.load(Ordering::Relaxed);
            let quantum = quantum_ms
                .map(|value| alloc::format!("{} ms", value))
                .unwrap_or_else(|| alloc::string::String::from("N/A"));
            
            info!(
                "CPU {}: Policy: {} | Quantum: {} | Completed tasks: {} | Avg wait time: {} ns | Avg completion time: {} ns | Context switches: {}",
                CpuId::this().value(),
                policy,
                quantum,
                completed_tasks,
                avg_wait_ns,
                avg_completion_ns,
                context_switches
            );
        }
    }
    
}