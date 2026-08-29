//! 实测：自己调 Authorization Services 申请 `system.privilege.admin` 时，
//! 系统弹出的授权框会不会提供 Touch ID。
//!
//! 与现在走的 `osascript ... with administrator privileges` 的区别在于
//! 「谁来弹这个框」：osascript 是让它自己去申请，我们看不到也控制不了；
//! 这里是本进程直接申请，因此还能自定义提示文案。
//!
//!   cargo run --features authprobe --example authprobe
#[cfg(target_os = "macos")]
mod probe {
    use std::ffi::{c_char, c_void, CString};

    #[repr(C)]
    struct AuthorizationItem {
        name: *const c_char,
        value_length: usize,
        value: *mut c_void,
        flags: u32,
    }
    #[repr(C)]
    struct AuthorizationItemSet {
        count: u32,
        items: *mut AuthorizationItem,
    }

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn AuthorizationCreate(
            rights: *const AuthorizationItemSet,
            environment: *const AuthorizationItemSet,
            flags: u32,
            authorization: *mut *mut c_void,
        ) -> i32;
        fn AuthorizationCopyRights(
            authorization: *mut c_void,
            rights: *const AuthorizationItemSet,
            environment: *const AuthorizationItemSet,
            flags: u32,
            authorized_rights: *mut *mut AuthorizationItemSet,
        ) -> i32;
        fn AuthorizationFree(authorization: *mut c_void, flags: u32) -> i32;
    }

    const INTERACTION_ALLOWED: u32 = 1 << 0;
    const EXTEND_RIGHTS: u32 = 1 << 1;
    const DESTROY_RIGHTS: u32 = 1 << 3;

    pub fn run() {
        let right = CString::new("system.privilege.admin").unwrap();
        let prompt_key = CString::new("prompt").unwrap();
        // 自定义提示：现在那个框写的是「osascript 想要进行更改」，用户看不出
        // 是谁、要干什么。自己申请就能把话说清楚。
        let mut prompt = String::from("QuickCleaner 需要安装风扇控制组件。\n\n").into_bytes();
        prompt.extend_from_slice("这是本次唯一一次授权，之后切换风扇档位不再需要密码。".as_bytes());

        let mut items = [AuthorizationItem {
            name: right.as_ptr(),
            value_length: 0,
            value: std::ptr::null_mut(),
            flags: 0,
        }];
        let rights = AuthorizationItemSet {
            count: 1,
            items: items.as_mut_ptr(),
        };
        let mut env_items = [AuthorizationItem {
            name: prompt_key.as_ptr(),
            value_length: prompt.len(),
            value: prompt.as_mut_ptr() as *mut c_void,
            flags: 0,
        }];
        let env = AuthorizationItemSet {
            count: 1,
            items: env_items.as_mut_ptr(),
        };

        let mut auth: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { AuthorizationCreate(std::ptr::null(), std::ptr::null(), 0, &mut auth) };
        if rc != 0 {
            println!("AuthorizationCreate 失败: {rc}");
            return;
        }
        println!("正在申请 system.privilege.admin —— 注意看弹出的框里有没有指纹选项…");
        let rc = unsafe {
            AuthorizationCopyRights(
                auth,
                &rights,
                &env,
                INTERACTION_ALLOWED | EXTEND_RIGHTS,
                std::ptr::null_mut(),
            )
        };
        match rc {
            0 => println!("结果: 授权成功（errAuthorizationSuccess）"),
            -60006 => println!("结果: 用户取消（errAuthorizationCanceled）"),
            -60007 => println!("结果: 交互不被允许（errAuthorizationInteractionNotAllowed）"),
            -60005 => println!("结果: 拒绝（errAuthorizationDenied）"),
            other => println!("结果: OSStatus {other}"),
        }
        unsafe { AuthorizationFree(auth, DESTROY_RIGHTS) };
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    probe::run();
}
