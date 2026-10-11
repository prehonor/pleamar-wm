//! Test: Can Vulkan create a buffer and export it as DMA-BUF?
//! This tests the reverse flow: Vulkan -> DMA-BUF -> KMS

pub fn test_vulkan_export() -> Result<(), String> {
    println!("export-test · Testing Vulkan -> DMA-BUF export on AMD RX 580 (Polaris)");
    
    // Load Vulkan directly via ash
    let entry = unsafe { ash::Entry::load() }.map_err(|e| format!("Failed to load Vulkan: {e}"))?;
    
    // Create instance with external memory extensions
    let app_info = ash::vk::ApplicationInfo::default()
        .api_version(ash::vk::API_VERSION_1_2);
    
    let extensions = [
        ash::khr::external_memory_capabilities::NAME.as_ptr(),
    ];
    
    let create_info = ash::vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extensions);
    
    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(|e| format!("Failed to create Vulkan instance: {e}"))?;
    
    // Enumerate physical devices
    let devices = unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| format!("Failed to enumerate devices: {e}"))?;
    
    println!("export-test · Found {} Vulkan device(s)", devices.len());
    
    for (i, &device) in devices.iter().enumerate() {
        let props = unsafe { instance.get_physical_device_properties(device) };
        let name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) }
            .to_string_lossy();
        
        println!("export-test · Device {}: {}", i, name);
        
        // Check external memory capabilities
        let mut external_image_format = ash::vk::PhysicalDeviceExternalImageFormatInfo::default()
            .handle_type(ash::vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        
        let image_format_info = ash::vk::PhysicalDeviceImageFormatInfo2::default()
            .format(ash::vk::Format::B8G8R8A8_UNORM)
            .ty(ash::vk::ImageType::TYPE_2D)
            .tiling(ash::vk::ImageTiling::LINEAR)
            .usage(ash::vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .push_next(&mut external_image_format);
        
        let mut external_format_props = ash::vk::ExternalImageFormatProperties::default();
        let mut image_format_props = ash::vk::ImageFormatProperties2::default()
            .push_next(&mut external_format_props);
        
        let result = unsafe {
            instance.get_physical_device_image_format_properties2(
                device,
                &image_format_info,
                &mut image_format_props,
            )
        };
        
        match result {
            Ok(_) => {
                let features = external_format_props.external_memory_properties.external_memory_features;
                println!("export-test ·   DMA_BUF_EXT export: {}", 
                    features.contains(ash::vk::ExternalMemoryFeatureFlags::EXPORTABLE));
                println!("export-test ·   DMA_BUF_EXT import: {}", 
                    features.contains(ash::vk::ExternalMemoryFeatureFlags::IMPORTABLE));
                println!("export-test ·   Dedicated only: {}", 
                    features.contains(ash::vk::ExternalMemoryFeatureFlags::DEDICATED_ONLY));
            }
            Err(e) => {
                println!("export-test ·   Image format not supported for DMA-BUF: {e}");
            }
        }
        
        // Also test OPTIMAL tiling
        let mut external_image_format2 = ash::vk::PhysicalDeviceExternalImageFormatInfo::default()
            .handle_type(ash::vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        
        let image_format_info2 = ash::vk::PhysicalDeviceImageFormatInfo2::default()
            .format(ash::vk::Format::B8G8R8A8_UNORM)
            .ty(ash::vk::ImageType::TYPE_2D)
            .tiling(ash::vk::ImageTiling::OPTIMAL)
            .usage(ash::vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .push_next(&mut external_image_format2);
        
        let mut external_format_props2 = ash::vk::ExternalImageFormatProperties::default();
        let mut image_format_props2 = ash::vk::ImageFormatProperties2::default()
            .push_next(&mut external_format_props2);
        
        let result2 = unsafe {
            instance.get_physical_device_image_format_properties2(
                device,
                &image_format_info2,
                &mut image_format_props2,
            )
        };
        
        match result2 {
            Ok(_) => {
                let features = external_format_props2.external_memory_properties.external_memory_features;
                println!("export-test ·   DMA_BUF_EXT export (OPTIMAL): {}", 
                    features.contains(ash::vk::ExternalMemoryFeatureFlags::EXPORTABLE));
            }
            Err(e) => {
                println!("export-test ·   OPTIMAL tiling not supported for DMA-BUF: {e}");
            }
        }
    }
    
    unsafe { instance.destroy_instance(None) };
    
    println!("export-test · Test complete");
    Ok(())
}
