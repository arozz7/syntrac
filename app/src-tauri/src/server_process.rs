//! Phase 4, slice L4c (docs/PHASE4-PLAN.md): the real way to start the user's model server. The program is started directly (no shell), with
//! no window and no input, and is put in a Windows job object so that stopping it also ends the programs it started (a server that fronts
//! another one would otherwise leave that one running). The job has no "end everything when closed" limit: when this app quits the server
//! keeps running unless the user chose to stop it on quit.
//!
//! Known limit: the job is joined just after the program starts, so a program that starts its own helpers in its first moments could
//! escape it. A server that loads a model first is not affected in practice.

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use syntrac_core::server_launch::LaunchCommand;
use syntrac_core::server_manager::{ChildHandle, LaunchError, ProcessSpawner};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation, QueryInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Starts programs. With a log file, what the program prints (output and errors) is saved there, the file starting fresh at each start; without
/// one, the output is discarded.
#[derive(Default)]
pub struct WindowsSpawner {
    log: Option<PathBuf>,
}

impl WindowsSpawner {
    pub fn with_log(log: PathBuf) -> Self {
        Self { log: Some(log) }
    }

    /// With a log when the folder for it is known, without one (output discarded) when it is not.
    pub fn with_optional_log(log: Option<PathBuf>) -> Self {
        Self { log }
    }

    /// The two ends of the program's output: both into the log file when there is one.
    fn output_streams(&self) -> (Stdio, Stdio) {
        let Some(path) = &self.log else { return (Stdio::null(), Stdio::null()) };
        let opened = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::File::create(path))
            .and_then(|file| file.try_clone().map(|second| (file, second)));
        match opened {
            Ok((out, err)) => (Stdio::from(out), Stdio::from(err)),
            // A log that cannot be written must not stop the server from starting.
            Err(_) => (Stdio::null(), Stdio::null()),
        }
    }

    /// Starts the command and returns the concrete handle (the trait method boxes it).
    pub fn spawn_job(&self, command: &LaunchCommand) -> Result<JobChild, LaunchError> {
        command.check().map_err(LaunchError::Refused)?;
        let program = command.program_path();
        if !Path::new(program).is_file() {
            return Err(LaunchError::ProgramMissing);
        }
        let folder = command.working_folder();
        if folder.is_some_and(|folder| !Path::new(folder).is_dir()) {
            return Err(LaunchError::FolderMissing);
        }
        let (out, err) = self.output_streams();
        let mut process = Command::new(program);
        process.args(&command.args).stdin(Stdio::null()).stdout(out).stderr(err).creation_flags(CREATE_NO_WINDOW);
        if let Some(folder) = folder {
            process.current_dir(folder);
        }
        let child = process.spawn().map_err(|error| LaunchError::Spawn(error.to_string()))?;
        // Without a job only the program itself can be stopped; that is still better than refusing to run.
        let job = Job::around(&child);
        Ok(JobChild { child, job })
    }
}

impl ProcessSpawner for WindowsSpawner {
    fn spawn(&self, command: &LaunchCommand) -> Result<Box<dyn ChildHandle>, LaunchError> {
        Ok(Box::new(self.spawn_job(command)?))
    }
}

/// A job object holding the started program and what it starts.
struct Job(HANDLE);

// A job handle is only used from the one place that owns it, one call at a time.
unsafe impl Send for Job {}

impl Job {
    fn around(child: &Child) -> Option<Self> {
        // SAFETY: plain calls with a valid process handle owned by `child`, which outlives them; the new job handle is owned by `Job`.
        unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).ok()?;
            let joined = AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as isize));
            if joined.is_err() {
                let _ = CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }

    fn end_all(&self) -> bool {
        // SAFETY: the handle is a job handle owned by `self`.
        unsafe { TerminateJobObject(self.0, 1).is_ok() }
    }

    fn active_processes(&self) -> Option<u32> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: `info` is a correctly sized, writable buffer for the class asked for.
        let read = unsafe {
            QueryInformationJobObject(
                self.0,
                JobObjectBasicAccountingInformation,
                std::ptr::addr_of_mut!(info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>()).ok()?,
                None,
            )
        };
        read.ok().map(|()| info.ActiveProcesses)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Closing the handle does not end the programs in the job (no kill-on-close limit was set).
        // SAFETY: the handle is owned by `self` and closed once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

pub struct JobChild {
    child: Child,
    job: Option<Job>,
}

impl JobChild {
    /// How many programs are still alive in the job (the server and what it started); `None` when there is no job.
    pub fn active_processes(&self) -> Option<u32> {
        self.job.as_ref().and_then(Job::active_processes)
    }
}

impl ChildHandle for JobChild {
    fn try_exit(&mut self) -> Result<Option<i32>, LaunchError> {
        match self.child.try_wait() {
            Ok(Some(status)) => Ok(Some(status.code().unwrap_or(-1))),
            Ok(None) => Ok(None),
            Err(error) => Err(LaunchError::Spawn(error.to_string())),
        }
    }

    fn kill(&mut self) -> Result<(), LaunchError> {
        let ended_all = self.job.as_ref().is_some_and(Job::end_all);
        if !ended_all {
            // No job (or it refused): at least end the program itself.
            if let Err(error) = self.child.kill() {
                // Already gone is fine.
                if self.child.try_wait().ok().flatten().is_none() {
                    return Err(LaunchError::Kill(error.to_string()));
                }
            }
        }
        // Collect the exit status so the process is not left half-closed.
        let _ = self.child.wait();
        Ok(())
    }
}
