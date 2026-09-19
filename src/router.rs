//! HTTP router: public API, admin API, embedded dashboard.

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::admin;
use crate::api;
use crate::app::AppState;
use crate::assets;

pub fn build(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::mirror_request())
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::HeaderName::from_static("x-api-key"),
            axum::http::HeaderName::from_static("anthropic-version"),
            axum::http::HeaderName::from_static("x-kinetix-admin-token"),
        ]);

    let public = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/v1/chat/completions", post(api::chat_completions))
        .route("/v1/messages", post(api::messages))
        .route("/v1/models", get(api::list_models));

    let admin_api = Router::new()
        .route("/login", post(admin::login))
        .route("/logout", post(admin::logout))
        .route("/me", get(admin::me))
        .route("/overview", get(admin::overview))
        .route("/test-stream", post(admin::test_stream))
        // keys
        .route("/keys", get(admin::list_keys).post(admin::create_key))
        .route(
            "/keys/{id}",
            put(admin::update_key).delete(admin::delete_key),
        )
        // providers
        .route(
            "/providers",
            get(admin::list_providers).post(admin::create_provider),
        )
        .route(
            "/providers/{id}",
            put(admin::update_provider).delete(admin::delete_provider),
        )
        .route("/providers/{id}/discover", post(admin::discover_models))
        .route("/providers/{id}/test", post(admin::test_provider))
        // models
        .route("/models", get(admin::list_models))
        .route("/providers/{id}/models", post(admin::create_model))
        .route(
            "/models/{id}",
            put(admin::update_model).delete(admin::delete_model),
        )
        // accounts
        .route(
            "/accounts",
            get(admin::list_accounts).post(admin::create_account),
        )
        .route(
            "/accounts/{id}",
            put(admin::update_account).delete(admin::delete_account),
        )
        .route("/accounts/{id}/reset", post(admin::reset_account))
        // routes
        .route("/routes", get(admin::list_routes).post(admin::create_route))
        .route(
            "/routes/{id}",
            put(admin::update_route).delete(admin::delete_route),
        )
        .route("/routes/dry-run", post(admin::dry_run_route))
        .route("/validate", post(admin::validate_endpoint))
        .route("/config/export", get(admin::export_config))
        .route("/config/import", post(admin::import_config))
        // aliases
        .route(
            "/aliases",
            get(admin::list_aliases).post(admin::create_alias),
        )
        .route("/aliases/{id}", delete(admin::delete_alias))
        // usage / requests / audit / metrics
        .route("/usage", get(admin::usage))
        .route("/requests", get(admin::usage))
        .route("/requests/live", get(admin::live_requests))
        .route(
            "/requests/{id}/route-trace",
            get(admin::request_route_trace),
        )
        .route(
            "/requests/{id}/diagnostics",
            get(admin::request_diagnostics),
        )
        .route("/audit", get(admin::audit))
        .route("/metrics", get(admin::metrics));

    let dashboard = Router::new()
        .route("/", get(assets::serve))
        .route("/admin", get(assets::serve))
        .route("/admin/", get(assets::serve))
        .route("/admin/{*path}", get(assets::serve));

    Router::new()
        .merge(public)
        .nest("/admin/api", admin_api)
        .merge(dashboard)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(state)
}
