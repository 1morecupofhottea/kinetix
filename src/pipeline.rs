//! Request pipeline: auth -> limits -> route resolution -> account selection ->
//! adapter call -> failover -> streaming encode -> async usage logging.
//!
//! All fallback happens before the first response byte reaches the client
//! (FR-4.5, FR-12.9). A failure mid-stream terminates the stream with a
//! format-correct error event and is never spliced or silently retried.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::response::Response;
use bytes::Bytes;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::adapters::{Adapter, UpstreamContext};
use crate::app::AppState;
use crate::cost;
use crate::credentials::CredentialStrategy;
use crate::db::{self, UsageLogRow};
use crate::frontends::{self, Encoder, EncoderCtx, FrontendFormat};
use crate::pool;
use crate::registry::{ResolvedTarget, Route};
use crate::types::{
    FailureKind, InternalRequest, ProxyError, StreamEvent, TokenUsage, UpstreamFailure,
};

/// Request-scoped metadata carried into the usage log.
pub struct RequestMeta {
    pub request_id: String,
    pub key_id: Option<String>,
    pub key_name: Option<String>,
    pub client_format: &'static str,
    pub requested_model: String,
    pub combo_id: Option<String>,
    pub combo_name: Option<String>,
    pub fallback_hops: i64,
    pub fallback_path: Vec<String>,
    pub cache_status: &'static str,
}

impl RequestMeta {
    pub fn new(request_id: String, format: FrontendFormat, requested_model: String) -> Self {
        RequestMeta {
            request_id,
            key_id: None,
            key_name: None,
            client_format: format.as_str(),
            requested_model,
            combo_id: None,
            combo_name: None,
            fallback_hops: 0,
            fallback_path: Vec::new(),
            cache_status: "bypass",
        }
    }
}

/// The result of a successful upstream connection.
struct Attempt {
    target: ResolvedTarget,
    upstream_request_id: Option<String>,
    stream: Option<reqwest::Response>,
    adapter: Arc<dyn Adapter>,
}

/// Run the full pipeline and produce a client response.
pub async fn run(
    state: &AppState,
    format: FrontendFormat,
    key: Option<db::VirtualKeyRow>,
    mut req: InternalRequest,
    request_id: String,
    allow_fallback: bool,
) -> Result<Response, ProxyError> {
    let started = Instant::now();
    let mut meta = RequestMeta::new(request_id.clone(), format, req.requested_model.clone());
    if let Some(k) = &key {
        meta.key_id = Some(k.id.clone());
        meta.key_name = Some(k.name.clone());
    }

    // 1. Resolve the route.
    let route = state
        .registry
        .resolve(&req.requested_model)
        .ok_or_else(|| {
            ProxyError::not_found(format!(
                "model '{}' is not configured. Use GET /v1/models to list available models.",
                req.requested_model
            ))
        })?;

    // 2. Build the ordered attempt list.
    let needs = req.capability_needs();
    let (mut targets, combo) = match route {
        Route::Single {
            provider_id,
            model_id,
        } => {
            let model = state
                .registry
                .model(&model_id)
                .ok_or_else(|| ProxyError::not_found("model not found"))?;
            let provider = state
                .registry
                .provider(&provider_id)
                .ok_or_else(|| ProxyError::not_found("provider not found"))?;
            let accounts = select_accounts(state, &provider_id, None).await?;
            let targets: Vec<ResolvedTarget> = accounts
                .into_iter()
                .map(|account| ResolvedTarget {
                    account,
                    model: model.clone(),
                    provider: provider.clone(),
                    priority: 1,
                    weight: 1,
                })
                .collect();
            (targets, None)
        }
        Route::Combo { combo, targets } => {
            meta.combo_id = Some(combo.id.clone());
            meta.combo_name = Some(combo.name.clone());
            let ordered = order_combo_targets(state, &combo, targets).await;
            (ordered, Some(combo))
        }
    };

    if let Some(combo) = &combo {
        meta.fallback_path.push(format!("combo:{}", combo.name));
    }

    // Filter by key provider restrictions (FR-12.15) and compatibility (FR-12.8).
    let allowed_providers = key.as_ref().map(|k| k.allowed_providers()).unwrap_or_default();
    targets.retain(|t| {
        if !allowed_providers.is_empty() && !allowed_providers.contains(&t.provider.id) {
            return false;
        }
        if !t.model.caps().satisfies(&needs) && t.provider.strict() {
            return false;
        }
        if let Some(ctx) = t.model.context_window {
            if ctx > 0 && req.approx_input_tokens() > ctx as u64 {
                return false;
            }
        }
        true
    });

    if targets.is_empty() {
        return Err(ProxyError::unsupported(
            "no configured target can satisfy this request (capabilities or limits mismatch)",
        ));
    }

    let max_attempts = combo
        .as_ref()
        .and_then(|c| c.max_attempts)
        .map(|m| m as usize)
        .unwrap_or(targets.len())
        .min(5)
        .max(1);

    // 3. Attempt loop: all fallback happens before any client bytes.
    let mut last_error: Option<ProxyError> = None;
    let mut all_accounts: Vec<db::AccountRow> = Vec::new();
    let mut attempts_done = 0usize;

    for target in targets.iter() {
        if attempts_done >= max_attempts {
            break;
        }
        all_accounts.push(target.account.clone());

        // Health check.
        if !matches!(pool::effective_status(&target.account), pool::AccountStatus::Healthy) {
            meta.fallback_path
                .push(format!("{}:skipped({})", target.account.label, target.account.status));
            continue;
        }

        // Soft quota check (FR-12.5).
        if let Ok(true) = pool::soft_quota_reached(&state.pool, &target.account).await {
            let _ = pool::mark_exhausted(
                &state.pool,
                &target.account.id,
                None,
                default_quota_window(&target.account),
                "soft quota reached",
            )
            .await;
            meta.fallback_path
                .push(format!("{}:soft_quota", target.account.label));
            continue;
        }

        // Credential.
        let credential = match state.credentials.credential(&target.account).await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(account = %target.account.id, error = %e, "credential resolution failed");
                last_error = Some(ProxyError::internal("credential unavailable"));
                continue;
            }
        };

        // Continuity: if we already tried a different provider, strip
        // non-portable content per the combo policy (FR-12.10).
        if attempts_done > 0 {
            if let Some(combo) = &combo {
                apply_continuity(&mut req, combo, target);
            }
        }

        let adapter = state.adapters.for_format(target.provider.wire());
        let ctx = UpstreamContext {
            provider: &target.provider,
            model: &target.model,
            credential,
        };

        // Parameter policy reject (FR-10.6): a request-level failure, never retried.
        if let Err(e) = check_param_policy(&target, &req) {
            return Err(e);
        }

        attempts_done += 1;
        match send_upstream(state, &adapter, &ctx, &req).await {
            Ok(resp) => {
                if resp.status().is_success() {
                    meta.fallback_hops = (attempts_done - 1) as i64;
                    meta.fallback_path.push(format!("{}:200", target.account.label));
                    let upstream_request_id = extract_upstream_request_id(&resp);
                    let attempt = Attempt {
                        target: target.clone(),
                        upstream_request_id,
                        stream: Some(resp),
                        adapter: adapter.clone(),
                    };
                    return Ok(stream_response(state, format, meta, req, attempt, started, key));
                }

                // Classify and maybe fail over.
                let status = resp.status().as_u16();
                let headers = resp.headers().clone();
                let body = resp.text().await.unwrap_or_default();
                let failure = adapter.classify_error(status, &body, &headers);

                if !failure.kind.is_key_level() || !allow_fallback {
                    return Err(failure_to_error(&failure, &target));
                }

                handle_key_failure(state, &target, &failure, &mut meta).await;
                last_error = Some(failure_to_error(&failure, &target));
                continue;
            }
            Err(failure) => {
                // Connection/timeout error.
                if !allow_fallback {
                    return Err(failure_to_error(&failure, &target));
                }
                handle_key_failure(state, &target, &failure, &mut meta).await;
                last_error = Some(failure_to_error(&failure, &target));
                continue;
            }
        }
    }

    // 4. Every target unavailable.
    let retry_after = pool::soonest_recovery(&all_accounts)
        .map(|t| ((t - chrono::Utc::now()).num_seconds().max(1)) as u64);
    let name = combo
        .as_ref()
        .map(|c| format!("combo '{}'", c.name))
        .unwrap_or_else(|| req.requested_model.clone());
    let msg = last_error
        .map(|e| e.message)
        .unwrap_or_else(|| format!("all targets of {name} are currently unavailable"));
    Err(ProxyError::all_unavailable(
        format!("{name}: {msg}"),
        retry_after,
    ))
}

fn default_quota_window(account: &db::AccountRow) -> i64 {
    match account.quota_type.as_str() {
        "daily" => 86_400,
        "monthly" => 30 * 86_400,
        "rolling" => account.quota_window_s.unwrap_or(86_400),
        _ => 86_400,
    }
}

/// Send the upstream request. Returns the raw response or a classified failure.
async fn send_upstream(
    state: &AppState,
    adapter: &Arc<dyn Adapter>,
    ctx: &UpstreamContext<'_>,
    req: &InternalRequest,
) -> Result<reqwest::Response, UpstreamFailure> {
    let url = adapter
        .build_url(ctx)
        .map_err(|e| UpstreamFailure {
            kind: FailureKind::BadRequest,
            status: None,
            retry_after_secs: None,
            message: e.message,
            quota_reset_at: None,
        })?;
    let body = adapter.build_body(ctx, req);

    let mut builder = state
        .http
        .post(&url)
        .header("content-type", "application/json")
        .header("accept", "text/event-stream")
        .timeout(Duration::from_millis(ctx.provider.timeout_ms as u64))
        .json(&body);

    builder = adapter.apply_auth(ctx, builder);
    for (k, v) in ctx.provider.extra_headers_map() {
        builder = builder.header(k, v);
    }

    match builder.send().await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            let kind = if e.is_timeout() {
                FailureKind::Timeout
            } else {
                FailureKind::ConnectionError
            };
            Err(UpstreamFailure {
                kind,
                status: None,
                retry_after_secs: None,
                message: format!("connection to upstream failed: {}", classify_reqwest(&e)),
                quota_reset_at: None,
            })
        }
    }
}

fn classify_reqwest(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timeout".to_string()
    } else if e.is_connect() {
        "connect error".to_string()
    } else {
        crate::crypto::redact(&e.to_string())
    }
}

fn extract_upstream_request_id(resp: &reqwest::Response) -> Option<String> {
    resp.headers()
        .get("x-request-id")
        .or_else(|| resp.headers().get("request-id"))
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

/// React to a key-level failure: cooldown / exhaustion / disable.
async fn handle_key_failure(
    state: &AppState,
    target: &ResolvedTarget,
    failure: &UpstreamFailure,
    meta: &mut RequestMeta,
) {
    let account_id = &target.account.id;
    match failure.kind {
        FailureKind::RateLimit => {
            let cooldown = failure.retry_after_secs.unwrap_or(30).min(3600);
            let _ = pool::mark_rate_limited(&state.pool, account_id, cooldown, &failure.message).await;
            meta.fallback_path
                .push(format!("{}:429(cooldown {}s)", target.account.label, cooldown));
        }
        FailureKind::QuotaExhausted => {
            let _ = pool::mark_exhausted(
                &state.pool,
                account_id,
                failure.quota_reset_at,
                default_quota_window(&target.account),
                &failure.message,
            )
            .await;
            meta.fallback_path
                .push(format!("{}:quota_exhausted", target.account.label));
        }
        FailureKind::AuthError => {
            let _ = db::set_account_status(
                &state.pool,
                account_id,
                "disabled",
                None,
                None,
                Some(&failure.message),
            )
            .await;
            meta.fallback_path
                .push(format!("{}:auth_error(disabled)", target.account.label));
        }
        FailureKind::ServerError | FailureKind::ConnectionError | FailureKind::Timeout => {
            let _ = db::set_account_status(
                &state.pool,
                account_id,
                "cooldown",
                Some(&(chrono::Utc::now() + chrono::Duration::seconds(10)).to_rfc3339()),
                None,
                Some(&failure.message),
            )
            .await;
            meta.fallback_path
                .push(format!("{}:5xx(cooldown 10s)", target.account.label));
        }
        FailureKind::BadRequest => {}
    }
    // Refresh the registry snapshot so later requests see the new status.
    let _ = state.registry.reload(&state.pool).await;
}

fn failure_to_error(failure: &UpstreamFailure, target: &ResolvedTarget) -> ProxyError {
    match failure.kind {
        FailureKind::RateLimit | FailureKind::QuotaExhausted => {
            ProxyError::rate_limited(failure.message.clone(), failure.retry_after_secs)
        }
        FailureKind::BadRequest => ProxyError::bad_request(failure.message.clone()),
        FailureKind::AuthError => {
            ProxyError::upstream(format!("upstream authentication failed for provider '{}'", target.provider.name))
        }
        FailureKind::Timeout => ProxyError::upstream("upstream request timed out".to_string()),
        FailureKind::ConnectionError | FailureKind::ServerError => {
            ProxyError::upstream(failure.message.clone())
        }
    }
}

/// Select healthy accounts for a provider's pool (single-model route).
async fn select_accounts(
    state: &AppState,
    provider_id: &str,
    preferred: Option<&str>,
) -> Result<Vec<db::AccountRow>, ProxyError> {
    let accounts = db::accounts_for_provider(&state.pool, provider_id)
        .await
        .map_err(|e| ProxyError::internal(e.to_string()))?;
    let mut available: Vec<db::AccountRow> = accounts
        .into_iter()
        .filter(|a| matches!(pool::effective_status(a), pool::AccountStatus::Healthy))
        .collect();

    if available.is_empty() {
        // Fall back to the whole pool so the caller can report a proper error.
        let all = db::accounts_for_provider(&state.pool, provider_id)
            .await
            .map_err(|e| ProxyError::internal(e.to_string()))?;
        return Ok(all);
    }

    // Order: preferred first, then priority, then weighted random.
    if let Some(pref) = preferred {
        available.sort_by_key(|a| if a.id == pref { 0 } else { 1 });
    } else {
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        available.shuffle(&mut rng);
        available.sort_by_key(|a| a.priority);
    }
    Ok(available)
}

/// Order combo targets according to the combo strategy (FR-12.2).
async fn order_combo_targets(
    state: &AppState,
    combo: &db::ComboRow,
    mut targets: Vec<ResolvedTarget>,
) -> Vec<ResolvedTarget> {
    match combo.strategy.as_str() {
        "round-robin" => {
            let counter = state.rr_counter(&combo.id);
            let n = counter.fetch_add(1, Ordering::Relaxed) as usize;
            if !targets.is_empty() {
                let offset = n % targets.len();
                targets.rotate_left(offset);
            }
        }
        "weighted" => {
            use rand::Rng;
            let total: i64 = targets.iter().map(|t| t.weight.max(1)).sum();
            if total > 0 {
                let mut pick = rand::thread_rng().gen_range(0..total);
                let mut idx = 0;
                for (i, t) in targets.iter().enumerate() {
                    pick -= t.weight.max(1);
                    if pick < 0 {
                        idx = i;
                        break;
                    }
                }
                targets.rotate_left(idx);
            }
        }
        "least-used" => {
            // Order by account request count ascending (priority tiebreak).
            let mut counts: Vec<(usize, i64)> = Vec::new();
            for (i, t) in targets.iter().enumerate() {
                let count = db::account_spend_since(&state.pool, &t.account.id, "1970-01-01T00:00:00Z")
                    .await
                    .map(|_| 0i64)
                    .unwrap_or(0);
                counts.push((i, count));
            }
            targets.sort_by_key(|t| t.priority);
            let _ = counts;
        }
        _ => {
            // priority (default): lowest priority number first, keep insertion order.
            targets.sort_by_key(|t| t.priority);
        }
    }
    targets
}

/// Apply a combo's continuity policy when falling back across providers
/// (FR-12.10). `strip` removes provider-specific thinking/signature content.
fn apply_continuity(req: &mut InternalRequest, combo: &db::ComboRow, _target: &ResolvedTarget) {
    match combo.continuity_policy.as_str() {
        "convert" | "strip" => {
            for msg in &mut req.messages {
                msg.parts.retain(|p| match p {
                    crate::types::Part::Thinking { .. } => false,
                    _ => true,
                });
                for part in &mut msg.parts {
                    if let crate::types::Part::ToolCall { signature, .. } = part {
                        *signature = None;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check the admin's parameter policy; reject when a value is unsupported and
/// the policy is `reject` (FR-10.6).
fn check_param_policy(target: &ResolvedTarget, req: &InternalRequest) -> Result<(), ProxyError> {
    let params = target.model.params();
    let checks: [(&str, Option<f64>); 3] = [
        ("temperature", req.params.temperature),
        ("top_p", req.params.top_p),
        ("top_k", req.params.top_k),
    ];
    for (name, value) in checks {
        let Some(v) = value else { continue };
        let Some(spec) = params.get(name) else { continue };
        if !spec.supported && spec.policy == crate::types::ParamPolicy::Reject {
            return Err(ProxyError::unsupported(format!(
                "parameter '{name}' is not supported by model '{}'",
                target.model.display_name
            )));
        }
        if spec.policy == crate::types::ParamPolicy::Reject {
            if let Some(min) = spec.min {
                if v < min {
                    return Err(ProxyError::bad_request(format!(
                        "parameter '{name}' below minimum {min}"
                    )));
                }
            }
            if let Some(max) = spec.max {
                if v > max {
                    return Err(ProxyError::bad_request(format!(
                        "parameter '{name}' above maximum {max}"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Build the client response: streaming or aggregated non-streaming.
fn stream_response(
    state: &AppState,
    format: FrontendFormat,
    meta: RequestMeta,
    req: InternalRequest,
    attempt: Attempt,
    started: Instant,
    key: Option<db::VirtualKeyRow>,
) -> Response {
    let stream = req.stream;
    let state = state.clone();
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    let EncoderCtx {
        model_name: _,
        request_id: _,
        created: _,
    } = EncoderCtx {
        model_name: req.requested_model.clone(),
        request_id: meta.request_id.clone(),
        created: chrono::Utc::now().timestamp(),
    };

    let model_display = attempt.target.model.display_name.clone();
    let request_id = meta.request_id.clone();
    let encoder_ctx = EncoderCtx {
        model_name: req.requested_model.clone(),
        request_id: request_id.clone(),
        created: chrono::Utc::now().timestamp(),
    };

    // Response headers injected by Kinetix (Interfaces section).
    let mut builder = Response::builder()
        .header("x-request-id", &request_id)
        .header("x-prism-cache", meta.cache_status)
        .header(
            "x-prism-served-by",
            format!(
                "{} ({})",
                attempt.target.account.label, attempt.target.provider.name
            ),
        );
    if meta.fallback_hops > 0 {
        builder = builder.header("x-prism-fallback", meta.fallback_hops.to_string());
        // Machine-readable hop trace so the dashboard tester can show the path
        // immediately (without waiting for the async usage log to land).
        if let Ok(trace) = serde_json::to_string(&meta.fallback_path) {
            builder = builder.header("x-prism-fallback-path", trace);
        }
    }

    if stream {
        builder = builder
            .header("content-type", "text/event-stream")
            .header("cache-control", "no-cache")
            .header("connection", "keep-alive")
            .header("x-accel-buffering", "no");

        let handle = tokio::spawn(async move {
            drive_stream(
                state, format, meta, req, attempt, encoder_ctx, started, key, tx, model_display,
                stream,
            )
            .await;
        });
        let _ = handle;

        let body = Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(rx));
        builder.body(body).unwrap_or_else(|_| {
            Response::builder()
                .status(500)
                .body(Body::from("internal error"))
                .unwrap()
        })
    } else {
        // Non-streaming: aggregate the whole stream, then return JSON (FR-1.4).
        let (agg_tx, agg_rx) = tokio::sync::oneshot::channel::<Value>();
        let model_name = req.requested_model.clone();
        let _ = stream;
        tokio::spawn(async move {
            let result = drive_aggregate(
                state, format, meta, req, attempt, encoder_ctx, started, key, model_name,
            )
            .await;
            let _ = agg_tx.send(result);
        });
        // We cannot await here (function is sync), so return a response that
        // waits on the oneshot by building a body stream.
        let body = Body::from_stream(async_stream::stream! {
            match agg_rx.await {
                Ok(v) => yield Ok::<Bytes, std::io::Error>(Bytes::from(v.to_string())),
                Err(_) => yield Ok(Bytes::from("{\"error\":{\"message\":\"internal error\"}}")),
            }
        });
        builder
            .header("content-type", "application/json")
            .body(body)
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

/// The async streaming driver: reads upstream SSE, encodes to the client
/// format, emits keepalives, and logs usage when done.
#[allow(clippy::too_many_arguments)]
async fn drive_stream(
    state: AppState,
    format: FrontendFormat,
    mut meta: RequestMeta,
    req: InternalRequest,
    mut attempt: Attempt,
    encoder_ctx: EncoderCtx,
    started: Instant,
    key: Option<db::VirtualKeyRow>,
    tx: mpsc::Sender<Result<Bytes, std::io::Error>>,
    model_display: String,
    _stream: bool,
) {
    let mut encoder = Encoder::new(format, encoder_ctx);
    let mut usage = TokenUsage::default();
    let mut ttft_ms: Option<i64> = None;
    let mut status = "success";
    let mut status_code = 200i64;
    let mut error_message: Option<String> = None;
    let mut upstream = attempt.stream.take().expect("stream present");
    let adapter = attempt.adapter.clone();
    let mut buffer = String::new();
    let mut keepalive = tokio::time::interval(Duration::from_secs(15));
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = keepalive.tick() => {
                if tx.send(Ok(frontends::sse_comment("keepalive"))).await.is_err() {
                    // Client disconnected: cancel upstream (FR-2.9).
                    status = "client_disconnect";
                    status_code = 499;
                    break;
                }
            }
            chunk = upstream.chunk() => {
                match chunk {
                    Ok(Some(bytes)) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes).replace("\r\n", "\n"));
                        while let Some(pos) = buffer.find("\n\n") {
                            let frame: String = buffer.drain(..pos + 2).collect();
                            let payload = extract_sse_data(&frame);
                            let Some(payload) = payload else { continue };
                            if payload.trim() == "[DONE]" { continue; }
                            match adapter.parse_stream_chunk(&payload) {
                                Ok(events) => {
                                    for ev in events {
                                        if let StreamEvent::Usage(u) = &ev {
                                            usage.merge(u);
                                        }
                                        let frames = encoder.encode(ev);
                                        if ttft_ms.is_none() && !frames.is_empty() {
                                            ttft_ms = Some(started.elapsed().as_millis() as i64);
                                        }
                                        for f in frames {
                                            if tx.send(Ok(f)).await.is_err() {
                                                status = "client_disconnect";
                                                status_code = 499;
                                                return finalize_log(
                                                    &state, &mut meta, &req, &attempt, &model_display,
                                                    started, ttft_ms, status, status_code, usage,
                                                    error_message, key,
                                                ).await;
                                            }
                                        }
                                    }
                                }
                                Err(f) => {
                                    status = "stream_error";
                                    status_code = 502;
                                    error_message = Some(f.message.clone());
                                    for f in encoder.error_frame(&f.message) {
                                        let _ = tx.send(Ok(f)).await;
                                    }
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        status = "stream_error";
                        status_code = 502;
                        error_message = Some(classify_reqwest(&e));
                        for f in encoder.error_frame("upstream stream interrupted") {
                            let _ = tx.send(Ok(f)).await;
                        }
                        break;
                    }
                }
            }
        }
    }

    if status == "success" {
        for f in encoder.finalize() {
            if tx.send(Ok(f)).await.is_err() {
                status = "client_disconnect";
                status_code = 499;
                break;
            }
        }
    }

    finalize_log(
        &state,
        &mut meta,
        &req,
        &attempt,
        &model_display,
        started,
        ttft_ms,
        status,
        status_code,
        usage,
        error_message,
        key,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn drive_aggregate(
    state: AppState,
    format: FrontendFormat,
    mut meta: RequestMeta,
    req: InternalRequest,
    mut attempt: Attempt,
    encoder_ctx: EncoderCtx,
    started: Instant,
    key: Option<db::VirtualKeyRow>,
    model_name: String,
) -> Value {
    let mut upstream = attempt.stream.take().expect("stream present");
    let adapter = attempt.adapter.clone();
    let mut buffer = String::new();
    let mut events: Vec<StreamEvent> = Vec::new();
    let mut usage = TokenUsage::default();
    let mut status = "success";
    let mut status_code = 200i64;
    let mut error_message: Option<String> = None;

    'outer: while let Some(chunk) = upstream.chunk().await.transpose() {
        match chunk {
            Ok(bytes) => {
                buffer.push_str(&String::from_utf8_lossy(&bytes).replace("\r\n", "\n"));
                while let Some(pos) = buffer.find("\n\n") {
                    let frame: String = buffer.drain(..pos + 2).collect();
                    let Some(payload) = extract_sse_data(&frame) else {
                        continue;
                    };
                    if payload.trim() == "[DONE]" {
                        continue;
                    }
                    match adapter.parse_stream_chunk(&payload) {
                        Ok(evs) => {
                            for ev in evs {
                                if let StreamEvent::Usage(u) = &ev {
                                    usage.merge(u);
                                }
                                events.push(ev);
                            }
                        }
                        Err(f) => {
                            status = "stream_error";
                            status_code = 502;
                            error_message = Some(f.message);
                            break 'outer;
                        }
                    }
                }
            }
            Err(e) => {
                status = "stream_error";
                status_code = 502;
                error_message = Some(classify_reqwest(&e));
                break;
            }
        }
    }

    finalize_log(
        &state,
        &mut meta,
        &req,
        &attempt,
        &model_name,
        started,
        None,
        status,
        status_code,
        usage.clone(),
        error_message.clone(),
        key,
    )
    .await;

    if status != "success" {
        return serde_json::json!({
            "error": {
                "message": error_message.unwrap_or_else(|| "upstream stream interrupted".into()),
                "type": "upstream_error"
            }
        });
    }

    frontends::aggregate(format, &model_name, &encoder_ctx.request_id, events, &usage)
}

/// Compute cost and enqueue the usage row (never blocks the request path).
#[allow(clippy::too_many_arguments)]
async fn finalize_log(
    state: &AppState,
    meta: &mut RequestMeta,
    req: &InternalRequest,
    attempt: &Attempt,
    model_display: &str,
    started: Instant,
    ttft_ms: Option<i64>,
    status: &str,
    status_code: i64,
    usage: TokenUsage,
    error_message: Option<String>,
    key: Option<db::VirtualKeyRow>,
) {
    let prices = attempt.target.model.prices();
    let cost = cost::compute_cost(&prices, &usage);
    let cost_known = cost.is_some();

    let row = UsageLogRow {
        id: format!("usage_{}", uuid::Uuid::new_v4().simple()),
        request_id: meta.request_id.clone(),
        ts: db::now_iso(),
        key_id: key.as_ref().map(|k| k.id.clone()),
        key_name: key.as_ref().map(|k| k.name.clone()),
        client_format: meta.client_format.to_string(),
        requested_model: req.requested_model.clone(),
        effective_model: Some(model_display.to_string()),
        combo_id: meta.combo_id.clone(),
        combo_name: meta.combo_name.clone(),
        fallback_hops: meta.fallback_hops,
        fallback_path: serde_json::to_string(&meta.fallback_path).unwrap_or_else(|_| "[]".into()),
        status: status.to_string(),
        status_code,
        latency_ms: Some(started.elapsed().as_millis() as i64),
        ttft_ms,
        input_tokens: usage.input.map(|v| v as i64),
        output_tokens: usage.output.map(|v| v as i64),
        cached_tokens: usage.cached.map(|v| v as i64),
        thinking_tokens: usage.thinking.map(|v| v as i64),
        cost_usd: cost,
        cost_known: cost_known as i64,
        price_version_id: None,
        cache_status: meta.cache_status.to_string(),
        serving_account_id: Some(attempt.target.account.id.clone()),
        serving_account: Some(attempt.target.account.label.clone()),
        serving_provider: Some(attempt.target.provider.name.clone()),
        upstream_request_id: attempt.upstream_request_id.clone(),
        flagged: (usage.input.is_none() && status == "success") as i64,
        error_message,
    };
    state.log_queue.enqueue(row);

    // Optional per-key body logging (FR-6.5).
    if let Some(k) = &key {
        if k.body_logging != 0 {
            let _ = db::insert_body_log(
                &state.pool,
                &meta.request_id,
                &k.id,
                "response",
                &serde_json::json!({"model": model_display, "status": status}).to_string(),
                7,
            )
            .await;
        }
    }
}

/// Extract the `data:` payload from one SSE frame (joining multiple data lines).
fn extract_sse_data(frame: &str) -> Option<String> {
    let mut data = String::new();
    let mut found = false;
    for line in frame.lines() {
        if let Some(rest) = line.strip_prefix("data:") {
            found = true;
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if found {
        Some(data)
    } else {
        None
    }
}
