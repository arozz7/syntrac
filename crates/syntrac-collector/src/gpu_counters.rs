//! The Windows GPU performance counters (`GPU Engine` and `GPU Process Memory`), read through the Performance Data Helper.
//!
//! A thin adapter: it only moves the raw rows into `syntrac_core::GpuReading`; what they MEAN is decided by the pure, tested functions
//! in `syntrac_core::gpu`. A machine without the counters (an old driver, a virtual machine) simply gives no reading: `open` returns
//! `None` and the caller records "not available", never zero.

use syntrac_core::{CounterRow, GpuReading, GpuSource};
use windows::core::{w, PCWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhOpenQueryW,
    PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
};

/// How often the first call (which only asks how big the answer is) may have to be repeated because the answer grew in between.
const GROW_RETRIES: usize = 3;

pub struct PdhGpuSource {
    query: isize,
    engines: isize,
    memory: isize,
}

impl PdhGpuSource {
    /// Opens the two counters and takes a first reading (percentages are a difference between two readings, so the first one is
    /// only a starting point). `None` when this machine does not have them.
    pub fn open() -> Option<Self> {
        let mut query = 0_isize;
        // SAFETY: `query` is a live local that receives the handle; a null data source means the live counters.
        if unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } != 0 {
            return None;
        }
        let (mut engines, mut memory) = (0_isize, 0_isize);
        // SAFETY: the query handle was just opened; the counter paths are static wide strings; the out-parameters are live locals.
        let added = unsafe {
            PdhAddEnglishCounterW(query, w!(r"\GPU Engine(*)\Utilization Percentage"), 0, &mut engines) == 0
                && PdhAddEnglishCounterW(query, w!(r"\GPU Process Memory(*)\Dedicated Usage"), 0, &mut memory) == 0
        };
        // SAFETY: the query handle is valid.
        if !added || unsafe { PdhCollectQueryData(query) } != 0 {
            // SAFETY: the query handle is valid and not used again.
            unsafe { PdhCloseQuery(query) };
            return None;
        }
        Some(Self { query, engines, memory })
    }
}

impl Drop for PdhGpuSource {
    fn drop(&mut self) {
        // SAFETY: the query handle is valid, owned by this value, and not used after this.
        unsafe { PdhCloseQuery(self.query) };
    }
}

impl GpuSource for PdhGpuSource {
    fn read(&mut self) -> Option<GpuReading> {
        // SAFETY: the query handle is valid for as long as `self` lives.
        if unsafe { PdhCollectQueryData(self.query) } != 0 {
            return None;
        }
        Some(GpuReading { engines: rows_of(self.engines)?, memory: rows_of(self.memory)? })
    }
}

/// Every instance of one wildcard counter, as formatted doubles.
fn rows_of(counter: isize) -> Option<Vec<CounterRow>> {
    let mut size = 0_u32;
    let mut count = 0_u32;
    for _ in 0..GROW_RETRIES {
        // SAFETY: with no buffer the call only reports the size and the item count into the two live locals.
        let status = unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None) };
        if status == 0 {
            return Some(Vec::new());
        }
        if status != PDH_MORE_DATA {
            return None;
        }
        // 8-byte units, so the items inside are aligned.
        let mut buffer = vec![0_u64; (size as usize).div_ceil(8)];
        let items = buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        // SAFETY: the buffer is at least `size` bytes and aligned for the item type; on success it holds `count` items whose names
        // point inside the same buffer, which stays alive until after they are read below.
        let status =
            unsafe { PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) };
        if status == PDH_MORE_DATA {
            continue;
        }
        if status != 0 {
            return None;
        }
        // SAFETY: on success `items` points at `count` initialised items inside `buffer`.
        let slice = unsafe { std::slice::from_raw_parts(items, count as usize) };
        let mut rows = Vec::with_capacity(slice.len());
        for item in slice {
            let status = item.FmtValue.CStatus;
            if status != PDH_CSTATUS_VALID_DATA && status != PDH_CSTATUS_NEW_DATA {
                continue;
            }
            // SAFETY: the name points at a nul-terminated wide string inside `buffer`.
            let Ok(name) = (unsafe { item.szName.to_string() }) else { continue };
            // SAFETY: the format asked for doubles, and the status says the value is valid.
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            rows.push(CounterRow { instance: name, value });
        }
        return Some(rows);
    }
    None
}
