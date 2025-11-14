use std::num::NonZeroU64;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use bytemuck::{Pod, Zeroable};
use futures::channel::oneshot;
use pollster::block_on;
use wgpu::util::DeviceExt;

pub struct SimilarityComputer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    max_storage_bytes: u64,
    _poller: DevicePoller,
}

pub enum GpuTileHandle {
    Pending {
        device: Arc<wgpu::Device>,
        staging: Arc<wgpu::Buffer>,
        output_bytes: u64,
    },
    Immediate(Result<Vec<f32>, String>),
}

impl GpuTileHandle {
    pub fn wait(self) -> Result<Vec<f32>, String> {
        match self {
            GpuTileHandle::Immediate(result) => result,
            GpuTileHandle::Pending {
                device,
                staging,
                output_bytes,
            } => {
                if output_bytes == 0 {
                    return Ok(Vec::new());
                }
                let slice = staging.slice(..output_bytes);
                let (sender, receiver) = oneshot::channel();
                slice.map_async(wgpu::MapMode::Read, move |res| {
                    let _ = sender.send(res);
                });
                match block_on(receiver) {
                    Ok(Ok(())) => {
                        let view = slice.get_mapped_range();
                        let floats = bytemuck::cast_slice(&view).to_vec();
                        drop(view);
                        staging.unmap();
                        device.poll(wgpu::Maintain::Poll);
                        Ok(floats)
                    }
                    Ok(Err(err)) => Err(format!("Failed to map GPU buffer: {:?}", err)),
                    Err(_) => Err("GPU map receiver dropped before completion".to_string()),
                }
            }
        }
    }

    fn immediate(result: Result<Vec<f32>, String>) -> Self {
        GpuTileHandle::Immediate(result)
    }
}

impl SimilarityComputer {
    pub fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "No suitable GPU adapter found".to_string())?;

        let limits = adapter.limits();
        let max_storage = limits.max_storage_buffer_binding_size as u64;
        let (device, queue) = block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("gpu-matcher-device"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
            },
            None,
        ))
        .map_err(|e| format!("Failed to create GPU device: {}", e))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("similarity-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("similarity-bind-group-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<ShaderParams>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("similarity-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("similarity-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });

        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let poller = DevicePoller::start(Arc::clone(&device));

        Ok(Self {
            device,
            queue,
            pipeline,
            bind_group_layout,
            max_storage_bytes: max_storage,
            _poller: poller,
        })
    }

    pub fn max_storage_bytes(&self) -> u64 {
        self.max_storage_bytes
    }

    pub fn create_file_buffer(&self, vectors: &[f32]) -> Arc<wgpu::Buffer> {
        Arc::new(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gpu-file-buffer"),
                    contents: bytemuck::cast_slice(vectors),
                    usage: wgpu::BufferUsages::STORAGE,
                }),
        )
    }

    pub fn dispatch_tile(
        &self,
        query_vectors: &[f32],
        query_len: usize,
        file_buffer: &Arc<wgpu::Buffer>,
        file_offset: usize,
        file_len: usize,
        dim: usize,
    ) -> Result<GpuTileHandle, String> {
        if query_len == 0 || file_len == 0 {
            return Ok(GpuTileHandle::immediate(Ok(Vec::new())));
        }

        catch_unwind(AssertUnwindSafe(|| {
            self.dispatch_tile_inner(
                query_vectors,
                query_len,
                file_buffer,
                file_offset,
                file_len,
                dim,
            )
        }))
        .map_err(|_| "GPU dispatch panicked".to_string())?
    }

    #[allow(dead_code)]
    pub fn compute_with_file_buffer(
        &self,
        query_vectors: &[f32],
        query_len: usize,
        file_buffer: &Arc<wgpu::Buffer>,
        file_offset: usize,
        file_len: usize,
        dim: usize,
    ) -> Result<Vec<f32>, String> {
        self.dispatch_tile(
            query_vectors,
            query_len,
            file_buffer,
            file_offset,
            file_len,
            dim,
        )?
        .wait()
    }

    fn dispatch_tile_inner(
        &self,
        query_vectors: &[f32],
        query_len: usize,
        file_buffer: &Arc<wgpu::Buffer>,
        file_offset: usize,
        file_len: usize,
        dim: usize,
    ) -> Result<GpuTileHandle, String> {
        let stride_bytes = (dim * std::mem::size_of::<f32>()) as u64;
        let file_chunk_bytes = file_len as u64 * stride_bytes;
        let file_offset_bytes = file_offset as u64 * stride_bytes;
        if file_chunk_bytes == 0 {
            return Ok(GpuTileHandle::immediate(Ok(Vec::new())));
        }
        let file_binding_size = NonZeroU64::new(file_chunk_bytes)
            .ok_or_else(|| "File binding size cannot be zero".to_string())?;
        if file_offset_bytes + file_chunk_bytes > file_buffer.size() {
            return Err("Requested file chunk exceeds GPU buffer size".to_string());
        }

        let query_bytes = query_vectors.len() * std::mem::size_of::<f32>();
        if query_bytes == 0 {
            return Ok(GpuTileHandle::immediate(Ok(Vec::new())));
        }

        let output_floats = query_len * file_len;
        let output_bytes = output_floats
            .checked_mul(std::mem::size_of::<f32>())
            .ok_or_else(|| "Output buffer size overflow".to_string())?
            as u64;
        if output_bytes > self.max_storage_bytes {
            return Err(format!(
                "Output buffer ({} bytes) exceeds GPU limit {} bytes",
                output_bytes, self.max_storage_bytes
            ));
        }

        let query_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu-query-buffer"),
                contents: bytemuck::cast_slice(query_vectors),
                usage: wgpu::BufferUsages::STORAGE,
            });

        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-output-buffer"),
            size: output_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-staging-buffer"),
            size: output_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params = ShaderParams {
            query_len: query_len as u32,
            file_len: file_len as u32,
            dim: dim as u32,
            _pad: 0,
        };

        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu-params-buffer"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let queries_binding = query_buffer.as_entire_buffer_binding();
        let files_binding = wgpu::BufferBinding {
            buffer: file_buffer,
            offset: file_offset_bytes,
            size: Some(file_binding_size),
        };
        let output_binding = output_buffer.as_entire_buffer_binding();
        let params_binding = params_buffer.as_entire_buffer_binding();

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("similarity-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(queries_binding),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(files_binding),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(output_binding),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(params_binding),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("similarity-encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("similarity-pass"),
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let x_groups = (query_len as u32 + WORKGROUP_X - 1) / WORKGROUP_X;
            let y_groups = (file_len as u32 + WORKGROUP_Y - 1) / WORKGROUP_Y;
            pass.dispatch_workgroups(x_groups.max(1), y_groups.max(1), 1);
        }

        encoder.copy_buffer_to_buffer(&output_buffer, 0, &staging_buffer, 0, output_bytes);
        self.queue.submit(std::iter::once(encoder.finish()));
        self.device.poll(wgpu::Maintain::Poll);

        Ok(GpuTileHandle::Pending {
            device: Arc::clone(&self.device),
            staging: Arc::new(staging_buffer),
            output_bytes,
        })
    }
}

struct DevicePoller {
    active: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl DevicePoller {
    fn start(device: Arc<wgpu::Device>) -> Self {
        let active = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&active);
        let handle = thread::Builder::new()
            .name("wgpu-poller".to_string())
            .spawn(move || {
                while flag.load(Ordering::Relaxed) {
                    device.poll(wgpu::Maintain::Poll);
                    // Reduced from 1ms to 10ms to lower CPU overhead
                    thread::sleep(Duration::from_millis(10));
                }
                device.poll(wgpu::Maintain::Wait);
            })
            .ok();

        Self { active, handle }
    }
}

impl Drop for DevicePoller {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShaderParams {
    query_len: u32,
    file_len: u32,
    dim: u32,
    _pad: u32,
}

const WORKGROUP_X: u32 = 8;
const WORKGROUP_Y: u32 = 8;

const SHADER: &str = r#"
struct Params {
    query_len: u32,
    file_len: u32,
    dim: u32,
    _pad: u32,
};

@group(0) @binding(0)
var<storage, read> queries: array<f32>;

@group(0) @binding(1)
var<storage, read> files: array<f32>;

@group(0) @binding(2)
var<storage, read_write> output: array<f32>;

@group(0) @binding(3)
var<uniform> params: Params;

const WORKGROUP_X: u32 = 8u;
const WORKGROUP_Y: u32 = 8u;

@compute @workgroup_size(WORKGROUP_X, WORKGROUP_Y, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let q = global_id.x;
    let f = global_id.y;

    if (q >= params.query_len || f >= params.file_len) {
        return;
    }

    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < params.dim; i = i + 1u) {
        let q_index = q * params.dim + i;
        let f_index = f * params.dim + i;
        sum = sum + queries[q_index] * files[f_index];
    }

    let out_index = q * params.file_len + f;
    output[out_index] = sum;
}
"#;

#[cfg(all(test, feature = "gpu-smoke"))]
mod tests {
    use super::*;

    #[test]
    fn gpu_similarity_small_job() {
        let Ok(computer) = SimilarityComputer::new() else {
            eprintln!("GPU unavailable on this host; skipping smoke test");
            return;
        };

        let file_vectors: Vec<f32> = vec![1.0, 0.0, 0.0, 1.0];
        let file_buffer = computer.create_file_buffer(&file_vectors);
        let queries = vec![1.0, 0.0];
        let result = computer.compute_with_file_buffer(&queries, 1, &file_buffer, 0, 1, 2);
        assert!(result.is_ok());
        let scores = result.unwrap();
        assert_eq!(scores.len(), 1);
        assert!(scores[0] > 0.5);
    }
}
