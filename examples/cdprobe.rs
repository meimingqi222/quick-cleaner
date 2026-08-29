//! 对比两种取 cdhash 的方式在「磁盘文件被换掉」之后各自返回什么。
#[cfg(target_os = "macos")]
mod probe {
    use std::ffi::c_void;
    extern "C" {
        fn csops(pid: i32, ops: u32, useraddr: *mut c_void, usersize: usize) -> i32;
    }
    const CS_OPS_CDHASH: u32 = 5;
    pub fn kernel_cdhash() -> String {
        let mut buf = [0u8; 20];
        let rc = unsafe {
            csops(
                std::process::id() as i32,
                CS_OPS_CDHASH,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
            )
        };
        if rc != 0 {
            return format!("<csops errno {}>", std::io::Error::last_os_error());
        }
        buf.iter().map(|b| format!("{b:02x}")).collect()
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    {
        let sec = || {
            quick_cleaner::platform::macos::codesign::self_cdhash()
                .unwrap_or_else(|| "<none>".into())
        };
        println!(
            "before  SecCode={}  csops={}",
            sec(),
            probe::kernel_cdhash()
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();
        std::thread::sleep(std::time::Duration::from_secs(6));
        println!(
            "after   SecCode={}  csops={}",
            sec(),
            probe::kernel_cdhash()
        );
    }
}
