//! Test wgpu-external-frame DMA-BUF import on GL backend (radeonsi).

use pleamar::wgpu;
use smithay::reexports::gbm;
use std::os::fd::OwnedFd;
use wgpu_external_frame::dma_buf::{DmaBufFormat, DmaBufFrame, DmaBufImporter, DmaBufPlane};

pub fn run() -> Result<(), String> {
    eprintln!("test_external_frame · starting...");

    // Create wgpu instance with GL backend (for EGL DMA-BUF import on radeonsi)
    let backends = wgpu::Backends::GL;
    eprintln!("test_external_frame · backends = {:?}", backends);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    eprintln!("test_external_frame · instance created");

    // Enumerate adapters
    let adapters = pollster::block_on(instance.enumerate_adapters(backends));
    eprintln!("test_external_frame · adapters found = {}", adapters.len());

    if adapters.is_empty() {
        return Err("no GL adapter found".into());
    }

    let adapter = &adapters[0];
    let info = adapter.get_info();
    eprintln!("test_external_frame · adapter: {:?} backend={:?}", info.name, info.backend);

    // Request device
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("test-external-frame"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            experimental_features: wgpu::ExperimentalFeatures::default(),
        },
    ))
    .map_err(|e| format!("failed to request device: {e}"))?;

    eprintln!("test_external_frame · device created");

    // Create DmaBufImporter
    let importer = DmaBufImporter::new(&device, &queue, adapter);
    eprintln!("test_external_frame · DmaBufImporter created");

    // Create a GBM buffer
    let node: OwnedFd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/dri/renderD128")
        .map_err(|e| format!("no render node: {e}"))?
        .into();

    let gbm_device = gbm::Device::new(node).map_err(|e| format!("no gbm: {e}"))?;
    let (w, h) = (256u32, 256u32);

    // Try creating a buffer with LINEAR modifier (0) for compatibility
    let bo = gbm_device
        .create_buffer_object::<()>(w, h, gbm::Format::Xrgb8888, gbm::BufferObjectFlags::RENDERING)
        .map_err(|e| format!("gbm made no buffer: {e}"))?;

    let modifier: u64 = bo.modifier().into();
    let stride = bo.stride_for_plane(0);
    let offset = bo.offset(0);

    eprintln!("test_external_frame · buffer created: modifier={modifier:#x}, stride={stride}, offset={offset}");

    // Get FD for the buffer
    let fd = bo.fd_for_plane(0).map_err(|e| e.to_string())?;

    // Create DmaBufFrame
    let frame = DmaBufFrame::new(
        w,
        h,
        DmaBufFormat::Bgrx8, // XRGB8888
        modifier,
        vec![DmaBufPlane {
            fd,
            offset,
            stride,
        }],
        None, // no rendering fence
    );

    eprintln!("test_external_frame · DmaBufFrame created");

    // Import into wgpu texture
    let texture = importer.copy_to_texture(frame);
    eprintln!("test_external_frame · texture imported successfully!");
    eprintln!("test_external_frame · texture size: {:?}x{:?}", texture.width(), texture.height());

    // The texture from copy_to_texture has COPY_DST | TEXTURE_BINDING usage,
    // suitable for sampling in shaders, not as a render target.
    eprintln!("test_external_frame · texture usage: {:?}", texture.usage());

    // Verify the texture can be used as a binding (sampled texture)
    let _view = texture.create_view(&Default::default());
    eprintln!("test_external_frame · texture view created successfully!");
    eprintln!("test_external_frame · ALL TESTS PASSED");

    Ok(())
}