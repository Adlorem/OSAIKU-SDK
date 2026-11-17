//! Cached GPU administration facts. Reading these records never starts hardware.
pub const VERSION: u64 = 1;
pub const RESTART: u64 = 1;
pub const CONFLICT: u64 = 2;
pub const GPU: u64 = 4;
pub const GOP: u64 = 8;
/// The explicit CPU boot profile prevents activation of a selected package.
pub const BOOT_CPU: u64 = 16;
/// A persisted failure is not itself a request to reboot. The caller supplies
/// whether a current instance or a pending selection still requires restart.
pub fn failure_reason(failure: u64, restart: bool) -> &'static str {
    match (failure, restart) {
        (1, true) => "Przerwana próba startu; wymagany restart",
        (2, true) => "Przekroczono czas odpowiedzi; wymagany restart",
        (3 | 4, true) => "Błąd ochrony DMA lub IRQ; wymagany restart",
        (5 | 6, true) => "Zakończył się proces GPU; wymagany restart",
        (7, _) => "Zmiana polityki zaufania; wymagana ponowna weryfikacja",
        (8 | 9, true) => "Nieudane dopuszczenie GPU; wymagany restart",
        (1, false) => "Poprzednia próba GPU przerwana; sterownik wyłączony",
        (2, false) => "Poprzednia próba GPU: brak odpowiedzi; sterownik wyłączony",
        (3 | 4, false) => "Poprzednia próba GPU: błąd ochrony DMA lub IRQ; sterownik wyłączony",
        (5 | 6, false) => "Poprzednia próba: proces GPU zakończył pracę; sterownik wyłączony",
        (8 | 9, false) => "Poprzednia próba GPU: nieudane dopuszczenie; sterownik wyłączony",
        _ => "",
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Facts {
    pub version: u64,
    pub device: u64,
    pub connection: u64,
    pub catalog: u64,
    pub policy: u64,
    pub installed: u64,
    pub selected: u64,
    pub active: u64,
    pub trial: u64,
    pub failure: u64,
    pub flags: u64,
    pub backend: u64,
    pub renderer: u64,
    pub fence: u64,
    pub presents: u64,
    pub memory_budget: u64,
    pub dma_budget: u64,
    pub renderer_budget: u64,
    pub publisher: [u8; 32],
    pub package_id: [u8; 128],
    pub family: [u8; 64],
    pub selected_digest: [u8; 32],
    pub active_digest: [u8; 32],
}
impl Facts {
    pub const EMPTY: Self = Self { version: VERSION, device: 0, connection: 0,
        catalog: 0, policy: 0, installed: 0, selected: 0, active: 0, trial: 0,
        failure: 0, flags: 0, backend: 0, renderer: 0, fence: 0, presents: 0,
        memory_budget: 0, dma_budget: 0, renderer_budget: 0, publisher: [0;32],
        package_id: [0;128], family: [0;64], selected_digest: [0;32], active_digest: [0;32] };
    pub fn reason(&self) -> &'static str {
        if self.flags & CONFLICT != 0 { return "Konflikt wybranych sterowników"; }
        let failure = failure_reason(self.failure, self.flags & RESTART != 0);
        if !failure.is_empty() { return failure; }
        match () {
            _ if self.flags & GPU != 0 => "GPU",
            _ if self.trial != 0 => "Trwa próba GPU",
            _ if self.selected != 0 && self.flags & BOOT_CPU != 0 => "CPU/GOP · sterownik wybrany; profil CPU blokuje uruchomienie",
            _ if self.selected != 0 => "CPU/GOP · wybrana wersja oczekuje na restart",
            _ if self.installed != 0 => "CPU/GOP · brak wybranej wersji",
            _ => "CPU/GOP · brak zgodnego pakietu",
        }
    }
}
#[cfg(target_os = "none")]
pub fn query(index: u64) -> Result<Facts, isize> {
    let mut facts=Facts::EMPTY; let result:isize;
    unsafe { core::arch::asm!("int 0x80", inlateout("rax") super::SYSCALL as isize => result,
        in("rdi") index, in("rsi") &mut facts as *mut Facts, in("rdx") super::READ_GPU,
        in("r8") 0u64, in("r9") 0u64); }
    if result==0 && facts.version==VERSION { Ok(facts) } else { Err(result) }
}
