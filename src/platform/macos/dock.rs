//! macOS 运行时 Dock 栏图标注入
//!
//! 当直接运行独立二进制（如 `cargo run` 或从终端启动）时，进程不在 `.app` Bundle 内，
//! 系统不会读取 `Info.plist`，Dock 栏默认显示通用终端图标。
//! 此模块通过 Cocoa 原生 `NSApplication.setApplicationIconImage:` 动态注入高分辨率图标。

use objc::runtime::Object;
use objc::{class, msg_send, sel, sel_impl};

/// 在 macOS 上为当前进程设置 Dock 栏图标。
pub fn set_dock_icon() {
    static ICON_BYTES: &[u8] = include_bytes!("../../../assets/icon-512.png");
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];

        let data: *mut Object = msg_send![
            class!(NSData),
            dataWithBytes: ICON_BYTES.as_ptr() as *const std::ffi::c_void
            length: ICON_BYTES.len()
        ];
        if !data.is_null() {
            let image_alloc: *mut Object = msg_send![class!(NSImage), alloc];
            let image: *mut Object = msg_send![image_alloc, initWithData: data];
            if !image.is_null() {
                let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                if !app.is_null() {
                    let _: () = msg_send![app, setApplicationIconImage: image];
                }
                let _: () = msg_send![image, release];
            }
        }

        let _: () = msg_send![pool, drain];
    }
}
