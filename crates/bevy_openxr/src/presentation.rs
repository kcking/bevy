use ash::vk::{self, Handle, InstanceCreateFlags};
use bevy_xr::presentation::XrGraphicsContext;
use openxr as xr;
use std::{error::Error, ffi::CStr, sync::Arc};
use wgpu::{Backends, DeviceDescriptor, Features};
use wgpu_hal as hal;
use xr::sys::platform::VkInstanceCreateInfo;

#[derive(Clone)]
pub enum GraphicsContextHandles {
    Vulkan {
        instance: ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: ash::Device,
        queue_family_index: u32,
        queue_index: u32,
    },
}

#[derive(Debug, thiserror::Error)]
#[error("Error creating HAL adapter")]
pub struct AdapterError;

pub fn create_graphics_context(
    instance: &xr::Instance,
    system: xr::SystemId,
) -> Result<(GraphicsContextHandles, XrGraphicsContext), Box<dyn Error>> {
    let device_descriptor = wgpu::DeviceDescriptor::default();

    if instance.exts().khr_vulkan_enable2.is_some() {
        let vk_entry = unsafe { ash::Entry::load().unwrap() };

        // Vulkan 1.0 constrained by Oculus Go support.
        // NOTE: multiview support will require Vulkan 1.1 or specific extensions
        let vk_version = vk::make_api_version(0, 1, 1, 0);

        // todo: check requirements
        let _requirements = instance
            .graphics_requirements::<xr::Vulkan>(system)
            .unwrap();

        let vk_app_info = vk::ApplicationInfo::builder()
            .application_version(0)
            .engine_version(0)
            .api_version(vk_version);

        let mut flags = hal::InstanceFlags::empty();
        if cfg!(debug_assertions) {
            flags |= hal::InstanceFlags::VALIDATION;
            flags |= hal::InstanceFlags::DEBUG;
        }

        let mut instance_extensions =
            <hal::api::Vulkan as hal::Api>::Instance::required_extensions(&vk_entry, flags)
                .map_err(Box::new)?;
        if cfg!(target_os = "macos") {
            instance_extensions
                .push(CStr::from_bytes_with_nul(b"VK_KHR_portability_enumeration\0").unwrap());
        }
        //  quest incorrectly reports TimelineSemaphore availability
        #[cfg(target_os = "android")]
        instance_extensions.retain(|ext| ext != &vk::KhrGetPhysicalDeviceProperties2Fn::name());
        let instance_extensions_ptrs = instance_extensions
            .iter()
            .map(|x| x.as_ptr())
            .collect::<Vec<_>>();

        let create_info = vk::InstanceCreateInfo::builder()
            .application_info(&vk_app_info)
            .enabled_extension_names(&instance_extensions_ptrs)
            .flags(InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR)
            .build();

        let vk_instance = unsafe {
            let vk_instance = instance
                .create_vulkan_instance(
                    system,
                    std::mem::transmute(vk_entry.static_fn().get_instance_proc_addr),
                    &create_info as *const vk::InstanceCreateInfo as *const VkInstanceCreateInfo,
                )
                .map_err(Box::new)?
                .map_err(|e| Box::new(vk::Result::from_raw(e)))?;

            ash::Instance::load(
                vk_entry.static_fn(),
                vk::Instance::from_raw(vk_instance as _),
            )
        };
        let hal_instance = unsafe {
            <hal::api::Vulkan as hal::Api>::Instance::from_raw(
                vk_entry.clone(),
                vk_instance.clone(),
                vk_version,
                29,
                instance_extensions,
                flags,
                false, //TODO: is this correct?
                Some(Box::new(instance.clone())),
            )
            .map_err(Box::new)?
        };

        let wgpu_instance = unsafe { wgpu::Instance::from_hal::<hal::api::Vulkan>(hal_instance) };
        let wgpu_adapter = wgpu_instance
            .enumerate_adapters(Backends::VULKAN)
            .next()
            .unwrap();
        let (wgpu_device, wgpu_queue) =
            futures_lite::future::block_on(wgpu_adapter.request_device(
                &DeviceDescriptor {
                    //  MUTLIVIEW doesn't work here on quest 2
                    features: Features::empty(),
                    ..Default::default()
                },
                None,
            ))
            .unwrap();

        let (vk_physical, vk_device, queue_family_index, queue_index) = unsafe {
            wgpu_device.as_hal::<wgpu_hal::api::Vulkan, _, _>(|dev| {
                let dev = dev.unwrap();
                (
                    dev.raw_physical_device(),
                    dev.raw_device().clone(),
                    dev.queue_family_index(),
                    dev.queue_index(),
                )
            })
        };

        Ok((
            GraphicsContextHandles::Vulkan {
                instance: vk_instance,
                physical_device: vk_physical,
                device: vk_device,
                queue_family_index,
                queue_index,
            },
            XrGraphicsContext {
                instance: Some(wgpu_instance),
                device: Arc::new(wgpu_device),
                queue: Arc::new(wgpu_queue),
                adapter_info: wgpu_adapter.get_info(),
                adapter: wgpu_adapter.into(),
            },
        ))
    } else {
        Err(Box::new(xr::sys::Result::ERROR_EXTENSION_NOT_PRESENT))
    }
}
