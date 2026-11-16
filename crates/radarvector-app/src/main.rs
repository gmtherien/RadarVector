//! RadarVector desktop application.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod app;
mod backend;
mod devmode;
mod i18n;
mod map_view;
mod search;
mod settings;
mod tabs;
mod tracon;
#[cfg(test)]
mod ui_tests;
mod updates;

const MSAA: u32 = 4;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        multisampling: MSAA as u16,
        viewport: egui::ViewportBuilder::default()
            .with_title("RadarVector")
            .with_icon(icon())
            .with_inner_size([1400.0, 880.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "RadarVector",
        options,
        Box::new(|cc| Ok(Box::new(app::RadarVectorApp::new(cc, MSAA)))),
    )
}

fn icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icon/icon-256.png");
    let decoded = (|| {
        let mut r = png::Decoder::new(std::io::Cursor::new(&bytes[..])).read_info().ok()?;
        let mut buf = vec![0; r.output_buffer_size()?];
        let info = r.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        Some(egui::IconData {
            rgba: buf,
            width: info.width,
            height: info.height,
        })
    })();
    decoded.unwrap_or_default()
}
