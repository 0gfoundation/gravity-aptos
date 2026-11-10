// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::{tests::suite, KVStorage, OnDiskStorage, Storage};
use aptos_temppath::TempPath;

#[test]
fn on_disk() {
    let path_buf = TempPath::new().path().to_path_buf();
    let mut storage = Storage::from(OnDiskStorage::new(path_buf));
    suite::execute_all_storage_tests(&mut storage);
}

#[test]
fn on_disk_write_is_atomic_and_leaves_no_temp_file() {
    let dir = TempPath::new();
    dir.create_as_dir().unwrap();
    let file_path = dir.path().join("secure_storage.json");

    let mut storage = OnDiskStorage::new(file_path.clone());
    for round in 0..10u64 {
        storage.set("last_voted_round", round).unwrap();
        assert_eq!(storage.get::<u64>("last_voted_round").unwrap().value, round);
    }
    storage.set("other", "value".to_string()).unwrap();

    // Only the target file exists in the directory: the temp file was renamed over it.
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries, vec![file_path.clone()]);

    // A fresh instance reading the same file sees the latest persisted values.
    let reopened = OnDiskStorage::new(file_path);
    assert_eq!(reopened.get::<u64>("last_voted_round").unwrap().value, 9);
    assert_eq!(reopened.get::<String>("other").unwrap().value, "value");
}
