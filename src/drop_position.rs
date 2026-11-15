//! Read the native cursor: winit file-drop events do not include drop coordinates.
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
pub fn current(frame: &eframe::Frame, scale: f32) -> Option<egui::Pos2> {
    let handle = frame.window_handle().ok()?.as_raw();
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::POINT, Graphics::Gdi::ScreenToClient, UI::WindowsAndMessaging::GetCursorPos,
        };
        let RawWindowHandle::Win32(handle) = handle else {
            return None;
        };
        let mut point = POINT { x: 0, y: 0 };
        // The HWND belongs to this live eframe window.
        unsafe {
            if GetCursorPos(&mut point) == 0
                || ScreenToClient(handle.hwnd.get() as _, &mut point) == 0
            {
                return None;
            }
        }
        Some(egui::pos2(point.x as f32 / scale, point.y as f32 / scale))
    }
    #[cfg(target_os = "linux")]
    {
        use x11rb::protocol::xproto::ConnectionExt;
        let RawWindowHandle::Xlib(handle) = handle else {
            return None;
        };
        let (conn, _) = x11rb::connect(None).ok()?;
        let reply = conn
            .query_pointer(handle.window as u32)
            .ok()?
            .reply()
            .ok()?;
        Some(egui::pos2(
            reply.win_x as f32 / scale,
            reply.win_y as f32 / scale,
        ))
    }
}
