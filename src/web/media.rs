use std::io::Cursor;

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
};

use crate::{
    domain::{ChannelId, MediaId, MediaUpload, MediaVariant},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};

const MAX_MEDIA_BYTES: usize = 35 * 1024 * 1024;
const MEDIA_PREVIEW_LONG_EDGE: u32 = 720;

pub(crate) async fn upload_media(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let channel_id = match ChannelId::new(channel) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => {
            return (axum::http::StatusCode::BAD_REQUEST, "missing media file").into_response();
        }
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid multipart upload",
            )
                .into_response();
        }
    };
    let filename = field
        .file_name()
        .unwrap_or("upload")
        .chars()
        .filter(|c| !c.is_control())
        .take(255)
        .collect::<String>();
    let declared_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_owned();
    let content = match field.bytes().await {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_MEDIA_BYTES => bytes.to_vec(),
        Ok(_) => {
            return (
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                "media must contain 1 to 35 MiB",
            )
                .into_response();
        }
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "could not read media").into_response();
        }
    };
    let content_type = match detected_media_type(&content, &declared_type) {
        Some(value) => value,
        None => {
            return (
                axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "only supported images and videos can be uploaded",
            )
                .into_response();
        }
    };
    let (content, dimensions, preview) = match prepare_uploaded_media(content, &content_type).await
    {
        Ok(value) => value,
        Err(MediaPreparationError::InvalidImage) => {
            return (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                "image is incomplete or invalid",
            )
                .into_response();
        }
        Err(MediaPreparationError::Worker) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "could not prepare image",
            )
                .into_response();
        }
    };
    match state
        .chat
        .store_media(MediaUpload {
            actor: principal.user.id,
            channel_id,
            filename: if filename.is_empty() {
                "upload".into()
            } else {
                filename
            },
            content_type,
            content,
            dimensions,
            preview,
        })
        .await
    {
        Ok(media) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({"media": media, "url": format!("/api/v1/media/{}", media.id)})),
        )
            .into_response(),
        Err(error) => chat_error_response(error),
    }
}

#[derive(Debug)]
pub(crate) enum MediaPreparationError {
    InvalidImage,
    Worker,
}

pub(crate) async fn prepare_uploaded_media(
    content: Vec<u8>,
    content_type: &str,
) -> Result<(Vec<u8>, Option<(u32, u32)>, Option<MediaVariant>), MediaPreparationError> {
    if !matches!(
        content_type,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    ) {
        return Ok((content, None, None));
    }
    if !has_complete_image_container(&content, content_type) {
        return Err(MediaPreparationError::InvalidImage);
    }
    let is_jpeg = content_type == "image/jpeg";
    tokio::task::spawn_blocking(move || {
        use image::GenericImageView;
        let image =
            image::load_from_memory(&content).map_err(|_| MediaPreparationError::InvalidImage)?;
        let orientation = exif_orientation(&content, is_jpeg);
        let image = apply_exif_orientation(image, orientation);
        let normalized_content = if is_jpeg && orientation != 1 {
            let mut output = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 92)
                .encode_image(&image)
                .map_err(|_| MediaPreparationError::InvalidImage)?;
            output
        } else {
            content
        };
        let dimensions = image.dimensions();
        if dimensions.0 <= MEDIA_PREVIEW_LONG_EDGE && dimensions.1 <= MEDIA_PREVIEW_LONG_EDGE {
            return Ok((normalized_content, Some(dimensions), None));
        }
        let preview_image = image.resize(
            MEDIA_PREVIEW_LONG_EDGE,
            MEDIA_PREVIEW_LONG_EDGE,
            image::imageops::FilterType::Lanczos3,
        );
        let preview_dimensions = preview_image.dimensions();
        let (preview_type, preview_content) = if preview_image.color().has_alpha() {
            let mut output = Cursor::new(Vec::new());
            preview_image
                .write_to(&mut output, image::ImageFormat::Png)
                .map_err(|_| MediaPreparationError::InvalidImage)?;
            ("image/png", output.into_inner())
        } else {
            let mut output = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 82)
                .encode_image(&preview_image)
                .map_err(|_| MediaPreparationError::InvalidImage)?;
            ("image/jpeg", output)
        };
        Ok((
            normalized_content,
            Some(dimensions),
            Some(MediaVariant {
                content_type: preview_type.to_owned(),
                width: preview_dimensions.0,
                height: preview_dimensions.1,
                content: preview_content,
            }),
        ))
    })
    .await
    .map_err(|_| MediaPreparationError::Worker)?
}

fn exif_orientation(content: &[u8], is_jpeg: bool) -> u32 {
    if !is_jpeg {
        return 1;
    }
    let mut cursor = Cursor::new(content);
    exif::Reader::new()
        .read_from_container(&mut cursor)
        .ok()
        .and_then(|metadata| {
            metadata
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|field| field.value.get_uint(0))
        })
        .unwrap_or(1)
}

fn apply_exif_orientation(image: image::DynamicImage, orientation: u32) -> image::DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

fn has_complete_image_container(content: &[u8], content_type: &str) -> bool {
    match content_type {
        // Android camera apps commonly create motion photos by appending video or
        // metadata after the complete JPEG stream. The EOI marker still proves
        // that the image stream is complete; requiring it to be the final two
        // bytes rejects those otherwise valid uploads.
        "image/jpeg" => {
            content.starts_with(&[0xff, 0xd8])
                && content.windows(2).any(|window| window == [0xff, 0xd9])
        }
        "image/png" => content.ends_with(b"\0\0\0\0IEND\xaeB`\x82"),
        "image/gif" => content.last() == Some(&0x3b),
        "image/webp" => content
            .get(4..8)
            .and_then(|size| size.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|size| usize::try_from(size).ok() == content.len().checked_sub(8)),
        _ => false,
    }
}

pub(crate) async fn download_media(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let media_id = match MediaId::new(id) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    let (media, content) = match state.chat.load_media(principal.user.id, media_id).await {
        Ok(value) => value,
        Err(error) => return chat_error_response(error),
    };
    let mut response = content.into_response();
    if let Ok(value) = HeaderValue::from_str(&media.content_type) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, value);
    }
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

pub(crate) async fn download_media_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let media_id = match MediaId::new(id) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .chat
        .load_media_preview(principal.user.id.clone(), media_id)
        .await
    {
        Ok(Some(preview)) => media_content_response(preview.content_type, preview.content),
        Ok(None) => match state.chat.load_media(principal.user.id, media_id).await {
            Ok((media, content)) => media_content_response(media.content_type, content),
            Err(error) => chat_error_response(error),
        },
        Err(error) => chat_error_response(error),
    }
}

fn media_content_response(content_type: String, content: Vec<u8>) -> axum::response::Response {
    let mut response = content.into_response();
    if let Ok(value) = HeaderValue::from_str(&content_type) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, value);
    }
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400, immutable"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

pub(crate) fn detected_media_type(content: &[u8], declared: &str) -> Option<String> {
    let detected = if content.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if content.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if content.starts_with(b"RIFF") && content.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if content.get(4..8) == Some(b"ftyp") {
        match content.get(8..12) {
            Some(b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1") => Some("image/heic"),
            Some(b"avif" | b"avis") => Some("image/avif"),
            Some(b"qt  ") => Some("video/quicktime"),
            _ => Some("video/mp4"),
        }
    } else if content.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        Some(if declared == "video/webm" {
            "video/webm"
        } else {
            "video/x-matroska"
        })
    } else {
        None
    };
    detected.map(str::to_owned)
}
