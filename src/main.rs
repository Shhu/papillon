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

fn main() -> eframe::Result {
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
    // OpenGL stays as a last resort for GPUs without D3D12.
    #[cfg(windows)]
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends =
            eframe::wgpu::Backends::DX12 | eframe::wgpu::Backends::GL;
    }
    // Notes don't need the big graphics card of a laptop that has two.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.power_preference = eframe::wgpu::PowerPreference::from_env()
            .unwrap_or(eframe::wgpu::PowerPreference::LowPower);
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
