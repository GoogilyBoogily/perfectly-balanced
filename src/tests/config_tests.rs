use crate::config::AppConfig;

#[test]
fn test_parse_ini() {
    let mut config = AppConfig::default();
    let ini = r#"
PORT="7092"
SCAN_THREADS="4"
SLIDER_ALPHA="0.75"
EXCLUDED_DISKS="disk3,cache"
WARN_PARITY_CHECK="yes"
"#;
    config.parse_ini(ini);
    assert_eq!(config.port, 7092, "PORT should be parsed from INI");
    assert_eq!(config.scan_threads, 4, "SCAN_THREADS should be parsed from INI");
    assert!((config.slider_alpha - 0.75).abs() < f64::EPSILON, "SLIDER_ALPHA should be 0.75");
    assert!(config.excluded_disks.contains("disk3"), "disk3 should be in excluded disks");
    assert!(config.excluded_disks.contains("cache"), "cache should be in excluded disks");
    assert!(config.warn_parity_check, "WARN_PARITY_CHECK should be true");
}

#[test]
fn test_default_config_validates() {
    let config = AppConfig::default();
    assert!(config.validate().is_ok(), "default config should pass validation");
}

#[test]
fn test_catalog_path_default_is_tmpfs() {
    let config = AppConfig::default();
    assert!(config.db_path.starts_with("/tmp/"), "Default db_path should be in /tmp");
}

#[test]
fn test_catalog_path_override() {
    let mut config = AppConfig::default();
    config.parse_ini(r#"CATALOG_PATH="/mnt/cache/appdata/perfectly-balanced/catalog.db""#);
    assert_eq!(config.db_path, "/mnt/cache/appdata/perfectly-balanced/catalog.db");
}

#[test]
fn test_catalog_path_empty_keeps_default() {
    let mut config = AppConfig::default();
    let default_path = config.db_path.clone();
    config.parse_ini(r#"CATALOG_PATH="""#);
    assert_eq!(config.db_path, default_path);
}
