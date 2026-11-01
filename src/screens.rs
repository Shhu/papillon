//! Whether a window would be seen, for notes left on a monitor that is gone.
//! Saying yes when unsure: moving a visible note would be worse.

/// The part of a note that must be on a monitor to be reached: its strip.
const STRIP_HEIGHT: f32 = 30.0;

#[cfg(windows)]
pub fn shows(position: [f32; 2], size: [f32; 2]) -> bool {
    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    const MONITOR_DEFAULTTONULL: u32 = 0;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MonitorFromRect(rect: *const Rect, flags: u32) -> *mut core::ffi::c_void;
    }
    let rect = Rect {
        left: position[0] as i32,
        top: position[1] as i32,
        right: (position[0] + size[0]) as i32,
        bottom: (position[1] + STRIP_HEIGHT) as i32,
    };
    // Only reads the rectangle, which outlives the call.
    !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_null()
}

/// X11's screen spans the monitors, and shrinks when one goes.
#[cfg(not(windows))]
pub fn shows(position: [f32; 2], size: [f32; 2]) -> bool {
    use x11_dl::xlib::Xlib;
    let Ok(xlib) = Xlib::open() else {
        return true;
    };
    // The display is checked before use, and closed once it's been read.
    let (width, height) = unsafe {
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return true;
        }
        let screen = (xlib.XDefaultScreen)(display);
        let dimensions = (
            (xlib.XDisplayWidth)(display, screen),
            (xlib.XDisplayHeight)(display, screen),
        );
        (xlib.XCloseDisplay)(display);
        dimensions
    };
    position[0] < width as f32
        && position[0] + size[0] > 0.0
        && position[1] < height as f32
        && position[1] + STRIP_HEIGHT > 0.0
}
