use std::io::Cursor;

use axum::{
    Json,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, HeaderValue, Method},
    response::IntoResponse,
};

use crate::{
    domain::{ChannelId, MediaId, MediaUpload, MediaVariant},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};

const MAX_MEDIA_BYTES: usize = 35 * 1024 * 1024;
const MEDIA_PREVIEW_LONG_EDGE: u32 = 720;
const MAX_IMAGE_PIXELS: u64 = 24_000_000;
const MAX_IMAGE_ALLOC: u64 = 96 * 1024 * 1024;

// Admission precedes reading large bodies. Do not queue compressed originals.
static MEDIA_ADMISSION: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
static IMAGE_PROCESSING: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
// Slow clients retain one permit, never a database connection or whole original.
static MEDIA_DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

pub(crate) fn admit_media()
-> Result<tokio::sync::SemaphorePermit<'static>, Box<axum::response::Response>> {
    MEDIA_ADMISSION.try_acquire().map_err(|_| {
        Box::new(
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                [(axum::http::header::RETRY_AFTER, "2")],
                "Biletebehandling er opptatt. Prøv igjen om litt.",
            )
                .into_response(),
        )
    })
}

pub(crate) async fn process_image<F, T>(work: F) -> Result<T, tokio::task::JoinError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let permit = IMAGE_PROCESSING
        .acquire()
        .await
        .expect("image processing semaphore stays open");
    tokio::task::spawn_blocking(move || {
        // A disconnected request must not release the budget while its worker runs.
        let _permit = permit;
        work()
    })
    .await
}

pub(crate) fn decode_image(content: &[u8]) -> Result<image::DynamicImage, MediaPreparationError> {
    let limits = || {
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(MAX_IMAGE_ALLOC);
        limits
    };
    let reader = || {
        let mut reader = image::ImageReader::new(Cursor::new(content))
            .with_guessed_format()
            .map_err(|_| MediaPreparationError::InvalidImage)?;
        reader.limits(limits());
        Ok::<_, MediaPreparationError>(reader)
    };
    let map_error = |error| match error {
        image::ImageError::Limits(_) => MediaPreparationError::TooLarge,
        _ => MediaPreparationError::InvalidImage,
    };
    let (width, height) = reader()?.into_dimensions().map_err(map_error)?;
    if u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(MediaPreparationError::TooLarge);
    }
    reader()?.decode().map_err(map_error)
}

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
    if state
        .media_store
        .as_ref()
        .is_some_and(|store| store.channel_enabled(*channel_id.as_uuid()))
    {
        return (axum::http::StatusCode::CONFLICT,"Oppdater Sprøyt og prøv opplastinga igjen. Denne serveren brukar den nye opplastingsflyten.").into_response();
    }
    let _admission = match admit_media() {
        Ok(permit) => permit,
        Err(response) => return *response,
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
        Err(MediaPreparationError::TooLarge) => {
            return (
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                "Biletet er for stort å behandle. Bruk eit bilete på høgst 24 megapikslar og 8192 pikslar per side.",
            ).into_response();
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
    TooLarge,
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
    process_image(move || {
        use image::GenericImageView;
        let image = decode_image(&content)?;
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
    method: Method,
) -> axum::response::Response {
    serve_media(state, id, query, headers, method, false).await
}

pub(crate) async fn download_media_attachment(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    method: Method,
) -> axum::response::Response {
    serve_media(state, id, query, headers, method, true).await
}

async fn serve_media(
    state: AppState,
    id: String,
    query: WsQuery,
    headers: HeaderMap,
    method: Method,
    attachment: bool,
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
    serve_authorized_media(
        state.chat,
        principal.user.id,
        media_id,
        headers,
        method,
        attachment,
    )
    .await
}

async fn serve_authorized_media(
    chat: crate::chat::ChatEngine,
    actor: crate::domain::UserId,
    media_id: MediaId,
    headers: HeaderMap,
    method: Method,
    attachment: bool,
) -> axum::response::Response {
    let media = match chat.media_metadata(actor.clone(), media_id).await {
        Ok(value) => value,
        Err(error) => return chat_error_response(error),
    };
    let etag = format!("\"{}\"", media.sha256);
    // Range applies to GET only. Dates and mismatching If-Range validators fall
    // back to a complete response; we support one strong entity tag.
    let requested_range = (method == Method::GET
        && headers
            .get(axum::http::header::IF_RANGE)
            .is_none_or(|value| value.as_bytes() == etag.as_bytes()))
    .then(|| {
        headers
            .get(axum::http::header::RANGE)
            .and_then(|value| value.to_str().ok())
    })
    .flatten();
    let range = match byte_range(requested_range, media.size_bytes) {
        Ok(value) => value,
        Err(()) => {
            return (
                axum::http::StatusCode::RANGE_NOT_SATISFIABLE,
                [(
                    axum::http::header::CONTENT_RANGE,
                    format!("bytes */{}", media.size_bytes),
                )],
            )
                .into_response();
        }
    };
    let (start, length) = range.unwrap_or((0, media.size_bytes));
    let body = if method == Method::HEAD || length == 0 {
        axum::body::Body::empty()
    } else {
        let permit = match MEDIA_DOWNLOADS.try_acquire() {
            Ok(permit) => permit,
            Err(_) => {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    [(axum::http::header::RETRY_AFTER, "2")],
                    "Nedlasting er opptatt. Prøv igjen om litt.",
                )
                    .into_response();
            }
        };
        media_stream(chat, actor, media_id, start, length, permit)
    };
    let mut response = body.into_response();
    if range.is_some() {
        *response.status_mut() = axum::http::StatusCode::PARTIAL_CONTENT;
        response.headers_mut().insert(
            axum::http::header::CONTENT_RANGE,
            HeaderValue::from_str(&format!(
                "bytes {start}-{}/{}",
                start + length - 1,
                media.size_bytes
            ))
            .unwrap(),
        );
    }
    response.headers_mut().insert(
        axum::http::header::CONTENT_LENGTH,
        HeaderValue::from(length),
    );
    response.headers_mut().insert(
        axum::http::header::ACCEPT_RANGES,
        HeaderValue::from_static("bytes"),
    );
    if let Ok(value) = HeaderValue::from_str(&etag) {
        response
            .headers_mut()
            .insert(axum::http::header::ETAG, value);
    }
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
    if attachment {
        response.headers_mut().insert(
            axum::http::header::CONTENT_DISPOSITION,
            attachment_disposition(&media.original_filename, &media.id, &media.content_type),
        );
    }
    response
}

fn media_stream(
    chat: crate::chat::ChatEngine,
    actor: crate::domain::UserId,
    id: MediaId,
    start: u64,
    length: u64,
    permit: tokio::sync::SemaphorePermit<'static>,
) -> axum::body::Body {
    axum::body::Body::from_stream(async_stream::stream! {
        let _permit = permit;
        let mut offset = start;
        let end = start + length;
        while offset < end {
            let size = (end - offset).min(crate::domain::MEDIA_READ_BLOCK_BYTES as u64) as usize;
            // This future releases the SQL connection before yielding to HTTP.
            let bytes = match chat.read_media_slice(actor.clone(), id, offset, size).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    yield Err(std::io::Error::other(error.to_string()));
                    break;
                }
            };
            if bytes.len() != size {
                yield Err(std::io::Error::other("incomplete stored media"));
                break;
            }
            offset += bytes.len() as u64;
            yield Ok::<_, std::io::Error>(axum::body::Bytes::from(bytes));
        }
    })
}

/// Returns (offset, length); unsupported units/multiple ranges use a full GET.
fn byte_range(header: Option<&str>, size: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = header.and_then(|value| value.strip_prefix("bytes=")) else {
        return Ok(None);
    };
    if value.contains(',') {
        return Ok(None);
    }
    let (left, right) = value.split_once('-').ok_or(())?;
    let parse = |value: &str| -> Result<u64, ()> {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(());
        }
        value.parse().map_err(|_| ())
    };
    if size == 0 {
        return Err(());
    }
    if left.is_empty() {
        let length = parse(right)?.min(size);
        if length == 0 {
            return Err(());
        }
        return Ok(Some((size - length, length)));
    }
    let start = parse(left)?;
    if start >= size {
        return Err(());
    }
    let end = if right.is_empty() {
        size - 1
    } else {
        parse(right)?.min(size - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(Some((start, end - start + 1)))
}

fn attachment_disposition(filename: &str, id: &MediaId, content_type: &str) -> HeaderValue {
    let extension = match content_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/heic" => "heic",
        _ => "bin",
    };
    let fallback = format!("sproyt-media-{id}.{extension}");
    let cleaned = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(180)
        .collect::<String>();
    let cleaned = cleaned.trim();
    let name = if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        &fallback
    } else {
        cleaned
    };
    let encoded = name
        .as_bytes()
        .iter()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                (*byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect::<String>();
    let mut ascii = String::new();
    for ch in name.chars() {
        match ch {
            'æ' => ascii.push_str("ae"),
            'Æ' => ascii.push_str("Ae"),
            'ø' | 'ö' => ascii.push('o'),
            'Ø' | 'Ö' => ascii.push('O'),
            'å' | 'ä' => ascii.push('a'),
            'Å' | 'Ä' => ascii.push('A'),
            ch if ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.' | '(' | ')') => {
                ascii.push(ch)
            }
            _ => ascii.push('_'),
        }
    }
    let ascii = ascii.trim_matches([' ', '_']);
    let ascii = if ascii.is_empty() || ascii == "." || ascii == ".." {
        &fallback
    } else {
        ascii
    };
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

pub(crate) async fn download_media_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    method: Method,
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
        // Legacy files may use the original as their preview. Keep that path
        // bounded too; chunked uploads will instead expose pending status.
        Ok(None) => {
            let status = match state
                .chat
                .media_preview_state(principal.user.id.clone(), media_id)
                .await
            {
                Ok(value) => value,
                Err(error) => return chat_error_response(error),
            };
            if status != "legacy" {
                return (axum::http::StatusCode::ACCEPTED,
                    [(axum::http::header::CONTENT_TYPE,"image/svg+xml"),(axum::http::header::CACHE_CONTROL,"private, no-store"),(axum::http::HeaderName::from_static("x-content-type-options"),"nosniff")],
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"320\" height=\"180\" viewBox=\"0 0 320 180\"><rect width=\"320\" height=\"180\" fill=\"#e8e8e1\"/><text x=\"160\" y=\"90\" text-anchor=\"middle\" fill=\"#50554a\" font-family=\"sans-serif\" font-size=\"14\">Førehandsvising blir laga …</text></svg>".replace("Førehandsvising blir laga …",if status=="failed" {"Førehandsvisinga kunne ikkje lagast"} else {"Førehandsvising blir laga …"})).into_response();
            }
            serve_authorized_media(
                state.chat,
                principal.user.id,
                media_id,
                headers,
                method,
                false,
            )
            .await
        }
        Err(error) => chat_error_response(error),
    }
}

pub(crate) async fn media_preview_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let id = match MediaId::new(id) {
        Ok(id) => id,
        Err(_) => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };
    let status = match state.chat.media_preview_state(principal.user.id, id).await {
        Ok(value) => value,
        Err(error) => return chat_error_response(error),
    };
    (
        [(axum::http::header::CACHE_CONTROL, "private, no-store")],
        axum::Json(serde_json::json!({"state":status})),
    )
        .into_response()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_ranges_cover_video_seeking_and_bad_bounds() {
        assert_eq!(byte_range(Some("bytes=2-5"), 10), Ok(Some((2, 4))));
        assert_eq!(byte_range(Some("bytes=2-"), 10), Ok(Some((2, 8))));
        assert_eq!(byte_range(Some("bytes=-3"), 10), Ok(Some((7, 3))));
        assert_eq!(byte_range(Some("bytes=-20"), 10), Ok(Some((0, 10))));
        assert_eq!(byte_range(Some("bytes=2-20"), 10), Ok(Some((2, 8))));
        for range in [
            "bytes=10-",
            "bytes=-0",
            "bytes=4-2",
            "bytes=x-",
            "bytes=+2-",
            "bytes=18446744073709551616-",
        ] {
            assert_eq!(byte_range(Some(range), 10), Err(()), "{range}");
        }
        assert_eq!(byte_range(Some("bytes=0-"), 0), Err(()));
        assert_eq!(byte_range(Some("items=0-1"), 10), Ok(None));
        assert_eq!(byte_range(Some("bytes=0-1,4-5"), 10), Ok(None));
    }

    #[tokio::test]
    async fn dropping_or_failing_download_releases_its_permit() {
        static TEST_DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        let chat = crate::chat::ChatEngine::start(std::sync::Arc::new(
            crate::domain::InMemoryChatRepository::default(),
        ));
        let body = media_stream(
            chat.clone(),
            crate::domain::UserId::named("reader"),
            MediaId::generate(),
            0,
            10,
            TEST_DOWNLOADS.try_acquire().unwrap(),
        );
        assert_eq!(TEST_DOWNLOADS.available_permits(), 0);
        drop(body);
        assert_eq!(TEST_DOWNLOADS.available_permits(), 1);
        let body = media_stream(
            chat.clone(),
            crate::domain::UserId::named("reader"),
            MediaId::generate(),
            0,
            10,
            TEST_DOWNLOADS.try_acquire().unwrap(),
        );
        assert!(axum::body::to_bytes(body, 10).await.is_err());
        assert_eq!(TEST_DOWNLOADS.available_permits(), 1);

        let owner = crate::auth::AuthService::development()
            .authenticate_request(Some("stream-reader".into()), None)
            .await
            .unwrap();
        chat.ensure_user(owner.user.clone()).await.unwrap();
        let channel = chat
            .create_channel(
                owner.user.id.clone(),
                crate::domain::ChannelSlug::new("stream-cancel").unwrap(),
                crate::domain::DisplayName::new("Stream cancel").unwrap(),
                crate::domain::ChannelKind::Private,
                None,
            )
            .await
            .unwrap();
        let size = crate::domain::MEDIA_READ_BLOCK_BYTES + 1;
        let media = chat
            .store_media(MediaUpload {
                actor: owner.user.id.clone(),
                channel_id: channel.id,
                filename: "video.mp4".into(),
                content_type: "video/mp4".into(),
                content: vec![7; size],
                dimensions: None,
                preview: None,
            })
            .await
            .unwrap();
        let mut stream = media_stream(
            chat,
            owner.user.id,
            media.id,
            0,
            size as u64,
            TEST_DOWNLOADS.try_acquire().unwrap(),
        )
        .into_data_stream();
        use futures_util::StreamExt;
        assert_eq!(
            stream.next().await.unwrap().unwrap().len(),
            crate::domain::MEDIA_READ_BLOCK_BYTES
        );
        assert_eq!(TEST_DOWNLOADS.available_permits(), 0);
        drop(stream);
        assert_eq!(TEST_DOWNLOADS.available_permits(), 1);
    }

    #[test]
    fn rejects_large_jpeg_header_before_decoding_pixels() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode_image(&image::DynamicImage::new_rgb8(1, 1))
            .unwrap();
        let sof = jpeg
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .unwrap();
        // The file still contains just one pixel: success would attempt to allocate
        // from an untrusted header before discovering its invalid pixel payload.
        jpeg[sof + 5..sof + 7].copy_from_slice(&6000u16.to_be_bytes());
        jpeg[sof + 7..sof + 9].copy_from_slice(&8000u16.to_be_bytes());
        assert!(matches!(
            decode_image(&jpeg),
            Err(MediaPreparationError::TooLarge)
        ));
    }

    #[test]
    fn small_valid_image_keeps_its_pixels() {
        let source = image::RgbImage::from_pixel(4, 3, image::Rgb([11, 22, 33]));
        let mut png = Cursor::new(Vec::new());
        source.write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert_eq!(decode_image(png.get_ref()).unwrap().to_rgb8(), source);
    }

    #[tokio::test]
    async fn large_rotated_phone_image_preserves_original_and_preview() {
        let source = image::DynamicImage::new_rgb8(6000, 4000);
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
            .encode_image(&source)
            .unwrap();
        drop(source);
        let orientation_6 = [
            0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 0x2a, 0, 8, 0, 0, 0,
            1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        jpeg.splice(2..2, orientation_6);
        let (original, dimensions, preview) =
            prepare_uploaded_media(jpeg, "image/jpeg").await.unwrap();
        assert_eq!(dimensions, Some((4000, 6000)));
        let original_dimensions = image::ImageReader::new(Cursor::new(original))
            .with_guessed_format()
            .unwrap()
            .into_dimensions()
            .unwrap();
        assert_eq!(original_dimensions, (4000, 6000));
        let preview = preview.unwrap();
        assert_eq!((preview.width, preview.height), (480, 720));
    }

    #[tokio::test]
    async fn processing_budget_survives_request_cancellation() {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let first = tokio::spawn(process_image(move || {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        }));
        entered_rx.await.unwrap();
        first.abort();
        let _ = first.await;
        let still_reserved = IMAGE_PROCESSING.available_permits() == 0;
        release_tx.send(()).unwrap();
        process_image(|| ()).await.unwrap();
        assert!(
            still_reserved,
            "blocking worker must retain its memory permit"
        );
    }

    #[test]
    fn download_name_cannot_escape_the_attachment_header() {
        let id = MediaId::new(uuid::Uuid::now_v7().to_string()).unwrap();
        let header = attachment_disposition("C:\\Bilder\\Måne\"\r\n.png", &id, "image/png");
        let value = header.to_str().unwrap();
        assert!(value.starts_with("attachment; filename=\"Mane_.png\""));
        assert!(value.contains("filename*=UTF-8''M%C3%A5ne%22.png"));
        assert!(!value.contains("\r"));
        assert!(!value.contains("\n"));
        assert!(!value.contains("Bilder"));
    }
}
