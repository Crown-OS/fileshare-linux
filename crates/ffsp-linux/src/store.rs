use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ffsp_protocol::device::{Capabilities, DeviceClass, DeviceId};
use ffsp_protocol::link::{LinkRecord, LinkSecret, LinkStore, LinkStoreError, StaticIdentity};

use crate::error::{LinuxError, Result};

const RECORD_LEN: usize = 32 + 32 + 32 + 2 + 1 + 2;

/// Pairing records on disk, readable only by their owner.
///
/// Not the desktop keyring, deliberately. Secret Service wants a session bus and an unlocked
/// collection, neither of which exists when a benchmark run is driven over SSH — and a rig that
/// cannot re-pair unattended is a rig that stops measuring.
#[derive(Debug)]
pub struct FileLinkStore {
    path: PathBuf,
    records: Mutex<BTreeMap<DeviceId, LinkRecord>>,
}

impl FileLinkStore {
    /// Opens, or creates, the store under the user's data directory.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let records = if path.exists() {
            decode(&std::fs::read(&path)?)?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path,
            records: Mutex::new(records),
        })
    }

    /// The conventional location, `$XDG_DATA_HOME/crownos/fileshare/links`.
    pub fn default_path() -> PathBuf {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("crownos/fileshare/links")
    }

    /// This device's long-term identity, generated once and kept beside the links.
    pub fn identity(&self) -> Result<StaticIdentity> {
        let path = self.path.with_extension("identity");
        if let Ok(bytes) = std::fs::read(&path)
            && bytes.len() == 64
        {
            let mut secret = [0u8; 32];
            let mut public = [0u8; 32];
            secret.copy_from_slice(bytes.get(..32).unwrap_or_default());
            public.copy_from_slice(bytes.get(32..).unwrap_or_default());
            return Ok(StaticIdentity::new(secret, public));
        }
        let identity =
            ffsp_protocol::link::generate_identity().map_err(|error| LinuxError::Unavailable {
                medium: ffsp_protocol::medium::Medium::Loopback,
                reason: error.to_string(),
            })?;
        let mut bytes = Vec::with_capacity(64);
        bytes.extend_from_slice(identity.secret_key());
        bytes.extend_from_slice(identity.public_key());
        write_private(&path, &bytes)?;
        Ok(identity)
    }

    fn persist(&self, records: &BTreeMap<DeviceId, LinkRecord>) -> Result<()> {
        write_private(&self.path, &encode(records))
    }
}

impl LinkStore for FileLinkStore {
    fn load(&self, device: &DeviceId) -> std::result::Result<Option<LinkRecord>, LinkStoreError> {
        Ok(guard(&self.records)?.get(device).cloned())
    }

    fn store(&self, record: &LinkRecord) -> std::result::Result<(), LinkStoreError> {
        let mut records = guard(&self.records)?;
        records.insert(record.device, record.clone());
        self.persist(&records)
            .map_err(|error| LinkStoreError::Unavailable(error.to_string()))
    }

    fn remove(&self, device: &DeviceId) -> std::result::Result<(), LinkStoreError> {
        let mut records = guard(&self.records)?;
        records.remove(device);
        self.persist(&records)
            .map_err(|error| LinkStoreError::Unavailable(error.to_string()))
    }

    fn all(&self) -> std::result::Result<Vec<LinkRecord>, LinkStoreError> {
        Ok(guard(&self.records)?.values().cloned().collect())
    }
}

fn guard(
    records: &Mutex<BTreeMap<DeviceId, LinkRecord>>,
) -> std::result::Result<std::sync::MutexGuard<'_, BTreeMap<DeviceId, LinkRecord>>, LinkStoreError>
{
    records
        .lock()
        .map_err(|_| LinkStoreError::Unavailable("the link store lock was poisoned".to_owned()))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
    }
    // Replacing in one step, so a crash mid-write cannot leave a half-written secret behind.
    std::fs::rename(&temporary, path)?;
    Ok(())
}

fn encode(records: &BTreeMap<DeviceId, LinkRecord>) -> Vec<u8> {
    let mut out = Vec::with_capacity(records.len() * (RECORD_LEN + 32));
    for record in records.values() {
        out.extend_from_slice(record.device.as_bytes());
        out.extend_from_slice(&record.public_key);
        out.extend_from_slice(record.secret.as_bytes());
        out.extend_from_slice(&record.capabilities.bits().to_le_bytes());
        out.push(record.class.as_byte());
        let name = record.name.as_bytes();
        let len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(name.get(..len as usize).unwrap_or_default());
    }
    out
}

fn decode(bytes: &[u8]) -> Result<BTreeMap<DeviceId, LinkRecord>> {
    let mut records = BTreeMap::new();
    let mut offset = 0usize;
    let mut ordinal = 0usize;
    while offset < bytes.len() {
        let corrupt = || LinuxError::CorruptStore { ordinal };
        let take = |from: usize, len: usize| bytes.get(from..from + len).ok_or_else(corrupt);

        let mut public_key = [0u8; 32];
        let mut secret = [0u8; 32];
        public_key.copy_from_slice(take(offset + 32, 32)?);
        secret.copy_from_slice(take(offset + 64, 32)?);
        let capability_bits = u16::from_le_bytes(
            take(offset + 96, 2)?
                .try_into()
                .map_err(|_| LinuxError::CorruptStore { ordinal })?,
        );
        let class = DeviceClass::from_byte(*take(offset + 98, 1)?.first().ok_or_else(corrupt)?)
            .map_err(|_| LinuxError::CorruptStore { ordinal })?;
        let name_len = usize::from(u16::from_le_bytes(
            take(offset + 99, 2)?
                .try_into()
                .map_err(|_| LinuxError::CorruptStore { ordinal })?,
        ));
        let name = String::from_utf8(take(offset + RECORD_LEN, name_len)?.to_vec())
            .map_err(|_| LinuxError::CorruptStore { ordinal })?;

        let record = LinkRecord::new(
            public_key,
            name,
            class,
            Capabilities::from_bits_lossy(capability_bits),
            LinkSecret::from_bytes(secret),
        );
        records.insert(record.device, record);
        offset += RECORD_LEN + name_len;
        ordinal += 1;
    }
    Ok(records)
}
