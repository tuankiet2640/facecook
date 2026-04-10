use axum::{
    body::Body,
    extract::Multipart,
    http::StatusCode,
    response::Response,
};
use std::path::PathBuf;
use uuid::Uuid;

/// Allowed MIME types for uploaded media.
const ALLOWED_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "video/mp4",
    "video/webm",
    "video/quicktime",
];

/// Max upload size: 100 MB
const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

pub fn uploads_dir() -> PathBuf {
    // Resolve relative to the current working directory (project root in dev).
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("uploads")
}

/// POST /api/v1/upload
/// Accepts multipart/form-data with a single `file` field.
/// Returns { "url": "/uploads/<uuid>.<ext>" }
pub async fn upload_media(mut multipart: Multipart) -> Response {
    let dir = uploads_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("Storage error: {e}"));
    }

    while let Ok(Some(field)) = multipart.next_field().await {
        // Only process the field named "file"
        if field.name() != Some("file") {
            continue;
        }

        let content_type = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_default();

        if !ALLOWED_MIME_TYPES.contains(&content_type.as_str()) {
            return error_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("Unsupported media type: {content_type}. Allowed: image/jpeg, image/png, image/gif, image/webp, video/mp4, video/webm, video/quicktime"),
            );
        }

        let ext = mime_to_ext(&content_type);
        let filename = format!("{}.{}", Uuid::new_v4(), ext);
        let path = dir.join(&filename);

        let data = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, format!("Failed to read upload: {e}")),
        };

        if data.len() > MAX_UPLOAD_BYTES {
            return error_response(StatusCode::PAYLOAD_TOO_LARGE, "File exceeds 100 MB limit".to_string());
        }

        if let Err(e) = std::fs::write(&path, &data) {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to save file: {e}"));
        }

        let url = format!("/uploads/{filename}");
        let body = serde_json::json!({ "url": url }).to_string();
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
    }

    error_response(StatusCode::BAD_REQUEST, "No file field found in request".to_string())
}

fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/jpeg"      => "jpg",
        "image/png"       => "png",
        "image/gif"       => "gif",
        "image/webp"      => "webp",
        "video/mp4"       => "mp4",
        "video/webm"      => "webm",
        "video/quicktime" => "mov",
        _                 => "bin",
    }
}

fn error_response(status: StatusCode, message: String) -> Response {
    let body = serde_json::json!({ "error": { "code": "UPLOAD_ERROR", "message": message } }).to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap()
}
