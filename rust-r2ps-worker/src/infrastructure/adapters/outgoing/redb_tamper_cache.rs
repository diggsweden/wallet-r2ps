use crate::application::port::outgoing::state_cache_port::TamperDetectionCache;
use redb::{Database, TableDefinition};
use std::path::Path;
use tracing::{error, info};

/// Table: device_id → version (as string)
const STATE_VERSIONS: TableDefinition<&str, &str> = TableDefinition::new("state_versions");
/// Table: partition (as string) → offset (as string)
const SNAPSHOT_OFFSETS: TableDefinition<&str, &str> = TableDefinition::new("snapshot_offsets");

/// On-disk tamper detection cache backed by redb.
/// Acts as a monotonic version witness — only moves forward.
pub struct RedbTamperCache {
    db: Database,
}

impl RedbTamperCache {
    pub fn new(path: &str) -> Result<Self, String> {
        let db = Database::create(Path::new(path)).map_err(|e| {
            error!("Failed to open redb database at {}: {:?}", path, e);
            format!("redb open failed: {}", e)
        })?;

        // Ensure tables exist
        {
            let write_txn = db
                .begin_write()
                .map_err(|e| format!("redb write txn: {}", e))?;
            {
                let _table = write_txn
                    .open_table(STATE_VERSIONS)
                    .map_err(|e| format!("redb open table: {}", e))?;
                let _table = write_txn
                    .open_table(SNAPSHOT_OFFSETS)
                    .map_err(|e| format!("redb open table: {}", e))?;
            }
            write_txn
                .commit()
                .map_err(|e| format!("redb commit: {}", e))?;
        }

        info!("Redb tamper detection cache opened at {}", path);
        Ok(Self { db })
    }
}

impl TamperDetectionCache for RedbTamperCache {
    fn get(&self, device_id: &str) -> Option<u64> {
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(STATE_VERSIONS).ok()?;
        let guard = table.get(device_id).ok()??;
        guard.value().parse().ok()
    }

    fn put(&self, device_id: &str, version: u64) {
        let value = version.to_string();
        match self.db.begin_write() {
            Ok(write_txn) => {
                {
                    match write_txn.open_table(STATE_VERSIONS) {
                        Ok(mut table) => {
                            if let Err(e) = table.insert(device_id, value.as_str()) {
                                error!("redb insert failed: {:?}", e);
                                return;
                            }
                        }
                        Err(e) => {
                            error!("redb open table failed: {:?}", e);
                            return;
                        }
                    }
                }
                if let Err(e) = write_txn.commit() {
                    error!("redb commit failed: {:?}", e);
                }
            }
            Err(e) => {
                error!("redb begin_write failed: {:?}", e);
            }
        }
    }

    fn get_snapshot_offset(&self, partition: i32) -> Option<i64> {
        let key = partition.to_string();
        let read_txn = self.db.begin_read().ok()?;
        let table = read_txn.open_table(SNAPSHOT_OFFSETS).ok()?;
        let guard = table.get(key.as_str()).ok()??;
        guard.value().parse().ok()
    }

    fn put_snapshot_offset(&self, partition: i32, offset: i64) {
        let key = partition.to_string();
        let value = offset.to_string();
        match self.db.begin_write() {
            Ok(write_txn) => {
                {
                    match write_txn.open_table(SNAPSHOT_OFFSETS) {
                        Ok(mut table) => {
                            if let Err(e) = table.insert(key.as_str(), value.as_str()) {
                                error!("redb insert snapshot offset failed: {:?}", e);
                                return;
                            }
                        }
                        Err(e) => {
                            error!("redb open table failed: {:?}", e);
                            return;
                        }
                    }
                }
                if let Err(e) = write_txn.commit() {
                    error!("redb commit failed: {:?}", e);
                }
            }
            Err(e) => {
                error!("redb begin_write failed: {:?}", e);
            }
        }
    }
}
