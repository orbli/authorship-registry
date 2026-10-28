//! Lambda Function URL entry. Boots once per execution environment and serves
//! the same router. When CloudFront fronts the function (custom domain), only
//! requests carrying the shared origin header are accepted.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};

use authorship_registry_web::origin::{ORIGIN_HEADER, origin_allowed};
use authorship_registry_web::{boot, init_tracing};

async fn origin_guard(req: Request, next: Next) -> Response {
    if let Ok(secret) = std::env::var("ORIGIN_SECRET") {
        let got = req.headers().get(ORIGIN_HEADER).and_then(|v| v.to_str().ok());
        if !origin_allowed(got, &secret) {
            return (StatusCode::FORBIDDEN, "Forbidden: use the public hostname.").into_response();
        }
    }
    next.run(req).await
}

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    init_tracing();
    let b = boot().await?;
    let app = b.router.layer(middleware::from_fn(origin_guard));
    lambda_http::run(app).await
}
