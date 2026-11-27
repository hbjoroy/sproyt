use crate::imagegen::{ImageGeneration, Job, PromptMode, avatar::REFERENCE_PREFIX};
use crate::web::{
    media::{admit_media, decode_image, process_image},
    personal_space::authenticated_user,
};
use crate::{
    domain::{PrincipalKind, UserId},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};
use axum::{Json, body::Bytes};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

fn error(status: StatusCode, message: &'static str) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], message).into_response()
}
fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Avatartenesta er utilgjengeleg. Prøv igjen om litt.",
    )
}
fn service(state: &AppState) -> Result<&ImageGeneration, Response> {
    state.imagegen.as_ref().ok_or_else(unavailable)
}
fn draft(job: &Job) -> serde_json::Value {
    json!({"id":job.id,"state":job.state})
}
fn private(value: serde_json::Value) -> Response {
    ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}

pub(crate) async fn current(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let user = authenticated_user(&state, query, &headers).await?;
    let job = service(&state)?
        .avatar_job(user.id)
        .await
        .map_err(|_| unavailable())?;
    Ok(private(job.as_ref().map(draft).unwrap_or_default()))
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Style {
    Pencil,
    Comic,
    Oil,
}

impl Style {
    fn material(&self) -> &'static str {
        match self {
            Self::Pencil => "coloured pencil drawing on fine paper",
            Self::Comic => "comic-book illustration with ink outlines and flat colours",
            Self::Oil => "oil painting with visible brushstrokes",
        }
    }

    fn prompt(&self) -> String {
        let article = if matches!(self, Self::Oil) { "an" } else { "a" };
        format!(
            "A studio headshot of this person as {article} {}. Compose a near-frontal view, with the face at a slight 15-degree angle to camera-right and the eyes meeting the lens. The nose is a little to the right of the midpoint between the pupils; we see almost equal amounts of both cheeks. On the left side of the picture, a little more ear and cheek are visible. Relaxed expression, level head, shoulders facing the camera. Frame the whole head within the central square. Keep the recognizable face, age and hairstyle. Plain background.",
            self.material()
        )
    }
}

pub(crate) async fn generate(
    State(state): State<AppState>,
    Path((style, request)): Path<(Style, Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    photo: Bytes,
) -> Result<Response, Response> {
    let user = authenticated_user(&state, query, &headers).await?;
    let svc = service(&state)?;
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _slot = SLOTS.try_acquire().map_err(|_| {
        error(
            StatusCode::TOO_MANY_REQUESTS,
            "Avatarbehandling er opptatt. Prøv igjen om litt.",
        )
    })?;
    let jpeg = {
        let _permit = admit_media().map_err(|r| *r)?;
        process_image(move || -> Result<Vec<u8>, &'static str> {
            let photo = crate::web::media::decode_oriented_photo(&photo).map_err(|_| "Vel eit gyldig JPEG-, PNG- eller WebP-bilete på høgst 8 MiB.")?;
            if !super::avatar_head::contains_head(&photo) {
                return Err("Vi fann ikkje eit menneskehovud i biletet. Vel eit tydeleg portrett teke framanfrå.");
            }
            let image = photo.thumbnail(768, 768).to_rgb8();
            let mut dst = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut dst, 88)
                .encode_image(&image).map_err(|_| "Kunne ikkje klargjere biletet.")?;
            Ok(dst)
        }).await.map_err(|_| unavailable())?.map_err(|message| error(StatusCode::UNPROCESSABLE_ENTITY, message))?
    };
    let marker = format!(
        "{REFERENCE_PREFIX}{}",
        STANDARD.encode(Sha256::digest(&jpeg))
    );
    let encoded = STANDARD.encode(jpeg);
    let prompt = style.prompt();
    let owner = user.id.clone();
    state
        .chat
        .ensure_user(user)
        .await
        .map_err(chat_error_response)?;
    let channel = state
        .chat
        .open_personal_space(owner.clone())
        .await
        .map_err(chat_error_response)?;
    let job = svc
        .enqueue_with_mode(
            owner,
            channel.id,
            request,
            prompt,
            vec![(marker, encoded)],
            PromptMode::Literal,
        )
        .await
        .map_err(|_| {
            error(
                StatusCode::CONFLICT,
                "Sjå gjennom den aktive biletejobben din, eller prøv igjen når køen har plass.",
            )
        })?;
    Ok(private(draft(&job)))
}

async fn owned(svc: &ImageGeneration, owner: UserId, id: Uuid) -> Result<Job, Response> {
    svc.get(&id.to_string())
        .await
        .map_err(|_| unavailable())?
        .filter(|job| job.owner_id == owner && job.is_avatar())
        .ok_or_else(|| StatusCode::NOT_FOUND.into_response())
}

pub(crate) async fn retry(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let user = authenticated_user(&state, query, &headers).await?;
    let svc = service(&state)?;
    let prior = owned(svc, user.id, id).await?;
    // The material is present in prompts written by both avatar versions.
    let style = [Style::Pencil, Style::Comic, Style::Oil]
        .into_iter()
        .find(|s| prior.prompt.contains(s.material()))
        .ok_or_else(|| StatusCode::CONFLICT.into_response())?;
    let next = svc.retry_avatar(prior, style.prompt()).await.map_err(|cause| {
        tracing::warn!(%cause, "could not retry avatar generation");
        error(StatusCode::SERVICE_UNAVAILABLE, "Kunne ikkje lage ein ny variant. Prøv igjen. Viss originalfotoet er borte, kan du forkaste utkastet og velje fotoet på nytt.")
    })?.ok_or_else(|| error(StatusCode::CONFLICT, "Utkastet har endra seg, eller biletkøen er full. Hent status og prøv igjen."))?;
    Ok(private(draft(&next)))
}

async fn thumbnail(job: &Job) -> Result<Vec<u8>, Response> {
    let _permit = admit_media().map_err(|r| *r)?;
    let png = job
        .image
        .as_ref()
        .and_then(|data| STANDARD.decode(data).ok())
        .ok_or_else(unavailable)?;
    process_image(move || {
        let image = decode_image(&png)
            .ok()?
            .resize_to_fill(256, 256, image::imageops::FilterType::Lanczos3)
            .to_rgb8();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
            .encode_image(&image)
            .ok()?;
        Some(out)
    })
    .await
    .map_err(|_| unavailable())?
    .ok_or_else(unavailable)
}

pub(crate) async fn preview(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let user = authenticate_http(&state, query, &headers)
        .await
        .map_err(auth_error_response)?
        .user;
    if user.kind != PrincipalKind::Human {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let job = owned(service(&state)?, user.id, id).await?;
    if job.state != "ready" {
        return Err(StatusCode::NOT_FOUND.into_response());
    }
    Ok((
        [
            (header::CACHE_CONTROL, "private, no-store"),
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        thumbnail(&job).await?,
    )
        .into_response())
}

#[derive(Deserialize)]
pub(crate) struct Review {
    approve: bool,
}

pub(crate) async fn review(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Review>,
) -> Result<Response, Response> {
    let user = authenticated_user(&state, query, &headers).await?;
    let svc = service(&state)?;
    let mut job = owned(svc, user.id, id).await?;
    let changed = if body.approve && job.state == "ready" {
        let jpeg = thumbnail(&job).await?;
        svc.approve_avatar(job, jpeg).await
    } else if !body.approve && matches!(job.state.as_str(), "ready" | "failed") {
        job.transition("declined");
        job.image = None;
        job.reference_images.clear();
        svc.save(&mut job).await
    } else {
        return Err(StatusCode::CONFLICT.into_response());
    };
    if !changed.map_err(|cause| {
        tracing::warn!(%cause, "could not save profile avatar decision");
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Kunne ikkje lagre valet ditt. Prøv igjen om litt.",
        )
    })? {
        return Err(StatusCode::CONFLICT.into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn published(
    State(state): State<AppState>,
    Path(person): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let viewer = authenticate_http(&state, query, &headers)
        .await
        .map_err(auth_error_response)?;
    if viewer.user.kind != PrincipalKind::Human {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let jpeg = state
        .chat
        .profile_avatar(UserId::from_uuid(person))
        .await
        .map_err(chat_error_response)?;
    let cache = [
        (header::CACHE_CONTROL, "private, max-age=60"),
        (header::VARY, "Cookie, Authorization"),
    ];
    Ok(match jpeg {
        Some(jpeg) => (
            cache,
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            jpeg,
        )
            .into_response(),
        None => (StatusCode::NO_CONTENT, cache).into_response(),
    })
}
