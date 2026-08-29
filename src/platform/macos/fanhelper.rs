//! 常驻风扇特权守护进程（方案 A：自装 LaunchDaemon）。
//!
//! # 为什么不是 SMJobBless / SMAppService
//!
//! 那两套官方接口会校验 `SMPrivilegedExecutables` 里的代码签名要求串，
//! 落地需要 Developer ID 证书；本项目默认打包走的是 ad-hoc 签名
//! （`scripts/package-macos.sh`），没有证书可锚。这里改用「一次管理员授权
//! 装一个 LaunchDaemon」的老办法：不依赖任何证书，装完之后**永远不再弹框**。
//!
//! # 结构
//!
//! * 主应用（普通权限）→ unix socket → 守护进程（root）→ SMC 写入。
//! * 守护进程由 launchd 在开机时拉起（`RunAtLoad` + `KeepAlive`）。
//! * 安装只需一次 `osascript ... with administrator privileges`：把自身二进制
//!   拷进 `/Library/PrivilegedHelperTools/`，再以 root 跑一次
//!   `--fanhelper-install` 让它自己写 plist 并 `launchctl bootstrap`。
//!   plist 内容来自二进制里的常量，不经过 shell 拼接，也不经过用户可写的临时文件。
//!
//! # 安全边界
//!
//! 一个常驻 root 进程在本地 socket 上收命令就是一条本地提权面，这里靠三条收窄：
//!
//! 1. **命令面只有两条**：`auto` 和 `pct <白名单档位>`。没有路径、没有命令、没有
//!    文件名参数，解析失败一律拒绝，所以不存在「让 root 去执行/写入某个路径」
//!    这类经典利用。整行长度上限 [`MAX_LINE`]，防止无边界缓冲增长。
//! 2. **socket 只对当前控制台用户开放**：`0600` + chown 到 `/dev/console` 的属主，
//!    每次 accept 前重新校准（覆盖快速用户切换）。其它本地用户连不上。
//! 3. **连接断开即恢复自动调速**：强制档位的生命周期绑定在这条连接上，
//!    主应用退出 / 崩溃都会让守护进程立刻把风扇交还系统——与旧的
//!    `--fanctl` 看父进程 PID 是同一套安全语义，不会出现「App 没了风扇还锁着」。
//!
//! 安装侧（一次性，见 [`INSTALL_SCRIPT`]）收窄成：root 只会执行**与当前运行
//! 映像 cdhash 相同、且签名自洽**的那份二进制；校验在 root-only 目录里的
//! 暂存文件上进行，落位前不可读、不可被替换，失败不动正式路径。
//!
//! **仍然关不掉的**：能在本机以同一用户执行代码的攻击者，自己弹一个
//! `osascript ... with administrator privileges` 骗到密码，就直接拿到任意
//! root 命令——这条与本模块无关，也不是本模块能封的。要把「只有本应用能装
//! 这个守护进程」变成系统强制的约束，只有 SMJobBless / SMAppService 配
//! Developer ID（Team ID 写进签名要求串）能做到。

use crate::core::status::{FanError, FanMode};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

pub const LABEL: &str = "com.quickcleaner.fanhelper";
pub const HELPER_DIR: &str = "/Library/PrivilegedHelperTools";
pub const HELPER_PATH: &str = "/Library/PrivilegedHelperTools/com.quickcleaner.fanhelper";
/// 校验期间的落脚点。**绝不能直接往正式路径上写**：校验不过要删文件，
/// 删在正式路径上就等于把上一次装好的守护进程一起拆了。
pub const HELPER_STAGE: &str = "/Library/PrivilegedHelperTools/com.quickcleaner.fanhelper.new";
pub const LAUNCHD_DIR: &str = "/Library/LaunchDaemons";
pub const PLIST_PATH: &str = "/Library/LaunchDaemons/com.quickcleaner.fanhelper.plist";
pub const SOCKET_PATH: &str = "/var/run/com.quickcleaner.fanhelper.sock";

/// 单条命令的长度上限。最长的合法命令是 `pct 100\n`（8 字节），给到 64
/// 已经宽松得离谱；超过即判定为异常输入并断开。
const MAX_LINE: usize = 64;

/// 守护进程重申目标转速的周期。thermalmonitord 会尝试收回风扇，
/// 与旧 `--fanctl` 守护进程同一节奏。
const REASSERT: Duration = Duration::from_secs(3);

/// 客户端等应答的上限。首次设定全速档要等 thermalmonitord 让出控制权
/// （[`unlock_fan_control`](super::status) 最长 8 秒），20 秒留足余量，
/// 同时保证守护进程卡死时后台任务不会永远挂着。
const CLIENT_TIMEOUT: Duration = Duration::from_secs(20);

// ---------------------------------------------------------------- 安装状态

/// 守护进程是否已安装。二进制和 plist 都在才算数——只剩其一说明上一次
/// 安装/卸载被打断，按未安装处理，重装会把两者都补齐。
pub fn is_installed() -> bool {
    Path::new(HELPER_PATH).exists() && Path::new(PLIST_PATH).exists()
}

/// 把一个字面量包成单引号 shell 字符串。
///
/// 这里拼进去的只有编译期常量路径和我们自己算的十六进制，但安装脚本是以
/// root 执行的，宁可多一层机械保证，也不要依赖「调用方一定传的是好值」。
fn sh_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', r"'\''"))
}

/// 安装前的两道校验，缺一不可——这是整个安装流程的安全闸门。
///
/// * `codesign --verify --strict`：证明文件内容与它自己的签名一致。
///   单靠 cdhash 比对**挡不住篡改**——`codesign -d` 只是把签名块里*声明*的
///   CDHash 打出来，不重算页哈希，改代码字节而不动签名块时声明值不变
///   （实测：改一个字节，cdhash 一模一样）。
/// * cdhash == 期望值：证明这是**我们这份**二进制。单靠 `--verify` 也不够
///   ——任何一个 Apple 签过的系统程序都能通过验证（实测 `/bin/ls` 通过）。
///
/// 任何一道不过就删掉文件并以 3 退出，绝不执行它。
///
/// 逻辑放在 Rust 而不是 AppleScript 字符串里，是为了能被 [`tests`] 直接
/// 喂各种篡改样本验证——埋在脚本常量里的安全检查没人测得动。
fn verify_snippet(helper: &str, expected_cdhash: &str) -> Result<String, FanError> {
    // 只接受 40 位小写十六进制（SHA-1 长度的 cdhash 截断值）。这个值会被
    // 原样拼进 root 执行的命令里，格式必须在这里就钉死。
    if expected_cdhash.len() != 40
        || !expected_cdhash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(FanError::Other(format!(
            "代码签名标识格式异常，拒绝安装: {expected_cdhash}"
        )));
    }
    let h = sh_quote(helper);
    Ok(format!(
        "if ! /usr/bin/codesign --verify --strict {h}; then \
         /bin/rm -f {h}; echo signature-invalid >&2; exit 3; fi; \
         installed=$(/usr/bin/codesign -d --verbose=4 {h} 2>&1 \
         | /usr/bin/awk -F= '/^CDHash=/{{print $2}}'); \
         if [ \"$installed\" != {hash} ]; then \
         /bin/rm -f {h}; echo cdhash-mismatch >&2; exit 3; fi;",
        h = h,
        hash = sh_quote(expected_cdhash),
    ))
}

/// 安装脚本。**顺序本身就是安全措施**，每一步都有对应的攻击要挡：
///
/// 1. `umask 077` + 目录 chown/chmod 回 root:wheel 0755。macOS 某些版本上
///    `/Library` 是 admin 组可写的，攻击者能抢先建出
///    `/Library/PrivilegedHelperTools` 并持有它——能否替换目录里的文件由
///    **目录**权限决定，所以即便 root 写进去的是好文件，目录属主照样能
///    unlink 换掉。
/// 2. 拒绝源路径是符号链接。`cp -f` 会**解引用**符号链接（实测），bundle 里
///    的可执行文件用户可写，换成指向 `/private/var/root/…` 的链接就能让 root
///    把任意文件内容拷到一个可预测的路径上——那是一条任意文件读。
/// 3. `ulimit -f`。上一条即使挡住了普通文件，`ln -s /dev/zero` 仍能让 root
///    一直写到把卷撑满（实测 1 秒 1.3 GB）。本二进制 23 MB，200 MB 足够宽。
/// 4. 先拷到 **`.new` 暂存路径**（umask 077 → 0600），在那里校验。
///    * 不能先 `chmod 544` 再校验：544 是 `-r-xr--r--`，世界可读，校验的那
///      一两百毫秒里内容对所有本地用户敞开。
///    * 不能直接写正式路径：校验失败要删，删在正式路径上就把上一次能用的
///      守护进程拆了——正在跑的那个还在从已 unlink 的 inode 里跑，`KeepAlive`
///      下次拉起时找不到文件。失败只删 `.new`，正式 helper 原样不动。
/// 5. 校验通过后才 chown/chmod 并 `mv -f` 原子落位，然后才执行。
/// 6. 校验必须在这里做，不能交给 `--fanhelper-install`——那是被复制过去的那个
///    二进制本身，如果它已经被掉包，让它自证清白毫无意义。
///
/// 校验片段由 Rust 生成后经 argv 传入（见 [`verify_snippet`]），AppleScript
/// 只做拼接。
const INSTALL_SCRIPT: &str = r#"on run argv
	set exePath to item 1 of argv
	set helperPath to item 2 of argv
	set stagePath to item 3 of argv
	set helperDir to item 4 of argv
	set verifyCmd to item 5 of argv
	set promptText to item 6 of argv
	set cmd to "set -e; umask 077; " & ¬
		"/bin/mkdir -p " & quoted form of helperDir & "; " & ¬
		"/usr/sbin/chown root:wheel " & quoted form of helperDir & "; " & ¬
		"/bin/chmod 755 " & quoted form of helperDir & "; " & ¬
		"if [ -L " & quoted form of exePath & " ]; then " & ¬
		"echo source-is-symlink >&2; exit 3; fi; " & ¬
		"ulimit -f 409600; " & ¬
		"/bin/rm -f " & quoted form of stagePath & "; " & ¬
		"/bin/cp -f " & quoted form of exePath & " " & quoted form of stagePath & "; " & ¬
		verifyCmd & " " & ¬
		"/usr/sbin/chown root:wheel " & quoted form of stagePath & "; " & ¬
		"/bin/chmod 544 " & quoted form of stagePath & "; " & ¬
		"/bin/mv -f " & quoted form of stagePath & " " & quoted form of helperPath & "; " & ¬
		quoted form of helperPath & " --fanhelper-install"
	-- with prompt：授权框默认只写「osascript 想要进行更改」，看不出是谁要干什么。
	-- 装一个常驻 root 组件必须把话说清楚，正文由调用方按界面语言传进来。
	do shell script cmd with administrator privileges with prompt promptText
end run"#;

const UNINSTALL_SCRIPT: &str = r#"on run argv
	set label to item 1 of argv
	set helperPath to item 2 of argv
	set plistPath to item 3 of argv
	set sockPath to item 4 of argv
	set promptText to item 5 of argv
	set cmd to "/bin/launchctl bootout system/" & quoted form of label & " 2>/dev/null; " & ¬
		"/bin/rm -f " & quoted form of plistPath & " " & quoted form of helperPath & " " & quoted form of sockPath & "; exit 0"
	do shell script cmd with administrator privileges with prompt promptText
end run"#;

/// 安装守护进程。**这是整个功能唯一一次密码框**，之后所有档位切换都走
/// socket，不再需要授权。
pub fn install(prompt: &str) -> Result<(), FanError> {
    // 覆盖前先丢掉旧连接：守护进程把强制档位绑在这条连接上，不放掉的话
    // bootout 时它还卡在 serve() 里，socket 迟迟不释放。
    drop_client();
    let exe = std::env::current_exe()
        .map_err(|e| FanError::Other(format!("无法定位自身可执行文件: {e}")))?;
    // 锚点必须是**正在运行的映像**而不是磁盘上那个文件：后者随时可能被同用户
    // 的进程改写，先算哈希再授权只是把窗口从「授权那几十秒」挪到「算哈希之前」，
    // 并没有关上。详见 `codesign` 模块文档。
    let cdhash = super::codesign::self_cdhash().ok_or_else(|| {
        FanError::Other(
            "本程序没有代码签名，无法安全安装守护进程（用 codesign -s - 至少做一次 ad-hoc 签名）"
                .into(),
        )
    })?;
    // 校验打在暂存文件上，正式路径在校验通过后才被覆盖。
    let verify = verify_snippet(HELPER_STAGE, &cdhash)?;
    run_osascript(
        INSTALL_SCRIPT,
        &[
            exe.to_string_lossy().into_owned(),
            HELPER_PATH.to_string(),
            HELPER_STAGE.to_string(),
            HELPER_DIR.to_string(),
            verify,
            prompt.to_string(),
        ],
    )?;
    // 等到**真的能连上并握手成功**为止，而不是等 socket 文件出现。
    //
    // 这里踩过一个坑：覆盖安装时 `launchctl bootout` 拆掉旧守护进程，但它的
    // socket 文件要等旧进程自己清理才消失，所以「文件存在」在新进程 bind 之前
    // 就已经为真——install() 立刻返回成功，紧接着的 control() 连到一个没有
    // 监听者的路径，被判成「未安装」，于是又弹一次密码框。用户看到的是
    // 「输两次密码才装上」。
    let mut last = FanError::Other("守护进程未在 6 秒内就绪".into());
    for _ in 0..60 {
        match connect() {
            Ok(_) => return Ok(()),
            Err(err) => last = err,
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(last)
}

/// 卸载守护进程：停服务、删 plist / 二进制 / socket。恢复自动调速由守护
/// 进程收到 SIGTERM 时自己完成（见 [`run_daemon`]）。
pub fn uninstall(prompt: &str) -> Result<(), FanError> {
    drop_client();
    run_osascript(
        UNINSTALL_SCRIPT,
        &[
            LABEL.to_string(),
            HELPER_PATH.to_string(),
            PLIST_PATH.to_string(),
            SOCKET_PATH.to_string(),
            prompt.to_string(),
        ],
    )
}

fn run_osascript(script: &str, args: &[String]) -> Result<(), FanError> {
    let out = std::process::Command::new("osascript")
        .arg("-")
        .args(args)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(script.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(|e| FanError::Other(format!("osascript 启动失败: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    // 用户在密码框点了取消是最常见的失败，单独说人话。
    if stderr.contains("User canceled") || stderr.contains("(-128)") {
        return Err(FanError::Canceled);
    }
    // 安装闸门拦下来的两种情况。原始 stderr 是一大段 osascript/codesign 输出，
    // 用户看不懂；这是**安全中止**，必须给出能据以行动的说法。
    if stderr.contains("signature-invalid") {
        return Err(FanError::Other(
            "本程序的代码签名校验未通过，安装已中止——可执行文件可能已被改动，请重新下载".into(),
        ));
    }
    if stderr.contains("cdhash-mismatch") {
        // 现实中最常见的触发原因不是攻击，而是磁盘上的程序比正在运行的这个
        // 进程新：重新编译过、或更新器原地替换了 bundle 却没重启。校验锚在
        // 内核记录的运行映像上，这种情况下拒绝是**正确**行为，但话要说清楚。
        return Err(FanError::Other(
            "待安装的文件与正在运行的程序不一致，安装已中止。\
             若刚更新或重新编译过，请退出并重新打开本程序后再安装；\
             否则可能有其它程序在授权期间替换了它"
                .into(),
        ));
    }
    if stderr.contains("source-is-symlink") {
        return Err(FanError::Other(
            "本程序的可执行文件被替换成了符号链接，安装已中止".into(),
        ));
    }
    Err(FanError::Other(stderr.trim().to_string()))
}

// ------------------------------------------------------------------ 客户端

/// 与守护进程的长连接。**必须**跨调用保活：守护进程把强制档位的生命周期
/// 绑在这条连接上，断开即恢复自动调速。放静态量里，主应用退出时连接随进程
/// 关闭，风扇自动交还系统。
static CLIENT: Mutex<Option<UnixStream>> = Mutex::new(None);

fn drop_client() {
    let mut guard = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// 经守护进程设定档位。[`FanError::NotInstalled`] 是从没装过；
/// [`FanError::NeedsUpgrade`] 是装过但握手对不上，直接覆盖即可。
pub fn control(mode: FanMode) -> Result<(), FanError> {
    if !mode.is_supported() {
        return Err(FanError::Other("仅支持自动、降温或全速风扇模式".into()));
    }
    let mut guard = CLIENT.lock().unwrap_or_else(|e| e.into_inner());
    let line = command_line(mode);
    // 两轮：第一轮可能用的是已被对端关掉的旧连接（守护进程重启 / 升级过），
    // 传输层失败就丢掉重连一次。注意只重试**传输失败**——守护进程正经回的
    // `err ...`（比如机型没有可控风扇）是协议级结论，重试一次结果一样，
    // 白白多跑一遍 SMC。
    for attempt in 0..2 {
        if guard.is_none() {
            *guard = Some(connect()?);
        }
        let stream = guard.as_mut().expect("上一步刚填过");
        match request_line(stream, &line) {
            Ok(reply) if reply == "ok" => return Ok(()),
            Ok(reply) => {
                return Err(FanError::Other(
                    reply.strip_prefix("err ").unwrap_or(&reply).to_string(),
                ))
            }
            Err(transport) => {
                *guard = None;
                if attempt == 1 {
                    return Err(transport);
                }
            }
        }
    }
    Err(FanError::Other("守护进程无应答".into()))
}

/// 协议里允许的手动档位。UI 只提供这两档，守护进程也只认这两个数——
/// 输入面越小越好，没必要为了「以后可能加档」把 1..=100 整段开着。
pub const ALLOWED_DUTY: [u8; 2] = [60, 100];

fn command_line(mode: FanMode) -> String {
    match mode {
        FanMode::Auto => "auto\n".to_string(),
        // 不在白名单里的一律当全速：宁可多吹也不能少吹。
        FanMode::Percent(p) if ALLOWED_DUTY.contains(&p) => format!("pct {p}\n"),
        FanMode::Percent(_) => "pct 100\n".to_string(),
    }
}

fn connect() -> Result<UnixStream, FanError> {
    if !is_installed() {
        return Err(FanError::NotInstalled);
    }
    // 文件都在却连不上 = 守护进程正在重启 / 还没 bind，**不是**没装。
    // 这两种情况必须分开：判成未安装会让调用方去弹安装授权框，而它明明
    // 已经装好了，用户平白多输一次密码。
    let stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|e| FanError::Other(format!("守护进程已安装但连接失败: {e}")))?;
    stream.set_read_timeout(Some(CLIENT_TIMEOUT)).ok();
    stream.set_write_timeout(Some(CLIENT_TIMEOUT)).ok();
    let mut stream = stream;
    // 握手：/Library 里的守护进程必须与当前进程是同一份二进制，否则它的
    // 协议、档位白名单、SMC 逻辑都可能是旧的。比 cdhash 而不是版本号——
    // 版本号在两次构建之间通常不变，认不出过时的守护进程。
    // 对不上走 [`FanError::NeedsUpgrade`]，调用方直接覆盖，不必先卸。
    let expected = match super::codesign::self_cdhash() {
        Some(h) => format!("ok {} {h}", env!("CARGO_PKG_VERSION")),
        None => return Err(FanError::NotInstalled),
    };
    match request_line(&mut stream, "version\n") {
        Ok(line) => classify_handshake(&line, &expected).map(|()| stream),
        Err(err) => Err(err),
    }
}

/// 握手回包与当前进程一致才算可用；对不上是覆盖升级，不是「没装」。
fn classify_handshake(reply: &str, expected: &str) -> Result<(), FanError> {
    if reply == expected {
        Ok(())
    } else {
        Err(FanError::NeedsUpgrade)
    }
}

fn request_line(stream: &mut UnixStream, line: &str) -> Result<String, FanError> {
    stream
        .write_all(line.as_bytes())
        .map_err(|e| FanError::Other(format!("守护进程写入失败: {e}")))?;
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    while buf.len() < MAX_LINE * 4 {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) if byte[0] == b'\n' => {
                return Ok(String::from_utf8_lossy(&buf).trim().to_string());
            }
            Ok(_) => buf.push(byte[0]),
            Err(e) => return Err(FanError::Other(format!("守护进程读取失败: {e}"))),
        }
    }
    Err(FanError::Other("守护进程应答不完整".into()))
}

// ------------------------------------------------------------ 守护进程本体

static STOPPED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_sig: i32) {
    STOPPED.store(true, Ordering::Relaxed);
}

/// root 守护进程主循环。由 launchd 以 `--fanhelper` 拉起。
pub fn run_daemon() {
    // 先确认身份再动硬件：非 root 跑到这里说明有人手工执行了 --fanhelper，
    // 下面那次「复位到自动」会白白去戳一遍 SMC 并失败。
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("fanhelper: --fanhelper 必须以 root 运行（由 launchd 拉起）");
        std::process::exit(2);
    }
    // 必须走 sigaction 且不带 SA_RESTART：macOS 的 signal(3) 是 BSD 语义，
    // 默认给处理函数装上 SA_RESTART，被打断的 accept() 会自动重启——SIGTERM
    // 就永远唤不醒 accept 循环，launchctl bootout 只能等超时后 SIGKILL，
    // 风扇来不及交还系统。sigaction + sa_flags=0 让 accept 返回 EINTR。
    unsafe {
        let mut act: libc::sigaction = std::mem::zeroed();
        act.sa_sigaction = on_signal as *const () as libc::sighandler_t;
        act.sa_flags = 0;
        libc::sigemptyset(&mut act.sa_mask);
        libc::sigaction(libc::SIGTERM, &act, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &act, std::ptr::null_mut());
        // 客户端半路消失时 write 会触发 SIGPIPE，默认动作是直接杀进程。
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    // 上一轮被 kill -9 打断时风扇可能还锁在手动档。必须确认 Auto 写成功
    // 再对外服务：写失败就按节拍重试，不能「试一次、忽略错误」后接着接命令。
    restore_auto();

    let _ = std::fs::remove_file(SOCKET_PATH); // 清掉上次残留的 socket 文件
                                               // bind 出来的 socket 权限由 umask 决定，默认是 0755——从 bind 到下面
                                               // chmod 0600 之间有个任意本地用户都能连上的窗口。先把 umask 收紧，
                                               // 让 socket 一出生就是 0600，不留这个窗口。
    let saved_umask = unsafe { libc::umask(0o177) };
    let bound = UnixListener::bind(SOCKET_PATH);
    unsafe {
        libc::umask(saved_umask);
    }
    let Ok(listener) = bound else {
        eprintln!("fanhelper: 无法在 {SOCKET_PATH} 上监听");
        std::process::exit(1);
    };

    while !STOPPED.load(Ordering::Relaxed) {
        // 每次 accept 前重新校准属主：控制台用户可能因快速用户切换换了人。
        restrict_socket_to_console_user();
        match listener.accept() {
            Ok((stream, _)) => serve(stream),
            // accept 被信号打断（SIGTERM）时回到 while 判条件退出。
            Err(_) => continue,
        }
    }
    restore_auto();
    let _ = std::fs::remove_file(SOCKET_PATH);
}

/// socket 只对当前控制台用户开放：`0600` + chown 到 `/dev/console` 的属主。
/// 拿不到控制台用户时保持 root-only（宁可谁都连不上，也不放宽）。
fn restrict_socket_to_console_user() {
    let path = match std::ffi::CString::new(SOCKET_PATH) {
        Ok(p) => p,
        Err(_) => return,
    };
    unsafe {
        libc::chmod(path.as_ptr(), 0o600);
    }
    if let Some(uid) = console_user_uid() {
        unsafe {
            // gid_t::MAX 就是 chown(2) 约定的 (gid_t)-1：只改属主，不动属组。
            libc::chown(path.as_ptr(), uid, libc::gid_t::MAX);
        }
    }
}

fn console_user_uid() -> Option<libc::uid_t> {
    let console = std::ffi::CString::new("/dev/console").ok()?;
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::stat(console.as_ptr(), &mut st) } != 0 || st.st_uid == 0 {
        None
    } else {
        Some(st.st_uid)
    }
}

/// 服务一条连接。强制档位的生命周期 = 这条连接的生命周期：读到 EOF
/// （主应用退出或崩溃）立刻恢复自动调速。
fn serve(mut stream: UnixStream) {
    let mut peer_uid = 0;
    let mut peer_gid = 0;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut peer_uid, &mut peer_gid) } != 0
        || console_user_uid() != Some(peer_uid)
    {
        return;
    }
    // 读超时兼作重申节拍器：没有新命令时每 3 秒醒一次重申目标转速。
    stream.set_read_timeout(Some(REASSERT)).ok();
    let mut target = FanMode::Auto;
    let mut pending = Vec::new();
    let mut chunk = [0u8; MAX_LINE];

    while !STOPPED.load(Ordering::Relaxed) {
        // 快速用户切换后，旧控制台用户已不再有权控制全局硬件。
        if console_user_uid() != Some(peer_uid) {
            break;
        }
        match stream.read(&mut chunk) {
            Ok(0) => break, // 对端关闭
            Ok(n) => {
                pending.extend_from_slice(&chunk[..n]);
                if pending.len() > MAX_LINE {
                    // 超长输入不是本协议的客户端，直接断开。
                    break;
                }
                while let Some(pos) = pending.iter().position(|b| *b == b'\n') {
                    let line: Vec<u8> = pending.drain(..=pos).collect();
                    let line = String::from_utf8_lossy(&line).trim().to_string();
                    let reply = handle(&line, &mut target);
                    if stream.write_all(reply.as_bytes()).is_err() {
                        return restore(&mut target);
                    }
                }
            }
            Err(ref e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // 到点重申：thermalmonitord 会尝试收回风扇。
                if let FanMode::Percent(_) = target {
                    if super::status::set_fan_mode(target).is_err() {
                        // 温度 / 系统热状态读取失败或 SMC 写入失败时不能静默
                        // 保持旧手动目标。退出服务循环，下面的 restore 会持续
                        // 重试，直到把控制权确实交还系统。
                        break;
                    }
                }
            }
            Err(_) => break,
        }
    }
    restore(&mut target);
}

/// 交还风扇。只在连接结束时调用一次，之后这条连接没有别的职责。
///
/// 恢复**必须确认成功**：SMC 写入失败时硬件可能还停在手动档，而目标值
/// 一旦提前清掉就再也没有人补写。所以失败时按 [`REASSERT`] 节拍原地重试，
/// 直到写入成功。不看 [`STOPPED`]：SIGTERM / 卸载时恰恰需要把风扇交还，
/// 那一刻停机标志已经是 true。卡住时由 launchd 的 SIGKILL 兜底。
/// 这也是有意阻塞 accept 循环：恢复完成前不接受新连接。
fn restore(target: &mut FanMode) {
    restore_auto();
    *target = FanMode::Auto;
}

/// 确认把风扇写回 Auto。逻辑状态只能在这之后改成 Auto。
fn restore_auto() {
    loop {
        match super::status::set_fan_mode(FanMode::Auto) {
            Ok(()) => return,
            Err(_) => std::thread::sleep(REASSERT),
        }
    }
}

/// 命令解析。**整个 root 进程的输入面就这里三行**：`version` / `auto` /
/// `pct <[ALLOWED_DUTY] 里的值>`，其余一律拒绝。没有任何取路径或命令的参数。
///
/// 中间档（60）的安全性不靠这里的白名单，而靠 `status::effective_duty`
/// 的温度联动升档 + `F{i}Mn` 下界夹紧；白名单只是把输入面收到最小。
fn handle(line: &str, target: &mut FanMode) -> String {
    let mode = match line {
        // 握手回本进程的 cdhash，而不只是版本号：`CARGO_PKG_VERSION` 在两次
        // 构建之间通常不变，靠它根本认不出「/Library 里躺着的是上个月编的
        // 守护进程」。cdhash 是内容指纹，代码一改就不同，正是要的判据。
        "version" => {
            return match super::codesign::self_cdhash() {
                Some(h) => format!("ok {} {h}\n", env!("CARGO_PKG_VERSION")),
                None => "err 守护进程无代码签名\n".to_string(),
            }
        }
        "auto" => FanMode::Auto,
        other => match other.strip_prefix("pct ") {
            Some(rest) => match rest.parse::<u8>() {
                Ok(p) if ALLOWED_DUTY.contains(&p) => FanMode::Percent(p),
                _ => return "err 档位不在允许列表内\n".to_string(),
            },
            None => return "err 未知命令\n".to_string(),
        },
    };
    match super::status::set_fan_mode(mode) {
        Ok(()) => {
            *target = mode;
            "ok\n".to_string()
        }
        Err(err) => {
            // 写入失败时硬件可能停在半手动。逻辑状态不能先改成 Auto（否则
            // restore 会以为没事、周期重申也不会再补写）。先确认交还系统。
            restore(target);
            if mode == FanMode::Auto {
                "ok\n".to_string()
            } else {
                format!("err {err}\n")
            }
        }
    }
}

// -------------------------------------------------------------- root 侧安装

const PLIST_BODY: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
    "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
    "<plist version=\"1.0\">\n<dict>\n",
    "\t<key>Label</key>\n\t<string>com.quickcleaner.fanhelper</string>\n",
    "\t<key>ProgramArguments</key>\n\t<array>\n",
    "\t\t<string>/Library/PrivilegedHelperTools/com.quickcleaner.fanhelper</string>\n",
    "\t\t<string>--fanhelper</string>\n\t</array>\n",
    "\t<key>RunAtLoad</key>\n\t<true/>\n",
    "\t<key>KeepAlive</key>\n\t<true/>\n",
    "</dict>\n</plist>\n"
);

/// 卸掉已有 job，并等到进程真正退出、socket 文件消失。
///
/// `launchctl bootout` 返回时旧进程可能还活着。立刻 `bootstrap` 会失败，
/// 覆盖安装就只能先手工移除再重装。
fn bootout_existing_job() {
    let _ = std::process::Command::new("/bin/launchctl")
        .args(["bootout", &format!("system/{LABEL}")])
        .output();
    for _ in 0..80 {
        let loaded = std::process::Command::new("/bin/launchctl")
            .args(["print", &format!("system/{LABEL}")])
            .output()
            .is_ok_and(|out| out.status.success());
        if !loaded && !Path::new(SOCKET_PATH).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 以 root 跑的安装步骤：写 plist 并让 launchd 接管。
///
/// 由 [`install`] 经 osascript 调起，plist 内容取自本二进制里的常量——
/// 不经 shell 拼接，也不落用户可写的临时文件，避开 TOCTOU / 符号链接替换。
pub fn run_install_step() -> i32 {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("fanhelper: --fanhelper-install 必须以 root 运行");
        return 2;
    }
    // 与 helper 目录同理：`/Library/LaunchDaemons` 在部分 macOS 版本上属于
    // admin 组可写，攻击者抢先持有这个目录就能在我们写完之后把 plist 换掉，
    // 让 launchd 去拉起别的程序。写之前先把目录收回 root:wheel 0755。
    if let Ok(dir) = std::ffi::CString::new(LAUNCHD_DIR) {
        unsafe {
            libc::chown(dir.as_ptr(), 0, 0);
            libc::chmod(dir.as_ptr(), 0o755);
        }
    }
    if let Err(err) = std::fs::write(PLIST_PATH, PLIST_BODY) {
        eprintln!("fanhelper: 写 {PLIST_PATH} 失败: {err}");
        return 1;
    }
    let plist = match std::ffi::CString::new(PLIST_PATH) {
        Ok(p) => p,
        Err(_) => return 1,
    };
    unsafe {
        libc::chown(plist.as_ptr(), 0, 0);
        libc::chmod(plist.as_ptr(), 0o644);
    }
    // 覆盖安装时先卸旧的；没装过会报错，忽略。bootout 返回时进程可能还
    // 占着 socket，立刻 bootstrap 会失败，看起来就像「必须先手动移除」。
    bootout_existing_job();
    match std::process::Command::new("/bin/launchctl")
        .args(["bootstrap", "system", PLIST_PATH])
        .output()
    {
        Ok(out) if out.status.success() => 0,
        Ok(out) => {
            eprintln!(
                "fanhelper: launchctl bootstrap 失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            1
        }
        Err(err) => {
            eprintln!("fanhelper: launchctl 启动失败: {err}");
            1
        }
    }
}

// -------------------------------------------------------------- 门面适配

/// `platform` 门面契约里的提权风扇通道：直写被固件拒绝后走这条。
/// 没装返回 [`FanError::NotInstalled`]（UI 先确认再装）；握手对不上返回
/// [`FanError::NeedsUpgrade`]（UI 直接覆盖，不再确认）。
pub fn elevated_fan_control(mode: FanMode) -> Result<(), FanError> {
    control(mode)
}

pub fn fan_helper_installed() -> bool {
    is_installed()
}

pub fn install_fan_helper(prompt: &str) -> Result<(), FanError> {
    install(prompt)
}

pub fn uninstall_fan_helper(prompt: &str) -> Result<(), FanError> {
    uninstall(prompt)
}

#[cfg(test)]
mod tests {
    /// 钉死安装脚本的三条顺序性质。这些是「校验有没有用」的前提，光有
    /// [`verify_snippet`] 本身不够——它可以完全正确，却被放在错误的位置。
    #[test]
    fn install_script_verifies_before_landing_and_never_rm_s_the_live_helper() {
        let snippet = verify_snippet(HELPER_STAGE, &"a".repeat(40)).expect("合法 cdhash");

        // 1) 失败时只删暂存文件。带上闭合引号才能区分开——正式路径是暂存
        //    路径的前缀（…fanhelper 之于 …fanhelper.new），少了引号会漏判。
        assert!(
            snippet.contains(&format!("rm -f '{HELPER_STAGE}'")),
            "校验失败应当删掉暂存文件"
        );
        assert!(
            !snippet.contains(&format!("rm -f '{HELPER_PATH}'")),
            "校验失败绝不能删正式 helper：那会把上一次装好的守护进程一起拆掉"
        );

        // 2) 顺序：校验 → 提权限 → 原子落位 → 才执行。
        let at = |needle: &str| {
            INSTALL_SCRIPT
                .find(needle)
                .unwrap_or_else(|| panic!("安装脚本里找不到 {needle:?}"))
        };
        assert!(at("verifyCmd") < at("chmod 544"), "校验必须在放开权限之前");
        assert!(at("chmod 544") < at("mv -f"), "落位前才该 chmod 544");
        assert!(at("mv -f") < at("--fanhelper-install"), "先落位再执行");

        // 3) 两道输入侧防护还在。
        assert!(INSTALL_SCRIPT.contains("umask 077"), "暂存文件不能世界可读");
        assert!(
            INSTALL_SCRIPT.contains("[ -L "),
            "必须拒绝符号链接源：cp -f 会解引用，那是一条 root 任意文件读"
        );
        assert!(
            INSTALL_SCRIPT.contains("ulimit -f"),
            "必须限制写入量：ln -s /dev/zero 能让 root 把卷写满"
        );
    }

    /// 安装闸门的核心回归：喂四种样本，只有「原样复制的自己」能过。
    ///
    /// 这个测试当初就抓到过一版真 bug——只比 cdhash 不做 `--verify` 时，
    /// 改掉一个字节的二进制照样放行（`codesign -d` 报的是签名块里*声明*的
    /// 值，不重算页哈希）。两道校验缺哪一道，下面都会有一行变红。
    #[test]
    fn install_gate_only_accepts_an_untouched_copy_of_ourselves() {
        let exe = std::env::current_exe().expect("拿不到自身路径");
        let Some(cdhash) = super::super::codesign::self_cdhash() else {
            panic!("测试二进制应当是（至少 ad-hoc）签名的");
        };
        let dir = std::env::temp_dir().join(format!("fanhelper-gate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");

        let place = |name: &str, from: &std::path::Path| {
            let dst = dir.join(name);
            std::fs::copy(from, &dst).expect("复制样本");
            dst
        };
        // 1) 原样复制的自己 —— 唯一该放行的
        let good = place("good", &exe);
        // 2) 改掉一个字节：签名块没动，声明的 cdhash 不变，只有 --verify 抓得住
        let tampered = place("tampered", &exe);
        {
            use std::io::{Seek, SeekFrom, Write};
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(&tampered)
                .expect("打开样本");
            f.seek(SeekFrom::Start(40960)).expect("定位");
            f.write_all(b"X").expect("改一个字节");
        }
        // 3) 别人的合法签名程序：--verify 通过，只有 cdhash 比对抓得住
        let other = place("other", std::path::Path::new("/bin/ls"));
        // 4) 完全没签名的脚本
        let unsigned = dir.join("unsigned");
        std::fs::write(&unsigned, b"#!/bin/sh\nid\n").expect("写脚本");

        let passes = |path: &std::path::Path| {
            let snippet = verify_snippet(&path.to_string_lossy(), &cdhash).expect("生成校验片段");
            // 片段失败时会 `exit 3`，所以只要还能走到 echo 就是放行。
            let out = std::process::Command::new("/bin/sh")
                .arg("-c")
                .arg(format!("{snippet} echo PASSED"))
                .output()
                .expect("跑校验片段");
            String::from_utf8_lossy(&out.stdout).contains("PASSED")
        };

        assert!(passes(&good), "原样复制的自己应当放行");
        assert!(
            !passes(&tampered),
            "改过字节的二进制必须拒绝（签名校验这道）"
        );
        assert!(!passes(&other), "别人的合法签名程序必须拒绝（cdhash 这道）");
        assert!(!passes(&unsigned), "未签名文件必须拒绝");
        // 拒绝时闸门会把文件删掉，避免把可疑内容留在特权目录里
        assert!(!tampered.exists(), "被拒的样本应当已被删除");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// cdhash 会被原样拼进 root 执行的命令，格式必须在生成时就卡死。
    #[test]
    fn verify_snippet_rejects_a_malformed_cdhash() {
        for bad in [
            "",
            "not-hex",
            "3E7D0D8D44D827984C7BD8890E0E79C0B276D0BA", // 大写
            "3e7d0d8d44d827984c7bd8890e0e79c0b276d0b",  // 少一位
            "3e7d0d8d44d827984c7bd8890e0e79c0b276d0ba'; rm -rf /; '",
        ] {
            assert!(
                verify_snippet(HELPER_PATH, bad).is_err(),
                "{bad:?} 不该被接受"
            );
        }
    }

    use super::*;

    /// 命令解析是这个 root 进程的**全部**输入面，拒绝路径必须被钉死：
    /// 任何不在白名单里的输入都不能走到 SMC 写入。下面这些用例全部在
    /// `handle` 里被挡下，不触碰硬件，所以在 CI 上跑也是安全的。
    #[test]
    fn daemon_rejects_everything_outside_the_two_commands() {
        let mut target = FanMode::Auto;
        for hostile in [
            "",
            "pct",
            "pct ",
            "pct 0",   // 白名单外
            "pct 30",  // 白名单外：UI 不提供的档位守护进程也不该认
            "pct 101", // 上界外
            "pct 256", // 溢出 u8
            "pct -1",
            "pct 60 extra",
            "pct60",
            "PCT 60",         // 大小写不放行
            "auto; rm -rf /", // 命令面里根本没有 shell
            "/bin/sh",
            "version extra",
        ] {
            let reply = handle(hostile, &mut target);
            assert!(
                reply.starts_with("err "),
                "{hostile:?} 应被拒绝，实际回了 {reply:?}"
            );
            assert_eq!(target, FanMode::Auto, "{hostile:?} 不该改动目标档位");
        }
    }

    #[test]
    fn handshake_mismatch_is_an_upgrade_not_a_missing_install() {
        // 重新打包后旧 helper 仍在 /Library：必须走覆盖，不能走「未安装」
        // 确认框（那会让用户以为要先点移除）。
        let expected = format!("ok {} deadbeef", env!("CARGO_PKG_VERSION"));
        let stale = format!("ok {} cafebabe", env!("CARGO_PKG_VERSION"));
        assert_ne!(stale, expected);
        assert_eq!(
            super::classify_handshake(&stale, &expected),
            Err(FanError::NeedsUpgrade)
        );
        assert_eq!(super::classify_handshake(&expected, &expected), Ok(()));
    }

    #[test]
    fn version_handshake_matches_the_client_expectation() {
        let mut target = FanMode::Auto;
        let cdhash = super::super::codesign::self_cdhash().expect("测试二进制应当已签名");
        assert_eq!(
            handle("version", &mut target),
            format!("ok {} {cdhash}\n", env!("CARGO_PKG_VERSION")),
            "握手两端的格式必须一字不差，否则会被判成需要覆盖升级"
        );
        // 客户端拿这一行做版本比对，两边格式必须一致，否则升级后会
        // 一直判成 NeedsUpgrade，每次切换档位都要再授权覆盖一次。
        assert_eq!(target, FanMode::Auto);
    }

    #[test]
    fn command_line_only_emits_whitelisted_duties() {
        assert_eq!(command_line(FanMode::Auto), "auto\n");
        assert_eq!(command_line(FanMode::Percent(60)), "pct 60\n");
        assert_eq!(command_line(FanMode::Percent(100)), "pct 100\n");
        // 白名单外的一律抬成全速——宁可多吹也不能少吹。
        assert_eq!(command_line(FanMode::Percent(0)), "pct 100\n");
        assert_eq!(command_line(FanMode::Percent(30)), "pct 100\n");
        assert_eq!(command_line(FanMode::Percent(255)), "pct 100\n");
    }
}
