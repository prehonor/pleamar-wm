//! DMA-BUF import for EGL/GLES backend.
//!
//! This module creates wgpu textures that share memory with DMA-BUF buffers,
//! enabling zero-copy rendering to DRM buffers on hardware that lacks
//! VK_EXT_external_memory_dma_buf (e.g., AMD Polaris10).
//!
//! The approach:
//! 1. Create EGLImageKHR from DMA-BUF fd via EGL_LINUX_DMA_BUF_EXT
//! 2. Bind EGLImage to a GL texture via glEGLImageTargetTexture2DOES
//! 3. Wrap the GL texture as a wgpu HAL texture
//! 4. Create wgpu texture from HAL texture with RENDER_ATTACHMENT usage

use std::ffi::{c_char, c_int, c_uint, c_void};
use std::os::fd::AsRawFd;
use pleamar::wgpu;
use glow::HasContext;

// EGL constants
const EGL_NONE: c_int = 0x3038;
const EGL_LINUX_DMA_BUF_EXT: c_uint = 0x3270;
const EGL_LINUX_DRM_FOURCC_EXT: c_int = 0x3271;
const EGL_DMA_BUF_PLANE0_FD_EXT: c_int = 0x3272;
const EGL_DMA_BUF_PLANE0_OFFSET_EXT: c_int = 0x3273;
const EGL_DMA_BUF_PLANE0_PITCH_EXT: c_int = 0x3274;
const EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT: c_int = 0x3443;
const EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT: c_int = 0x3444;
const EGL_WIDTH: c_int = 0x3057;
const EGL_HEIGHT: c_int = 0x3056;

// DRM formats
const DRM_FORMAT_XRGB8888: u32 = 0x34325258;

// EGL function types
type EglDisplay = *mut c_void;
type EglImage = *mut c_void;
type EglGetCurrentDisplay = unsafe extern "C" fn() -> EglDisplay;
type EglCreateImage = unsafe extern "C" fn(EglDisplay, *mut c_void, c_uint, *mut c_void, *const c_int) -> EglImage;
type EglDestroyImage = unsafe extern "C" fn(EglDisplay, EglImage) -> c_uint;
type EglGetProcAddress = unsafe extern "C" fn(*const c_char) -> *const c_void;
type GlEglImageTargetTexture2d = unsafe extern "C" fn(c_uint, EglImage);

/// Manages EGL DMA-BUF import lifecycle.
pub struct DmaBufEglTexture {
    texture: wgpu::Texture,
    _egl_image: EglImage,
    _egl_display: EglDisplay,
    egl_destroy_image: EglDestroyImage,
}

impl DmaBufEglTexture {
    /// Import a DMA-BUF as a wgpu texture with RENDER_ATTACHMENT usage.
    ///
    /// # Safety
    ///
    /// The device must be a GLES backend. The fd must be valid for the duration
    /// of the returned texture's lifetime.
    pub unsafe fn import(
        device: &wgpu::Device,
        fd: impl AsRawFd,
        width: u32,
        height: u32,
        modifier: u64,
        stride: u32,
        offset: u32,
    ) -> Result<Self, String> {
        let raw_fd = fd.as_raw_fd();

        // Get the HAL device to access the GL context
        let hal_device = unsafe {
            device
                .as_hal::<wgpu::hal::api::Gles>()
                .ok_or("DMA-BUF EGL import requires a GLES device")?
        };

        // Lock the GL context
        let _context_lock = hal_device.context().lock();

        // Load EGL functions
        let egl_library = unsafe {
            libloading::Library::new("libEGL.so.1")
                .map_err(|e| format!("failed to load libEGL.so.1: {e}"))?
        };
        let gles_library = unsafe {
            libloading::Library::new("libGLESv2.so.2")
                .map_err(|e| format!("failed to load libGLESv2.so.2: {e}"))?
        };

        // Get eglGetProcAddress
        let get_proc_address: libloading::Symbol<EglGetProcAddress> = unsafe {
            egl_library
                .get(b"eglGetProcAddress\0")
                .map_err(|e| format!("eglGetProcAddress not found: {e}"))?
        };

        // Helper to load EGL/GL functions
        let load_egl = |name: &str| -> Option<*const c_void> {
            let c_name = std::ffi::CString::new(name).unwrap();
            let ptr = unsafe { get_proc_address(c_name.as_ptr()) };
            if ptr.is_null() {
                // Try dlsym as fallback
                unsafe {
                    let sym = libc::dlsym(
                        libc::RTLD_DEFAULT,
                        c_name.as_ptr() as *const c_char,
                    );
                    if !sym.is_null() {
                        return Some(sym as *const c_void);
                    }
                }
                None
            } else {
                Some(ptr)
            }
        };

        // Load required functions
        let egl_get_current_display: EglGetCurrentDisplay = unsafe {
            std::mem::transmute(
                load_egl("eglGetCurrentDisplay")
                    .ok_or("eglGetCurrentDisplay not found")?,
            )
        };
        let egl_create_image: EglCreateImage = unsafe {
            std::mem::transmute(
                load_egl("eglCreateImageKHR")
                    .ok_or("eglCreateImageKHR not found")?,
            )
        };
        let egl_destroy_image: EglDestroyImage = unsafe {
            std::mem::transmute(
                load_egl("eglDestroyImageKHR")
                    .ok_or("eglDestroyImageKHR not found")?,
            )
        };
        let image_target_texture: GlEglImageTargetTexture2d = unsafe {
            std::mem::transmute(
                load_egl("glEGLImageTargetTexture2DOES")
                    .ok_or("glEGLImageTargetTexture2DOES not found")?,
            )
        };

        // Get current EGL display
        let egl_display = unsafe { egl_get_current_display() };
        if egl_display.is_null() {
            return Err("no current EGL display".to_string());
        }

        // Build EGL attributes for DMA-BUF import
        let modifier_lo = (modifier & 0xFFFFFFFF) as c_int;
        let modifier_hi = ((modifier >> 32) & 0xFFFFFFFF) as c_int;

        let attribs: Vec<c_int> = vec![
            EGL_LINUX_DRM_FOURCC_EXT,
            DRM_FORMAT_XRGB8888 as c_int,
            EGL_DMA_BUF_PLANE0_FD_EXT,
            raw_fd,
            EGL_DMA_BUF_PLANE0_OFFSET_EXT,
            offset as c_int,
            EGL_DMA_BUF_PLANE0_PITCH_EXT,
            stride as c_int,
            EGL_DMA_BUF_PLANE0_MODIFIER_LO_EXT,
            modifier_lo,
            EGL_DMA_BUF_PLANE0_MODIFIER_HI_EXT,
            modifier_hi,
            EGL_WIDTH,
            width as c_int,
            EGL_HEIGHT,
            height as c_int,
            EGL_NONE,
        ];

        // Create EGLImage from DMA-BUF
        let egl_image = unsafe {
            egl_create_image(
                egl_display,
                std::ptr::null_mut(), // EGL_NO_CONTEXT
                EGL_LINUX_DMA_BUF_EXT,
                std::ptr::null_mut(), // NULL buffer
                attribs.as_ptr(),
            )
        };
        if egl_image.is_null() {
            return Err("eglCreateImageKHR failed".to_string());
        }

        // Create a glow context to issue GL commands
        let gl = unsafe {
            glow::Context::from_loader_function(|name| {
                let c_name = std::ffi::CString::new(name).unwrap();
                let ptr = get_proc_address(c_name.as_ptr());
                if ptr.is_null() {
                    // Try dlsym fallback
                    let sym = libc::dlsym(
                        libc::RTLD_DEFAULT,
                        c_name.as_ptr() as *const c_char,
                    );
                    if !sym.is_null() {
                        sym as *const c_void
                    } else {
                        std::ptr::null()
                    }
                } else {
                    ptr as *const c_void
                }
            })
        };

        // Create GL texture and bind EGLImage to it
        let raw_texture = unsafe {
            let tex = gl.create_texture().map_err(|e| format!("glCreateTexture failed: {e}"))?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            image_target_texture(glow::TEXTURE_2D, egl_image);
            gl.bind_texture(glow::TEXTURE_2D, None);
            tex
        };

        // Construct HAL texture
        let hal_texture = wgpu::hal::gles::Texture {
            inner: wgpu::hal::gles::TextureInner::Texture {
                raw: raw_texture,
                target: glow::TEXTURE_2D,
            },
            mip_level_count: 1,
            array_layer_count: 1,
            format: wgpu::TextureFormat::Bgra8Unorm,
            format_desc: wgpu::hal::gles::TextureFormatDesc {
                internal: glow::RGBA8,
                external: glow::BGRA,
                data_type: glow::UNSIGNED_BYTE,
            },
            copy_size: wgpu::hal::CopyExtent {
                width,
                height,
                depth: 1,
            },
            drop_guard: None,
        };

        // Create wgpu texture from HAL texture
        let texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Gles>(
                hal_texture,
                &wgpu::TextureDescriptor {
                    label: Some("dma_buf_egl_render_target"),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                },
                wgpu::TextureUses::COLOR_TARGET,
            )
        };

        // Keep libraries loaded for the lifetime of the texture
        let _ = (egl_library, gles_library);

        Ok(DmaBufEglTexture {
            texture,
            _egl_image: egl_image,
            _egl_display: egl_display,
            egl_destroy_image,
        })
    }

    /// Get the wgpu texture.
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
}

impl Drop for DmaBufEglTexture {
    fn drop(&mut self) {
        if !self._egl_image.is_null() {
            unsafe {
                (self.egl_destroy_image)(self._egl_display, self._egl_image);
            }
        }
    }
}