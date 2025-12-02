use base64::{engine::general_purpose, Engine as _};
use image::{DynamicImage, Rgb, RgbImage};
use qrcode::QrCode;

pub fn generate_qr_code(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let code = QrCode::new(url.as_bytes())?;
    let qr_matrix = code.to_colors();
    let width = code.width();
    let scale = 10;
    let img_size = width * scale;
    let mut img = RgbImage::new(img_size as u32, img_size as u32);

    for (y, row) in qr_matrix.chunks(width).enumerate() {
        for (x, color) in row.iter().enumerate() {
            let pixel_color = match color {
                qrcode::Color::Dark => Rgb([0u8, 0u8, 0u8]),
                qrcode::Color::Light => Rgb([255u8, 255u8, 255u8]),
            };

            for dy in 0..scale {
                for dx in 0..scale {
                    let px = (x * scale + dx) as u32;
                    let py = (y * scale + dy) as u32;
                    if px < img_size as u32 && py < img_size as u32 {
                        img.put_pixel(px, py, pixel_color);
                    }
                }
            }
        }
    }

    let mut png_bytes = Vec::new();
    let dynamic_image = DynamicImage::ImageRgb8(img);
    dynamic_image.write_to(
        &mut std::io::Cursor::new(&mut png_bytes),
        image::ImageFormat::Png,
    )?;

    let base64_image = general_purpose::STANDARD.encode(&png_bytes);
    Ok(format!("data:image/png;base64,{}", base64_image))
}