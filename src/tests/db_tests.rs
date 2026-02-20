use crate::db::Database;

#[test]
fn test_open_and_migrate() {
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().unwrap();
    let conn = db.conn();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='disks'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}
