mod models;
mod sources;

use axum::{
    extract::State,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use models::{SearchQuery, SearchResponse, SimilarRequest, SimilarResponse};
use reqwest::Client;
use std::net::SocketAddr;
use std::time::Duration;
use tower_http::cors::CorsLayer;

const PORTFOLIO_HTML: &str = include_str!("../static/portfolio.html");
const OYUN_BULUCU_HTML: &str = include_str!("../static/index.html");
const CAD_STUDIO_HTML: &str = include_str!("../static/cad_studio.html");

#[derive(Clone)]
struct AppState {
    client: Client,
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

    let state = AppState { client };

    let app = Router::new()
        .route("/", get(serve_portfolio))
        .route("/oyun-bulucu", get(serve_oyun_bulucu))
        .route("/oyun-bulucu/", get(serve_oyun_bulucu))
        .route("/cad-studio", get(serve_cad_studio))
        .route("/cad-studio/", get(serve_cad_studio))
        .route("/api/search", post(api_search))
        .route("/api/similar", post(api_similar))
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
    println!("  🏠 Ana Sayfa:    http://{}", addr);
    println!("  🎮 Oyun Bulucu:  http://{}/oyun-bulucu/", addr);
    println!("  🧊 CAD Studio:   http://{}/cad-studio/", addr);
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
