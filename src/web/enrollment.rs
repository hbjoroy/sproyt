use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::CACHE_CONTROL},
    response::IntoResponse,
};
use chrono::{Duration, Utc};
use serde::Deserialize;

use crate::{
    domain::CircleId,
    enrollment::{EnrollmentError, validate_invitee},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};

#[derive(Deserialize)]
pub(crate) struct EnrollmentInvitationRequest {
    email: String,
    display_name: Option<String>,
}

pub(crate) async fn create_enrollment_invitation(
    State(state): State<AppState>,
    Path(circle): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(request): Json<EnrollmentInvitationRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let circle_id = match uuid::Uuid::parse_str(&circle) {
        Ok(circle_id) => CircleId::from_uuid(circle_id),
        Err(_) => return (StatusCode::BAD_REQUEST, "ugyldig vennekrets").into_response(),
    };
    let Some(enrollment) = &state.enrollment else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Invitasjon av nye brukarar er ikkje konfigurert enno.",
        )
            .into_response();
    };
    if let Err(EnrollmentError::Validation(message)) =
        validate_invitee(&request.email, request.display_name.as_deref())
    {
        return (StatusCode::BAD_REQUEST, message).into_response();
    }
    // The repository creates an inactive token first.  It is deliberately not
    // redeemable until Authentik has accepted the corresponding invitation.
    // Preparing it here still enforces that only the circle owner can invite.
    let expires_at = Utc::now() + Duration::hours(crate::enrollment::INVITATION_LIFETIME_HOURS);
    let invitation = match state
        .chat
        .prepare_enrollment_invitation(
            principal.user.id,
            circle_id,
            request.email.clone(),
            expires_at,
        )
        .await
    {
        Ok(invitation) => invitation,
        Err(error) => return chat_error_response(error),
    };
    let result = match enrollment
        .create(
            &invitation.token,
            &request.email,
            request.display_name.as_deref(),
            expires_at,
        )
        .await
    {
        Ok(result) => result,
        Err(EnrollmentError::Validation(message)) => {
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
        Err(EnrollmentError::Rejected) => {
            return (
                StatusCode::BAD_GATEWAY,
                "Authentik avviste invitasjonen. Kontroller oppsettet.",
            )
                .into_response();
        }
        Err(EnrollmentError::Unavailable) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Authentik svarar ikkje akkurat no. Prøv igjen.",
            )
                .into_response();
        }
        Err(EnrollmentError::Configuration(_) | EnrollmentError::InvalidResponse) => {
            return (
                StatusCode::BAD_GATEWAY,
                "Kunne ikkje lage ei trygg registreringslenkje.",
            )
                .into_response();
        }
    };
    if let Err(error) = enrollment
        .send_email(result.authentik_invitation_id, &request.email)
        .await
    {
        if let Err(revoke_error) = enrollment.revoke(result.authentik_invitation_id).await {
            tracing::warn!(
                ?revoke_error,
                authentik_invitation_id = %result.authentik_invitation_id,
                "could not revoke Authentik invitation after email delivery failed"
            );
        }
        return match error {
            EnrollmentError::Validation(message) => {
                (StatusCode::BAD_REQUEST, message).into_response()
            }
            EnrollmentError::Rejected => (
                StatusCode::BAD_GATEWAY,
                "Authentik kunne ikkje sende invitasjonsmeldinga. Kontroller oppsettet.",
            )
                .into_response(),
            EnrollmentError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Authentik svarar ikkje akkurat no. Prøv igjen.",
            )
                .into_response(),
            EnrollmentError::Configuration(_) | EnrollmentError::InvalidResponse => (
                StatusCode::BAD_GATEWAY,
                "Kunne ikkje sende invitasjonsmeldinga trygt.",
            )
                .into_response(),
        };
    }
    // If the local activation fails, do not leave a usable Authentik link
    // pointing at a token that Sprøyt will never accept.  Failure to revoke is
    // logged but cannot change the safe failure of the local operation.
    if let Err(error) = state
        .chat
        .activate_enrollment_invitation(invitation.token, result.authentik_invitation_id)
        .await
    {
        if let Err(revoke_error) = enrollment.revoke(result.authentik_invitation_id).await {
            tracing::warn!(
                ?revoke_error,
                authentik_invitation_id = %result.authentik_invitation_id,
                "could not revoke Authentik invitation after local activation failed"
            );
        }
        return chat_error_response(error);
    }

    // Keep the Authentik resource id internal; the browser needs only the
    // single-use flow URL and its expiry.
    let mut response = Json(result.invitation).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
