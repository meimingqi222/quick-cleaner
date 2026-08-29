//! 代码签名标识（cdhash）读取：**当前进程正在运行的那份映像**的身份。
//!
//! # 为什么需要它
//!
//! 安装特权守护进程时，root 要把「当前这个可执行文件」复制到
//! `/Library/PrivilegedHelperTools/`。问题在于「当前这个可执行文件」是一条
//! **路径**，而同用户的恶意进程随时可以改写那条路径上的文件——授权框弹着的
//! 那几十秒尤其好下手，最终让被替换的内容以 root 身份常驻。
//!
//! 关键在于：任何**回到磁盘去读**的做法都关不上这个窗口。先算一遍文件的
//! SHA-256 不行，攻击者在算之前替换即可；
//! **`SecCodeCopySelf` + `SecCodeCopySigningInformation` 同样不行**——实测它
//! 对 self 也是顺着进程路径回磁盘取静态代码，换掉文件后返回的是新文件的
//! cdhash（见 `examples/cdprobe.rs`，那个实验就是为此写的）。
//!
//! 真正锚在运行映像上的是内核：`csops(pid, CS_OPS_CDHASH)` 返回的是内核在
//! 加载这份代码时记下的 cdhash，磁盘文件之后怎么换都不影响。同一个实验里：
//!
//! ```text
//! before  SecCode=2257fb33…  csops=2257fb33…
//! （外部 unlink + 换成别的二进制）
//! after   SecCode=52a08fa0…  csops=2257fb33…   ← 只有 csops 没被带跑
//! ```
//!
//! # 局限
//!
//! * 二进制必须带签名（ad-hoc 也行）。Apple Silicon 上链接器会自动 ad-hoc
//!   签名，`scripts/package-macos.sh` 也显式签了；拿不到 cdhash 时安装流程
//!   直接拒绝，而不是退回到弱校验。
//! * ad-hoc 签名人人都能造，所以**不能**只验「签名有效」；安装闸门里 cdhash
//!   相等和 `codesign --verify` 是合取关系，见 `fanhelper::verify_snippet`。
//! * 通用二进制每个架构 slice 的 cdhash 不同。内核记的是当前运行架构那一个，
//!   root 侧 `codesign -d` 默认也取本机架构——两边一致，但当前打包是单架构
//!   host 构建，这条**尚未实测**，出 universal 包时需要补验。
//! * 这个身份是「运行中的进程是谁」。它保证不了「这个程序可信」：用户跑的
//!   要是已经被换过的 QuickCleaner，它会用自己的 cdhash 正常通过校验。那属于
//!   分发链路问题，只有 Developer ID + 公证能解决。

use std::ffi::c_void;

extern "C" {
    /// `<sys/codesign.h>`。libc crate 没导出，自己声明——只用到只读的
    /// `CS_OPS_CDHASH`，不碰任何会改进程状态的 ops。
    fn csops(pid: i32, ops: u32, useraddr: *mut c_void, usersize: usize) -> i32;
}

/// `CS_OPS_CDHASH`。注意是 5：20 是 `CS_CDHASH_LEN`（哈希字节数），
/// 两个常量挨着放很容易抄错，抄错的表现是 `csops` 返回 EINVAL。
const CS_OPS_CDHASH: u32 = 5;
/// `CS_CDHASH_LEN`：cdhash 截断到 20 字节。
const CS_CDHASH_LEN: usize = 20;

/// **当前进程正在运行的那份映像**的 cdhash，小写十六进制。
///
/// 取自内核记录而不是磁盘文件——这正是它能挡住 TOCTOU 的原因（见模块文档）。
/// 未签名的二进制拿不到，返回 `None`。
pub fn self_cdhash() -> Option<String> {
    let mut buf = [0u8; CS_CDHASH_LEN];
    let rc = unsafe {
        csops(
            std::process::id() as i32,
            CS_OPS_CDHASH,
            buf.as_mut_ptr() as *mut c_void,
            buf.len(),
        )
    };
    if rc != 0 {
        return None;
    }
    // 全 0 说明内核没有这份代码的哈希记录（未签名），别当成合法身份往下传。
    if buf.iter().all(|b| *b == 0) {
        return None;
    }
    Some(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 钉死两件事：内核那份 cdhash 拿得到，且与 root 侧将要使用的
    /// `codesign -d` 口径**完全一致**——安装校验两端比的必须是同一个值，
    /// 否则不是「装不上」就是「校验形同虚设」。
    ///
    /// 「换掉磁盘文件后仍返回旧值」这条关键性质没法在进程内自测（要外部
    /// 替换文件再回来问同一个进程），由 `examples/cdprobe.rs` 覆盖。
    #[test]
    fn self_cdhash_matches_what_codesign_reports_for_the_same_binary() {
        let mine = self_cdhash().expect("测试二进制应当是（至少 ad-hoc）签名的");
        assert_eq!(mine.len(), 40, "cdhash 应是 20 字节的十六进制：{mine}");

        let exe = std::env::current_exe().expect("拿不到自身路径");
        let out = std::process::Command::new("/usr/bin/codesign")
            .args(["-d", "--verbose=4"])
            .arg(&exe)
            .output()
            .expect("codesign 是系统自带工具");
        // codesign 把这些信息打到 stderr。
        let text = String::from_utf8_lossy(&out.stderr);
        let reported = text
            .lines()
            .find_map(|l| l.strip_prefix("CDHash="))
            .expect("codesign 应当输出 CDHash= 行")
            .trim()
            .to_string();
        assert_eq!(mine, reported, "两侧 cdhash 口径不一致");
    }
}
