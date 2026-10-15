use std::fmt;

pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[derive(Debug)]
pub enum GfxError {
    NoAdapter(String),
    Device(String),
    Surface(String),
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GfxError::NoAdapter(e) => write!(f, "no suitable GPU adapter: {e}"),
            GfxError::Device(e) => write!(f, "GPU device error: {e}"),
            GfxError::Surface(e) => write!(f, "surface error: {e}"),
        }
    }
}

impl std::error::Error for GfxError {}

/// Device, queue and adapter shared by every renderer in the process.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn new(
        compatible_surface: Option<&wgpu::Surface<'_>>,
        instance: wgpu::Instance,
    ) -> Result<Self, GfxError> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface,
            apply_limit_buckets: false,
        }))
        .map_err(|e| GfxError::NoAdapter(e.to_string()))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("deckforge-gfx"),
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .map_err(|e| GfxError::Device(e.to_string()))?;
        Ok(Self {
            instance,
            adapter,
            device,
            queue,
        })
    }

    pub fn headless() -> Result<Self, GfxError> {
        Self::new(None, new_instance())
    }

    pub fn adapter_summary(&self) -> String {
        let i = self.adapter.get_info();
        format!("{} ({:?}, {:?})", i.name, i.backend, i.device_type)
    }
}

pub fn new_instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}
