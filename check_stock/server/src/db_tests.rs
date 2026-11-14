use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn open(dir: &Path, readers: usize) -> Db {
    Db::open(&InventoryDb::in_dir(dir), readers).unwrap()
}

fn count_meta(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM srv_meta", [], |row| row.get(0))
}

#[tokio::test]
async fn open_prepares_core_schema_wal_and_server_tables() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 2);
    assert_eq!(db.path(), dir.path().join("inventory.db"));

    let (mode, server_version, has_core) = db
        .read(|conn| {
            let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            let version = migrations::current_version(conn)?;
            let core: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'inventory_cards'",
                [],
                |r| r.get(0),
            )?;
            Ok((mode, version, core == 1))
        })
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    assert_eq!(server_version, migrations::LATEST);
    assert!(has_core);
}

#[tokio::test]
async fn readers_cannot_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 1);
    let result = db
        .read(|conn| conn.execute("INSERT INTO srv_meta (key, value) VALUES ('a', 'b')", []))
        .await;
    assert!(matches!(result, Err(DbError::Sqlite(_))), "{result:?}");
    assert_eq!(db.read(count_meta).await.unwrap(), 0);
}

#[tokio::test]
async fn writes_are_visible_to_readers() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 2);
    db.write(|conn| migrations::set_meta(conn, "k", "v"))
        .await
        .unwrap();
    let value = db
        .read(|conn| migrations::get_meta(conn, "k"))
        .await
        .unwrap();
    assert_eq!(value.as_deref(), Some("v"));
}

#[tokio::test]
async fn reads_do_not_wait_for_an_open_write_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 1);
    let (in_tx_tx, in_tx_rx) = mpsc::channel::<()>();
    let (read_done_tx, read_done_rx) = mpsc::channel::<()>();

    let writer = {
        let db = db.clone();
        tokio::spawn(async move {
            db.write(move |conn| {
                let tx = conn.transaction()?;
                migrations::set_meta(&tx, "k", "v")?;
                in_tx_tx.send(()).unwrap();
                // Hold the transaction open until the read has finished.
                read_done_rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("read finished");
                tx.commit()
            })
            .await
        })
    };

    tokio::task::spawn_blocking(move || in_tx_rx.recv_timeout(Duration::from_secs(10)))
        .await
        .unwrap()
        .expect("writer is inside its transaction");
    // WAL: the reader sees the last committed state without waiting.
    let seen = tokio::time::timeout(Duration::from_secs(5), db.read(count_meta))
        .await
        .expect("read did not wait for the writer")
        .unwrap();
    assert_eq!(seen, 0);
    read_done_tx.send(()).unwrap();
    writer.await.unwrap().unwrap();
    assert_eq!(db.read(count_meta).await.unwrap(), 1);
}

#[tokio::test]
async fn more_reads_than_connections_all_finish() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 2);
    let reads: Vec<_> = (0..20)
        .map(|_| {
            let db = db.clone();
            tokio::spawn(async move { db.read(count_meta).await })
        })
        .collect();
    for read in reads {
        assert_eq!(read.await.unwrap().unwrap(), 0);
    }
}

#[tokio::test]
async fn a_panicking_read_does_not_break_the_pool() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 1);
    let result = db
        .read(|_| -> rusqlite::Result<()> { panic!("boom") })
        .await;
    assert!(matches!(result, Err(DbError::Task(_))), "{result:?}");
    // The lost connection is replaced.
    assert_eq!(db.read(count_meta).await.unwrap(), 0);
    assert_eq!(db.read(count_meta).await.unwrap(), 0);
}

#[tokio::test]
async fn a_panicking_write_rolls_back_and_the_writer_keeps_working() {
    let dir = tempfile::tempdir().unwrap();
    let db = open(dir.path(), 1);
    let result = db
        .write(|conn| -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            migrations::set_meta(&tx, "half", "done")?;
            panic!("crash in the middle of a change");
        })
        .await;
    assert!(matches!(result, Err(DbError::Task(_))), "{result:?}");
    assert_eq!(db.read(count_meta).await.unwrap(), 0, "rolled back");
    db.write(|conn| migrations::set_meta(conn, "k", "v"))
        .await
        .unwrap();
    assert_eq!(db.read(count_meta).await.unwrap(), 1);
}

#[test]
fn a_database_from_a_newer_server_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    {
        let db = open(dir.path(), 1);
        drop(db);
    }
    let conn = Connection::open(dir.path().join("inventory.db")).unwrap();
    conn.execute(
        "INSERT INTO srv_migrations (version, applied_at) VALUES (999, 'later')",
        [],
    )
    .unwrap();
    drop(conn);
    let result = Db::open(&InventoryDb::in_dir(dir.path()), 1);
    assert!(
        matches!(
            result,
            Err(DbError::Migration(MigrationError::NewerSchema {
                found: 999,
                ..
            }))
        ),
        "{:?}",
        result.err()
    );
}
