//! 一次性验证 GPU / 电池采集是否真的读到了东西（不是产品入口）。
fn main() {
    let t = std::time::Instant::now();
    let gpu = quick_cleaner::platform::read_gpu();
    let gpu_ms = t.elapsed();
    let t = std::time::Instant::now();
    let bat = quick_cleaner::platform::read_battery();
    let bat_ms = t.elapsed();
    println!("GPU  ({gpu_ms:?}) = {gpu:?}");
    println!("BATT ({bat_ms:?}) = {bat:?}");

    // 每 2 秒采一拍，跑 20 次看有没有句柄泄漏导致的劣化。
    let t = std::time::Instant::now();
    for _ in 0..20 {
        let _ = quick_cleaner::platform::read_gpu();
        let _ = quick_cleaner::platform::read_battery();
    }
    println!("20 轮合计 {:?}", t.elapsed());
}
