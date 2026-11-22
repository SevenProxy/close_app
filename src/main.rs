mod app;
mod hardware;
mod processes;

use app::ProcessApp;
use eframe::egui_wgpu::{WgpuSetup, wgpu};

fn main() -> eframe::Result<()> {
    let mut options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = wgpu::Backends::VULKAN;
    }

    eframe::run_native(
        "Close App",
        options,
        Box::new(|cc| Ok(Box::new(ProcessApp::new(&cc.egui_ctx)) as Box<dyn eframe::App>)),
    )
}
