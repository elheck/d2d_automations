use super::*;

#[test]
fn acquire_creates_the_folder_and_lock_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("nested").join("data");
    let lock = DataDirLock::acquire(&dir).unwrap();
    assert!(dir.is_dir());
    assert_eq!(lock.path(), dir.join(LOCK_FILE));
    assert!(lock.path().exists());
}

#[test]
fn a_second_holder_is_refused_until_the_first_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let first = DataDirLock::acquire(tmp.path()).unwrap();
    match DataDirLock::acquire(tmp.path()) {
        Err(LockError::InUse(dir)) => assert_eq!(dir, tmp.path()),
        other => panic!("expected InUse, got {other:?}"),
    }
    drop(first);
    assert!(DataDirLock::acquire(tmp.path()).is_ok());
}

#[test]
fn folders_are_locked_independently() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let _a = DataDirLock::acquire(a.path()).unwrap();
    assert!(DataDirLock::acquire(b.path()).is_ok());
}

#[test]
fn messages_name_the_folder() {
    let message = LockError::InUse(PathBuf::from("/srv/data")).to_string();
    assert!(message.contains("/srv/data"), "{message}");
    assert!(message.contains("stop it first"), "{message}");
}
