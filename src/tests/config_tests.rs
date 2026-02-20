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
    assert_eq!(config.port, 7092);
    assert_eq!(config.scan_threads, 4);
    assert!((config.slider_alpha - 0.75).abs() < f64::EPSILON);
    assert!(config.excluded_disks.contains("disk3"));
    assert!(config.excluded_disks.contains("cache"));
    assert!(config.warn_parity_check);
}

#[test]
fn test_default_config_validates() {
    let config = AppConfig::default();
    assert!(config.validate().is_ok());
}
