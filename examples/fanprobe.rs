//! 风扇 SMC 键探测。
//!
//!   cargo run --features fanprobe --example fanprobe            只读转储
//!   sudo ./target/debug/examples/fanprobe --raise               实测抬高最小转速（自恢复）
fn main() {
    #[cfg(target_os = "macos")]
    {
        use quick_cleaner::platform::macos::status;
        if std::env::args().any(|a| a == "--chain") {
            // 复现 UI 点「降温」时走的那条链，看每一步各返回什么。
            use quick_cleaner::core::status::FanMode;
            let m = FanMode::Percent(60);
            println!(
                "1) 进程内直写 set_fan_mode(60%)  -> {:?}",
                status::set_fan_mode(m)
            );
            println!(
                "2) fan_helper_installed()          -> {}",
                quick_cleaner::platform::fan_helper_installed()
            );
            println!(
                "3) elevated_fan_control(60%)       -> {:?}",
                quick_cleaner::platform::elevated_fan_control(m)
            );
            return;
        }
        if std::env::args().any(|a| a == "--raise") {
            status::probe_raise_min_speed(0.60, 12);
        } else {
            status::dump_fan_keys();
        }
    }
}
