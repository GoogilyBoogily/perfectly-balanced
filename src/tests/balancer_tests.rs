#[test]
fn test_disk_classification() {
    let target = 0.50;
    let tolerance = 0.10;

    // 80% utilized -> over
    assert!(0.80 > target + tolerance, "80% should be over the target+tolerance band");
    // 55% utilized -> above average
    assert!(0.55 > target && 0.55 <= target + tolerance, "55% should be above average but within tolerance");
    // 45% utilized -> below average
    assert!(0.45 < target && 0.45 >= target - tolerance, "45% should be below average but within tolerance");
    // 30% utilized -> under
    assert!(0.30 < target - tolerance, "30% should be under the target-tolerance band");
}
