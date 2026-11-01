// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autostart;
mod config;
mod fonts;
mod hub;
mod i18n;
mod instance;
mod note;
mod screens;
mod store;
mod theme;

use eframe::egui;

/// DLLs asked for by name while running (Direct3D's) come from System32 only.
/// By default the folder of the .exe comes first, and a portable .exe often
/// sits in Downloads, where anything may have been dropped beside it.
#[cfg(windows)]
fn restrict_dll_search() {
    const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetDefaultDllDirectories(flags: u32) -> i32;
    }
    // Takes a plain value, and exists since Windows 8.
    unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
}

fn main() -> eframe::Result {
    #[cfg(windows)]
    restrict_dll_search();

    // Notes must come back where they were left and may stay on top, which
    // Wayland doesn't let an app ask for: X11 (XWayland) when there is one.
    // Nothing else runs yet, so changing the environment is safe.
    if std::env::var_os("DISPLAY").is_some() {
        unsafe {
            std::env::remove_var("WAYLAND_DISPLAY");
            std::env::remove_var("WAYLAND_SOCKET");
        }
    }

    // A second launch only asks the first one to show its window.
    let Some(instance) = instance::acquire() else {
        return Ok(());
    };

    i18n::init();
    let state = store::load();
    let any_open = state.notes.iter().any(|n| n.open);

    let viewport = egui::ViewportBuilder::default()
        .with_title(app::APP_NAME)
        .with_icon(app::icon())
        .with_app_id(app::APP_ID)
        .with_inner_size([360.0, 540.0])
        .with_min_inner_size([280.0, 320.0])
        // The list of notes stays out of the way of the notes themselves.
        .with_visible(!any_open);
    let mut options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    // On Windows, Direct3D 12 rather than Vulkan: with Vulkan, resizing the
    // window can leave its surface stuck at the old size (seen on NVIDIA).
    // OpenGL only as a last resort, for GPUs without D3D12.
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        // Asked first: merely allowing OpenGL loads the graphics driver's
        // OpenGL too, for nothing on most computers.
        let mut descriptor = eframe::wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = eframe::wgpu::Backends::DX12;
        let direct3d = eframe::wgpu::Instance::new(descriptor);
        let adapters =
            pollster::block_on(direct3d.enumerate_adapters(eframe::wgpu::Backends::DX12));
        setup.instance_descriptor.backends = if adapters.is_empty() {
            eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL
        } else {
            eframe::wgpu::Backends::DX12
        };
    }
    // On battery power, notes don't need the big graphics card of a laptop that
    // has two. On a desktop the big one drives the screen anyway: drawing there
    // costs less, in memory and processor, than going through the other one.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let preferred = if screens::has_no_battery() {
            eframe::wgpu::PowerPreference::HighPerformance
        } else {
            eframe::wgpu::PowerPreference::LowPower
        };
        setup.power_preference = eframe::wgpu::PowerPreference::from_env().unwrap_or(preferred);
    }
    eframe::run_native(
        app::APP_NAME,
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(&cc.egui_ctx, state, instance)))
        }),
    )
}
