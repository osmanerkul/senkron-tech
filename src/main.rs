mod ai_promo_radar;
mod models;
mod outside_lens;
mod sources;

use ai_promo_radar::{AiPromoCache, AiPromoResponse};
use axum::{
    extract::{Query, State},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use models::{SearchQuery, SearchResponse, SimilarRequest, SimilarResponse};
use outside_lens::{OutsideLensCache, OutsideLensResponse};
use reqwest::Client;
use serde::Deserialize;
use std::net::SocketAddr;
use std::time::Duration;
use tower_http::cors::CorsLayer;

const PORTFOLIO_HTML: &str = include_str!("../static/portfolio.html");
const OYUN_BULUCU_HTML: &str = include_str!("../static/index.html");
const CAD_STUDIO_HTML: &str = include_str!("../static/cad_studio.html");
const OUTSIDE_LENS_HTML: &str = include_str!("../static/outside_lens.html");
const AI_PROMO_RADAR_HTML: &str = include_str!("../static/ai_promo_radar.html");
const RAINMETER_STUDIO_HTML: &str = include_str!("../static/rainmeter_studio.html");
const GOOGLEBOOK_OS_HTML: &str = include_str!("../static/googlebook_os.html");

#[derive(Clone)]
struct AppState {
    client: Client,
    outside_lens_cache: OutsideLensCache,
    ai_promo_cache: AiPromoCache,
}

#[derive(Deserialize)]
struct RefreshParams {
    refresh: Option<u8>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "oyun_bulucu=info".into()),
        )
        .init();

    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(10))
        .build()
        .expect("HTTP istemcisi oluşturulamadı");

    // Optional CLI mode: `oyun-bulucu --cli <arama>`
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 2 && args[1] == "--cli" {
        let query_str = args[2..].join(" ");
        println!("🔍 Aranıyor: '{}' ...", query_str);
        let req = SearchQuery {
            query: query_str,
            tags: vec![],
            platforms: vec![],
            stores: vec![],
            price_mode: "all".into(),
            max_price_usd: None,
            languages: vec![],
            include_unknown_lang: true,
            include_niche_news: true,
            custom_feeds: vec![],
            sort_by: "relevance".into(),
        };
        let resp = sources::run_aggregated_search(&client, &req).await;
        println!(
            "\n✅ {} sonuç bulundu ({} ms):\n",
            resp.filtered_count, resp.elapsed_ms
        );
        for s in &resp.sources {
            println!(
                "  [{}] {}: {} sonuç ({} ms)",
                if s.ok { "OK" } else { "ERR" },
                s.name,
                s.count,
                s.elapsed_ms
            );
        }
        println!("\n--- İLK 20 OYUN ---");
        for (i, g) in resp.results.iter().take(20).enumerate() {
            let en = match g.has_english {
                Some(true) => "EN:✓",
                Some(false) => "EN:✗",
                None => "EN:?",
            };
            println!(
                "{:2}. {} | {} | {} | {} | {}",
                i + 1,
                g.title,
                g.store,
                g.price_display,
                en,
                g.url
            );
        }
        return;
    }

    let state = AppState {
        client,
        outside_lens_cache: OutsideLensCache::new(),
        ai_promo_cache: AiPromoCache::new(),
    };

    let app = Router::new()
        .route("/", get(serve_portfolio))
        .route("/oyun-bulucu", get(serve_oyun_bulucu))
        .route("/oyun-bulucu/", get(serve_oyun_bulucu))
        .route("/cad-studio", get(serve_cad_studio))
        .route("/cad-studio/", get(serve_cad_studio))
        .route("/outside-lens", get(serve_outside_lens))
        .route("/outside-lens/", get(serve_outside_lens))
        .route("/ai-promo-radar", get(serve_ai_promo_radar))
        .route("/ai-promo-radar/", get(serve_ai_promo_radar))
        .route("/rainmeter-studio", get(serve_rainmeter_studio))
        .route("/rainmeter-studio/", get(serve_rainmeter_studio))
        .route("/googlebook-os", get(serve_googlebook_os))
        .route("/googlebook-os/", get(serve_googlebook_os))
        .route("/api/search", post(api_search))
        .route("/api/similar", post(api_similar))
        .route("/api/outside-lens", get(api_outside_lens))
        .route("/api/ai-promo-radar", get(api_ai_promo_radar))
        .route("/api/health", get(api_health))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let env_port = std::env::var("PORT").ok().and_then(|p| p.parse::<u16>().ok());
    let (ip, port) = match env_port {
        Some(p) => ([0, 0, 0, 0], p),
        None => ([127, 0, 0, 1], 3847),
    };

    let addr = SocketAddr::from((ip, port));
    println!("====================================================");
    println!("  🌐 SENKRONT PORTFÖY & OYUN BULUCU (Rust) başlatıldı!");
    println!("  🏠 Ana Sayfa:      http://{}", addr);
    println!("  🎮 Oyun Bulucu:    http://{}/oyun-bulucu/", addr);
    println!("  🧊 CAD Studio:     http://{}/cad-studio/", addr);
    println!("  🌐 Outside Lens:   http://{}/outside-lens/", addr);
    println!("  📡 AI Promo Radar: http://{}/ai-promo-radar/", addr);
    println!("  🎨 Rainmeter:      http://{}/rainmeter-studio/", addr);
    println!("  💻 Googlebook OS:  http://{}/googlebook-os/", addr);
    println!("====================================================");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Port bağlanamadı");

    axum::serve(listener, app)
        .await
        .expect("Sunucu hatası");
}

async fn serve_portfolio() -> Html<&'static str> {
    Html(PORTFOLIO_HTML)
}

async fn serve_oyun_bulucu() -> Html<&'static str> {
    Html(OYUN_BULUCU_HTML)
}

async fn serve_cad_studio() -> Html<&'static str> {
    Html(CAD_STUDIO_HTML)
}

async fn serve_outside_lens() -> Html<&'static str> {
    Html(OUTSIDE_LENS_HTML)
}

async fn serve_ai_promo_radar() -> Html<&'static str> {
    Html(AI_PROMO_RADAR_HTML)
}

async fn serve_rainmeter_studio() -> Html<&'static str> {
    Html(RAINMETER_STUDIO_HTML)
}

async fn serve_googlebook_os() -> Html<&'static str> {
    Html(GOOGLEBOOK_OS_HTML)
}

async fn api_outside_lens(
    State(state): State<AppState>,
    Query(params): Query<RefreshParams>,
) -> Json<OutsideLensResponse> {
    let force = params.refresh.unwrap_or(0) == 1;
    let resp = state.outside_lens_cache.get_or_fetch(&state.client, force).await;
    Json(resp)
}

async fn api_ai_promo_radar(
    State(state): State<AppState>,
    Query(params): Query<RefreshParams>,
) -> Json<AiPromoResponse> {
    let force = params.refresh.unwrap_or(0) == 1;
    let resp = state.ai_promo_cache.get_or_fetch(&state.client, force).await;
    Json(resp)
}

async fn api_health() -> &'static str {
    "ok"
}

async fn api_search(
    State(state): State<AppState>,
    Json(payload): Json<SearchQuery>,
) -> Json<SearchResponse> {
    let resp = sources::run_aggregated_search(&state.client, &payload).await;
    Json(resp)
}

async fn api_similar(
    State(state): State<AppState>,
    Json(payload): Json<SimilarRequest>,
) -> Json<SimilarResponse> {
    let resp = sources::find_similar_games(&state.client, &payload).await;
    Json(resp)
}
