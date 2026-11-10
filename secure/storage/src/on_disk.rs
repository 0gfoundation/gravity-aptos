// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{CryptoKVStorage, Error, GetResponse, KVStorage};
use aptos_logger::info;
use aptos_temppath::TempPath;
use aptos_time_service::{TimeService, TimeServiceTrait};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};

/// OnDiskStorage represents a key value store that is persisted to the local filesystem and is
/// intended for single threads (or must be wrapped by a Arc<RwLock<>>). This provides no permission
/// checks and simply offers a proof of concept to unblock building of applications without more
/// complex data stores. Internally, it reads and writes all data to a file, which means that it
/// must make copies of all key material which violates the code base. It violates it because
/// the anticipation is that data stores would securely handle key material. This should not be used
/// in production.
pub struct OnDiskStorage {
    file_path: PathBuf,
    file_dir: PathBuf,
    temp_path: TempPath,
    time_service: TimeService,
}

impl OnDiskStorage {
    pub fn new(file_path: PathBuf) -> Self {
        info!("OnDiskStorage::new: {:?}", file_path);
        Self::new_with_time_service(file_path, TimeService::real())
    }

    fn new_with_time_service(file_path: PathBuf, time_service: TimeService) -> Self {
        if !file_path.exists() {
            File::create(&file_path)
                .unwrap_or_else(|_| panic!("Unable to create storage at path: {:?}", file_path));
        }

        // The parent will be one when only a filename is supplied. Therefore use the current
        // working directory provided by PathBuf::new().
        let file_dir = file_path
            .parent()
            .map_or_else(PathBuf::new, |p| p.to_path_buf());

        Self {
            file_path,
            temp_path: TempPath::new_with_temp_dir(file_dir.clone()),
            file_dir,
            time_service,
        }
    }

    fn read(&self) -> Result<HashMap<String, Value>, Error> {
        let mut file = File::open(&self.file_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        if contents.is_empty() {
            return Ok(HashMap::new());
        }
        let data = serde_json::from_str(&contents)?;
        aptos_logger::debug!("OnDiskStorage::read: path {:?}, data {:?}", self.file_path, data);
        Ok(data)
    }

    /// Atomically and durably replaces the file contents. This file holds consensus safety data
    /// (e.g. last_voted_round); if a write could be lost on power failure, a restarted validator
    /// might sign a vote conflicting with one it already sent. So the new contents are written to
    /// a temp file in the same directory and fsynced, renamed over the target (atomic, so readers
    /// never see a partial file), and then the directory is fsynced so the rename itself survives
    /// a crash.
    fn write(&self, data: &HashMap<String, Value>) -> Result<(), Error> {
        let contents = serde_json::to_vec(data)?;
        let mut file = File::create(self.temp_path.path())?;
        file.write_all(&contents)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&self.temp_path, &self.file_path)?;
        self.sync_dir()?;
        Ok(())
    }

    #[cfg(unix)]
    fn sync_dir(&self) -> Result<(), Error> {
        // An empty parent means the file lives in the current working directory.
        let dir = if self.file_dir.as_os_str().is_empty() {
            std::path::Path::new(".")
        } else {
            self.file_dir.as_path()
        };
        File::open(dir)?.sync_all()?;
        Ok(())
    }

    /// Directories cannot be opened for fsync on non-unix platforms; the rename is still atomic.
    #[cfg(not(unix))]
    fn sync_dir(&self) -> Result<(), Error> {
        Ok(())
    }
}

impl KVStorage for OnDiskStorage {
    fn available(&self) -> Result<(), Error> {
        Ok(())
    }

    fn get<V: DeserializeOwned>(&self, key: &str) -> Result<GetResponse<V>, Error> {
        let mut data = self.read()?;
        data.remove(key)
            .ok_or_else(|| Error::KeyNotSet(key.to_string()))
            .and_then(|value| serde_json::from_value(value).map_err(|e| e.into()))
    }

    fn set<V: Serialize>(&mut self, key: &str, value: V) -> Result<(), Error> {
        let now = self.time_service.now_secs();
        let mut data = self.read()?;
        data.insert(
            key.to_string(),
            serde_json::to_value(&GetResponse::new(value, now))?,
        );
        self.write(&data)
    }

    #[cfg(any(test, feature = "testing"))]
    fn reset_and_clear(&mut self) -> Result<(), Error> {
        self.write(&HashMap::new())
    }
}

impl CryptoKVStorage for OnDiskStorage {}
