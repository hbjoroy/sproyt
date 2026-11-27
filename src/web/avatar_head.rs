use image::DynamicImage;

// SeetaFace frontal detector shipped with rustface v0.1.7 (BSD-2-Clause).
// Model and licence: github.com/atomashpolskiy/rustface/tree/v0.1.7/model
const MODEL: &[u8] = include_bytes!("../../assets/avatar-detection/seeta_fd_frontal_v1.0.bin");

pub(super) fn contains_head(photo: &DynamicImage) -> bool {
    let gray = photo.thumbnail(384, 384).into_luma8();
    if gray.width().min(gray.height()) < 32 {
        return false;
    }
    let model = rustface::read_model(MODEL).expect("bundled avatar detector");
    let mut detector = rustface::create_detector_with_model(model);
    detector.set_min_face_size(32);
    detector.set_slide_window_step(3, 3);
    detector.set_pyramid_scale_factor(0.8);
    detector.set_score_thresh(2.0);
    let pixels = rustface::ImageData::new(gray.as_raw(), gray.width(), gray.height());
    !detector.detect(&pixels).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avatar_requires_a_visible_head() {
        assert!(!contains_head(&DynamicImage::new_rgb8(240, 240)));
        let portrait = image::load_from_memory(include_bytes!(
            "../../assets/imagegen-characters/maria/references/00-canonical-maria.jpg"
        ))
        .unwrap();
        assert!(contains_head(&portrait));
    }
}
