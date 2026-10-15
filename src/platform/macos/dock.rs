//! 产品图标资源；原生对象生命周期由共享宿主管理。
pub fn set_dock_icon(cx: &mut gpui::App) {
    gpui_app_host::desktop::set_dock_icon(include_bytes!("../../../assets/icon-512.png"), cx);
}
