//! Address-free, bounded system monitor view. The active observer owns its
//! sampling session; SystemMetricsRead grants no process control.
pub const MAX_PROCESSORS: usize = 128;
pub const SYSCALL: u64 = 339;
pub const ROWS: usize = 6;
pub const HISTORY: usize = 60;
pub const CPU_ROWS: usize = 6;
// Every online row reports local (user + kernel) execution / (execution + idle).
// The overview CPU value is the mean of rows with a valid sample.
pub const CPU_UNAVAILABLE: u64 = 10_001;
pub const TAB_OVERVIEW: u64 = 0;
pub const TAB_PROCESSORS: u64 = 1;
pub const WORDS: usize = 128;
pub const VIEW_BYTES: usize = WORDS * 8;
// Version 6 adds physical RAM and model accounting in the former tail padding.
pub const MAGIC: u64 = 0x4f5341494d4f4e36;
pub const MANAGER_ELEMENT_ID: u64 = 0x3106;

/// Offsets in the explicit little-endian wire layout; never machine addresses.
pub mod field {
    pub const SIGNATURE: usize = 0;
    pub const GENERATION: usize = 1;
    pub const ELAPSED: usize = 2;
    pub const CPU: usize = 3;
    pub const MEMORY_TOTAL: usize = 4;
    pub const MEMORY_AVAILABLE: usize = 5;
    pub const PROCESS_COUNT: usize = 6;
    pub const PAGE: usize = 7;
    pub const SAMPLE: usize = 8;
    pub const HISTORY_LEN: usize = 9;
    pub const ROW_COUNT: usize = 10;
    pub const COMPOSITOR_MEMORY: usize = 11;
    pub const SURFACE_MEMORY: usize = 12;
    pub const STORAGE_MEMORY: usize = 13;
    pub const SESSION_MEMORY: usize = 14;
    pub const QUARANTINED_MEMORY: usize = 15;
    pub const TAB: usize = 94;
    pub const CPU_MASK_LOW: usize = 95;
    pub const CPU_MASK_HIGH: usize = 96;
    pub const CPU_ROW_COUNT: usize = 97;
    pub const CPU_ONLINE_MASK_LOW: usize = 110;
    pub const CPU_ONLINE_MASK_HIGH: usize = 111;
    /// Monotonic time of this measurement, not of a repaint.
    pub const SAMPLE_TIME_MS: usize = 112;
    pub const CPU_VALID: usize = 113;
    /// Four byte counts each: weights, KV, workspace, transfer.
    pub const MODEL_ADMITTED: usize = 114;
    pub const MODEL_CHARGED: usize = 118;
    pub const MODEL_PEAK: usize = 122;
    /// Admitted capacity without physical backing; never used RAM.
    pub const MODEL_UNBACKED: usize = 126;
    /// Low physical pages funding lease metadata, model and transfer tables.
    pub const MODEL_METADATA: usize = 127;
}
pub const ROW_START: usize = 16;
pub const ROW_WORDS: usize = 8;
pub const HISTORY_START: usize = 64;
pub const HISTORY_END: usize = HISTORY_START + HISTORY / 2;
pub const CPU_ROW_START: usize = 98;
pub const CPU_ROW_WORDS: usize = 2;
pub const CPU_ROW_END: usize = CPU_ROW_START + CPU_ROWS * CPU_ROW_WORDS;
pub const CPU_METADATA_END: usize = field::CPU_VALID + 1;
pub mod row {
    pub const PID: usize = 0;
    pub const NAME_LOW: usize = 1;
    pub const NAME_HIGH: usize = 2;
    pub const STATE: usize = 3;
    pub const CPU: usize = 4;
    pub const MEMORY: usize = 5;
    pub const GENERATION: usize = 6;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonitorView {
    pub words: [u64; WORDS],
}
impl MonitorView {
    pub const EMPTY: Self = Self { words: [0; WORDS] };
    pub fn active(&self) -> bool {
        self.words[field::SIGNATURE] == MAGIC
    }
    pub fn cpu_mask(&self) -> u128 {
        u128::from(self.words[field::CPU_MASK_LOW])
            | (u128::from(self.words[field::CPU_MASK_HIGH]) << 64)
    }
    pub fn cpu_count(&self) -> usize {
        self.cpu_mask().count_ones() as usize
    }
    pub fn online_mask(&self) -> u128 {
        u128::from(self.words[field::CPU_ONLINE_MASK_LOW])
            | (u128::from(self.words[field::CPU_ONLINE_MASK_HIGH]) << 64)
    }
    pub fn online_count(&self) -> usize {
        self.online_mask().count_ones() as usize
    }
    pub fn cpu_id_at(&self, ordinal: usize) -> Option<usize> {
        let mut mask = self.cpu_mask();
        for _ in 0..ordinal {
            mask &= mask.checked_sub(1)?;
        }
        (mask != 0).then(|| mask.trailing_zeros() as usize)
    }
    pub fn canonical(&self) -> bool {
        if !self.active() {
            return *self == Self::EMPTY;
        }
        let count = self.words[field::ROW_COUNT] as usize;
        let tab = self.words[field::TAB];
        let cpu_count = self.cpu_count();
        let cpu_page = if tab == TAB_PROCESSORS {
            self.words[field::PAGE] as usize
        } else {
            0
        };
        let expected_cpu_rows = cpu_count
            .saturating_sub(cpu_page.saturating_mul(CPU_ROWS))
            .min(CPU_ROWS);
        self.words[field::GENERATION] != 0
            && self.words[field::SAMPLE] != 0
            && self.words[field::CPU] <= 10_000
            && self.words[field::CPU_VALID] <= 1
            && self.words[field::MEMORY_AVAILABLE] <= self.words[field::MEMORY_TOTAL]
            && self.words[field::HISTORY_LEN] <= HISTORY as u64
            && self.words[field::ROW_COUNT] <= ROWS as u64
            && self.words[field::PROCESS_COUNT] >= self.words[field::ROW_COUNT]
            && matches!(tab, TAB_OVERVIEW | TAB_PROCESSORS)
            && self.cpu_mask() & 1 != 0
            && self.online_mask() & 1 != 0
            && self.online_mask() & !self.cpu_mask() == 0
            && cpu_count <= MAX_PROCESSORS
            && self.words[field::PAGE]
                <= if tab == TAB_PROCESSORS {
                    (cpu_count.saturating_sub(1) / CPU_ROWS) as u64
                } else {
                    self.words[field::PROCESS_COUNT].saturating_sub(1) / ROWS as u64
                }
            && (0..count).all(|index| {
                let base = ROW_START + index * ROW_WORDS;
                self.words[base + row::PID] != 0
                    && self.words[base + row::PID] <= u64::from(u32::MAX)
                    && (self.words[base + row::PID] <= crate::process::FIXED_SERVICE_COUNT as u64
                        || self.words[base + row::GENERATION] != 0)
                    && self.words[base + row::GENERATION] <= u64::from(u32::MAX)
                    && self.words[base + row::CPU] <= 10_000
                    && self.words[base + 7] == 0
                    && live_state(self.words[base + row::STATE])
            })
            && self.words[ROW_START + count * ROW_WORDS..HISTORY_START]
                .iter()
                .all(|word| *word == 0)
            && (0..HISTORY).all(|index| {
                let (cpu, memory) = self.history(index);
                if index < self.words[field::HISTORY_LEN] as usize {
                    cpu <= 10_000 && memory <= 10_000
                } else {
                    cpu == 0 && memory == 0
                }
            })
            && self.words[field::CPU_ROW_COUNT] == expected_cpu_rows as u64
            && (0..CPU_ROWS).all(|index| {
                let base = CPU_ROW_START + index * CPU_ROW_WORDS;
                if index >= expected_cpu_rows {
                    return self.words[base] == 0 && self.words[base + 1] == 0;
                }
                let id = self.cpu_id_at(cpu_page * CPU_ROWS + index);
                id == Some(self.words[base] as usize)
                    && self.words[base + 1] <= CPU_UNAVAILABLE
                    && (self.online_mask() & (1u128 << self.words[base]) != 0
                        || self.words[base + 1] == CPU_UNAVAILABLE)
            })
            && self.model_memory_canonical()
    }
    fn model_memory_canonical(&self) -> bool {
        let mut charged = 0u64;
        let mut unbacked = 0u64;
        for i in 0..4 {
            let admitted = self.words[field::MODEL_ADMITTED + i];
            let used = self.words[field::MODEL_CHARGED + i];
            let peak = self.words[field::MODEL_PEAK + i];
            if used > peak
                || peak > admitted
                || admitted > self.words[field::MEMORY_TOTAL]
                || [admitted, used, peak].iter().any(|n| n % 4096 != 0)
            {
                return false;
            }
            let Some(sum) = charged.checked_add(used) else {
                return false;
            };
            charged = sum;
            let Some(sum) = unbacked.checked_add(admitted - used) else {
                return false;
            };
            unbacked = sum;
        }
        let metadata = self.words[field::MODEL_METADATA];
        metadata % 4096 == 0
            && unbacked == self.words[field::MODEL_UNBACKED]
            && unbacked <= self.words[field::MEMORY_AVAILABLE]
            && charged.checked_add(metadata).is_some_and(|n| {
                n <= self.words[field::MEMORY_TOTAL]
                    .saturating_sub(self.words[field::MEMORY_AVAILABLE])
            })
    }
    pub fn history(&self, index: usize) -> (u16, u16) {
        let value = self.words[HISTORY_START + index / 2] >> ((index % 2) * 32);
        (value as u16, (value >> 16) as u16)
    }
    pub fn set_history(&mut self, index: usize, cpu: u16, memory: u16) {
        let shift = (index % 2) * 32;
        let slot = &mut self.words[HISTORY_START + index / 2];
        *slot = (*slot & !(0xffff_ffffu64 << shift))
            | ((u64::from(cpu) | (u64::from(memory) << 16)) << shift);
    }
}

pub fn live_state(state: u64) -> bool {
    use crate::process::ProcessState;
    [
        ProcessState::Ready,
        ProcessState::Running,
        ProcessState::Waiting,
        ProcessState::Runnable,
        ProcessState::BlockedOnIpc,
        ProcessState::BlockedOnReply,
        ProcessState::BlockedOnStorage,
        ProcessState::BlockedOnGpu,
    ]
    .iter()
    .any(|value| state == *value as u64)
}

/// Saturating ratio avoids overflow and never labels unavailable timing as 0%.
pub fn basis_points(part: u64, whole: u64) -> Option<u16> {
    (whole != 0).then(|| ((u128::from(part) * 10_000 / u128::from(whole)).min(10_000)) as u16)
}

/// Read the current sample, or a page of that exact sample. The kernel binds
/// the observation session to this process; this API cannot control processes.
pub fn read_with(
    page: Option<(u64, u64)>,
    mut exchange: impl FnMut(&mut MonitorView, u64, u64) -> isize,
) -> Result<MonitorView, isize> {
    use crate::abi::*;
    let (encoded, sample) = match page {
        None => (0, 0),
        Some((page, sample)) => (
            page.checked_add(1)
                .filter(|_| sample != 0)
                .ok_or(SYSCALL_ERR_INVALID_STATE)?,
            sample,
        ),
    };
    let mut view = MonitorView::EMPTY;
    let code = exchange(&mut view, encoded, sample);
    if code != 0 {
        return Err(code);
    }
    if !view.active()
        || !view.canonical()
        || page.is_some_and(|(p, s)| view.words[field::PAGE] != p || view.words[field::SAMPLE] != s)
    {
        return Err(SYSCALL_ERR_INVALID_STATE);
    }
    Ok(view)
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn read(page: Option<(u64, u64)>) -> Result<MonitorView, isize> {
    read_with(page, |view, page, sample| unsafe {
        crate::raw::syscall(
            SYSCALL,
            [view as *mut MonitorView as u64, page, sample, 0, 0],
        ) as isize
    })
}

/// Select this caller's observation tab/page. Never a process-control request.
pub fn select_with(
    operation: u64,
    value: u64,
    mut exchange: impl FnMut(u64, u64) -> isize,
) -> Result<(), isize> {
    if !matches!((operation, value), (0, 0..=1) | (1, 1 | u64::MAX)) {
        return Err(crate::abi::SYSCALL_ERR_INVALID_STATE);
    }
    match exchange(operation, value) {
        0 => Ok(()),
        code => Err(code),
    }
}
#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub fn select(operation: u64, value: u64) -> Result<(), isize> {
    select_with(operation, value, |op, value| unsafe {
        crate::raw::syscall(367, [op, value, 0, 0, 0]) as isize
    })
}
