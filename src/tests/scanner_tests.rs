use crate::scanner::validation::validate_path;

#[test]
fn test_validate_path_rejects_fuse() {
    assert!(validate_path("/mnt/user/some/file").is_err(), "FUSE /mnt/user/ should be rejected");
    assert!(validate_path("/mnt/user0/some/file").is_err(), "FUSE /mnt/user0/ should be rejected");
    assert!(validate_path("/mnt/disk1/some/file").is_ok(), "/mnt/disk1/ should be allowed");
    assert!(validate_path("/mnt/cache/some/file").is_ok(), "/mnt/cache/ should be allowed");
}

#[test]
fn test_validate_path_allows_direct_disks() {
    assert!(validate_path("/mnt/disk1/movies/test.mkv").is_ok(), "disk1 path should be valid");
    assert!(validate_path("/mnt/disk25/data/file.txt").is_ok(), "disk25 path should be valid");
    assert!(validate_path("/mnt/cache/appdata/").is_ok(), "cache path should be valid");
}
