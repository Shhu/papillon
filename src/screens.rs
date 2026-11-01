//! Whether a window would be seen, for notes left on a monitor that is gone.
//! Positions are physical pixels and sizes points. A sliver of a note doesn't
//! count: it couldn't be grabbed.

/// The part of a note that must be on a monitor to be reached: its strip.
const STRIP_HEIGHT: i32 = 30;
/// Part of the strip that doesn't count, on each side.
const SLIVER: i32 = 20;
/// What must be within a monitor for a new note to be of use, in pixels.
const USABLE: [i32; 2] = [200, 120];

/// x, y, right, bottom.
type Area = [i32; 4];

#[cfg(any(test, not(windows)))]
fn overlap(a: Area, b: Area) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn contains(outer: Area, inner: Area) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

fn strip(position: [f32; 2], size: [f32; 2]) -> Area {
    let [x, y] = position.map(|v| v as i32);
    let width = (size[0] * platform::system_scale()) as i32;
    [
        x.saturating_add(SLIVER),
        y.saturating_add(STRIP_HEIGHT / 4),
        x.saturating_add(width - SLIVER),
        y.saturating_add(STRIP_HEIGHT),
    ]
}

fn usable(position: [f32; 2]) -> Area {
    let [x, y] = position.map(|v| v as i32);
    [
        x,
        y,
        x.saturating_add(USABLE[0]),
        y.saturating_add(USABLE[1]),
    ]
}

#[cfg(windows)]
mod platform {
    use super::Area;
    use core::ffi::c_void;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    const MONITOR_DEFAULTTONULL: u32 = 0;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MonitorFromRect(rect: *const Rect, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
        fn GetDpiForSystem() -> u32;
    }

    /// The main screen's scale: right for most, and a note on another monitor
    /// that comes back early has lost nothing.
    pub fn system_scale() -> f32 {
        // Takes nothing, and exists since Windows 10.
        unsafe { GetDpiForSystem() as f32 / 96.0 }
    }

    fn monitor_of(area: Area) -> *mut c_void {
        let rect = Rect {
            left: area[0],
            top: area[1],
            right: area[2],
            bottom: area[3],
        };
        // Only reads the rectangle, which outlives the call.
        unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }
    }

    pub fn shows(area: Area) -> bool {
        !monitor_of(area).is_null()
    }

    /// Within the monitor's work area, so clear of the taskbar.
    pub fn fits(area: Area) -> bool {
        let monitor = monitor_of(area);
        if monitor.is_null() {
            return false;
        }
        let empty = Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut info = MonitorInfo {
            size: size_of::<MonitorInfo>() as u32,
            monitor: empty,
            work: empty,
            flags: 0,
        };
        // The monitor was just given by the system; `info` has its size set.
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return true;
        }
        let work = info.work;
        super::contains([work.left, work.top, work.right, work.bottom], area)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Area;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use x11_dl::xlib::Xlib;
    use x11_dl::xrandr::Xrandr;

    /// Room left for a panel at the bottom when placing a new note.
    const PANEL: i32 = 60;
    /// Monitors change rarely, and asking opens a connection to the X server.
    const FRESH_FOR: Duration = Duration::from_secs(5);

    static KNOWN: Mutex<Option<(Instant, Vec<Area>)>> = Mutex::new(None);

    /// The monitors, or the whole screen if that's all that can be known.
    /// Empty when nothing is known at all.
    fn monitors() -> Vec<Area> {
        let mut known = crate::store::lock(&KNOWN);
        if let Some((when, areas)) = &*known
            && when.elapsed() < FRESH_FOR
        {
            return areas.clone();
        }
        let Some(areas) = ask_x() else {
            return Vec::new();
        };
        *known = Some((Instant::now(), areas.clone()));
        areas
    }

    fn ask_x() -> Option<Vec<Area>> {
        let xlib = Xlib::open().ok()?;
        // The display is checked before use and closed at the end; the list
        // of monitors is read within its given length, then freed.
        unsafe {
            let display = (xlib.XOpenDisplay)(std::ptr::null());
            if display.is_null() {
                return None;
            }
            let screen = (xlib.XDefaultScreen)(display);
            let root = (xlib.XRootWindow)(display, screen);
            let mut areas = Vec::new();
            if let Ok(xrandr) = Xrandr::open() {
                let mut count = 0;
                let list = (xrandr.XRRGetMonitors)(display, root, 1, &mut count);
                if !list.is_null() {
                    for m in std::slice::from_raw_parts(list, count.max(0) as usize) {
                        areas.push([m.x, m.y, m.x + m.width, m.y + m.height]);
                    }
                    (xrandr.XRRFreeMonitors)(list);
                }
            }
            if areas.is_empty() {
                let width = (xlib.XDisplayWidth)(display, screen);
                let height = (xlib.XDisplayHeight)(display, screen);
                areas.push([0, 0, width, height]);
            }
            (xlib.XCloseDisplay)(display);
            Some(areas)
        }
    }

    /// X11 has no scale of its own to ask for.
    pub fn system_scale() -> f32 {
        1.0
    }

    pub fn shows(area: Area) -> bool {
        let monitors = monitors();
        monitors.is_empty() || monitors.iter().any(|&m| super::overlap(m, area))
    }

    pub fn fits(area: Area) -> bool {
        let monitors = monitors();
        monitors.is_empty()
            || monitors
                .iter()
                .any(|&m| super::contains([m[0], m[1], m[2], m[3] - PANEL], area))
    }
}

/// Known not to run on a battery; unknown counts as a battery.
#[cfg(windows)]
pub fn has_no_battery() -> bool {
    #[repr(C)]
    #[derive(Default)]
    struct PowerStatus {
        ac_line: u8,
        battery_flag: u8,
        battery_percent: u8,
        saver: u8,
        battery_seconds: u32,
        full_seconds: u32,
    }
    const NO_SYSTEM_BATTERY: u8 = 128;
    const UNKNOWN: u8 = 255;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemPowerStatus(status: *mut PowerStatus) -> i32;
    }
    let mut status = PowerStatus::default();
    // Fills the structure given, of the layout the system documents.
    let known = unsafe { GetSystemPowerStatus(&mut status) } != 0;
    known && status.battery_flag != UNKNOWN && status.battery_flag & NO_SYSTEM_BATTERY != 0
}

/// Known not to run on a battery; unknown counts as a battery.
#[cfg(not(windows))]
pub fn has_no_battery() -> bool {
    let Ok(supplies) = std::fs::read_dir("/sys/class/power_supply") else {
        return false;
    };
    !supplies.filter_map(Result::ok).any(|supply| {
        std::fs::read_to_string(supply.path().join("type"))
            .is_ok_and(|kind| kind.trim() == "Battery")
    })
}

/// Whether the strip of a window placed there is on a monitor.
pub fn shows(position: [f32; 2], size: [f32; 2]) -> bool {
    platform::shows(strip(position, size))
}

/// Whether a new note placed there would be well within a monitor.
pub fn fits(position: [f32; 2], _size: [f32; 2]) -> bool {
    platform::fits(usable(position))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_compare() {
        let screen = [0, 0, 1000, 1000];
        assert!(overlap(screen, [-500, 10, 10, 40]));
        assert!(!overlap(screen, [1000, 10, 1200, 40]));
        assert!(contains(screen, [10, 10, 210, 130]));
        assert!(!contains(screen, [900, 10, 1100, 130]));
    }

    #[test]
    fn a_note_on_a_monitor_that_is_gone_is_lost_and_a_sliver_does_not_count() {
        let screen = [0, 0, 1000, 1000];
        let scale = platform::system_scale();
        let width = 280.0;
        // Entirely left of the screen, as on a left monitor now unplugged.
        let gone = strip([-2.0 * width * scale, 10.0], [width, 200.0]);
        assert!(!overlap(screen, gone));
        // Its right edge just inside.
        let sliver = strip([-(width * scale) + 10.0, 10.0], [width, 200.0]);
        assert!(!overlap(screen, sliver));
        // Half on screen.
        let half = strip([-(width * scale) / 2.0, 10.0], [width, 200.0]);
        assert!(overlap(screen, half));
    }

    #[test]
    fn huge_positions_do_not_overflow() {
        let area = strip([1e30, 1e30], [280.0, 200.0]);
        assert!(!overlap([0, 0, 1000, 1000], area));
        assert!(!contains([0, 0, 1000, 1000], usable([1e30, 1e30])));
    }

    #[cfg(windows)]
    #[test]
    fn the_system_knows_what_is_on_screen() {
        assert!(shows([100.0, 100.0], [280.0, 280.0]));
        assert!(!shows([500000.0, 500000.0], [280.0, 280.0]));
        assert!(fits([100.0, 100.0], [280.0, 280.0]));
        assert!(!fits([500000.0, 500000.0], [280.0, 280.0]));
    }
}
