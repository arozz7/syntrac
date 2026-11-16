const fs = require('fs');
function patch(file, edits) {
  let t = fs.readFileSync(file, 'utf8');
  for (const [a, b] of edits) { if (!t.includes(a)) throw new Error('missing in ' + file + ': ' + a); t = t.replace(a, b); }
  fs.writeFileSync(file, t);
}
patch('crates/syntrac-core/src/launch_run.rs', [
  ['    /// The sources that failed, with why.\n    pub failed: Vec<(String, String)>,\n}', '    /// The sources that failed, with why.\n    pub failed: Vec<(String, String)>,\n    /// The Uninstall keys read in the same pass (install records), when that source is used.\n    pub installs: Option<InstallSweep>,\n}'],
  ['use crate::launch_target::launch_target;', 'use crate::install_records::InstallSweep;\nuse crate::launch_target::launch_target;'],
]);
patch('crates/syntrac-service/src/launch_worker.rs', [
  ['use syntrac_core::{sweep_launchers, Clock, LaunchSource, LauncherRepository, SweepReport, Timestamp};', 'use syntrac_core::{\n    sweep_installs, sweep_launchers, Clock, InstallRepository, InstallSource, InstallSweepError, LaunchSource,\n    LauncherRepository, SweepReport, Timestamp,\n};'],
  ['    clock: C,\n    settings: LaunchWorkerSettings,\n    next_sweep_at: Timestamp,\n}', '    installs: Option<Box<dyn InstallSource + Send>>,\n    clock: C,\n    settings: LaunchWorkerSettings,\n    next_sweep_at: Timestamp,\n}'],
  ['impl<R: LauncherRepository, C: Clock> LaunchWorker<R, C> {', 'impl<R: LauncherRepository + InstallRepository, C: Clock> LaunchWorker<R, C> {'],
  ['        Self { repo, sources, env, exists, clock, settings, next_sweep_at: now }\n    }', '        Self { repo, sources, env, exists, installs: None, clock, settings, next_sweep_at: now }\n    }\n\n    /// Also reads the Uninstall keys (install records) in every sweep.\n    #[must_use]\n    pub fn with_installs(mut self, source: Box<dyn InstallSource + Send>) -> Self {\n        self.installs = Some(source);\n        self\n    }'],
  ['        match sweep_launchers(&sources, &self.env, &self.exists, &mut self.repo, now) {\n            Ok(report) => LaunchTick::Swept(report),\n            Err(error) => LaunchTick::Failed(error.to_string()),\n        }', '        let mut report = match sweep_launchers(&sources, &self.env, &self.exists, &mut self.repo, now) {\n            Ok(report) => report,\n            Err(error) => return LaunchTick::Failed(error.to_string()),\n        };\n        if let Some(source) = &self.installs {\n            match sweep_installs(source.as_ref(), &mut self.repo, now) {\n                Ok(installs) => report.installs = Some(installs),\n                Err(InstallSweepError::Source(why)) => report.failed.push(("install records".to_owned(), why)),\n                Err(InstallSweepError::Store(error)) => return LaunchTick::Failed(error.to_string()),\n            }\n        }\n        LaunchTick::Swept(report)'],
  ['                    if report.new > 0 {', '                    if let Some(installs) = &report.installs {\n                        if installs.new > 0 {\n                            info!(recorded = installs.recorded, new = installs.new, "Install records found products not seen before");\n                        }\n                    }\n                    if report.new > 0 {'],
]);
patch('crates/syntrac-service/src/host.rs', [
  ['    let worker = LaunchWorker::new(store, sources, launch_env::environment, launch_env::file_exists, SystemClock, settings);', '    let worker = LaunchWorker::new(store, sources, launch_env::environment, launch_env::file_exists, SystemClock, settings)\n        .with_installs(Box::new(UninstallKeySource));'],
  ['use syntrac_collector::launch_env;', 'use syntrac_collector::install_registry::UninstallKeySource;\nuse syntrac_collector::launch_env;'],
]);
patch('crates/syntrac-service/tests/launch_worker.rs', [
  ['    Executable, LaunchSource, LauncherKind, LauncherObservation, LauncherRepository, LauncherSeen, LauncherWrite,\n    RawEntry, RepoError, Timestamp,\n};', '    Executable, InstallRecord, InstallRepository, InstallSeen, InstallWrite, LaunchSource, LauncherKind,\n    LauncherObservation, LauncherRepository, LauncherSeen, LauncherWrite, RawEntry, RawInstall, RepoError, Timestamp,\n};'],
  ['use syntrac_testkit::{FakeLaunchSource, InMemoryRepository, ManualClock};', 'use syntrac_testkit::{FakeInstallSource, FakeLaunchSource, InMemoryRepository, ManualClock};'],
]);
fs.appendFileSync('crates/syntrac-service/tests/launch_worker.rs', `
impl InstallRepository for Flaky {
    fn record_installs(&mut self, rows: &[InstallRecord], at: Timestamp) -> Result<InstallWrite, RepoError> {
        self.inner.record_installs(rows, at)
    }
    fn installs(&self) -> Result<Vec<InstallSeen>, RepoError> {
        self.inner.installs()
    }
}

fn product(name: &str) -> RawInstall {
    RawInstall {
        hive: "machine".to_owned(),
        key_name: format!("{{{name}}}"),
        display_name: name.to_owned(),
        install_location: format!(r"C:\\Apps\\{name}"),
        ..RawInstall::default()
    }
}

#[test]
fn the_uninstall_keys_are_read_in_the_same_sweep_when_a_source_is_given() {
    let mut worker = LaunchWorker::new(
        InMemoryRepository::default(),
        tasks(vec![entry(r"\One", A)]),
        env,
        exists,
        ManualClock::new(at(0)),
        settings(),
    )
    .with_installs(Box::new(FakeInstallSource::new(vec![product("Alpha"), product("Beta")])));

    let tick = worker.tick();

    let LaunchTick::Swept(report) = tick else { panic!("expected a sweep: {tick:?}") };
    assert_eq!(report.recorded, 1);
    let installs = report.installs.expect("install records were read");
    assert_eq!((installs.recorded, installs.new), (2, 2));
}

#[test]
fn a_failing_uninstall_read_is_reported_and_the_launch_records_are_still_kept() {
    let mut worker = LaunchWorker::new(
        InMemoryRepository::default(),
        tasks(vec![entry(r"\One", A)]),
        env,
        exists,
        ManualClock::new(at(0)),
        settings(),
    )
    .with_installs(Box::new(FakeInstallSource::failing("the registry is busy")));

    let tick = worker.tick();

    let LaunchTick::Swept(report) = tick else { panic!("expected a sweep: {tick:?}") };
    assert_eq!(report.recorded, 1);
    assert!(report.installs.is_none());
    assert_eq!(report.failed, [("install records".to_owned(), "the registry is busy".to_owned())]);
}
`);
