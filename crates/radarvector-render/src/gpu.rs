//! wgpu pipelines and the egui paint callback that draws the map.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use egui_wgpu::wgpu::util::DeviceExt;
use radarvector_geo::{Camera, Projection};

use crate::geometry::{AircraftInstance, FillVertex, Mesh, SegmentInstance};

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Immutable layer contents tagged with a generation; the GPU copy is
/// re-uploaded only when the generation changes.
#[derive(Debug, Clone)]
pub struct Layer<T> {
    pub generation: u64,
    pub data: Arc<T>,
}

impl<T: Default> Default for Layer<T> {
    fn default() -> Self {
        Self {
            generation: 0,
            data: Arc::new(T::default()),
        }
    }
}

impl<T> Layer<T> {
    pub fn new(data: T) -> Self {
        Self {
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            data: Arc::new(data),
        }
    }
}

/// Everything needed to draw one frame of the map.
#[derive(Debug, Clone, Default)]
pub struct MapFrame {
    pub camera: Camera,
    pub land: Layer<Mesh>,
    pub fir_fill: Layer<Mesh>,
    pub base_lines: Layer<Vec<SegmentInstance>>,
    pub fir_lines: Layer<Vec<SegmentInstance>>,
    pub overlay_lines: Layer<Vec<SegmentInstance>>,
    pub aircraft: Layer<Vec<AircraftInstance>>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct CameraUniform {
    center: [f32; 2],
    scale: f32,
    mercator: f32,
    viewport: [f32; 2],
    world_offset: f32,
    _pad: f32,
}

const UNIFORM_SLOT: u64 = 256;
const MAX_COPIES: usize = 4;

struct GpuBuffer {
    buffer: Option<wgpu::Buffer>,
    capacity: u64,
    count: u32,
    generation: u64,
}

impl GpuBuffer {
    fn new() -> Self {
        Self {
            buffer: None,
            capacity: 0,
            count: 0,
            generation: u64::MAX,
        }
    }

    fn upload<T: Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        usage: wgpu::BufferUsages,
        data: &[T],
        generation: u64,
    ) {
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        self.count = data.len() as u32;
        let bytes: &[u8] = bytemuck::cast_slice(data);
        if bytes.is_empty() {
            return;
        }
        if self.buffer.is_none() || (bytes.len() as u64) > self.capacity {
            let cap = (bytes.len() as u64).next_power_of_two().max(1024);
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("radarvector buffer"),
                size: cap,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity = cap;
        }
        if let Some(b) = &self.buffer {
            queue.write_buffer(b, 0, bytes);
        }
    }
}

struct MeshBuffers {
    vertices: GpuBuffer,
    indices: GpuBuffer,
}

impl MeshBuffers {
    fn new() -> Self {
        Self {
            vertices: GpuBuffer::new(),
            indices: GpuBuffer::new(),
        }
    }
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, layer: &Layer<Mesh>) {
        self.vertices.upload(
            device,
            queue,
            wgpu::BufferUsages::VERTEX,
            &layer.data.vertices,
            layer.generation,
        );
        self.indices.upload(
            device,
            queue,
            wgpu::BufferUsages::INDEX,
            &layer.data.indices,
            layer.generation,
        );
    }
    fn draw(&self, pass: &mut wgpu::RenderPass<'static>) {
        if let (Some(v), Some(i)) = (&self.vertices.buffer, &self.indices.buffer)
            && self.indices.count > 0
        {
            pass.set_vertex_buffer(0, v.slice(..));
            pass.set_index_buffer(i.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.indices.count, 0, 0..1);
        }
    }
}

/// GPU resources, stored in egui-wgpu's callback resources.
pub struct MapGpu {
    fill_pipeline: wgpu::RenderPipeline,
    segment_pipeline: wgpu::RenderPipeline,
    aircraft_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    land: MeshBuffers,
    fir_fill: MeshBuffers,
    base_lines: GpuBuffer,
    fir_lines: GpuBuffer,
    overlay_lines: GpuBuffer,
    aircraft: GpuBuffer,
    copies: u32,
}

fn blend() -> wgpu::BlendState {
    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING
}

impl MapGpu {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, msaa_samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("radarvector map shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("map.wgsl").into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("radarvector camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: std::num::NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64),
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("radarvector map layout"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("radarvector camera uniform"),
            contents: &vec![0u8; (UNIFORM_SLOT as usize) * MAX_COPIES],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("radarvector camera bind group"),
            layout: &bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform,
                    offset: 0,
                    size: std::num::NonZeroU64::new(std::mem::size_of::<CameraUniform>() as u64),
                }),
            }],
        });
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(blend()),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let multisample = wgpu::MultisampleState {
            count: msaa_samples.max(1),
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let make = |label: &str, entry: &str, buffers: &[Option<wgpu::VertexBufferLayout>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_color"),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let fill_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FillVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Unorm8x4],
        };
        let seg_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<SegmentInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4, 3 => Float32],
        };
        let ac_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<AircraftInstance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32, 2 => Float32, 3 => Unorm8x4],
        };
        Self {
            fill_pipeline: make("radarvector fill", "vs_fill", &[Some(fill_layout)]),
            segment_pipeline: make("radarvector segments", "vs_segment", &[Some(seg_layout)]),
            aircraft_pipeline: make("radarvector aircraft", "vs_aircraft", &[Some(ac_layout)]),
            uniform,
            bind_group,
            land: MeshBuffers::new(),
            fir_fill: MeshBuffers::new(),
            base_lines: GpuBuffer::new(),
            fir_lines: GpuBuffer::new(),
            overlay_lines: GpuBuffer::new(),
            aircraft: GpuBuffer::new(),
            copies: 0,
        }
    }

    pub(crate) fn prepare_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &MapFrame) {
        self.land.upload(device, queue, &frame.land);
        self.fir_fill.upload(device, queue, &frame.fir_fill);
        let v = wgpu::BufferUsages::VERTEX;
        self.base_lines
            .upload(device, queue, v, &frame.base_lines.data, frame.base_lines.generation);
        self.fir_lines
            .upload(device, queue, v, &frame.fir_lines.data, frame.fir_lines.generation);
        self.overlay_lines.upload(
            device,
            queue,
            v,
            &frame.overlay_lines.data,
            frame.overlay_lines.generation,
        );
        self.aircraft
            .upload(device, queue, v, &frame.aircraft.data, frame.aircraft.generation);

        let c = &frame.camera;
        let offsets = c.world_offsets();
        self.copies = offsets.len().min(MAX_COPIES) as u32;
        let mut bytes = vec![0u8; UNIFORM_SLOT as usize * MAX_COPIES];
        for (i, off) in offsets.iter().take(MAX_COPIES).enumerate() {
            let u = CameraUniform {
                center: [c.center[0] as f32, c.center[1] as f32],
                scale: c.scale as f32,
                mercator: if c.projection == Projection::Mercator { 1.0 } else { 0.0 },
                viewport: [c.viewport[0] as f32, c.viewport[1] as f32],
                world_offset: *off as f32,
                _pad: 0.0,
            };
            let start = i * UNIFORM_SLOT as usize;
            bytes[start..start + std::mem::size_of::<CameraUniform>()].copy_from_slice(bytemuck::bytes_of(&u));
        }
        queue.write_buffer(&self.uniform, 0, &bytes);
    }

    pub(crate) fn paint_pass(&self, pass: &mut wgpu::RenderPass<'static>) {
        for copy in 0..self.copies {
            pass.set_bind_group(0, &self.bind_group, &[copy * UNIFORM_SLOT as u32]);
            pass.set_pipeline(&self.fill_pipeline);
            self.land.draw(pass);
            self.fir_fill.draw(pass);
            pass.set_pipeline(&self.segment_pipeline);
            for b in [&self.base_lines, &self.fir_lines, &self.overlay_lines] {
                if let Some(buf) = &b.buffer
                    && b.count > 0
                {
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw(0..6, 0..b.count);
                }
            }
            if let Some(buf) = &self.aircraft.buffer
                && self.aircraft.count > 0
            {
                pass.set_pipeline(&self.aircraft_pipeline);
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw(0..12, 0..self.aircraft.count);
            }
        }
    }
}

/// Paint callback handed to egui each frame.
pub struct MapCallback(pub MapFrame);

impl egui_wgpu::CallbackTrait for MapCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = resources.get_mut::<MapGpu>() {
            gpu.prepare_frame(device, queue, &self.0);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(gpu) = resources.get::<MapGpu>() {
            gpu.paint_pass(pass);
        }
    }
}

/// Register the map GPU resources with an eframe wgpu render state.
pub fn install(render_state: &egui_wgpu::RenderState, msaa_samples: u32) {
    let gpu = MapGpu::new(&render_state.device, render_state.target_format, msaa_samples);
    render_state.renderer.write().callback_resources.insert(gpu);
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("map.wgsl")).expect("WGSL parses");
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("WGSL validates");
    }
}
