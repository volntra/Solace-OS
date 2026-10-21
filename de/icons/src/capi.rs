//! The C API (`include/fern-icons.h`), for the Qt image provider.
//!
//! Every function catches panics, so none unwinds into C. Strings are
//! UTF-8 and NUL-terminated; an engine may be used from several threads.

// The FFI boundary: raw pointers from C, checked for null here.
#![allow(unsafe_code)]

use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use crate::{Engine, Error, Mode, Request};

/// Results of `fern_icons_render`.
pub const OK: i32 = 0;
pub const NOT_FOUND: i32 = 1;
pub const BAD_ICON: i32 = 2;
pub const BAD_REQUEST: i32 = -1;

/// A C string as `&str`; `None` for null or invalid UTF-8.
///
/// # Safety
/// `s` is null or a valid NUL-terminated string that outlives the result.
unsafe fn text<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    // SAFETY: non-null and NUL-terminated, as the caller promises.
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

/// A new engine. `dirs` (`ndirs` strings) are searched in order; when
/// `ndirs` is 0 the standard places are used. `cache_dir` may be null
/// (no disk cache) or empty (the standard cache). Free with
/// `fern_icons_free`.
///
/// # Safety
/// `dirs` points to `ndirs` valid strings (or is null when `ndirs` is 0);
/// `cache_dir` is null or a valid string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_icons_new(
    dirs: *const *const c_char,
    ndirs: usize,
    cache_dir: *const c_char,
) -> *mut Engine {
    let made = catch_unwind(AssertUnwindSafe(|| {
        if ndirs == 0 {
            let standard = Engine::standard();
            // SAFETY: as the caller promises.
            return match unsafe { text(cache_dir) } {
                None if !cache_dir.is_null() => None,
                None => Some(Engine::new(standard.dirs, None)),
                Some("") => Some(standard),
                Some(c) => Some(Engine::new(standard.dirs, Some(PathBuf::from(c)))),
            };
        }
        if dirs.is_null() {
            return None;
        }
        // SAFETY: `dirs` holds `ndirs` pointers.
        let list = unsafe { std::slice::from_raw_parts(dirs, ndirs) };
        let mut paths = Vec::with_capacity(ndirs);
        for &d in list {
            // SAFETY: each is a valid string.
            paths.push(PathBuf::from(unsafe { text(d) }?));
        }
        // SAFETY: as the caller promises.
        let cache = match unsafe { text(cache_dir) } {
            None if !cache_dir.is_null() => return None,
            None => None,
            Some("") => Engine::standard().cache,
            Some(c) => Some(PathBuf::from(c)),
        };
        Some(Engine::new(paths, cache))
    }));
    match made {
        Ok(Some(engine)) => Box::into_raw(Box::new(engine)),
        _ => std::ptr::null_mut(),
    }
}

/// Frees an engine (null is ignored).
///
/// # Safety
/// `engine` came from `fern_icons_new` and is not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_icons_free(engine: *mut Engine) {
    if !engine.is_null() {
        // SAFETY: made by Box::into_raw in fern_icons_new.
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Draws icon `id` (or, if it has no `.fernicon`, the legacy file
/// `legacy`, which may be null) in `mode` (0 default, 1 dark, 2 tinted,
/// 3 wallpaper, 4 clear) with `accent` (`0xRRGGBB`) at `size` pixels into
/// `out`: `size * size * 4` bytes of premultiplied RGBA. Returns 0, 1 (not
/// found), 2 (the icon cannot be drawn) or -1 (bad arguments).
///
/// # Safety
/// `engine` is a live engine; `id` and `legacy` are null or valid strings;
/// `out` points to `out_len` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_icons_render(
    engine: *const Engine,
    id: *const c_char,
    legacy: *const c_char,
    mode: u32,
    accent: u32,
    size: u32,
    out: *mut u8,
    out_len: usize,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if engine.is_null() || out.is_null() {
            return BAD_REQUEST;
        }
        let Some(&mode) = usize::try_from(mode).ok().and_then(|m| Mode::ALL.get(m)) else {
            return BAD_REQUEST;
        };
        // SAFETY: as the caller promises.
        let (id, legacy) = unsafe {
            (
                text(id).unwrap_or(""),
                text(legacy).filter(|l| !l.is_empty()),
            )
        };
        let req = Request {
            id,
            legacy: legacy.map(Path::new),
            mode,
            accent,
            size,
        };
        // SAFETY: a live engine, shared (the engine locks inside).
        match unsafe { &*engine }.render(&req) {
            Ok(image) => {
                if image.data.len() != out_len {
                    return BAD_REQUEST;
                }
                // SAFETY: `out` has `out_len` writable bytes.
                unsafe { std::ptr::copy_nonoverlapping(image.data.as_ptr(), out, out_len) };
                OK
            }
            Err(Error::NotFound) => NOT_FOUND,
            Err(Error::BadIcon(_)) => BAD_ICON,
            Err(Error::BadRequest(_)) => BAD_REQUEST,
        }
    }));
    result.unwrap_or(BAD_ICON)
}

/// The accent a wallpaper (PNG) suggests, `0xRRGGBB`, written to `out`.
/// Returns 0, or -1 if the file cannot be read.
///
/// # Safety
/// `path` is a valid string; `out` points to a writable `uint32_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn fern_icons_wallpaper_accent(path: *const c_char, out: *mut u32) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: as the caller promises.
        let Some(path) = (unsafe { text(path) }) else {
            return BAD_REQUEST;
        };
        if out.is_null() {
            return BAD_REQUEST;
        }
        match crate::wallpaper_accent(Path::new(path)) {
            Some(c) => {
                // SAFETY: a writable uint32_t.
                unsafe { *out = c };
                OK
            }
            None => BAD_REQUEST,
        }
    }));
    result.unwrap_or(BAD_REQUEST)
}

/// The colour `rgb` (`0xRRGGBB`) at another HCT tone (0 to 100).
#[unsafe(no_mangle)]
pub extern "C" fn fern_icons_tone(rgb: u32, tone: f64) -> u32 {
    catch_unwind(|| crate::tone(rgb, tone)).unwrap_or(rgb)
}

#[cfg(test)]
// Colours read as #rrggbb; pixel tests name their channels.
#[allow(clippy::unreadable_literal, clippy::many_single_char_names)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn renders_through_the_c_api() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("org.example.C.fernicon");
        std::fs::create_dir(&icon).unwrap();
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024"><rect width="1024" height="1024" fill="#2D64C8"/></svg>"##;
        std::fs::write(icon.join("background.svg"), svg).unwrap();
        std::fs::write(icon.join("foreground.svg"), svg).unwrap();
        std::fs::write(icon.join("icon.toml"), "format = 1\nkey = \"#2D64C8\"\n").unwrap();

        let d = CString::new(dir.path().to_str().unwrap()).unwrap();
        let dirs = [d.as_ptr()];
        // SAFETY: valid arguments, as documented.
        unsafe {
            let e = fern_icons_new(dirs.as_ptr(), 1, std::ptr::null());
            assert!(!e.is_null());
            let id = CString::new("org.example.C").unwrap();
            let mut out = vec![0u8; 32 * 32 * 4];
            assert_eq!(
                fern_icons_render(
                    e,
                    id.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    32,
                    out.as_mut_ptr(),
                    out.len()
                ),
                OK
            );
            assert!(out.iter().any(|&b| b != 0));
            // The wrong buffer size, an unknown mode, a missing icon.
            assert_eq!(
                fern_icons_render(
                    e,
                    id.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    32,
                    out.as_mut_ptr(),
                    10
                ),
                BAD_REQUEST
            );
            assert_eq!(
                fern_icons_render(
                    e,
                    id.as_ptr(),
                    std::ptr::null(),
                    9,
                    0,
                    32,
                    out.as_mut_ptr(),
                    out.len()
                ),
                BAD_REQUEST
            );
            let missing = CString::new("org.example.Missing").unwrap();
            assert_eq!(
                fern_icons_render(
                    e,
                    missing.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    32,
                    out.as_mut_ptr(),
                    out.len()
                ),
                NOT_FOUND
            );
            assert_eq!(
                fern_icons_render(
                    std::ptr::null(),
                    id.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    32,
                    out.as_mut_ptr(),
                    out.len()
                ),
                BAD_REQUEST
            );
            fern_icons_free(e);
            fern_icons_free(std::ptr::null_mut());

            let mut c = 0u32;
            let nope = CString::new("/nonexistent.png").unwrap();
            assert_eq!(
                fern_icons_wallpaper_accent(nope.as_ptr(), &raw mut c),
                BAD_REQUEST
            );
        }
        assert_eq!(fern_icons_tone(0xFFFFFF, 100.0), 0xFFFFFF);
    }
}
