//! 解碼圖檔給預覽區用。
//!
//! **跟實際繪製用同一套解碼器（Windows 內建的 WIC）**——換成別的
//! 解碼器的話，同一張圖在兩邊可能長得不一樣（色彩管理、gamma 處理
//! 各家不同），那預覽就失去意義了。也因此不必引入第三方的圖片套件。

use windows::core::PCWSTR;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppRGBA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapPaletteTypeMedianCut, WICDecodeMetadataCacheOnLoad,
};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

/// 載入圖檔，回傳 `(寬, 高, RGBA 像素)`。失敗回 `None`。
///
/// 轉成 **32bppRGBA**（非預乘）——egui 要的是這個順序與格式。
/// 實際繪製那邊要的是 32bppPBGRA（預乘、BGR 順序），兩者不同，
/// 各自轉換即可。
pub fn load_rgba(path: &std::path::Path) -> Option<(usize, usize, Vec<u8>)> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(wide.as_ptr()),
                None,
                windows::Win32::Foundation::GENERIC_READ,
                WICDecodeMetadataCacheOnLoad,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter
            .Initialize(
                &frame,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeMedianCut,
            )
            .ok()?;

        let mut w = 0u32;
        let mut h = 0u32;
        converter.GetSize(&mut w, &mut h).ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        let stride = w as usize * 4;
        let mut buf = vec![0u8; stride * h as usize];
        converter
            .CopyPixels(std::ptr::null(), stride as u32, &mut buf)
            .ok()?;
        Some((w as usize, h as usize, buf))
    }
}
