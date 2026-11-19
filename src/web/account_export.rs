use axum::{
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};

use crate::{
    account_data::AccountExportError,
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};

pub(crate) async fn export_my_data(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let filename = format!(
        "attachment; filename=\"sproyt-export-{}.json\"",
        principal.user.id
    );
    let export = match state.account_data.start_user_export(principal.user).await {
        Ok(export) => export,
        Err(error) => return export_error_response(error),
    };
    let Ok(disposition) = HeaderValue::from_str(&filename) else {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = axum::body::Body::from_stream(export.into_stream()).into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(axum::http::header::CONTENT_DISPOSITION, disposition);
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn export_error_response(error: AccountExportError) -> axum::response::Response {
    use axum::http::{StatusCode, header};

    let mut response = match error {
        AccountExportError::Busy => (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, "5")],
            "Ein dataeksport køyrer allereie. Prøv igjen om litt.",
        )
            .into_response(),
        AccountExportError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Dataeksport er mellombels utilgjengeleg. Prøv igjen seinare.",
        )
            .into_response(),
        AccountExportError::Timeout => (
            StatusCode::GATEWAY_TIMEOUT,
            "Dataeksporten tok for lang tid. Prøv igjen seinare.",
        )
            .into_response(),
        AccountExportError::Repository(error) => repository_response(error),
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
