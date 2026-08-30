//! WMI 的最小客户端：连命名空间、跑 WQL、调无入参的方法。
//!
//! 传感器（风扇转速、CPU 温度）在 Windows 上没有统一的免驱动接口，能读到
//! 的那几家都把数据挂在 WMI 上（见 [`super::thermal`]），所以这里只实现
//! 传感器用得到的三件事，不做通用 WMI 封装。
//!
//! COM 上的两个决定：
//!
//! - **每线程一条连接**，存在 `thread_local` 里。`ConnectServer` 要几十
//!   毫秒，两秒一拍重连太浪费；而 COM 代理是有公寓归属的，跨线程共享一条
//!   连接需要自己做封送。采样任务落在后台线程池的哪个线程上不固定，但线程
//!   总数有限，一线程一条正好。
//! - **不 Release、不 CoUninitialize**。连接活到进程结束；线程池线程退出时
//!   COM 可能已经拆了，那时候再 Release 是自找崩溃。少释放一个进程级单例
//!   没有代价。

use std::cell::RefCell;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::Mutex;
use winapi::ctypes::c_void;
use winapi::shared::rpcdce::{
    RPC_C_AUTHN_LEVEL_CALL, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use winapi::shared::winerror::{RPC_E_CHANGED_MODE, RPC_E_TOO_LATE, S_FALSE, S_OK};
use winapi::shared::wtypes::{BSTR, VT_BSTR, VT_I2, VT_I4, VT_R4, VT_R8, VT_UI1, VT_UI4};
use winapi::shared::wtypesbase::CLSCTX_INPROC_SERVER;
use winapi::um::combaseapi::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CoSetProxyBlanket,
};
use winapi::um::oaidl::VARIANT;
use winapi::um::objbase::COINIT_MULTITHREADED;
use winapi::um::objidlbase::EOAC_NONE;
use winapi::um::oleauto::{SysAllocString, SysFreeString, VariantClear};
use winapi::um::unknwnbase::IUnknown;
use winapi::um::wbemcli::{
    CLSID_WbemLocator, IEnumWbemClassObject, IID_IWbemLocator, IWbemClassObject, IWbemLocator,
    IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

use super::registry::{from_wide, to_wide};

/// 方法入参。CIM 的类型系统很大，这里只覆盖传感器接口用到的两种。
pub enum Arg<'a> {
    Number(u32),
    Text(&'a str),
}

/// WMI 里取到的一个值。只保留传感器会用到的两类。
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
}

impl Value {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::Text(_) => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            Value::Number(_) => None,
        }
    }
}

/// 一个命名空间的连接。
pub struct Wmi {
    services: *mut IWbemServices,
}

impl Wmi {
    /// 连某个命名空间（`root\\WMI`、`root\\LibreHardwareMonitor` …）。
    ///
    /// 命名空间不存在、或者当前权限读不了，都返回 `None`；调用方要把这个
    /// 结果缓存下来，别每拍重试——失败路径同样要几十毫秒。
    pub fn connect(namespace: &str) -> Option<Wmi> {
        Wmi::connect_diagnostic(namespace).ok()
    }

    /// 同 [`Wmi::connect`]，但把 HRESULT 带出来。COM 的失败原因全在返回码里
    /// （0x80041003 = 拒绝访问，0x8004100E = 命名空间不存在），排查时没有它
    /// 就只能靠猜。
    pub fn connect_diagnostic(namespace: &str) -> Result<Wmi, i32> {
        init_com_on_this_thread().ok_or(E_COM_INIT_FAILED)?;
        let mut locator: *mut IWbemLocator = null_mut();
        // SAFETY: 出参在栈上，失败时不会被写。
        let hr = unsafe {
            CoCreateInstance(
                &CLSID_WbemLocator,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_IWbemLocator,
                &mut locator as *mut *mut IWbemLocator as *mut *mut c_void,
            )
        };
        if hr < 0 || locator.is_null() {
            return Err(hr);
        }
        let path = Bstr::new(namespace);
        let mut services: *mut IWbemServices = null_mut();
        // SAFETY: locator 创建成功；除命名空间外全部传空 = 用当前用户身份连本机。
        let hr = unsafe {
            (*locator).ConnectServer(
                path.as_ptr(),
                null_mut(),
                null_mut(),
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                &mut services,
            )
        };
        // SAFETY: locator 只用来建连接，之后就没人引用了。
        unsafe { (*locator).Release() };
        if hr < 0 || services.is_null() {
            return Err(hr);
        }
        // 不设代理身份，WMI 会用匿名身份去问驱动，厂商类一律「拒绝访问」。
        // SAFETY: services 是刚拿到的代理，参数按 MSDN 的本机连接写法。
        let hr = unsafe {
            CoSetProxyBlanket(
                services as *mut IUnknown,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                null_mut(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                null_mut(),
                EOAC_NONE,
            )
        };
        if hr < 0 {
            // SAFETY: 拿到过 services，这里是唯一的引用。
            unsafe { (*services).Release() };
            return Err(hr);
        }
        Ok(Wmi { services })
    }

    /// 跑一条 WQL，按行取出指定属性。取不到的属性是 `None`。
    pub fn query(&self, wql: &str, props: &[&str]) -> Vec<Vec<Option<Value>>> {
        self.query_diagnostic(wql, props).unwrap_or_default()
    }

    /// 同 [`Wmi::query`]，失败时带上 HRESULT。
    pub fn query_diagnostic(
        &self,
        wql: &str,
        props: &[&str],
    ) -> Result<Vec<Vec<Option<Value>>>, i32> {
        let mut rows = Vec::new();
        let language = Bstr::new("WQL");
        let query = Bstr::new(wql);
        let mut enumerator: *mut IEnumWbemClassObject = null_mut();
        // SAFETY: services 有效，两个 BSTR 在调用期间存活。
        let hr = unsafe {
            (*self.services).ExecQuery(
                language.as_ptr(),
                query.as_ptr(),
                (WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY) as i32,
                null_mut(),
                &mut enumerator,
            )
        };
        if hr < 0 || enumerator.is_null() {
            return Err(hr);
        }
        // 半同步模式（`WBEM_FLAG_RETURN_IMMEDIATELY`）下 `ExecQuery` 几乎总是
        // 成功，真正的失败在 `Next` 上报——「拒绝访问」就是这么来的。把它当成
        // 「枚举完了」的话，没权限和没实例长得一模一样，排查时全是死路。
        let mut failure = 0;
        loop {
            let mut object: *mut IWbemClassObject = null_mut();
            let mut returned = 0;
            // SAFETY: enumerator 有效，一次要一个对象。
            let hr =
                unsafe { (*enumerator).Next(WBEM_INFINITE as i32, 1, &mut object, &mut returned) };
            if hr < 0 {
                failure = hr;
                break;
            }
            if returned == 0 || object.is_null() {
                break;
            }
            rows.push(props.iter().map(|p| get_property(object, p)).collect());
            // SAFETY: 这一行的值已经拷成 Rust 类型，对象可以放了。
            unsafe { (*object).Release() };
        }
        // SAFETY: 枚举结束，没有别的引用。
        unsafe { (*enumerator).Release() };
        if failure < 0 && rows.is_empty() {
            return Err(failure);
        }
        Ok(rows)
    }

    /// 取某个类第一个实例的 `__PATH`，调方法要用它定位对象。
    ///
    /// 查 `SELECT *` 而不是 `SELECT __PATH`：系统属性在投影里的行为各家
    /// provider 不一致，`*` 一定带上 `__PATH`。类只有一个实例，多取几个
    /// 字段不值几微秒。
    pub fn first_instance_path(&self, class: &str) -> Option<String> {
        self.query(&format!("SELECT * FROM {class}"), &["__PATH"])
            .into_iter()
            .find_map(|row| match row.into_iter().next() {
                Some(Some(Value::Text(path))) if !path.is_empty() => Some(path),
                _ => None,
            })
    }

    /// 调一个**没有入参**的方法，取出参里的一个数。
    ///
    /// 厂商传感器接口清一色是这个形状（`GetFan1Speed(out Data)`）。有入参的
    /// 方法还要 `GetMethod` + `SpawnInstance` 造入参对象，这里用不上就不做。
    pub fn call_number(&self, object_path: &str, method: &str, out_param: &str) -> Option<f64> {
        self.call_number_diagnostic(object_path, method, out_param)
            .ok()
            .flatten()
    }

    /// 调一个带入参、出参取一个数的方法。
    ///
    /// 有入参就不能像 [`Wmi::call_number`] 那样把 `pInParams` 传空：得先拿到
    /// 类定义、`GetMethod` 要到入参签名、`SpawnInstance` 造一个实例、`Put`
    /// 塞进参数，再 `ExecMethod`。联想新机型的传感器全在这种形状的
    /// `GetFeatureValue(IDs)` 后面。
    ///
    /// **入参要么全给、要么别给**：漏掉一个可选参数，provider 回的是
    /// 0x80041008（WBEM_E_INVALID_PARAMETER），和类型写错的报错很像。
    pub fn call_number_with_args(
        &self,
        class: &str,
        object_path: &str,
        method: &str,
        args: &[(&str, Arg)],
        out_param: &str,
    ) -> Result<Option<f64>, i32> {
        let class_object = self.get_object(class)?;
        let method_name = to_wide(method);
        let mut in_signature: *mut IWbemClassObject = null_mut();
        // SAFETY: class_object 有效，method_name 以 NUL 结尾，出参在栈上。
        let hr = unsafe {
            (*class_object.0).GetMethod(
                method_name.as_ptr(),
                0,
                &mut in_signature,
                std::ptr::null_mut(),
            )
        };
        if hr < 0 || in_signature.is_null() {
            return Err(hr);
        }
        let in_signature = Owned(in_signature);
        let mut instance: *mut IWbemClassObject = null_mut();
        // SAFETY: in_signature 有效，出参在栈上。
        let hr = unsafe { (*in_signature.0).SpawnInstance(0, &mut instance) };
        if hr < 0 || instance.is_null() {
            return Err(hr);
        }
        let instance = Owned(instance);
        for (name, arg) in args {
            let name_wide = to_wide(name);
            // 字符串参数的 BSTR 要活到 Put 返回（Put 会自己拷一份）。
            let text = match arg {
                Arg::Text(text) => Some(Bstr::new(text)),
                Arg::Number(_) => None,
            };
            // SAFETY: VARIANT 是 POD；下面按 vt 只写对应的那一支。
            let mut value: VARIANT = unsafe { std::mem::zeroed() };
            unsafe {
                let inner = value.n1.n2_mut();
                match arg {
                    // CIM 的 `uint32` 在 VARIANT 里是 **VT_I4**，不是 VT_UI4：
                    // WMI 只用自动化兼容的那一小撮类型，无符号 32 位不在其中，
                    // 按位塞进 i4。写成 VT_UI4 的话每次调用都是 0x80041005
                    // （WBEM_E_TYPE_MISMATCH），错误码只说「类型不对」，不会
                    // 告诉你问题出在 VARIANT 的标签上。
                    Arg::Number(number) => {
                        inner.vt = VT_I4 as u16;
                        *inner.n3.lVal_mut() = *number as i32;
                    }
                    Arg::Text(_) => {
                        inner.vt = VT_BSTR as u16;
                        *inner.n3.bstrVal_mut() = text.as_ref().map_or(null_mut(), Bstr::as_ptr);
                    }
                }
            }
            // 第四个参数传 0：往**实例**上 Put 时类型取自类定义，自己再指定
            // 一遍只会多一处对不上的机会。
            // SAFETY: instance 有效，name_wide 以 NUL 结尾，value 在栈上。
            let hr = unsafe { (*instance.0).Put(name_wide.as_ptr(), 0, &mut value, 0) };
            if hr < 0 {
                return Err(hr);
            }
        }
        let path = Bstr::new(object_path);
        let name = Bstr::new(method);
        let mut out: *mut IWbemClassObject = null_mut();
        // SAFETY: 两个 BSTR 与入参实例都在调用期间存活。
        let hr = unsafe {
            (*self.services).ExecMethod(
                path.as_ptr(),
                name.as_ptr(),
                0,
                null_mut(),
                instance.0,
                &mut out,
                null_mut(),
            )
        };
        if hr < 0 || out.is_null() {
            return Err(hr);
        }
        let out = Owned(out);
        Ok(get_property(out.0, out_param).and_then(|v| v.as_number()))
    }

    /// 取一个类定义或实例对象。
    fn get_object(&self, path: &str) -> Result<Owned<IWbemClassObject>, i32> {
        let path = Bstr::new(path);
        let mut object: *mut IWbemClassObject = null_mut();
        // SAFETY: services 有效，BSTR 在调用期间存活，出参在栈上。
        let hr = unsafe {
            (*self.services).GetObject(
                path.as_ptr(),
                0,
                null_mut(),
                &mut object,
                std::ptr::null_mut(),
            )
        };
        if hr < 0 || object.is_null() {
            return Err(hr);
        }
        Ok(Owned(object))
    }

    /// 同 [`Wmi::call_number`]，失败时带上 HRESULT。
    pub fn call_number_diagnostic(
        &self,
        object_path: &str,
        method: &str,
        out_param: &str,
    ) -> Result<Option<f64>, i32> {
        let path = Bstr::new(object_path);
        let name = Bstr::new(method);
        let mut out: *mut IWbemClassObject = null_mut();
        // SAFETY: services 有效，两个 BSTR 在调用期间存活，入参显式传空。
        let hr = unsafe {
            (*self.services).ExecMethod(
                path.as_ptr(),
                name.as_ptr(),
                0,
                null_mut(),
                null_mut(),
                &mut out,
                null_mut(),
            )
        };
        if hr < 0 || out.is_null() {
            return Err(hr);
        }
        let value = get_property(out, out_param).and_then(|v| v.as_number());
        // SAFETY: 值已经拷出来了。
        unsafe { (*out).Release() };
        Ok(value)
    }
}

/// COM 在本线程初始化不了时的自造错误码（不是真 HRESULT，只用来在日志里
/// 和「WMI 拒了我们」区分开）。
pub const E_COM_INIT_FAILED: i32 = -1;

/// 本线程的 COM 初始化。返回 `None` = 这条线程用不了 COM。
fn init_com_on_this_thread() -> Option<()> {
    // SAFETY: 只影响当前线程的公寓状态。
    let hr = unsafe { CoInitializeEx(null_mut(), COINIT_MULTITHREADED) };
    // S_FALSE = 本线程已经初始化过；RPC_E_CHANGED_MODE = 别人先按 STA 初始化
    // 了（gpui 的某些线程会）。两种都能继续用 WMI，只是走跨公寓封送。
    if hr != S_OK && hr != S_FALSE && hr != RPC_E_CHANGED_MODE {
        return None;
    }
    // 进程级只能设一次，第二次起返回 RPC_E_TOO_LATE，忽略即可。
    // SAFETY: 全部传默认值，等价于 MSDN 的 WMI 客户端样板。
    let hr = unsafe {
        CoInitializeSecurity(
            null_mut(),
            -1,
            null_mut(),
            null_mut(),
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            null_mut(),
            EOAC_NONE,
            null_mut(),
        )
    };
    (hr >= 0 || hr == RPC_E_TOO_LATE).then_some(())
}

/// 从一个 WMI 对象里取一个属性。
fn get_property(object: *mut IWbemClassObject, name: &str) -> Option<Value> {
    let wide = to_wide(name);
    // SAFETY: VARIANT 是 POD，全零等于 VT_EMPTY。
    let mut variant: VARIANT = unsafe { std::mem::zeroed() };
    // SAFETY: object 有效，wide 以 NUL 结尾，variant 是栈上出参。
    let hr = unsafe { (*object).Get(wide.as_ptr(), 0, &mut variant, null_mut(), null_mut()) };
    if hr < 0 {
        return None;
    }
    let value = read_variant(&variant);
    // SAFETY: 值已经拷成 Rust 类型，BSTR 之类的内存交还 OLE。
    unsafe { VariantClear(&mut variant) };
    value
}

/// VARIANT → [`Value`]。只认传感器会碰到的那几种标量。
fn read_variant(variant: &VARIANT) -> Option<Value> {
    // SAFETY: 读联合体前先看 vt，取的是 vt 指定的那一支。
    unsafe {
        let inner = variant.n1.n2();
        match inner.vt as u32 {
            VT_I2 => Some(Value::Number(*inner.n3.iVal() as f64)),
            VT_I4 => Some(Value::Number(*inner.n3.lVal() as f64)),
            VT_UI1 => Some(Value::Number(*inner.n3.bVal() as f64)),
            VT_UI4 => Some(Value::Number(*inner.n3.ulVal() as f64)),
            VT_R4 => Some(Value::Number(*inner.n3.fltVal() as f64)),
            VT_R8 => Some(Value::Number(*inner.n3.dblVal())),
            VT_BSTR => {
                let bstr = *inner.n3.bstrVal();
                if bstr.is_null() {
                    return None;
                }
                let mut len = 0;
                while *bstr.add(len) != 0 {
                    len += 1;
                }
                Some(Value::Text(from_wide(std::slice::from_raw_parts(
                    bstr, len,
                ))))
            }
            _ => None,
        }
    }
}

/// 自动释放的 COM 对象。中途 `return Err` 的路径太多，手写 Release 迟早漏。
struct Owned<T>(*mut T);

impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: COM 接口指针的头一格永远是 IUnknown 的虚表，转过去调
            // Release 是 COM 的通用约定。
            unsafe { (*(self.0 as *mut IUnknown)).Release() };
        }
    }
}

/// 自动释放的 BSTR。WMI 的字符串参数一律要这个类型，忘了释放就是每拍漏一块。
struct Bstr(BSTR);

impl Bstr {
    fn new(s: &str) -> Bstr {
        let wide = to_wide(s);
        // SAFETY: wide 以 NUL 结尾；SysAllocString 会自己拷一份。
        Bstr(unsafe { SysAllocString(wide.as_ptr()) })
    }

    fn as_ptr(&self) -> BSTR {
        self.0
    }
}

impl Drop for Bstr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: 这块内存由 SysAllocString 分配，只有这里持有。
            unsafe { SysFreeString(self.0) };
        }
    }
}

/// 一个命名空间的连接结果。`None` = 试过、连不上，别再试。
type Connection = (&'static str, Option<Rc<Wmi>>);

thread_local! {
    /// 本线程各命名空间的连接。
    static CONNECTIONS: RefCell<Vec<Connection>> = const { RefCell::new(Vec::new()) };
}

/// 连不上的命名空间记在**进程**级。
///
/// 连接失败和成功一样要几十毫秒（要起 COM、要问 WMI 服务），而采样任务每
/// 一拍可能落在线程池的另一条线程上——只有 thread_local 的话，「这台机器没
/// 装 LibreHardwareMonitor」这件事每换一条线程就要重新学一遍，每两秒白扔
/// 上百毫秒在必然失败的连接上。
static UNAVAILABLE: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// 拿本线程到某个命名空间的连接，第一次调用时建立。
///
/// `f` 收到 `None` 表示这台机器上没有这个命名空间（或者没权限）。
///
/// 连接先 `Rc` 克隆出来再调 `f`：直接把 `RefCell` 的借用跨到 `f` 里的话，
/// `f` 内部再问一次别的命名空间就会 panic（同一个 `RefCell` 借两次）。
pub fn with_namespace<T>(namespace: &'static str, f: impl FnOnce(Option<&Wmi>) -> T) -> T {
    if UNAVAILABLE
        .lock()
        .is_ok_and(|known| known.contains(&namespace))
    {
        return f(None);
    }
    let connection = CONNECTIONS.with(|cell| {
        let mut cache = cell.borrow_mut();
        if !cache.iter().any(|(ns, _)| *ns == namespace) {
            let connected = Wmi::connect(namespace).map(Rc::new);
            if connected.is_none() {
                if let Ok(mut known) = UNAVAILABLE.lock() {
                    known.push(namespace);
                }
            }
            cache.push((namespace, connected));
        }
        cache
            .iter()
            .find(|(ns, _)| *ns == namespace)
            .and_then(|(_, wmi)| wmi.clone())
    });
    f(connection.as_deref())
}
