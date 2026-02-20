use crate::scanner::validation::validate_path;

#[test]
fn test_validate_path_rejects_fuse() {
    assert!(validate_path("/mnt/user/some/file").is_err());
    assert!(validate_path("/mnt/user0/some/file").is_err());
    assert!(validate_path("/mnt/disk1/some/file").is_ok());
    assert!(validate_path("/mnt/cache/some/file").is_ok());
}

#[test]
fn test_validate_path_allows_direct_disks() {
    assert!(validate_path("/mnt/disk1/movies/test.mkv").is_ok());
    assert!(validate_path("/mnt/disk25/data/file.txt").is_ok());
    assert!(validate_path("/mnt/cache/appdata/").is_ok());
}
