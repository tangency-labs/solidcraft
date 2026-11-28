//! The COM in-process server File Explorer's thumbnail cache loads: a class factory and a
//! thumbnail handler (`IInitializeWithStream` and `IThumbnailProvider`).
//!
//! Windows runs the handler in its isolated thumbnail process (a COM surrogate), as it runs every
//! handler that takes a stream: it calls `Initialize` with the file's stream, then `GetThumbnail`
//! with the largest side it wants. The handler reads the picture through the stream
//! ([`crate::thumbnail`]) and returns a 32-bit top-down DIB section with straight alpha
//! (`WTSAT_ARGB`). A design without a usable picture fails the request, and Explorer keeps the
//! file's icon. The installer registers the class (assets/platforms/windows/solidcraft-registry.wxi);
//! this DLL writes nothing, the registry included.
//!
//! Exporting `DllGetClassObject` and `DllCanUnloadNow`, implementing COM interfaces (the vtables
//! `#[implement]` writes), reading an `IStream` and filling a DIB section have no safe API, hence
//! the `unsafe_code` allowance on this module (the crate denies it). Every entry point runs its
//! body under `catch_unwind`, so a panic becomes `E_UNEXPECTED` (`S_FALSE` from `DllCanUnloadNow`,
//! which keeps the DLL loaded) and never unwinds into Windows.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io::{self, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_POINTER, E_UNEXPECTED, ERROR_ALREADY_INITIALIZED, S_FALSE, S_OK,
};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP, HGDIOBJ};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, ISequentialStream, IStream, STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithStream, IInitializeWithStream_Impl};
use windows::Win32::UI::Shell::{IThumbnailProvider, IThumbnailProvider_Impl, WTS_ALPHATYPE, WTSAT_ARGB};
use windows::core::{BOOL, Error, GUID, HRESULT, IUnknown, Interface, Ref, Result, implement};

use crate::Bitmap;

const CLSID: GUID = GUID::from_u128(crate::CLSID_U128);

/// Providers and factories alive, and server locks: `DllCanUnloadNow` says yes only at none.
static OBJECTS: AtomicUsize = AtomicUsize::new(0);
static LOCKS: AtomicIsize = AtomicIsize::new(0);

/// Counts one live object for as long as it exists.
struct Live;

impl Live {
    fn new() -> Self {
        OBJECTS.fetch_add(1, Ordering::SeqCst);
        Live
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `work`, with a panic turned into `E_UNEXPECTED`.
fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err(E_UNEXPECTED.into()))
}

/// The thumbnail handler: the stream `Initialize` gave it, read when `GetThumbnail` asks.
#[implement(IInitializeWithStream, IThumbnailProvider)]
struct Provider {
    stream: Mutex<Option<IStream>>,
    _live: Live,
}

impl IInitializeWithStream_Impl for Provider_Impl {
    fn Initialize(&self, pstream: Ref<IStream>, _grfmode: u32) -> Result<()> {
        guarded(|| {
            let stream = pstream.ok()?.clone();
            let mut slot = self.stream.lock().map_err(|_| Error::from(E_UNEXPECTED))?;
            if slot.is_some() {
                return Err(ERROR_ALREADY_INITIALIZED.to_hresult().into());
            }
            *slot = Some(stream);
            Ok(())
        })
    }
}

impl IThumbnailProvider_Impl for Provider_Impl {
    fn GetThumbnail(&self, cx: u32, phbmp: *mut HBITMAP, pdwalpha: *mut WTS_ALPHATYPE) -> Result<()> {
        guarded(|| {
            // SAFETY: each pointer is written only when non-null; COM passes them as this call's
            // out-parameters.
            unsafe {
                if !phbmp.is_null() {
                    phbmp.write(HBITMAP::default());
                }
                if !pdwalpha.is_null() {
                    pdwalpha.write(WTS_ALPHATYPE(0));
                }
            }
            if phbmp.is_null() || pdwalpha.is_null() {
                return Err(E_POINTER.into());
            }
            if cx == 0 {
                return Err(E_INVALIDARG.into());
            }
            let stream = self.stream.lock().map_err(|_| Error::from(E_UNEXPECTED))?.clone().ok_or_else(|| Error::from(E_UNEXPECTED))?;
            let bitmap = crate::thumbnail(StreamReader(stream), cx).ok_or_else(|| Error::from(E_FAIL))?;
            let hbmp = dib(&bitmap)?;
            // SAFETY: as above; the caller owns the bitmap from here on.
            unsafe {
                phbmp.write(hbmp);
                pdwalpha.write(WTSAT_ARGB);
            }
            Ok(())
        })
    }
}

/// A 32-bit top-down DIB section holding `bitmap`'s pixels.
fn dib(bitmap: &Bitmap) -> Result<HBITMAP> {
    let width = i32::try_from(bitmap.width).map_err(|_| Error::from(E_FAIL))?;
    let height = i32::try_from(bitmap.height).map_err(|_| Error::from(E_FAIL))?;
    let bytes = usize::try_from(u64::from(bitmap.width) * u64::from(bitmap.height) * 4).map_err(|_| Error::from(E_FAIL))?;
    if bitmap.bgra.len() != bytes {
        return Err(E_FAIL.into());
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative: the first row in memory is the top one.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: `info` describes a 32-bit DIB of `width` x `height`; `bits` receives its pixel memory.
    let hbmp = unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }?;
    if bits.is_null() {
        // SAFETY: `hbmp` is the section just made, which nothing else holds.
        let _ = unsafe { DeleteObject(HGDIOBJ(hbmp.0)) };
        return Err(E_FAIL.into());
    }
    // SAFETY: the section has `width * height * 4` bytes at `bits`, which is `bgra.len()`, and
    // nothing else uses that memory yet.
    unsafe { std::ptr::copy_nonoverlapping(bitmap.bgra.as_ptr(), bits.cast::<u8>(), bytes) };
    Ok(hbmp)
}

/// `Read` and `Seek` over the stream Windows handed over.
struct StreamReader(IStream);

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let want = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        let mut got = 0_u32;
        let stream: &ISequentialStream = &self.0;
        // SAFETY: `buf` has at least `want` writable bytes; `got` receives how many were read.
        let hr = unsafe { stream.Read(buf.as_mut_ptr().cast(), want, Some(&mut got)) };
        if hr.is_err() {
            return Err(io::Error::other("IStream::Read failed"));
        }
        Ok(got.min(want) as usize)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let (offset, origin) = match pos {
            SeekFrom::Start(n) => (i64::try_from(n).map_err(io::Error::other)?, STREAM_SEEK_SET),
            SeekFrom::Current(n) => (n, STREAM_SEEK_CUR),
            SeekFrom::End(n) => (n, STREAM_SEEK_END),
        };
        let mut at = 0_u64;
        // SAFETY: `at` receives the new position.
        unsafe { self.0.Seek(offset, origin, Some(&mut at)) }.map_err(|_| io::Error::other("IStream::Seek failed"))?;
        Ok(at)
    }
}

/// Makes thumbnail handlers.
#[implement(IClassFactory)]
struct Factory {
    _live: Live,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, punkouter: Ref<IUnknown>, riid: *const GUID, ppvobject: *mut *mut c_void) -> Result<()> {
        guarded(|| {
            if ppvobject.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: a non-null out-parameter.
            unsafe { ppvobject.write(std::ptr::null_mut()) };
            if riid.is_null() {
                return Err(E_POINTER.into());
            }
            if !punkouter.is_null() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let provider: IUnknown = Provider { stream: Mutex::new(None), _live: Live::new() }.into();
            // SAFETY: `riid` and `ppvobject` are the caller's, checked non-null.
            unsafe { provider.query(riid, ppvobject) }.ok()
        })
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        guarded(|| {
            if flock.as_bool() {
                LOCKS.fetch_add(1, Ordering::SeqCst);
            } else {
                LOCKS.fetch_sub(1, Ordering::SeqCst);
            }
            Ok(())
        })
    }
}

/// COM asks the DLL for the class factory of `rclsid`, as the interface `riid`.
///
/// # Safety
/// COM passes valid pointers; null ones are refused.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(rclsid: *const GUID, riid: *const GUID, ppv: *mut *mut c_void) -> HRESULT {
    catch_unwind(|| {
        if ppv.is_null() {
            return E_POINTER;
        }
        // SAFETY: `ppv` is a non-null out-parameter.
        unsafe { ppv.write(std::ptr::null_mut()) };
        if rclsid.is_null() || riid.is_null() {
            return E_POINTER;
        }
        // SAFETY: checked non-null; COM passes a class id, an interface id and an out-parameter.
        unsafe {
            if *rclsid != CLSID {
                return CLASS_E_CLASSNOTAVAILABLE;
            }
            let factory: IClassFactory = Factory { _live: Live::new() }.into();
            factory.query(riid, ppv)
        }
    })
    .unwrap_or(E_UNEXPECTED)
}

/// COM asks whether it may unload the DLL: yes when no provider or factory is alive and no lock is held.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    // A panic keeps the DLL loaded: S_FALSE is the answer that is never wrong.
    catch_unwind(|| if OBJECTS.load(Ordering::SeqCst) == 0 && LOCKS.load(Ordering::SeqCst) <= 0 { S_OK } else { S_FALSE }).unwrap_or(S_FALSE)
}
