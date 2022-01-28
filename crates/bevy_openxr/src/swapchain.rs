use super::*;
use ash::vk;
use openxr as xr;

pub const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_SRGB;

pub struct EyeSwapchains {
    pub left: Swapchain,
    pub right: Swapchain,
}

impl EyeSwapchains {
    pub fn new(
        xr_session: &xr::Session<xr::Vulkan>,
        resolutions: &[vk::Extent2D],
        device: Arc<wgpu::Device>,
    ) -> Result<Self, OpenXrError> {
        Ok(Self {
            left: create_swapchain(xr_session, resolutions[0], device.clone())?,
            right: create_swapchain(xr_session, resolutions[1], device.clone())?,
        })
    }
}

pub fn create_swapchain(
    xr_session: &xr::Session<xr::Vulkan>,
    resolution: vk::Extent2D,
    device: Arc<wgpu::Device>,
) -> Result<Swapchain, OpenXrError> {
    let swapchain = xr_session
        .create_swapchain(&xr::SwapchainCreateInfo {
            create_flags: xr::SwapchainCreateFlags::EMPTY,
            usage_flags: xr::SwapchainUsageFlags::COLOR_ATTACHMENT,
            format: COLOR_FORMAT.as_raw() as u32,
            sample_count: 1,
            width: resolution.width,
            height: resolution.height,
            face_count: 1,
            array_size: 1,
            mip_count: 1,
        })
        .map_err(OpenXrError::SwapchainCreation)?;
    let images: Vec<_> = swapchain
        .enumerate_images()
        .unwrap()
        .into_iter()
        .map(|color_image| {
            let color_image = vk::Image::from_raw(color_image);

            color_image
        })
        .collect();

    let wgpu_resolution = wgpu::Extent3d {
        width: resolution.width,
        height: resolution.height,
        depth_or_array_layers: 1,
    };
    let textures = images
        .iter()
        .map(|image| {
            let tex = unsafe {
                <wgpu_hal::api::Vulkan as wgpu_hal::Api>::Device::texture_from_raw(
                    *image,
                    &wgpu_hal::TextureDescriptor {
                        label: None,
                        size: wgpu_resolution,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: TextureUses::COLOR_TARGET,
                        memory_flags: wgpu_hal::MemoryFlags::empty(),
                    },
                    Some(Box::new(())),
                )
            };

            let tex = unsafe {
                device.create_texture_from_hal::<wgpu_hal::api::Vulkan>(
                    tex,
                    &wgpu::TextureDescriptor {
                        size: wgpu_resolution,
                        sample_count: 1,
                        mip_level_count: 1,
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        usage: TextureUsages::RENDER_ATTACHMENT,
                        dimension: wgpu::TextureDimension::D2,
                        label: None,
                    },
                )
            };

            tex
        })
        .collect();
    Ok(Swapchain {
        resolution,
        handle: swapchain,
        device,

        textures,
    })
}

pub struct Swapchain {
    pub handle: xr::Swapchain<xr::Vulkan>,
    pub resolution: vk::Extent2D,
    pub device: Arc<wgpu::Device>,

    pub textures: Vec<wgpu::Texture>,
}

impl Swapchain {
    pub fn acquire_texture_view(&mut self) -> Result<wgpu::TextureView, xr::sys::Result> {
        let idx = self.handle.acquire_image()? as usize;
        self.handle.wait_image(xr::Duration::INFINITE)?;
        let tex = self.textures.get(idx).unwrap();

        let tex_view = tex.create_view(&TextureViewDescriptor {
            label: None,
            format: Some(wgpu::TextureFormat::Rgba8UnormSrgb),
            mip_level_count: None,
            base_mip_level: 0,
            array_layer_count: None,
            base_array_layer: 0,
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::All,
        });

        Ok(tex_view)
    }

    pub fn release(&mut self) -> Result<(), xr::sys::Result> {
        self.handle.release_image()
    }
}
