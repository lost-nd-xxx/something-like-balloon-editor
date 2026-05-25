/// Windows GDI を使ったテキスト描画ユーティリティ
/// GdiSession でDC・フォント・ビットマップを使い回し、draw_preview 全体を高速化する。
use image::RgbaImage;
use crate::core::color::Rgb;

#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::Foundation::{COLORREF, RECT},
    Win32::Graphics::Gdi::{
        CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, DeleteDC, DeleteObject,
        GetDIBits, SelectObject, SetBkMode, SetTextColor, TextOutW,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
        DEFAULT_PITCH, DIB_RGB_COLORS, FF_DONTCARE, NONANTIALIASED_QUALITY,
        OUT_TT_PRECIS, TRANSPARENT, FW_NORMAL, DEFAULT_QUALITY,
        CreateSolidBrush, FillRect,
        GetTextMetricsW, TEXTMETRICW,
        HDC, HFONT, HBITMAP,
    },
};

// ---------------------------------------------------------------------------
// GDI セッション（DC・フォント・ビットマップを使い回す）
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub struct GdiSession {
    hdc:      HDC,
    hfont:    HFONT,
    hbmp:     HBITMAP,
    bmp_w:    i32,
    bmp_h:    i32,
    bg_brush: windows::Win32::Graphics::Gdi::HBRUSH,
    pub raw_buf: Vec<u32>,
}

#[cfg(windows)]
impl GdiSession {
    pub fn new(font_name: &str, size_px: f32, no_aa: bool) -> Option<Self> {
        unsafe {
            let hdc_screen = windows::Win32::Graphics::Gdi::GetDC(None);
            if hdc_screen.is_invalid() { return None; }
            let hdc = CreateCompatibleDC(hdc_screen);
            windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc_screen);
            if hdc.is_invalid() { return None; }

            let hfont = create_gdi_font(font_name, size_px, no_aa);
            if hfont.is_invalid() { let _ = DeleteDC(hdc); return None; }
            let _ = SelectObject(hdc, hfont);

            SetTextColor(hdc, COLORREF(0x00000000));
            let _ = SetBkMode(hdc, TRANSPARENT);

            let bg_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));

            let hdc_screen2 = windows::Win32::Graphics::Gdi::GetDC(None);
            let hbmp = CreateCompatibleBitmap(hdc_screen2, 1, 1);
            windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc_screen2);
            let _ = SelectObject(hdc, hbmp);

            Some(Self { hdc, hfont, hbmp, bmp_w: 1, bmp_h: 1, bg_brush, raw_buf: Vec::new() })
        }
    }

    /// 必要なサイズのビットマップを確保する。
    /// サイズが変わるたびに作り直してストライドを描画幅に一致させる。
    pub fn ensure_bmp(&mut self, w: i32, h: i32) {
        if w == self.bmp_w && h == self.bmp_h { return; }
        unsafe {
            let hdc_screen = windows::Win32::Graphics::Gdi::GetDC(None);
            let new_bmp = CreateCompatibleBitmap(hdc_screen, w, h);
            windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc_screen);
            let _ = SelectObject(self.hdc, new_bmp);
            let _ = DeleteObject(self.hbmp);
            self.hbmp = new_bmp;
            self.bmp_w = w;
            self.bmp_h = h;
        }
    }

    /// ビットマップに描画して raw_buf を更新する。
    pub fn render_to_bmp(&mut self, text: &str, w: i32, h: i32) {
        unsafe {
            let rc = RECT { left: 0, top: 0, right: w, bottom: h };
            let _ = FillRect(self.hdc, &rc, self.bg_brush);
            let text_wide = to_wide(text);
            let wide_no_null = &text_wide[..text_wide.len() - 1];
            let _ = TextOutW(self.hdc, 0, 0, wide_no_null);
            let pixel_count = (w * h) as usize;
            self.raw_buf.resize(pixel_count, 0u32);
            let mut bmi = make_bmi(w, h);
            let _ = GetDIBits(
                self.hdc, self.hbmp, 0, h as u32,
                Some(self.raw_buf.as_mut_ptr() as *mut _),
                &mut bmi, DIB_RGB_COLORS,
            );
        }
    }

    pub fn bmp_stride(&self) -> i32 { self.bmp_w }

    pub fn measure(&self, text: &str) -> f32 {
        unsafe {
            let text_wide = to_wide(text);
            let wide_no_null = &text_wide[..text_wide.len() - 1];
            let mut sz = windows::Win32::Foundation::SIZE::default();
            let _ = windows::Win32::Graphics::Gdi::GetTextExtentPoint32W(
                self.hdc, wide_no_null, &mut sz,
            );
            sz.cx as f32
        }
    }

    pub fn internal_leading(&self) -> i32 {
        unsafe {
            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(self.hdc, &mut tm);
            tm.tmInternalLeading
        }
    }

    pub fn clip_to_max_x(&self, text: &str, px: i32, max_x: i32) -> String {
        let budget = (max_x - px).max(0) as i32;
        if text.is_empty() || budget <= 0 { return String::new(); }
        unsafe {
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let n = wide.len() - 1;
            let mut extents: Vec<i32> = vec![0i32; n];
            let mut sz = windows::Win32::Foundation::SIZE::default();
            let _ = windows::Win32::Graphics::Gdi::GetTextExtentExPointW(
                self.hdc,
                PCWSTR(wide.as_ptr()),
                n as i32,
                i32::MAX,
                None,
                Some(extents.as_mut_ptr()),
                &mut sz,
            );
            let mut fit_cu = 0usize;
            for i in 0..n {
                let start = if i == 0 { 0 } else { extents[i - 1] };
                if start >= budget { break; }
                fit_cu = i + 1;
            }
            String::from_utf16_lossy(&wide[..fit_cu]).to_string()
        }
    }
}

#[cfg(windows)]
impl Drop for GdiSession {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.bg_brush);
            let _ = DeleteObject(self.hbmp);
            let _ = DeleteObject(self.hfont);
            let _ = DeleteDC(self.hdc);
        }
    }
}

// ---------------------------------------------------------------------------
// 公開 API
// ---------------------------------------------------------------------------

/// GDI でテキストを描画し、RgbaImage の指定座標に合成する。
pub fn draw_text_gdi(
    img: &mut RgbaImage,
    font_name: &str,
    text: &str,
    px: i32,
    py: i32,
    size_px: f32,
    color: Rgb,
    shadow: Option<(Rgb, bool)>,
    max_x: Option<i32>,
    no_aa: bool,
) -> bool {
    #[cfg(windows)]
    { draw_text_gdi_impl(img, font_name, text, px, py, size_px, color, shadow, max_x, no_aa) }
    #[cfg(not(windows))]
    { let _ = (img, font_name, text, px, py, size_px, color, shadow, max_x, no_aa); false }
}

/// GDI フォントの internal leading を返す。
pub fn font_internal_leading(font_name: &str, size_px: f32, no_aa: bool) -> i32 {
    #[cfg(windows)]
    {
        if let Some(sess) = GdiSession::new(font_name, size_px, no_aa) {
            return sess.internal_leading();
        }
    }
    #[cfg(not(windows))]
    let _ = (font_name, size_px, no_aa);
    0
}

/// GDI でテキスト幅を計測する。
pub fn measure_text_gdi(font_name: &str, text: &str, size_px: f32, no_aa: bool) -> Option<f32> {
    #[cfg(windows)]
    { measure_text_gdi_impl(font_name, text, size_px, no_aa) }
    #[cfg(not(windows))]
    { let _ = (font_name, text, size_px, no_aa); None }
}

/// raw_buf を img に合成する（preview.rs から直接呼ぶ用）。
/// stride: raw_buf の1行あたりの画素数、draw_w: 実際に描画する幅
#[cfg(windows)]
pub fn paste_raw(
    img: &mut RgbaImage,
    raw: &[u32],
    stride: i32, draw_w: i32, bmp_h: i32,
    px: i32, py: i32,
    color: Rgb,
) {
    paste_alpha_raw(img, raw, stride, draw_w, bmp_h, px, py, color);
}

// ---------------------------------------------------------------------------
// Windows 内部実装
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn draw_text_gdi_impl(
    img: &mut RgbaImage,
    font_name: &str,
    text: &str,
    px: i32,
    py: i32,
    size_px: f32,
    color: Rgb,
    shadow: Option<(Rgb, bool)>,
    max_x: Option<i32>,
    no_aa: bool,
) -> bool {
    let mut sess = match GdiSession::new(font_name, size_px, no_aa) {
        Some(s) => s,
        None => return false,
    };
    let clipped = match max_x {
        Some(mx) => sess.clip_to_max_x(text, px, mx),
        None => text.to_string(),
    };
    let text = clipped.as_str();
    if text.is_empty() { return true; }

    let char_count = text.chars().count();
    let w = (size_px * char_count as f32 * 1.2 + 4.0) as i32;
    let h = (size_px * 1.5 + 4.0) as i32;
    if w == 0 || h == 0 { return false; }

    sess.ensure_bmp(w, h);
    sess.render_to_bmp(text, w, h);
    let stride = sess.bmp_stride();
    let alpha_mask: Vec<u32> = sess.raw_buf[..(stride * h) as usize].to_vec();

    if let Some((sc, is_outline)) = shadow {
        if is_outline {
            for (ddx, ddy) in [(-1i32,-1i32),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                paste_alpha_raw(img, &alpha_mask, stride, w, h, px + ddx, py + ddy, sc);
            }
        } else {
            paste_alpha_raw(img, &alpha_mask, stride, w, h, px + 1, py + 1, sc);
        }
    }
    paste_alpha_raw(img, &alpha_mask, stride, w, h, px, py, color);
    true
}

#[cfg(windows)]
fn measure_text_gdi_impl(font_name: &str, text: &str, size_px: f32, no_aa: bool) -> Option<f32> {
    let sess = GdiSession::new(font_name, size_px, no_aa)?;
    Some(sess.measure(text))
}

#[cfg(windows)]
fn paste_alpha_raw(
    img: &mut RgbaImage,
    raw: &[u32],
    stride: i32, draw_w: i32, bmp_h: i32,
    px: i32, py: i32,
    color: Rgb,
) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let Rgb(cr, cg, cb) = color;
    for ry in 0..bmp_h {
        for rx in 0..draw_w {
            let p = raw[(ry * stride + rx) as usize];
            let b = (p & 0xFF) as u8;
            let g = ((p >> 8) & 0xFF) as u8;
            let r = ((p >> 16) & 0xFF) as u8;
            // 輝度ベースでアルファを計算（ClearTypeサブピクセルでR/G/Bが異なる場合に対応）
            let lum = (r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000;
            let alpha = 255 - lum.min(255) as u8;
            if alpha == 0 { continue; }
            let dx = px + rx;
            let dy = py + ry;
            if dx < 0 || dy < 0 || dx >= iw || dy >= ih { continue; }
            blend_px(img, dx, dy, Rgb(cr, cg, cb), alpha);
        }
    }
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn create_gdi_font(font_name: &str, size_px: f32, no_aa: bool) -> HFONT {
    let height = -(size_px.round() as i32);
    let quality = if no_aa { NONANTIALIASED_QUALITY.0 } else { DEFAULT_QUALITY.0 };
    let name_wide = to_wide(font_name);
    unsafe {
        CreateFontW(
            height, 0, 0, 0,
            FW_NORMAL.0 as i32,
            0, 0, 0,
            DEFAULT_CHARSET.0 as u32,
            OUT_TT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            quality as u32,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            PCWSTR(name_wide.as_ptr()),
        )
    }
}

#[cfg(windows)]
fn make_bmi(w: i32, h: i32) -> BITMAPINFO {
    BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
    }
}

fn blend_px(img: &mut RgbaImage, x: i32, y: i32, col: Rgb, alpha: u8) {
    if alpha == 0 { return; }
    let px = img.get_pixel_mut(x as u32, y as u32);
    let a = alpha as u32;
    let ia = 255 - a;
    px[0] = ((col.0 as u32 * a + px[0] as u32 * ia) / 255) as u8;
    px[1] = ((col.1 as u32 * a + px[1] as u32 * ia) / 255) as u8;
    px[2] = ((col.2 as u32 * a + px[2] as u32 * ia) / 255) as u8;
    px[3] = (a + px[3] as u32 * ia / 255) as u8;
}
