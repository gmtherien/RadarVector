//! Headless rendering of a [`MapFrame`] to RGBA pixels (visual tests,
//! benchmarks, thumbnails). Uses the same pipelines as the app.

use egui_wgpu::wgpu;

use crate::gpu::{MapFrame, MapGpu};

pub struct Offscreen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    gpu: MapGpu,
    format: wgpu::TextureFormat,
}

#[derive(Debug, thiserror::Error)]
pub enum OffscreenError {
    #[error("no GPU adapter: {0}")]
    Adapter(String),
    #[error("device: {0}")]
    Device(String),
    #[error("readback failed")]
    Readback,
}

impl Offscreen {
    pub fn new() -> Result<Self, OffscreenError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|e| OffscreenError::Adapter(e.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .map_err(|e| OffscreenError::Device(e.to_string()))?;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let gpu = MapGpu::new(&device, format, 4);
        Ok(Self {
            device,
            queue,
            gpu,
            format,
        })
    }

    /// Render `frame` (its camera viewport is overridden by `w`×`h`) on a
    /// solid `background`, returning tightly packed RGBA rows.
    pub fn render(&mut self, frame: MapFrame, w: u32, h: u32, background: [u8; 4]) -> Result<Vec<u8>, OffscreenError> {
        self.render_inner(frame, w, h, background, true)
    }

    /// Render and wait for the GPU without reading pixels back (benchmarks).
    pub fn render_no_readback(
        &mut self,
        frame: MapFrame,
        w: u32,
        h: u32,
        background: [u8; 4],
    ) -> Result<(), OffscreenError> {
        self.render_inner(frame, w, h, background, false).map(|_| ())
    }

    fn render_inner(
        &mut self,
        mut frame: MapFrame,
        w: u32,
        h: u32,
        background: [u8; 4],
        read: bool,
    ) -> Result<Vec<u8>, OffscreenError> {
        frame.camera.set_viewport(w as f64, h as f64);
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let make_tex = |samples: u32, usage: wgpu::TextureUsages| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("offscreen"),
                size,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage,
                view_formats: &[],
            })
        };
        let msaa = make_tex(4, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let resolve = make_tex(
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let msaa_view = msaa.create_view(&Default::default());
        let resolve_view = resolve.create_view(&Default::default());

        self.gpu.prepare_frame(&self.device, &self.queue, &frame);
        let bpr = (w * 4).div_ceil(256) * 256;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (bpr * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let c = |v: u8| v as f64 / 255.0;
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa_view,
                    depth_slice: None,
                    resolve_target: Some(&resolve_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: c(background[0]),
                            g: c(background[1]),
                            b: c(background[2]),
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            self.gpu.paint_pass(&mut pass);
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(h),
                },
            },
            size,
        );
        self.queue.submit([enc.finish()]);
        if !read {
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(|_| OffscreenError::Readback)?;
            return Ok(Vec::new());
        }
        let (tx, rx) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r.is_ok());
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| OffscreenError::Readback)?;
        if !rx.recv().unwrap_or(false) {
            return Err(OffscreenError::Readback);
        }
        let data = readback
            .slice(..)
            .get_mapped_range()
            .map_err(|_| OffscreenError::Readback)?;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for row in 0..h {
            let start = (row * bpr) as usize;
            out.extend_from_slice(&data[start..start + (w * 4) as usize]);
        }
        Ok(out)
    }
}
