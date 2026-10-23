//! Canonical application manifest and component layout. Parsing is inspection;
//! only `verify` additionally proves publisher policy and exact content bytes.
use crate::*;
pub mod stream;
pub const MANIFEST_BYTES: usize = 512;
pub const DESCRIPTOR_BYTES: usize = 112;
pub const MAX_METADATA_BYTES: usize = 16 * 1024;
pub const MAX_RESOURCES: usize = 128;
pub const MAX_ELF_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_RESOURCE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_ICON_BYTES: u64 = 8 + 256 * 256 * 4;
pub const MAX_MEMORY_BYTES: u64 = 65535 * 4096;

/// Signed discovery metadata only; categories confer no runtime privileges.
pub mod category {
    pub const NAMES: [&str; 10] = [
        "other",
        "system",
        "utilities",
        "office",
        "graphics",
        "multimedia",
        "internet",
        "games",
        "development",
        "education",
    ];
}

/// Requests, never grants. Privileged system roles have no bit in this mask.
pub mod permission {
    pub const SELECTED_FILE_READ: u64 = 1;
    pub const SELECTED_FILE_WRITE: u64 = 2;
    pub const DESKTOP_APPEARANCE_WRITE: u64 = 4;
    pub const SYSTEM_METRICS_READ: u64 = 8;
    pub const KNOWN: u64 = 15;
}

#[derive(Clone, Copy, Debug)]
pub struct Component<'a> {
    record: &'a [u8],
}
impl<'a> Component<'a> {
    pub fn kind(&self) -> u16 {
        u16_at(self.record, 0)
    }
    pub fn offset(&self) -> u64 {
        u64_at(self.record, 8)
    }
    pub fn length(&self) -> u64 {
        u64_at(self.record, 16)
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.record[24..56].try_into().unwrap()
    }
    pub fn name(&self) -> &'a str {
        text(&self.record[56..112]).unwrap()
    }
}

#[derive(Debug)]
pub struct Manifest<'a> {
    bytes: &'a [u8],
    descriptors: &'a [u8],
}
impl<'a> Manifest<'a> {
    /// Parse metadata separately from large image bytes. Offsets are absolute
    /// within the envelope; exact component bytes are checked by the verifier.
    pub fn parse(bytes: &'a [u8], object_bytes: u64) -> Result<Self, Error> {
        if bytes.len() < MANIFEST_BYTES || bytes.len() > MAX_METADATA_BYTES {
            return Err(Error::Length);
        }
        let version = u32_at(bytes, 8);
        if &bytes[..8] != b"OSAMNF01" || !matches!(version, 1 | 2) {
            return Err(Error::Format);
        }
        if u32_at(bytes, 12) as usize != bytes.len() {
            return Err(Error::Length);
        }
        let compatibility_len = usize::from(u16_at(bytes, 60));
        let count = usize::from(u16_at(bytes, 62));
        if count == 0 || count > MAX_RESOURCES + 1 || compatibility_len > compatibility::MAX_BYTES {
            return Err(Error::Limit);
        }
        let table = (MANIFEST_BYTES + compatibility_len + 7) & !7;
        if table + count * DESCRIPTOR_BYTES != bytes.len() {
            return Err(Error::Length);
        }
        compatibility::Requirements::decode(
            &bytes[MANIFEST_BYTES..MANIFEST_BYTES + compatibility_len],
        )
        .map_err(|_| Error::Compatibility)?;
        zero(&bytes[MANIFEST_BYTES + compatibility_len..table])?;
        if version == 1 {
            zero(&bytes[448..512])?;
        } else {
            if usize::from(u16_at(bytes, 448)) >= category::NAMES.len() {
                return Err(Error::Manifest);
            }
            zero(&bytes[450..512])?;
        }
        let id = text(&bytes[64..192])?;
        if !identifier(id) || reserved_identity(id) {
            return Err(Error::Identity);
        }
        text(&bytes[192..320])?;
        for field in [&bytes[320..384], &bytes[384..448]] {
            if !text(field)?
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
            {
                return Err(Error::Identity);
            }
        }
        let entry = u16_at(bytes, 54);
        let width = u16_at(bytes, 56);
        let height = u16_at(bytes, 58);
        if u64_at(bytes, 16) == 0
            || u64_at(bytes, 24) & !permission::KNOWN != 0
            || !(4096..=MAX_MEMORY_BYTES).contains(&u64_at(bytes, 32))
            || !(1..=MAX_RESOURCE_BYTES).contains(&u64_at(bytes, 40))
            || !(1..=16).contains(&u16_at(bytes, 48))
            || !(1..=64).contains(&u16_at(bytes, 50))
            || u16_at(bytes, 52) > 16
            || !matches!(entry, 1 | 2)
            || (entry == 1 && (width != 0 || height != 0 || u16_at(bytes, 52) != 0))
            || (entry == 2
                && (!(64..=4096).contains(&width)
                    || !(64..=4096).contains(&height)
                    || u16_at(bytes, 52) == 0))
        {
            return Err(Error::Manifest);
        }
        let descriptors = &bytes[table..];
        let mut previous = "";
        let mut end = HEADER_BYTES as u64 + bytes.len() as u64;
        let mut icon = false;
        for (i, record) in descriptors.chunks_exact(DESCRIPTOR_BYTES).enumerate() {
            zero(&record[2..8])?;
            let name = text(&record[56..112])?;
            if !identifier(name) {
                return Err(Error::Identity);
            }
            let c = Component { record };
            let limit = match c.kind() {
                1 if i == 0 && name == "executable" => MAX_ELF_BYTES,
                2 if i > 0 => MAX_RESOURCE_BYTES,
                3 if i > 0 && !icon => {
                    icon = true;
                    MAX_ICON_BYTES
                }
                _ => return Err(Error::Manifest),
            };
            if i > 0 && (name == "executable" || name <= previous) {
                return Err(Error::Identity);
            }
            if i > 0 {
                previous = name;
            }
            if c.length() == 0 || c.length() > limit {
                return Err(Error::Limit);
            }
            let aligned = end.checked_add(7).ok_or(Error::Layout)? & !7;
            if c.offset() != aligned {
                return Err(Error::Layout);
            }
            end = c.offset().checked_add(c.length()).ok_or(Error::Layout)?;
        }
        if end != object_bytes || end > MAX_PACKAGE_BYTES {
            return Err(Error::Layout);
        }
        Ok(Self { bytes, descriptors })
    }
    pub fn id(&self) -> &'a str {
        text(&self.bytes[64..192]).unwrap()
    }
    pub fn name(&self) -> &'a str {
        text(&self.bytes[192..320]).unwrap()
    }
    pub fn version(&self) -> &'a str {
        text(&self.bytes[320..384]).unwrap()
    }
    pub fn category(&self) -> u16 {
        u16_at(self.bytes, 448)
    }
    pub fn category_name(&self) -> &'static str {
        category::NAMES[usize::from(self.category())]
    }
    pub fn release(&self) -> u64 {
        u64_at(self.bytes, 16)
    }
    pub fn memory_bytes(&self) -> u64 {
        u64_at(self.bytes, 32)
    }
    pub fn io_bytes(&self) -> u64 {
        u64_at(self.bytes, 40)
    }
    pub fn instances(&self) -> u16 {
        u16_at(self.bytes, 50)
    }
    pub fn permissions(&self) -> u64 {
        u64_at(self.bytes, 24)
    }
    pub fn compatibility(&self) -> compatibility::Requirements<'a> {
        compatibility::Requirements::decode(
            &self.bytes[MANIFEST_BYTES..MANIFEST_BYTES + usize::from(u16_at(self.bytes, 60))],
        )
        .unwrap()
    }
    pub fn components(&self) -> impl ExactSizeIterator<Item = Component<'a>> + '_ {
        self.descriptors
            .chunks_exact(DESCRIPTOR_BYTES)
            .map(|record| Component { record })
    }
}

pub fn reserved_identity(id: &str) -> bool {
    id == "osaiku"
        || id.starts_with("osaiku.")
        || id == "org.osaiku"
        || id.starts_with("org.osaiku.")
        || matches!(
            id,
            "initd"
                | "inputd"
                | "logd"
                | "consoled"
                | "compositord"
                | "policyd"
                | "serviced"
                | "assistantd"
                | "updated"
                | "modeld"
                | "themes"
                | "file-manager"
                | "system-manager"
                | "system-update"
                | "document"
                | "report-host"
                | "assistant-window"
        )
}

#[derive(Debug)]
pub struct Package<'a> {
    bytes: &'a [u8],
    manifest: Manifest<'a>,
    envelope: Envelope<'a>,
}
impl<'a> Package<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, Error> {
        let envelope = Envelope::parse(
            bytes.get(..HEADER_BYTES).ok_or(Error::Length)?,
            bytes.len() as u64,
            Domain::Application,
        )?;
        let prefix = bytes
            .get(HEADER_BYTES..HEADER_BYTES + MANIFEST_BYTES)
            .ok_or(Error::Length)?;
        let length = u32_at(prefix, 12) as usize;
        if length > MAX_METADATA_BYTES {
            return Err(Error::Limit);
        }
        let metadata = bytes
            .get(HEADER_BYTES..HEADER_BYTES + length)
            .ok_or(Error::Length)?;
        let manifest = Manifest::parse(metadata, bytes.len() as u64)?;
        let mut end = HEADER_BYTES + length;
        for component in manifest.components() {
            let start = component.offset() as usize;
            zero(&bytes[end..start])?;
            end = start + component.length() as usize;
            let content = &bytes[start..end];
            if digest(content) != component.sha256() {
                return Err(Error::Digest);
            }
            match component.kind() {
                1 => elf_header(content)?,
                3 => icon(content)?,
                _ => {}
            }
        }
        Ok(Self {
            bytes,
            manifest,
            envelope,
        })
    }
    pub fn manifest(&self) -> &Manifest<'a> {
        &self.manifest
    }
    pub fn publisher(&self) -> [u8; 32] {
        self.envelope.key_id()
    }
    pub fn verify(self, policy: &TrustPolicy<'_>) -> Result<VerifiedPackage<'a>, Error> {
        let sha256 = verify_bytes(self.bytes, Domain::Application, policy)?;
        Ok(VerifiedPackage {
            package: self,
            sha256,
        })
    }
}

#[derive(Debug)]
pub struct VerifiedPackage<'a> {
    package: Package<'a>,
    sha256: [u8; 32],
}
impl<'a> VerifiedPackage<'a> {
    pub fn manifest(&self) -> &Manifest<'a> {
        self.package.manifest()
    }
    pub fn publisher(&self) -> [u8; 32] {
        self.package.publisher()
    }
    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
    pub fn length(&self) -> u64 {
        self.package.bytes.len() as u64
    }
    /// Exact rollback approval is a host policy input, not a manifest flag.
    /// Persistence and user-consent binding are the catalog/broker's job.
    pub fn check_rollback(
        &self,
        highest_release: u64,
        approved_digest: Option<[u8; 32]>,
    ) -> Result<(), Error> {
        if self.manifest().release() < highest_release && approved_digest != Some(self.sha256) {
            Err(Error::Rollback)
        } else {
            Ok(())
        }
    }
}

fn elf_header(bytes: &[u8]) -> Result<(), Error> {
    // Only identify the target here. Segment/BSS/W^X/address validation belongs
    // to the shared process loader, not a second loader in the package parser.
    if bytes.len() < 64
        || &bytes[..7] != b"\x7fELF\x02\x01\x01"
        || u16_at(bytes, 16) != 2
        || u16_at(bytes, 18) != 62
        || u32_at(bytes, 20) != 1
        || u16_at(bytes, 52) != 64
    {
        Err(Error::Elf)
    } else {
        Ok(())
    }
}
fn icon(bytes: &[u8]) -> Result<(), Error> {
    icon_prefix(bytes, bytes.len() as u64)
}
fn icon_prefix(bytes: &[u8], length: u64) -> Result<(), Error> {
    if bytes.len() < 8 || &bytes[..4] != b"RGBA" {
        return Err(Error::Icon);
    }
    let width = usize::from(u16_at(bytes, 4));
    let height = usize::from(u16_at(bytes, 6));
    if !(1..=256).contains(&width)
        || !(1..=256).contains(&height)
        || length != (8 + width * height * 4) as u64
    {
        Err(Error::Icon)
    } else {
        Ok(())
    }
}
