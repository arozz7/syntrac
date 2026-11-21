//! A fake way to start programs (docs/PHASE4-PLAN.md, L4c): it records what it was asked to start and lets a test decide when the "process"
//! ends. No real program runs.

use std::sync::{Arc, Mutex};

use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::{ChildHandle, LaunchError, ProcessSpawner};

/// What a test can see and change about one fake process.
#[derive(Debug, Default)]
pub struct FakeProcess {
    pub exit_code: Option<i32>,
    pub killed: bool,
    pub kill_fails: bool,
}

#[derive(Default)]
pub struct FakeSpawner {
    pub spawned: Arc<Mutex<Vec<LaunchCommand>>>,
    pub processes: Arc<Mutex<Vec<Arc<Mutex<FakeProcess>>>>>,
    pub fail_with: Arc<Mutex<Option<LaunchError>>>,
}

struct FakeHandle(Arc<Mutex<FakeProcess>>);

impl ChildHandle for FakeHandle {
    fn try_exit(&mut self) -> Result<Option<i32>, LaunchError> {
        let process = self.0.lock().unwrap();
        Ok(if process.killed { Some(-1) } else { process.exit_code })
    }

    fn kill(&mut self) -> Result<(), LaunchError> {
        let mut process = self.0.lock().unwrap();
        if process.kill_fails {
            return Err(LaunchError::Kill("access denied".into()));
        }
        process.killed = true;
        Ok(())
    }
}

impl ProcessSpawner for FakeSpawner {
    fn spawn(&self, command: &LaunchCommand) -> Result<Box<dyn ChildHandle>, LaunchError> {
        if let Some(error) = self.fail_with.lock().unwrap().clone() {
            return Err(error);
        }
        let process = Arc::new(Mutex::new(FakeProcess::default()));
        self.spawned.lock().unwrap().push(command.clone());
        self.processes.lock().unwrap().push(Arc::clone(&process));
        Ok(Box::new(FakeHandle(process)))
    }
}

impl FakeSpawner {
    pub fn count(&self) -> usize {
        self.spawned.lock().unwrap().len()
    }

    /// The most recently started fake process.
    pub fn last(&self) -> Arc<Mutex<FakeProcess>> {
        Arc::clone(self.processes.lock().unwrap().last().expect("a process was started"))
    }
}
