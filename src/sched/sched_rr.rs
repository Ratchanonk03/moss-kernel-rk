use alloc::{
    collections::{VecDeque, btree_map::BTreeMap}, 
    boxed::Box
};
use core::{
    time::Duration,
    sync::atomic::Ordering,
};

use log::info;
use crate::kernel::cpu_id::CpuId;
use crate::arch::{Arch, ArchImpl};
use super::current::CUR_TASK_PTR;
use super::NUM_CONTEXT_SWITCHES;

use crate::process::{
    owned::OwnedTask, 
    TaskState, 
    TaskDescriptor
};
use crate::drivers::timer::{Instant, now, schedule_preempt};
use crate::per_cpu_private;

per_cpu_private! {
    pub(super) static RR_STATE: RRScheduler = RRScheduler::new;
}

pub const ROUND_ROBIN_QUANTUM: Duration = Duration::from_millis(10);

pub struct RRTask {
    pub task: Box<OwnedTask>,
    pub created_at: Instant,
    pub ready_since: Instant,
    pub slice_started_at: Instant,
}

impl RRTask {
    pub fn new(task: Box<OwnedTask>) -> Self {
        let current_time = now().expect("Failed to get current time");
        
        Self {
            task,
            created_at: current_time,
            ready_since: current_time,
            slice_started_at: current_time,
        }
    }

    pub fn is_quantum_expired_at(&self, current_time: Instant) -> bool {
        current_time - self.slice_started_at >= ROUND_ROBIN_QUANTUM
    }

    pub fn is_idle_task(&self) -> bool {
        self.task.is_idle_task()
    }

    pub fn get_state(&self) -> TaskState {
        *self.task.state.lock_save_irq()
    }

    pub fn set_state(&mut self, state: TaskState) {
        *self.task.state.lock_save_irq() = state;
    }

    pub fn get_descriptor(&self) -> TaskDescriptor {
        self.task.descriptor()
    }
}

pub struct RRScheduler {
    /// A queue of tasks that are ready to run.
    pub ready_queue: VecDeque<RRTask>,
    pub wait_queue: BTreeMap<TaskDescriptor, RRTask>,
    pub current_task: Option<RRTask>,
    pub idle_task: Option<RRTask>,
    pub yield_requested: bool,
    pub completed_tasks: u64,
    pub total_wait_ns: u128,
    pub total_completion_ns: u128,
}

impl RRScheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            wait_queue: BTreeMap::new(),
            current_task: None,
            idle_task: None,
            yield_requested: false,
            completed_tasks: 0,
            total_wait_ns: 0,
            total_completion_ns: 0,
        }
    }
    
    pub fn yield_current(&mut self) {
        self.yield_requested = true;
        self.do_schedule();
    }

    pub fn add_task(&mut self, task: Box<OwnedTask>) {
        let task = RRTask::new(task);

        if task.is_idle_task() {
            self.idle_task = Some(task);
        }
        else {
            self.ready_queue.push_back(task);
        }
    }

    pub fn do_schedule(&mut self) {
        let current_time = now().expect("system timer not running");
        let previous_desc = self
            .current_task
            .as_ref()
            .map(|task| task.get_descriptor());

        if let Some(mut task) = self.current_task.take() {
            task.task.update_accounting(Some(current_time));
            task.task.reset_last_account(current_time);

            if task.is_idle_task() {
                if self.ready_queue.is_empty() {
                    task.slice_started_at = current_time;
                    self.current_task = Some(task);
                    return;
                }

                self.idle_task = Some(task);
            }
            else 
            {
                let state = match task.get_state() {
                    TaskState::Woken => {
                        task.set_state(TaskState::Running);
                        TaskState::Running
                    }
                    state => state,
                };

                match state {
                    TaskState::Running | TaskState::Runnable => {
                        let must_rotate = self.yield_requested || task.is_quantum_expired_at(current_time);
                        self.yield_requested = false;

                        if !must_rotate {
                            self.current_task = Some(task);
                            return;
                        }
            
                        task.set_state(TaskState::Runnable);
                        task.ready_since = current_time;
                        self.ready_queue.push_back(task);
                        
                    }

                    TaskState::Sleeping | TaskState::Stopped => {
                        self.wait_queue.insert(task.get_descriptor(), task);
                    }

                    TaskState::Finished => {
                        if !task.is_idle_task() {
                            self.total_completion_ns +=
                                (current_time - task.created_at).as_nanos();

                            self.completed_tasks += 1;

                            self.log_stats();
                        }
                        // Finished task: do not requeue it.
                    }

                    TaskState::Woken => unreachable!(),
                }
            }
        }

        let mut next = self
            .ready_queue
            .pop_front()
            .or_else(|| self.idle_task.take())
            .expect("scheduler has no idle task");

        if !next.is_idle_task() {
            self.total_wait_ns += (current_time - next.ready_since).as_nanos();
        }

        next.slice_started_at = current_time;
        next.set_state(TaskState::Running);
        *next.task.last_cpu.lock_save_irq() = CpuId::this();

        schedule_preempt(current_time + ROUND_ROBIN_QUANTUM);

        let switched = previous_desc != Some(next.get_descriptor());

        self.current_task = Some(next);

        if switched {
            self.do_context_switch(current_time);
        }
    }

    pub fn wakeup(&mut self, descriptor: TaskDescriptor) {
        if let Some(mut task) = self.wait_queue.remove(&descriptor) {
            // waker.rs already changed Sleeping/Stopped → Runnable.
            task.ready_since = now().expect("system timer not running");
            self.ready_queue.push_back(task);
        }
    }

    fn log_stats(&self) {
        let completed_tasks = self.completed_tasks;
        let total_wait_ns = self.total_wait_ns;
        let total_completion_ns = self.total_completion_ns;

        if completed_tasks > 0 {
            let avg_wait_ns = total_wait_ns / completed_tasks as u128;
            let avg_completion_ns = total_completion_ns / completed_tasks as u128;
            
            info!(
                "CPU {}: Completed tasks: {}, Avg wait time: {} ns, Avg completion time: {} ns",
                CpuId::this().value(),
                completed_tasks,
                avg_wait_ns,
                avg_completion_ns
            );
        }
    }

    fn do_context_switch(&mut self, current_time: Instant) {
        let current = self.current_task.as_mut().expect("selected task must be current");

        ArchImpl::context_switch(current.task.t_shared.clone());
        CUR_TASK_PTR.borrow_mut().set_current(&mut current.task);
        NUM_CONTEXT_SWITCHES.fetch_add(1, Ordering::Relaxed);
        current.task.reset_last_account(current_time);
    }
    
}