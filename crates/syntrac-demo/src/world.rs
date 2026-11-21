//! The FICTIONAL machine behind the demo (docs/PUBLIC-PLAN.md, P1): its programs, makers, folders, addresses and how busy each program is at
//! any moment. Everything here is invented: the makers are the sample companies of documentation (Contoso, Fabrikam, Northwind, Acme,
//! Tailspin), the user is `demo`, the addresses are the three ranges reserved for documentation (RFC 5737) and the names end in `.example`.
//! Nothing is read from, or copied from, a real machine.
//!
//! Pure and deterministic: the same moment always gives the same numbers, so the generator (the past) and the live feeder (now) agree.

use syntrac_core::{MetricsSample, OtherProcesses, ProcessInstanceId, ProcessMetric, SystemMetrics, Timestamp};

pub const HOUR_SECS: i64 = 3600;
pub const DAY_SECS: i64 = 86_400;
const MB: u64 = 1024 * 1024;

/// The signed-in user of the fictional machine.
pub const PROFILE: &str = r"C:\Users\demo";

/// When in the day (UTC hours) a program is busy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Always,
    Workday,
    Evening,
}

impl When {
    /// The first hour of the day in which the program runs.
    fn first_hour(self) -> i64 {
        match self {
            Self::Always => 0,
            Self::Workday => 8,
            Self::Evening => 19,
        }
    }

    fn contains(self, hour: i64) -> bool {
        match self {
            Self::Always => true,
            Self::Workday => (8..18).contains(&hour),
            Self::Evening => (19..24).contains(&hour),
        }
    }
}

/// One thing a program sends to and receives from one destination, in bytes per ten minutes while it is busy.
#[derive(Debug, Clone, Copy)]
pub struct Traffic {
    pub dest: usize,
    pub sent: u64,
    pub received: u64,
}

/// A rise a program shows in the last two completed hours, so the usage rules have something to find.
#[derive(Debug, Clone, Copy)]
pub struct Rise {
    pub cpu: f32,
    pub sent: f32,
}

#[derive(Debug)]
pub struct Program {
    pub path: &'static str,
    /// The maker named in the signature and the version resource; empty for none.
    pub company: &'static str,
    pub product: &'static str,
    pub description: &'static str,
    pub signed: bool,
    /// Carries the "downloaded from the Internet" mark.
    pub downloaded: bool,
    /// Between these many hours ago and now (or until `ends_hours_ago`) the program exists.
    pub first_seen_hours_ago: i64,
    pub ends_hours_ago: Option<i64>,
    pub when: When,
    /// Typical share of the whole machine's CPU while busy (percent).
    pub cpu: f32,
    pub memory_mb: u64,
    /// Typical share of the program's busiest GPU engine while busy (percent), and its GPU memory.
    pub gpu: f32,
    pub gpu_memory_mb: u64,
    pub traffic: &'static [Traffic],
    pub rise: Option<Rise>,
}

/// An address the programs talk to: a documentation address with an invented name.
#[derive(Debug)]
pub struct Destination {
    pub addr: [u8; 4],
    pub name: &'static str,
}

pub const DESTINATIONS: &[Destination] = &[
    Destination { addr: [203, 0, 113, 10], name: "updates.fabrikam.example" },
    Destination { addr: [203, 0, 113, 20], name: "cdn.fabrikam.example" },
    Destination { addr: [198, 51, 100, 7], name: "sync.northwind.example" },
    Destination { addr: [198, 51, 100, 8], name: "storage.northwind.example" },
    Destination { addr: [192, 0, 2, 40], name: "backup.acme.example" },
    Destination { addr: [192, 0, 2, 41], name: "api.acme.example" },
    Destination { addr: [203, 0, 113, 99], name: "telemetry.contoso.example" },
    Destination { addr: [198, 51, 100, 200], name: "play.tailspin.example" },
    Destination { addr: [192, 0, 2, 250], name: "pool.example.org" },
    Destination { addr: [198, 51, 100, 77], name: "download.setuphelper.example" },
];

/// The networks that own those addresses (invented organisations on private-use AS numbers).
pub const OWNERS: &[(u32, [u8; 4], [u8; 4], &str)] = &[
    (64512, [203, 0, 113, 0], [203, 0, 113, 255], "EXAMPLE-CDN"),
    (64513, [198, 51, 100, 0], [198, 51, 100, 63], "NORTHWIND-NET"),
    (64516, [198, 51, 100, 64], [198, 51, 100, 127], "SETUPHELPER-NET"),
    (64514, [192, 0, 2, 0], [192, 0, 2, 127], "ACME-CLOUD"),
    (64515, [192, 0, 2, 128], [192, 0, 2, 255], "EXAMPLE-HOSTING"),
    (64517, [198, 51, 100, 128], [198, 51, 100, 255], "TAILSPIN-NET"),
];

const BROWSER_TRAFFIC: &[Traffic] =
    &[Traffic { dest: 0, sent: 60_000, received: 900_000 }, Traffic { dest: 1, sent: 90_000, received: 6_500_000 }];
const EDITOR_TRAFFIC: &[Traffic] = &[Traffic { dest: 6, sent: 20_000, received: 40_000 }];
const SYNC_TRAFFIC: &[Traffic] =
    &[Traffic { dest: 2, sent: 700_000, received: 150_000 }, Traffic { dest: 3, sent: 200_000, received: 60_000 }];
const BACKUP_TRAFFIC: &[Traffic] =
    &[Traffic { dest: 4, sent: 4_200_000, received: 80_000 }, Traffic { dest: 5, sent: 30_000, received: 20_000 }];
const GAME_TRAFFIC: &[Traffic] = &[Traffic { dest: 7, sent: 400_000, received: 1_800_000 }];
const MINER_TRAFFIC: &[Traffic] = &[Traffic { dest: 8, sent: 60_000, received: 90_000 }];
const HELPER_TRAFFIC: &[Traffic] = &[Traffic { dest: 9, sent: 2_000_000, received: 38_000_000 }];

pub const PROGRAMS: &[Program] = &[
    Program {
        path: r"C:\Program Files\Fabrikam\Browser\fabrikam.exe",
        company: "Fabrikam, Inc.",
        product: "Fabrikam Browser",
        description: "Fabrikam Browser",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 4.0,
        memory_mb: 1_100,
        gpu: 12.0,
        gpu_memory_mb: 350,
        traffic: BROWSER_TRAFFIC,
        rise: None,
    },
    Program {
        path: r"C:\Program Files\Contoso\Editor\contoso-edit.exe",
        company: "Contoso Ltd",
        product: "Contoso Editor",
        description: "Contoso text and code editor",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 3.0,
        memory_mb: 520,
        gpu: 3.0,
        gpu_memory_mb: 120,
        traffic: EDITOR_TRAFFIC,
        rise: Some(Rise { cpu: 16.0, sent: 1.0 }),
    },
    Program {
        path: r"C:\Program Files\Northwind\Sync\nwsync.exe",
        company: "Northwind Traders",
        product: "Northwind Sync",
        description: "Northwind file sync",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 0.8,
        memory_mb: 180,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: SYNC_TRAFFIC,
        rise: Some(Rise { cpu: 1.0, sent: 80.0 }),
    },
    Program {
        path: r"C:\Program Files\Northwind\Sync\nwsync-helper.exe",
        company: "Northwind Traders",
        product: "Northwind Sync",
        description: "Northwind file sync helper",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 0.3,
        memory_mb: 60,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Program Files\Acme\Backup\acmebackup.exe",
        company: "Acme Software, Inc.",
        product: "Acme Backup",
        description: "Acme Backup service",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 1.4,
        memory_mb: 240,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: BACKUP_TRAFFIC,
        rise: None,
    },
    Program {
        path: r"C:\Program Files\Acme\Backup\acmebackup-agent.exe",
        company: "Acme Software, Inc.",
        product: "Acme Backup",
        description: "Acme Backup agent",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 0.4,
        memory_mb: 90,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Program Files\Acme\Backup\acmebackup-ui.exe",
        company: "Acme Software, Inc.",
        product: "Acme Backup",
        description: "Acme Backup control panel",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Workday,
        cpu: 0.2,
        memory_mb: 140,
        gpu: 1.0,
        gpu_memory_mb: 40,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Program Files\Tailspin\Racing\tailspin.exe",
        company: "Tailspin Toys",
        product: "Tailspin Racing",
        description: "Tailspin Racing",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Evening,
        cpu: 14.0,
        memory_mb: 3_600,
        gpu: 78.0,
        gpu_memory_mb: 6_400,
        traffic: GAME_TRAFFIC,
        rise: None,
    },
    Program {
        path: r"C:\Users\demo\AppData\Local\Programs\LocalAI\localai-server.exe",
        company: "",
        product: "LocalAI Server",
        description: "Runs language models on this machine",
        signed: false,
        downloaded: false,
        first_seen_hours_ago: 24 * 12,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 2.0,
        memory_mb: 9_000,
        gpu: 62.0,
        gpu_memory_mb: 14_000,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Users\demo\AppData\Roaming\toolx\toolx.exe",
        company: "",
        product: "",
        description: "",
        signed: false,
        downloaded: false,
        first_seen_hours_ago: 24 * 2,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 1.2,
        memory_mb: 220,
        gpu: 88.0,
        gpu_memory_mb: 1_900,
        traffic: MINER_TRAFFIC,
        rise: None,
    },
    Program {
        path: r"C:\Users\demo\Downloads\SetupHelper.exe",
        company: "",
        product: "",
        description: "",
        signed: false,
        downloaded: true,
        first_seen_hours_ago: 20,
        ends_hours_ago: Some(17),
        when: When::Always,
        cpu: 6.0,
        memory_mb: 310,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: HELPER_TRAFFIC,
        rise: None,
    },
    Program {
        path: r"C:\Users\demo\tools\pdftool\pdftool.exe",
        company: "",
        product: "PDF Tool",
        description: "Joins and splits PDF files",
        signed: false,
        downloaded: false,
        first_seen_hours_ago: 24 * 14,
        ends_hours_ago: None,
        when: When::Workday,
        cpu: 0.6,
        memory_mb: 80,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Users\demo\tools\renamer\renamer.exe",
        company: "",
        product: "Renamer",
        description: "Renames files in bulk",
        signed: false,
        downloaded: false,
        first_seen_hours_ago: 24 * 13,
        ends_hours_ago: None,
        when: When::Workday,
        cpu: 0.2,
        memory_mb: 45,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Windows\System32\svchost.exe",
        company: "Microsoft Corporation",
        product: "Microsoft Windows Operating System",
        description: "Host Process for Windows Services",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 2.2,
        memory_mb: 140,
        gpu: 0.0,
        gpu_memory_mb: 0,
        traffic: &[],
        rise: None,
    },
    Program {
        path: r"C:\Windows\explorer.exe",
        company: "Microsoft Corporation",
        product: "Microsoft Windows Operating System",
        description: "Windows Explorer",
        signed: true,
        downloaded: false,
        first_seen_hours_ago: 24 * 16,
        ends_hours_ago: None,
        when: When::Always,
        cpu: 1.0,
        memory_mb: 330,
        gpu: 4.0,
        gpu_memory_mb: 180,
        traffic: &[],
        rise: None,
    },
];

/// A repeatable number between 0 and 1 for a moment and a program (a hash, so the demo never changes between runs).
pub fn noise(ts: i64, salt: u64) -> f32 {
    let mut x = (ts as u64) ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The moment the demo calls "now" and what follows from it.
#[derive(Debug, Clone, Copy)]
pub struct Moment {
    pub now: i64,
}

impl Moment {
    pub fn new(now: i64) -> Self {
        Self { now }
    }

    /// The start of the newest hour the rollups cover once they settle (an hour counts five minutes after it ends).
    pub fn last_rolled_hour(&self) -> i64 {
        (self.now - 300).div_euclid(HOUR_SECS) * HOUR_SECS - HOUR_SECS
    }

    /// When the first program was seen: the history starts here.
    pub fn history_start(&self) -> i64 {
        (self.now - 16 * DAY_SECS).div_euclid(HOUR_SECS) * HOUR_SECS
    }

    /// When "detection was switched on": six days ago. What was first seen later is new; what was here earlier is "already here".
    pub fn detection_switched_on(&self) -> i64 {
        self.now - 6 * DAY_SECS
    }
}

fn exists_at(program: &Program, moment: &Moment, ts: i64) -> bool {
    let start = moment.now - program.first_seen_hours_ago * HOUR_SECS;
    let end = program.ends_hours_ago.map_or(i64::MAX, |ago| moment.now - ago * HOUR_SECS);
    ts >= start && ts < end
}

/// Whether the program is running at `ts`.
pub fn is_busy(program: &Program, moment: &Moment, ts: i64) -> bool {
    exists_at(program, moment, ts) && program.when.contains(ts.div_euclid(HOUR_SECS).rem_euclid(24))
}

/// The process of this program on the day of `ts`: its id, process id and start. One process per program and day.
pub fn instance(index: usize, moment: &Moment, ts: i64) -> (ProcessInstanceId, u32, i64) {
    let program = &PROGRAMS[index];
    let day = ts.div_euclid(DAY_SECS);
    let day_start = day * DAY_SECS;
    let lifetime_start = moment.now - program.first_seen_hours_ago * HOUR_SECS;
    let start = (day_start + program.when.first_hour() * HOUR_SECS + index as i64 * 7).max(lifetime_start);
    let pid = 1_000 + (index as u32) * 100 + u32::try_from(day.rem_euclid(90)).unwrap_or(0);
    (ProcessInstanceId::from_pid_and_creation(pid, Timestamp::from_micros(start * 1_000_000)), pid, start)
}

/// Whether the rise (the usage rules' reason to look) applies at `ts`: exactly the last two completed hours, no longer (the hour in
/// progress is back to normal, so the live screens do not look pegged).
fn rising(moment: &Moment, ts: i64) -> bool {
    let first = moment.last_rolled_hour() - HOUR_SECS;
    (first..first + 2 * HOUR_SECS).contains(&ts)
}

/// The traffic of one ten-minute slot of one program, as (destination, sent, received).
pub fn slot_traffic(index: usize, moment: &Moment, slot_start: i64) -> Vec<(usize, u64, u64)> {
    let program = &PROGRAMS[index];
    if !is_busy(program, moment, slot_start) {
        return Vec::new();
    }
    let boost = program.rise.filter(|_| rising(moment, slot_start)).map_or(1.0, |rise| rise.sent);
    program
        .traffic
        .iter()
        .enumerate()
        .map(|(n, flow)| {
            let jitter = 0.6 + 0.8 * noise(slot_start, (index * 16 + n) as u64);
            let scale = |bytes: u64, extra: f32| (bytes as f32 * jitter * extra) as u64;
            (flow.dest, scale(flow.sent, boost), scale(flow.received, 1.0))
        })
        .collect()
}

/// One metrics sample of the whole fictional machine at `ts`, covering `step` seconds.
pub fn sample_at(moment: &Moment, ts: i64, step: i64) -> MetricsSample {
    let mut processes = Vec::new();
    let (mut cpu_total, mut memory_total, mut gpu_busiest) = (0.0_f32, 0_u64, 0.0_f32);
    let (mut sent_total, mut received_total) = (0_u64, 0_u64);
    for (index, program) in PROGRAMS.iter().enumerate() {
        if !is_busy(program, moment, ts) {
            continue;
        }
        let salt = index as u64;
        let rise = program.rise.filter(|_| rising(moment, ts)).map_or(1.0, |rise| rise.cpu);
        let wobble = 0.8 + 0.4 * noise(ts, salt);
        let cpu = (program.cpu * wobble * rise).clamp(0.0, 100.0);
        let gpu = (program.gpu * (0.85 + 0.3 * noise(ts, salt + 500))).clamp(0.0, 100.0);
        let (id, pid, _) = instance(index, moment, ts);
        let memory = program.memory_mb * MB;
        processes.push(ProcessMetric {
            pid,
            instance: Some(id),
            cpu_percent: cpu,
            memory_bytes: memory,
            gpu_percent: gpu,
            gpu_memory_bytes: program.gpu_memory_mb * MB,
        });
        cpu_total += cpu;
        memory_total += memory;
        gpu_busiest = gpu_busiest.max(gpu);
        let per_second = |bytes: u64| bytes / 600;
        for (_, sent, received) in slot_traffic(index, moment, ts.div_euclid(600) * 600) {
            sent_total += per_second(sent) * u64::try_from(step).unwrap_or(0);
            received_total += per_second(received) * u64::try_from(step).unwrap_or(0);
        }
    }
    let step_bytes = u64::try_from(step).unwrap_or(0);
    SampleSystem { cpu_total, memory_total, gpu_busiest, sent_total, received_total, step_bytes }.finish(ts, processes)
}

struct SampleSystem {
    cpu_total: f32,
    memory_total: u64,
    gpu_busiest: f32,
    sent_total: u64,
    received_total: u64,
    step_bytes: u64,
}

impl SampleSystem {
    fn finish(self, ts: i64, processes: Vec<ProcessMetric>) -> MetricsSample {
        let tail_cpu = 1.5 + noise(ts, 900);
        MetricsSample {
            ts: Timestamp::from_micros(ts * 1_000_000),
            system: SystemMetrics {
                cpu_percent: (self.cpu_total + tail_cpu + 1.0).clamp(0.0, 100.0),
                memory_used_bytes: 11 * 1024 * MB + self.memory_total,
                memory_total_bytes: 32 * 1024 * MB,
                disk_read_bytes: 300_000 * self.step_bytes,
                disk_write_bytes: 120_000 * self.step_bytes,
                net_received_bytes: self.received_total,
                net_sent_bytes: self.sent_total,
                gpu_percent: Some((self.gpu_busiest + 1.0 + 3.0 * noise(ts, 901)).min(100.0)),
            },
            processes,
            others: OtherProcesses {
                process_count: 96,
                cpu_percent: tail_cpu,
                memory_bytes: 3 * 1024 * MB,
                gpu_percent: 0.3,
                gpu_memory_bytes: 90 * MB,
            },
        }
    }
}
