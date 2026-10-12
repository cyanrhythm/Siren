//! 二维码图片生成（二维码登录用）。
//!
//! 将文本编码为二维码并输出灰度 PNG 字节，交由界面层（gpui）解码渲染。

use std::io::Cursor;

/// 将 `content` 编码为二维码并生成灰度 PNG。
///
/// `scale` 为每个模块的像素边长，`quiet` 为四周留白模块数。
pub fn qr_png(content: &str, scale: u32, quiet: u32) -> Option<Vec<u8>> {
    let scale = scale.max(1);
    let code = qrcode::QrCode::new(content.as_bytes()).ok()?;
    let modules = code.width() as u32;
    let colors = code.to_colors();

    let dim = modules + quiet * 2;
    let px = dim * scale;
    let mut data = vec![255u8; (px * px) as usize];
    for y in 0..modules {
        for x in 0..modules {
            if colors[(y * modules + x) as usize] != qrcode::Color::Dark {
                continue;
            }
            let base_x = (x + quiet) * scale;
            let base_y = (y + quiet) * scale;
            for dy in 0..scale {
                let row = (base_y + dy) * px;
                for dx in 0..scale {
                    data[(row + base_x + dx) as usize] = 0;
                }
            }
        }
    }

    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut buf), px, px);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&data).ok()?;
    }
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_png_magic() {
        let bytes = qr_png("https://music.163.com/login?codekey=test", 4, 2).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    }
}
