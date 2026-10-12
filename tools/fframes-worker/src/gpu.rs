//! Hardware-only Skia contexts. A missing GPU is an error, never a CPU fallback.
#[cfg(not(target_os = "macos"))]
pub fn context(
    width: usize,
    height: usize,
) -> Result<(fframes_skia_renderer::vulkan::SkiaVulkanCtx, String), String> {
    use ash::vk;
    use std::ffi::CStr;
    // SAFETY: ash owns the loader; the instance is transferred to SkiaVulkanCtx,
    // which outlives its borrowed frame renderer and all submitted work.
    unsafe {
        let entry = ash::Entry::load().map_err(|e| format!("Vulkan loader unavailable: {e}"))?;
        let info = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1);
        let extensions = [vk::KHR_SURFACE_NAME.as_ptr()];
        let create = vk::InstanceCreateInfo::default()
            .application_info(&info)
            .enabled_extension_names(&extensions);
        let instance = entry
            .create_instance(&create, None)
            .map_err(|e| e.to_string())?;
        let mut devices = instance
            .enumerate_physical_devices()
            .map_err(|e| e.to_string())?;
        devices.sort_by_key(
            |d| match instance.get_physical_device_properties(*d).device_type {
                vk::PhysicalDeviceType::DISCRETE_GPU => 0,
                vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
                _ => 2,
            },
        );
        let chosen = devices.into_iter().find(|d| {
            let properties = instance.get_physical_device_properties(*d);
            matches!(
                properties.device_type,
                vk::PhysicalDeviceType::DISCRETE_GPU | vk::PhysicalDeviceType::INTEGRATED_GPU
            ) && instance
                .get_physical_device_queue_family_properties(*d)
                .iter()
                .any(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        });
        let Some(device) = chosen else {
            instance.destroy_instance(None);
            return Err("no physical Vulkan GPU with a graphics queue".into());
        };
        let properties = instance.get_physical_device_properties(device);
        let name = CStr::from_ptr(properties.device_name.as_ptr())
            .to_string_lossy()
            .into_owned();
        fframes_skia_renderer::vulkan::SkiaVulkanCtx::new_with_device(
            entry, instance, device, width, height,
        )
        .map(|context| (context, format!("Vulkan / {name}")))
        .map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
pub fn context(
    width: usize,
    height: usize,
) -> Result<(fframes_skia_renderer::metal::SkiaMetalCtx, String), String> {
    use fframes_skia_renderer::metal::{SkiaMetalCtx, metal_rs};
    let device = metal_rs::Device::system_default().ok_or("no Metal GPU")?;
    let description = format!("Metal / {}", device.name());
    let queue = device.new_command_queue();
    SkiaMetalCtx::new_with_device(device, queue, width, height)
        .map(|context| (context, description))
        .map_err(|e| e.to_string())
}
