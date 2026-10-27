//! Capturing the window as a PNG without focus and without a permission
//! prompt. With Screen Recording already granted to whatever launched the
//! app, `screencapture -x -l` captures the window; without it (the usual
//! case for an agent's shell) `screencapture` cannot see the window, so the
//! app copies its own window from the window server, which needs no
//! permission. Either way the window first draws fresh frames: an occluded
//! window gets no display-link frames, and the transcript settles over
//! several (layout, then the follow scroll and entrance motions), so the
//! hook asks AppKit to display the view at about 60 Hz for `SETTLE`.
use crate::shell::Shell;
use gpui::{AsyncApp, WindowHandle};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

/// How long the hook draws frames before it copies the window.
const SETTLE: Duration = Duration::from_millis(1200);
/// The pause between those frames, and before the copy.
const FRAME: Duration = Duration::from_millis(16);

#[cfg(target_os = "macos")]
pub(crate) async fn capture(
    window: WindowHandle<Shell>,
    path: &Path,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let started = Instant::now();
    let number = loop {
        let number = draw(window, cx)?;
        if started.elapsed() >= SETTLE {
            break number;
        }
        cx.background_executor().timer(FRAME).await;
    };
    cx.background_executor().timer(FRAME).await;
    let path_owned = path.to_path_buf();
    let method = cx
        .background_executor()
        .spawn(async move { write(number, &path_owned) })
        .await?;
    Ok(json!({
        "path": path,
        "method": method,
        "windowNumber": number,
        "screenRecordingGranted": preflight(),
    }))
}

#[cfg(not(target_os = "macos"))]
pub(crate) async fn capture(
    _: WindowHandle<Shell>,
    _: &Path,
    _: &mut AsyncApp,
) -> Result<Value, String> {
    Err("window capture is only implemented on macOS".into())
}

/// Draws one fresh frame of the window now, even while it is occluded, and
/// returns the window's number.
#[cfg(target_os = "macos")]
fn draw(window: WindowHandle<Shell>, cx: &mut AsyncApp) -> Result<u32, String> {
    use objc2_app_kit::NSView;
    let native = window
        .update(cx, |_, window, _| {
            window.refresh();
            native(window)
        })
        .map_err(|e| e.to_string())?
        .ok_or("the window has no AppKit view")?;
    // SAFETY: on the main thread, outside any update, and with no await
    // since the window answered, so it is still open: AppKit displays the
    // layer now and GPUI answers with a frame.
    let view = unsafe { &*native.view.cast::<NSView>() };
    view.setNeedsDisplay(true);
    view.displayIfNeeded();
    Ok(native.number)
}

/// GPUI's view (the layer delegate that draws frames) and the window's number.
#[cfg(target_os = "macos")]
pub(crate) struct Native {
    view: *const std::ffi::c_void,
    pub(crate) number: u32,
}

#[cfg(target_os = "macos")]
pub(crate) fn native(window: &gpui::Window) -> Option<Native> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let ns_window = crate::traffic_lights::ns_window(window)?;
    Some(Native {
        view: appkit.ns_view.as_ptr().cast_const(),
        number: u32::try_from(ns_window.windowNumber()).ok()?,
    })
}

/// Writes window `number` to `path`, and says how.
#[cfg(target_os = "macos")]
fn write(number: u32, path: &Path) -> Result<&'static str, String> {
    if preflight() {
        let status = std::process::Command::new("/usr/sbin/screencapture")
            .args(["-x", "-o", "-l", &number.to_string()])
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() && path.is_file() {
            return Ok("screencapture");
        }
    }
    let image = cg::own_window(number)?;
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok("own-window")
}

/// Whether the process may record the screen; asks nothing.
#[cfg(target_os = "macos")]
pub(crate) fn preflight() -> bool {
    // SAFETY: a side-effect-free CoreGraphics query.
    unsafe { cg::CGPreflightScreenCaptureAccess() }
}

#[cfg(target_os = "macos")]
mod cg {
    //! CoreGraphics' copy of one window this process owns. The window
    //! server hands a process its own windows without Screen Recording.
    use std::ffi::c_void;

    #[repr(C)]
    struct Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }
    const INCLUDING_WINDOW: u32 = 1 << 3;
    const IGNORE_FRAMING: u32 = 1 << 0;
    const BEST_RESOLUTION: u32 = 1 << 3;
    const BYTE_ORDER_MASK: u32 = 0x7000;
    const BYTE_ORDER_32_LITTLE: u32 = 2 << 12;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        pub(super) fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGWindowListCreateImage(bounds: Rect, list: u32, window: u32, image: u32)
        -> *mut c_void;
        fn CGImageGetWidth(image: *mut c_void) -> usize;
        fn CGImageGetHeight(image: *mut c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *mut c_void) -> usize;
        fn CGImageGetBitsPerPixel(image: *mut c_void) -> usize;
        fn CGImageGetBitmapInfo(image: *mut c_void) -> u32;
        fn CGImageGetDataProvider(image: *mut c_void) -> *mut c_void;
        fn CGDataProviderCopyData(provider: *mut c_void) -> *const c_void;
        fn CGImageRelease(image: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFDataGetLength(data: *const c_void) -> isize;
        fn CFRelease(value: *const c_void);
    }

    /// Window `number`'s pixels as RGBA.
    pub(super) fn own_window(number: u32) -> Result<image::RgbaImage, String> {
        // CGRectNull: the window's own bounds.
        let null = Rect {
            x: f64::INFINITY,
            y: f64::INFINITY,
            width: 0.,
            height: 0.,
        };
        // SAFETY: CoreGraphics returns an owned image or null; every
        // pointer is checked, read within its length and released once.
        unsafe {
            let image = CGWindowListCreateImage(
                null,
                INCLUDING_WINDOW,
                number,
                IGNORE_FRAMING | BEST_RESOLUTION,
            );
            if image.is_null() {
                return Err(format!("CoreGraphics returned no image of window {number}"));
            }
            let result = pixels(image);
            CGImageRelease(image);
            result
        }
    }

    unsafe fn pixels(image: *mut c_void) -> Result<image::RgbaImage, String> {
        // SAFETY: `image` is a live CGImage (see `own_window`).
        unsafe {
            let (width, height) = (CGImageGetWidth(image), CGImageGetHeight(image));
            let stride = CGImageGetBytesPerRow(image);
            if CGImageGetBitsPerPixel(image) != 32 || width == 0 || height == 0 {
                return Err("the window image is not 32-bit pixels".into());
            }
            let data = CGDataProviderCopyData(CGImageGetDataProvider(image));
            if data.is_null() {
                return Err("the window image has no pixel data".into());
            }
            let length = usize::try_from(CFDataGetLength(data)).unwrap_or(0);
            if stride < width * 4 || length < stride * (height - 1) + width * 4 {
                CFRelease(data);
                return Err("the window image is shorter than its size".into());
            }
            let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data), length);
            let bgra = CGImageGetBitmapInfo(image) & BYTE_ORDER_MASK == BYTE_ORDER_32_LITTLE;
            let mut rgba = Vec::with_capacity(width * height * 4);
            for row in bytes.chunks(stride).take(height) {
                for &[first, second, third, alpha] in row[..width * 4].as_chunks::<4>().0 {
                    rgba.extend_from_slice(&if bgra {
                        [third, second, first, alpha]
                    } else {
                        [first, second, third, alpha]
                    });
                }
            }
            CFRelease(data);
            image::RgbaImage::from_raw(
                u32::try_from(width).map_err(|e| e.to_string())?,
                u32::try_from(height).map_err(|e| e.to_string())?,
                rgba,
            )
            .ok_or_else(|| "the window image has the wrong size".into())
        }
    }
}
