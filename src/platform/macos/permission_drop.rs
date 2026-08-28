//! 完全磁盘访问的「拖拽授权」助手面板
//!
//! 打开系统设置的隐私列表之后，用户还剩三步要自己摸索：找到 `+` 号、在文件
//! 选择器里翻到我们的 .app、回到列表里找对开关。这个面板把三步压成一步——
//! 在设置窗口正下方浮一个小条，里面是我们自己的 app 图标，拖进上方列表松手
//! 即完成添加并置为开启（随后系统会要一次 Touch ID，那是 TCC 的规矩，绕不过
//! 也不该绕）。
//!
//! 视觉语言直接沿用 .dmg 安装窗口的「图标 → 箭头 → 目标」，用户不用学。
//!
//! ## 为什么是裸 AppKit 而不是 gpui 的窗口
//!
//! gpui 的 [`WindowKind::PopUp`] 确实给了我们要的窗口属性（`NSPanel` +
//! `NSWindowStyleMaskNonactivatingPanel` + 高层级），但那只占本文件十几行。
//! 真正的活是**拖出**：gpui 只实现了拖入（`ExternalPaths` / `FileDropEvent`），
//! 全库没有 `beginDraggingSession`，拖拽源必须自己用运行时声明一个 `NSView`
//! 子类。走 gpui 的话还得把这个 ObjC 视图按 gpui 的布局结果贴到它的视图树上，
//! 两套坐标系（gpui 逻辑像素 vs AppKit 翻转坐标）来回对齐，重排时还要跟着动。
//! 面板内容是四个静态元素，不值当。整块用 AppKit 反而自成闭环。
//!
//! ## 线程
//!
//! AppKit 只能在主线程碰。所有公开函数都断言主线程——调用点都在 gpui 的事件
//! 回调里，本来就在主线程上；断言是为了将来有人从后台线程调过来时当场炸掉，
//! 而不是随机崩在 AppKit 内部。

use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, BOOL, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 当前面板的 `NSPanel`，0 表示没开。只在主线程读写。
static PANEL: AtomicUsize = AtomicUsize::new(0);
/// 箭头视图，用于在每次显示时确认弹跳动画还在。随面板一起生灭。
static ARROW: AtomicUsize = AtomicUsize::new(0);

// AppKit 常量。用到哪个抄哪个，不引 cocoa crate——仓库现有的 ObjC 代码
// （trash.rs / app_icons.rs）都是这个路数，保持一致。
const NS_UTF8_STRING_ENCODING: usize = 4;
const NS_WINDOW_STYLE_MASK_BORDERLESS: usize = 0;
const NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL: usize = 1 << 7;
const NS_BACKING_STORE_BUFFERED: usize = 2;
/// `NSPopUpWindowLevel`。系统设置是普通窗口（level 0），101 稳稳压在它上面。
const NS_POPUP_WINDOW_LEVEL: isize = 101;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY: usize = 1 << 4;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: usize = 1 << 8;
const NS_DRAG_OPERATION_COPY: usize = 1;
/// `NSTextAlignmentCenter`
const NS_TEXT_ALIGNMENT_CENTER: isize = 1;

// AppKit 的几何类型。`msg_send!` 的返回值和运行时注册的方法签名都要求它们
// 实现 `Encode`：ObjC 运行时靠类型编码字符串决定参数怎么传（尤其是结构体是
// 走寄存器还是走内存），编码写错不会报错，只会在运行时读到垃圾坐标。
// 编码串照抄 Apple 的定义，与 64 位 ABI 一致。
macro_rules! impl_encode {
    ($ty:ty, $code:expr) => {
        unsafe impl objc::Encode for $ty {
            fn encode() -> objc::Encoding {
                unsafe { objc::Encoding::from_str($code) }
            }
        }
    };
}

impl_encode!(CGPoint, "{CGPoint=dd}");
impl_encode!(CGSize, "{CGSize=dd}");
impl_encode!(CGRect, "{CGRect={CGPoint=dd}{CGSize=dd}}");

#[repr(C)]
#[derive(Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

impl CGRect {
    fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        CGRect {
            origin: CGPoint { x, y },
            size: CGSize { width, height },
        }
    }
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *mut Object;
}

// `CABasicAnimation` 来自 QuartzCore。AppKit 通常已经把它带进进程了
// （`setWantsLayer:` 就要用 `CALayer`），但不显式声明就等于赌链接顺序。
#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

/// `kCGWindowListOptionOnScreenOnly`
const CG_WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1 << 0;
/// `kCGNullWindowID`
const CG_NULL_WINDOW_ID: u32 = 0;

/// 面板尺寸。够放下图标条 + 两行提示，又不至于挡住设置窗口下面的东西。
const PANEL_WIDTH: f64 = 560.0;
const PANEL_HEIGHT: f64 = 132.0;
/// 面板顶边距设置窗口底边的间距。
const PANEL_GAP: f64 = 12.0;

/// 面板上要显示的文案。由 UI 层按当前语言填好传进来——平台层不碰 i18n。
pub struct DropHelperText<'a> {
    /// 拖拽条上的名字，通常是 app 名
    pub app_name: &'a str,
    /// 提示第一行
    pub line1: &'a str,
    /// 提示第二行
    pub line2: &'a str,
}

/// 显示（或重新定位）拖拽授权面板。返回面板此刻是否可见。
///
/// `bundle` 必须是我们自己的 .app 路径（[`super::tcc::enclosing_app_bundle`]）。
/// 从终端直接跑的二进制没有 bundle，调用方就不该调这里——TCC 的责任进程是
/// 终端，拖我们的裸二进制进去毫无意义。
///
/// `allow_fallback` 决定**找不到系统设置窗口时**怎么办：
///
/// - `false`：什么都不显示，返回 `false`。这是刚点完「打开系统设置」时该用的
///   ——`open` 是异步的，设置窗口要 0.5~2 秒才出现，这段时间里先把面板蹦出来
///   会让它孤零零悬在桌面上，指着一个还不存在的列表说「拖到上面去」。等窗口
///   真的出现了再一起亮相，两者才像是一体的。
/// - `true`：退到屏幕底部居中显示。留给「等了几秒还是没找到窗口」的兜底——
///   用户可能把设置开在别的 Space、或者窗口枚举出了意外，这时候显示一个位置
///   不完美的面板，也远好过什么都不给。
pub fn show(bundle: &Path, text: &DropHelperText<'_>, allow_fallback: bool) -> bool {
    debug_assert!(is_main_thread(), "AppKit 只能在主线程调用");
    if !is_main_thread() {
        return false;
    }
    let Some(bundle_str) = bundle.to_str() else {
        return false;
    };

    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];

        let settings_rect = system_settings_window_rect();
        if !should_display(settings_rect.is_some(), allow_fallback) {
            let _: () = msg_send![pool, drain];
            return false;
        }

        let existing = PANEL.load(Ordering::Relaxed);
        let panel = if existing != 0 {
            existing as *mut Object
        } else {
            let panel = build_panel(bundle_str, text);
            if panel.is_null() {
                let _: () = msg_send![pool, drain];
                return false;
            }
            PANEL.store(panel as usize, Ordering::Relaxed);
            panel
        };

        position_panel(panel, settings_rect);
        // 面板重新出现时动画可能已经被系统摘掉了，补挂一次。
        ensure_bounce_animation();
        // orderFrontRegardless：我们不是前台应用（前台是系统设置），
        // 普通的 orderFront: 在非活跃应用上会被忽略。
        let _: () = msg_send![panel, orderFrontRegardless];

        let _: () = msg_send![pool, drain];
    }
    true
}

/// 关掉面板。没开时是空操作。
pub fn hide() {
    // 没开面板就压根不碰 AppKit——UI 层会在好几条路径上无脑调 hide()，
    // 其中一些（比如任务收尾）不保证在主线程上，不该为一次空操作触发断言。
    if PANEL.load(Ordering::Relaxed) == 0 {
        return;
    }
    debug_assert!(is_main_thread(), "AppKit 只能在主线程调用");
    if !is_main_thread() {
        return;
    }
    let panel = PANEL.swap(0, Ordering::Relaxed);
    if panel == 0 {
        return;
    }
    // 箭头是面板内容视图的子视图，随面板一起销毁，这里只清掉悬空的引用。
    ARROW.store(0, Ordering::Relaxed);
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
        let panel = panel as *mut Object;
        let _: () = msg_send![panel, orderOut: std::ptr::null_mut::<Object>()];
        // setReleasedWhenClosed:NO 之后生命周期归我们，这里配平 alloc。
        let _: () = msg_send![panel, release];
        let _: () = msg_send![pool, drain];
    }
}

/// 面板当前是否开着。
pub fn is_visible() -> bool {
    PANEL.load(Ordering::Relaxed) != 0
}

/// 此刻该不该让面板露面。
///
/// 单独拎成一个函数是为了能被测试钉住：这条规则就是「面板先于系统设置窗口
/// 蹦出来」那个体验 bug 的修法，不能在后续重构里被悄悄改掉。
fn should_display(settings_window_found: bool, allow_fallback: bool) -> bool {
    settings_window_found || allow_fallback
}

fn is_main_thread() -> bool {
    unsafe {
        let is_main: BOOL = msg_send![class!(NSThread), isMainThread];
        is_main == YES
    }
}

/// SAFETY: 调用方保证在主线程、且已建好 autorelease pool。
unsafe fn build_panel(bundle_str: &str, text: &DropHelperText<'_>) -> *mut Object {
    let content_rect = CGRect::new(0.0, 0.0, PANEL_WIDTH, PANEL_HEIGHT);
    let style = NS_WINDOW_STYLE_MASK_BORDERLESS | NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL;

    let panel: *mut Object = msg_send![class!(NSPanel), alloc];
    let panel: *mut Object = msg_send![
        panel,
        initWithContentRect: content_rect
        styleMask: style
        backing: NS_BACKING_STORE_BUFFERED
        defer: NO
    ];
    if panel.is_null() {
        return std::ptr::null_mut();
    }

    let _: () = msg_send![panel, setLevel: NS_POPUP_WINDOW_LEVEL];
    let _: () = msg_send![
        panel,
        setCollectionBehavior: NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
            | NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY
            | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY
    ];
    // 我们始终是「别人在前台」时显示的，绝不能跟着自己失活而消失。
    let _: () = msg_send![panel, setHidesOnDeactivate: NO];
    let _: () = msg_send![panel, setOpaque: NO];
    let _: () = msg_send![panel, setHasShadow: YES];
    // 关闭时不自动 release：面板指针存在 PANEL 里，生命周期由 hide() 配平。
    let _: () = msg_send![panel, setReleasedWhenClosed: NO];
    let clear: *mut Object = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![panel, setBackgroundColor: clear];

    let content = build_content_view(bundle_str, text);
    let _: () = msg_send![panel, setContentView: content];

    panel
}

/// SAFETY: 主线程 + autorelease pool。
unsafe fn build_content_view(bundle_str: &str, text: &DropHelperText<'_>) -> *mut Object {
    let frame = CGRect::new(0.0, 0.0, PANEL_WIDTH, PANEL_HEIGHT);
    let container: *mut Object = msg_send![class!(NSView), alloc];
    let container: *mut Object = msg_send![container, initWithFrame: frame];
    let _: () = msg_send![container, setWantsLayer: YES];
    let layer: *mut Object = msg_send![container, layer];
    if !layer.is_null() {
        let _: () = msg_send![layer, setCornerRadius: 16.0f64];
        let bg = srgb_color(0.16, 0.16, 0.17, 0.98);
        let cg: *mut Object = msg_send![bg, CGColor];
        let _: () = msg_send![layer, setBackgroundColor: cg];
    }

    // 左侧：可拖拽的图标条。这是整个面板唯一有交互的元素。
    let chip_frame = CGRect::new(28.0, 34.0, 220.0, 64.0);
    let chip: *mut Object = msg_send![drag_source_class(), alloc];
    let chip: *mut Object = msg_send![chip, initWithFrame: chip_frame];
    set_drag_source_path(chip, bundle_str);
    let _: () = msg_send![chip, setWantsLayer: YES];
    let chip_layer: *mut Object = msg_send![chip, layer];
    if !chip_layer.is_null() {
        let _: () = msg_send![chip_layer, setCornerRadius: 12.0f64];
        let bg = srgb_color(0.26, 0.26, 0.28, 1.0);
        let cg: *mut Object = msg_send![bg, CGColor];
        let _: () = msg_send![chip_layer, setBackgroundColor: cg];
    }

    // 图标：直接问 NSWorkspace 要，拿到的就是访达里显示的那个。
    let icon = workspace_icon(bundle_str);
    if !icon.is_null() {
        let icon_frame = CGRect::new(12.0, 12.0, 40.0, 40.0);
        let view: *mut Object = msg_send![class!(NSImageView), alloc];
        let view: *mut Object = msg_send![view, initWithFrame: icon_frame];
        let _: () = msg_send![view, setImage: icon];
        let _: () = msg_send![chip, addSubview: view];
        let _: () = msg_send![view, release];
    }

    let name_label = label(
        CGRect::new(62.0, 20.0, 146.0, 24.0),
        text.app_name,
        15.0,
        true,
        false,
    );
    let _: () = msg_send![chip, addSubview: name_label];
    let _: () = msg_send![name_label, release];

    let _: () = msg_send![container, addSubview: chip];
    let _: () = msg_send![chip, release];

    // 中间：箭头。用文本而不是画图——一个字符省掉一整套绘制代码。
    let arrow = label(CGRect::new(268.0, 46.0, 40.0, 40.0), "↑", 30.0, false, true);
    let _: () = msg_send![arrow, setWantsLayer: YES];
    let _: () = msg_send![container, addSubview: arrow];
    ARROW.store(arrow as usize, Ordering::Relaxed);
    ensure_bounce_animation();
    let _: () = msg_send![arrow, release];

    // 右侧：两行提示。
    let line1 = label(
        CGRect::new(318.0, 62.0, 220.0, 22.0),
        text.line1,
        13.0,
        true,
        false,
    );
    let _: () = msg_send![container, addSubview: line1];
    let _: () = msg_send![line1, release];

    let line2 = label(
        CGRect::new(318.0, 40.0, 220.0, 22.0),
        text.line2,
        13.0,
        false,
        false,
    );
    let _: () = msg_send![container, addSubview: line2];
    let _: () = msg_send![line2, release];

    let _: () = msg_send![container, autorelease];
    container
}

/// 给箭头挂上上下弹跳的动画；已经挂着就什么都不做。
///
/// 为什么要「已经挂着就不做」：[`show`] 在跟贴设置窗口的十几秒里会被反复调用，
/// 每次都 `addAnimation:forKey:` 的话动画会被同 key 顶掉重来，四分之一秒重启
/// 一次，看上去就是箭头卡死在原地抖。
///
/// 弹跳幅度和节奏是照着 .dmg 安装窗口那类引导箭头调的：8pt、0.6 秒一个来回、
/// ease-in-out。再大就吵，再快就急。
///
/// SAFETY: 主线程 + autorelease pool。
unsafe fn ensure_bounce_animation() {
    let arrow = ARROW.load(Ordering::Relaxed);
    if arrow == 0 {
        return;
    }
    let arrow = arrow as *mut Object;
    let layer: *mut Object = msg_send![arrow, layer];
    if layer.is_null() {
        return;
    }

    let key = ns_string("bounce");
    let existing: *mut Object = msg_send![layer, animationForKey: key];
    if !existing.is_null() {
        let _: () = msg_send![key, release];
        return;
    }

    let path = ns_string("position.y");
    let anim: *mut Object = msg_send![class!(CABasicAnimation), animationWithKeyPath: path];
    let _: () = msg_send![path, release];
    if anim.is_null() {
        let _: () = msg_send![key, release];
        return;
    }

    // 动画作用在 layer 的 position 上，坐标系是父 layer 的。macOS 的视图默认
    // 不翻转，y 向上——所以「+8」是往上跳，正好指向要拖进去的列表。
    let position: CGPoint = msg_send![layer, position];
    let from: *mut Object = msg_send![class!(NSNumber), numberWithDouble: position.y];
    let to: *mut Object = msg_send![class!(NSNumber), numberWithDouble: position.y + 8.0];
    let _: () = msg_send![anim, setFromValue: from];
    let _: () = msg_send![anim, setToValue: to];
    let _: () = msg_send![anim, setDuration: 0.6f64];
    let _: () = msg_send![anim, setAutoreverses: YES];
    // `HUGE_VALF`，即无限重复。
    let _: () = msg_send![anim, setRepeatCount: f32::INFINITY];

    // 缓动函数按名字取。这里自己造 NSString 而不是引 `kCAMediaTimingFunctionEaseInEaseOut`
    // 符号——那个常量的值就是字面量 "easeInEaseOut"，自己传省掉一处外部链接。
    let name = ns_string("easeInEaseOut");
    let timing: *mut Object = msg_send![class!(CAMediaTimingFunction), functionWithName: name];
    let _: () = msg_send![name, release];
    if !timing.is_null() {
        let _: () = msg_send![anim, setTimingFunction: timing];
    }

    let _: () = msg_send![layer, addAnimation: anim forKey: key];
    let _: () = msg_send![key, release];
}

/// 造一个只读文本。`emphasized` 走加粗，`centered` 走居中。
///
/// SAFETY: 主线程 + autorelease pool。返回值是 +1 引用，调用方负责 release。
unsafe fn label(
    frame: CGRect,
    text: &str,
    size: f64,
    emphasized: bool,
    centered: bool,
) -> *mut Object {
    let field: *mut Object = msg_send![class!(NSTextField), alloc];
    let field: *mut Object = msg_send![field, initWithFrame: frame];
    let ns_text = ns_string(text);
    let _: () = msg_send![field, setStringValue: ns_text];
    let _: () = msg_send![ns_text, release];
    let _: () = msg_send![field, setEditable: NO];
    let _: () = msg_send![field, setSelectable: NO];
    let _: () = msg_send![field, setBezeled: NO];
    let _: () = msg_send![field, setDrawsBackground: NO];
    let font: *mut Object = if emphasized {
        msg_send![class!(NSFont), boldSystemFontOfSize: size]
    } else {
        msg_send![class!(NSFont), systemFontOfSize: size]
    };
    let _: () = msg_send![field, setFont: font];
    let color = if emphasized {
        srgb_color(1.0, 1.0, 1.0, 1.0)
    } else {
        srgb_color(0.78, 0.78, 0.80, 1.0)
    };
    let _: () = msg_send![field, setTextColor: color];
    if centered {
        let _: () = msg_send![field, setAlignment: NS_TEXT_ALIGNMENT_CENTER];
    }
    field
}

/// SAFETY: 主线程 + autorelease pool。返回 autoreleased 的 `NSColor`。
unsafe fn srgb_color(r: f64, g: f64, b: f64, a: f64) -> *mut Object {
    msg_send![class!(NSColor), colorWithSRGBRed: r green: g blue: b alpha: a]
}

/// SAFETY: 返回 +1 引用的 `NSString`，调用方负责 release。
unsafe fn ns_string(s: &str) -> *mut Object {
    let obj: *mut Object = msg_send![class!(NSString), alloc];
    msg_send![
        obj,
        initWithBytes: s.as_ptr() as *const std::ffi::c_void
        length: s.len()
        encoding: NS_UTF8_STRING_ENCODING
    ]
}

/// SAFETY: 主线程 + autorelease pool。返回 autoreleased 的 `NSImage`，可能为 null。
unsafe fn workspace_icon(path: &str) -> *mut Object {
    let ws: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
    if ws.is_null() {
        return std::ptr::null_mut();
    }
    let ns_path = ns_string(path);
    let icon: *mut Object = msg_send![ws, iconForFile: ns_path];
    let _: () = msg_send![ns_path, release];
    icon
}

// ---------------------------------------------------------------------------
// 拖拽源
// ---------------------------------------------------------------------------

/// 存 .app 路径的 ivar 名。存 `NSString` 而不是 Rust 字符串：ObjC 方法里要
/// 取出来直接喂给 `NSURL`，中间不必再过一次编码转换。
const DRAG_PATH_IVAR: &str = "_qcDragPath";

/// 运行时声明的拖拽源视图类。
///
/// 三个方法缺一不可：
///
/// - `acceptsFirstMouse:` 返回 YES。**这是最容易踩的坑**：面板是
///   nonactivating 的，我们的应用不在前台，默认情况下落在非活跃窗口上的第一
///   次点击只会用来激活窗口、不会传给视图——用户会觉得「第一下拖不动」。
/// - `mouseDown:` 直接起拖。按 Apple 的惯例应该在 `mouseDragged:` 里等超过
///   阈值再起，但那要自己维护起点和状态；这个视图除了被拖没有别的用途，
///   在 mouseDown 就起拖既简单又不会误触发。
/// - `draggingSession:sourceOperationMaskForDraggingContext:` 是
///   `NSDraggingSource` 的必需方法，不实现拖拽会直接被拒绝。
fn drag_source_class() -> &'static Class {
    use std::sync::OnceLock;
    static CLASS: OnceLock<usize> = OnceLock::new();
    let ptr = *CLASS.get_or_init(|| {
        let superclass = class!(NSView);
        let mut decl = ClassDecl::new("QCPermissionDragSourceView", superclass)
            .expect("QCPermissionDragSourceView 类名冲突");
        decl.add_ivar::<*mut Object>(DRAG_PATH_IVAR);

        unsafe {
            decl.add_method(
                sel!(acceptsFirstMouse:),
                accepts_first_mouse as extern "C" fn(&Object, Sel, *mut Object) -> BOOL,
            );
            decl.add_method(
                sel!(mouseDown:),
                mouse_down as extern "C" fn(&mut Object, Sel, *mut Object),
            );
            decl.add_method(
                sel!(hitTest:),
                hit_test as extern "C" fn(&mut Object, Sel, CGPoint) -> *mut Object,
            );
            decl.add_method(
                sel!(draggingSession:sourceOperationMaskForDraggingContext:),
                drag_operation
                    as extern "C" fn(&Object, Sel, *mut Object, isize) -> usize,
            );
        }

        decl.register() as *const Class as usize
    });
    unsafe { &*(ptr as *const Class) }
}

extern "C" fn accepts_first_mouse(_this: &Object, _cmd: Sel, _event: *mut Object) -> BOOL {
    YES
}

/// 子视图（图标、名字）会自己吃掉点击，让拖拽只在它们的缝隙里生效。
/// 整条 chip 都该是拖拽把手，所以命中测试统一收归自己。
extern "C" fn hit_test(this: &mut Object, _cmd: Sel, point: CGPoint) -> *mut Object {
    unsafe {
        let superview: *mut Object = msg_send![this, superview];
        if superview.is_null() {
            return std::ptr::null_mut();
        }
        let frame: CGRect = msg_send![this, frame];
        let inside = point.x >= frame.origin.x
            && point.x <= frame.origin.x + frame.size.width
            && point.y >= frame.origin.y
            && point.y <= frame.origin.y + frame.size.height;
        if inside {
            this as *mut Object
        } else {
            std::ptr::null_mut()
        }
    }
}

extern "C" fn drag_operation(
    _this: &Object,
    _cmd: Sel,
    _session: *mut Object,
    _context: isize,
) -> usize {
    // 拖的是「把这个 app 加进列表」，语义是拷贝一份引用，不是搬走文件。
    NS_DRAG_OPERATION_COPY
}

extern "C" fn mouse_down(this: &mut Object, _cmd: Sel, event: *mut Object) {
    unsafe {
        let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];

        let path: *mut Object = *this.get_ivar(DRAG_PATH_IVAR);
        if path.is_null() {
            let _: () = msg_send![pool, drain];
            return;
        }

        let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath: path];
        if url.is_null() {
            let _: () = msg_send![pool, drain];
            return;
        }

        // 拖拽项的载荷就是 .app 的 file URL——和点 `+` 号在文件选择器里选中它
        // 完全等价，隐私列表就认这个。
        let item: *mut Object = msg_send![class!(NSDraggingItem), alloc];
        let item: *mut Object = msg_send![item, initWithPasteboardWriter: url];
        if item.is_null() {
            let _: () = msg_send![pool, drain];
            return;
        }

        // 拖起来跟手的图像用 app 图标本身。取不到就让系统用默认表现，
        // 不值得为此中断整个拖拽。
        let bounds: CGRect = msg_send![this, bounds];
        let ws: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let icon: *mut Object = msg_send![ws, iconForFile: path];
        if !icon.is_null() {
            let size = CGSize {
                width: 48.0,
                height: 48.0,
            };
            let _: () = msg_send![icon, setSize: size];
            let frame = CGRect::new(
                bounds.origin.x + 12.0,
                bounds.origin.y + 8.0,
                size.width,
                size.height,
            );
            let _: () = msg_send![item, setDraggingFrame: frame contents: icon];
        }

        let items: *mut Object = msg_send![class!(NSArray), arrayWithObject: item];
        let _: () = msg_send![item, release];

        let _session: *mut Object = msg_send![
            this,
            beginDraggingSessionWithItems: items
            event: event
            source: this
        ];

        let _: () = msg_send![pool, drain];
    }
}

/// 把 .app 路径塞进视图的 ivar。存的是 +1 的 `NSString`——面板活多久它活多久，
/// 面板销毁时随视图一起走，不单独回收。
///
/// SAFETY: `view` 必须是 [`drag_source_class`] 的实例。
unsafe fn set_drag_source_path(view: *mut Object, path: &str) {
    let ns_path = ns_string(path);
    let view_ref = &mut *view;
    view_ref.set_ivar::<*mut Object>(DRAG_PATH_IVAR, ns_path);
}

// ---------------------------------------------------------------------------
// 定位
// ---------------------------------------------------------------------------

/// 把面板贴到系统设置窗口的正下方；`settings_rect` 为 `None` 时退到主屏底部。
///
/// 定位每 150ms 就跑一次，日志不能每次都写。只在**落位方式发生变化**时记一行
/// （首次出现、贴窗口↔兜底之间来回），既能看出「有没有找到设置窗口」，
/// 又不会把日志刷爆。
///
/// SAFETY: 主线程 + autorelease pool。
unsafe fn position_panel(panel: *mut Object, settings_rect: Option<CGRect>) {
    /// 0=还没定过位，1=贴着设置窗口，2=走了兜底
    static LAST_MODE: AtomicUsize = AtomicUsize::new(0);

    let how;
    let mode;
    let target = match settings_rect {
        // 设置窗口的矩形是 Quartz 坐标（原点在主屏左上、Y 向下），
        // 而 setFrameOrigin: 要 Cocoa 坐标（原点在主屏左下、Y 向上）。
        Some(rect) => {
            let main_height = main_screen_height();
            let settings_bottom_cocoa = main_height - (rect.origin.y + rect.size.height);
            let x = rect.origin.x + (rect.size.width - PANEL_WIDTH) / 2.0;
            let y = settings_bottom_cocoa - PANEL_GAP - PANEL_HEIGHT;
            how = format!(
                "贴设置窗口（{:.0}x{:.0} @ {:.0},{:.0}）",
                rect.size.width, rect.size.height, rect.origin.x, rect.origin.y
            );
            mode = 1;
            CGPoint { x, y }
        }
        None => {
            how = "未找到设置窗口，退到屏幕底部".to_string();
            mode = 2;
            fallback_origin()
        }
    };

    // 贴到屏幕外就没意义了。夹回可见区域，宁可和设置窗口重叠一点。
    let visible = main_screen_visible_frame();
    let x = target
        .x
        .max(visible.origin.x + 8.0)
        .min(visible.origin.x + visible.size.width - PANEL_WIDTH - 8.0);
    let y = target
        .y
        .max(visible.origin.y + 8.0)
        .min(visible.origin.y + visible.size.height - PANEL_HEIGHT - 8.0);

    let _: () = msg_send![panel, setFrameOrigin: CGPoint { x, y }];

    if LAST_MODE.swap(mode, Ordering::Relaxed) != mode {
        crate::log!("拖拽授权面板：{how} → 落点 {x:.0},{y:.0}");
    }
}

unsafe fn fallback_origin() -> CGPoint {
    let visible = main_screen_visible_frame();
    CGPoint {
        x: visible.origin.x + (visible.size.width - PANEL_WIDTH) / 2.0,
        y: visible.origin.y + 40.0,
    }
}

unsafe fn main_screen_height() -> f64 {
    let screens: *mut Object = msg_send![class!(NSScreen), screens];
    if screens.is_null() {
        return 0.0;
    }
    let count: usize = msg_send![screens, count];
    if count == 0 {
        return 0.0;
    }
    // Quartz 全局坐标的原点锚在**第一块**屏幕（screens[0]）的左上角，
    // 不是 mainScreen（那是当前有键盘焦点的屏幕）。换算必须用前者。
    let first: *mut Object = msg_send![screens, objectAtIndex: 0usize];
    let frame: CGRect = msg_send![first, frame];
    frame.size.height
}

unsafe fn main_screen_visible_frame() -> CGRect {
    let screen: *mut Object = msg_send![class!(NSScreen), mainScreen];
    if screen.is_null() {
        return CGRect::new(0.0, 0.0, 1440.0, 900.0);
    }
    msg_send![screen, visibleFrame]
}

/// 系统设置主窗口在 Quartz 全局坐标里的矩形。
///
/// 按 **PID** 而不是窗口标题或 owner 名字来找：后两者都是本地化的，中文系统
/// 上是「系统设置」，英文是 "System Settings"，Ventura 之前又叫
/// "System Preferences"。而且窗口标题（`kCGWindowName`）自 10.15 起要屏幕录制
/// 权限才读得到——我们只读 bounds 和 ownerPID，这两项不需要任何权限。
///
/// SAFETY: 主线程 + autorelease pool。
unsafe fn system_settings_window_rect() -> Option<CGRect> {
    let pid = system_settings_pid()?;

    let list = CGWindowListCopyWindowInfo(CG_WINDOW_LIST_ON_SCREEN_ONLY, CG_NULL_WINDOW_ID);
    if list.is_null() {
        return None;
    }
    // CFArray / CFDictionary 与 NSArray / NSDictionary 是 toll-free bridged 的，
    // 直接当 ObjC 对象发消息，省掉一整套 CF 的取值样板。
    let count: usize = msg_send![list, count];

    let key_pid = ns_string("kCGWindowOwnerPID");
    let key_bounds = ns_string("kCGWindowBounds");
    let mut best: Option<CGRect> = None;

    for i in 0..count {
        let info: *mut Object = msg_send![list, objectAtIndex: i];
        if info.is_null() {
            continue;
        }
        let owner: *mut Object = msg_send![info, objectForKey: key_pid];
        if owner.is_null() {
            continue;
        }
        let owner_pid: i32 = msg_send![owner, intValue];
        if owner_pid != pid {
            continue;
        }
        let bounds_dict: *mut Object = msg_send![info, objectForKey: key_bounds];
        if bounds_dict.is_null() {
            continue;
        }
        let Some(rect) = rect_from_bounds_dict(bounds_dict) else {
            continue;
        };
        // 设置进程还会有工具提示、弹出层之类的小窗口。取面积最大的那个，
        // 并要求它有个正经窗口的尺寸，免得贴到一个 tooltip 底下。
        if rect.size.width < 400.0 || rect.size.height < 300.0 {
            continue;
        }
        let area = rect.size.width * rect.size.height;
        if best.is_none_or(|b| area > b.size.width * b.size.height) {
            best = Some(rect);
        }
    }

    let _: () = msg_send![key_pid, release];
    let _: () = msg_send![key_bounds, release];
    let _: () = msg_send![list, release];
    best
}

/// `kCGWindowBounds` 是个 `{X, Y, Width, Height}` 的字典，不是 CGRect。
///
/// SAFETY: 主线程 + autorelease pool。
unsafe fn rect_from_bounds_dict(dict: *mut Object) -> Option<CGRect> {
    let mut values = [0.0f64; 4];
    for (slot, key) in values.iter_mut().zip(["X", "Y", "Width", "Height"]) {
        let ns_key = ns_string(key);
        let number: *mut Object = msg_send![dict, objectForKey: ns_key];
        let _: () = msg_send![ns_key, release];
        if number.is_null() {
            return None;
        }
        *slot = msg_send![number, doubleValue];
    }
    Some(CGRect::new(values[0], values[1], values[2], values[3]))
}

/// 系统设置进程的 PID。没在运行就返回 `None`。
///
/// Ventura 改名叫「系统设置」，但 bundle id 一直是 `com.apple.systempreferences`，
/// 没跟着改。
///
/// SAFETY: 主线程 + autorelease pool。
unsafe fn system_settings_pid() -> Option<i32> {
    let bundle_id = ns_string("com.apple.systempreferences");
    let apps: *mut Object = msg_send![
        class!(NSRunningApplication),
        runningApplicationsWithBundleIdentifier: bundle_id
    ];
    let _: () = msg_send![bundle_id, release];
    if apps.is_null() {
        return None;
    }
    let count: usize = msg_send![apps, count];
    if count == 0 {
        return None;
    }
    let app: *mut Object = msg_send![apps, objectAtIndex: 0usize];
    if app.is_null() {
        return None;
    }
    let pid: i32 = msg_send![app, processIdentifier];
    (pid > 0).then_some(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 类注册只能发生一次——`ClassDecl::new` 遇到重名会返回 `None` 并 panic。
    /// 这里连着取两次，验证 `OnceLock` 真的挡住了第二次注册。
    #[test]
    fn drag_source_class_registers_exactly_once() {
        let first = drag_source_class() as *const Class;
        let second = drag_source_class() as *const Class;
        assert_eq!(first, second, "拖拽源类不能被注册两次");
    }

    /// 面板绝不能比系统设置窗口先出现——那会让它孤零零悬在桌面上，
    /// 指着一个还不存在的列表说「拖到上面去」。只有在等超时之后，
    /// 才允许用兜底位置显示。
    #[test]
    fn panel_waits_for_the_settings_window_before_first_appearing() {
        assert!(
            !should_display(false, false),
            "设置窗口还没出现、也还没等到超时，此时必须什么都不显示"
        );
        assert!(
            should_display(true, false),
            "设置窗口一出现就该立刻贴上去"
        );
        assert!(
            should_display(false, true),
            "等超时之后要有兜底，不能让用户点完按钮什么都没等到"
        );
        assert!(should_display(true, true));
    }

    /// 没开面板时 hide() 必须是安全的空操作——UI 层会在多个路径上无脑调它。
    #[test]
    fn hide_without_show_is_a_noop() {
        assert!(!is_visible());
        hide();
        assert!(!is_visible());
    }

    /// 系统设置没开着时定位要老实返回 None，让调用方走屏幕底部的兜底，
    /// 而不是拿一个垃圾矩形去摆面板。
    #[test]
    fn settings_window_lookup_is_safe_when_not_running() {
        unsafe {
            let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
            // 结果依赖运行环境（设置可能正开着），这里只验证不崩、且返回的
            // 矩形（如果有）是个正经窗口尺寸。
            if let Some(rect) = system_settings_window_rect() {
                assert!(rect.size.width >= 400.0 && rect.size.height >= 300.0);
            }
            let _: () = msg_send![pool, drain];
        }
    }
}
