pub mod appstore;
pub mod cheapshark;
pub mod epic;
pub mod freetogame;
pub mod gog;
pub mod googleplay;
pub mod itchio;
pub mod news_niche;
pub mod niche_platforms;
pub mod steam;

use crate::models::{
    GameResult, ScreenshotItem, SearchQuery, SearchResponse, SimilarRequest, SimilarResponse,
    SourceStatus,
};
use futures::future::join_all;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Instant;

pub async fn run_aggregated_search(client: &Client, orig_req: &SearchQuery) -> SearchResponse {
    let total_start = Instant::now();

    // If user typed comma-separated terms like "survival, craft", split them into tags automatically!
    let mut normalized_req = orig_req.clone();
    if orig_req.query.contains(',') {
        let mut extra_tags = normalized_req.tags.clone();
        for part in orig_req.query.split(',') {
            let trimmed = part.trim();
            if !trimmed.is_empty() && !extra_tags.iter().any(|t| t.eq_ignore_ascii_case(trimmed)) {
                extra_tags.push(trimmed.to_string());
            }
        }
        normalized_req.tags = extra_tags;
        normalized_req.query = String::new();
    }
    let req = &normalized_req;

    let stores_empty = req.stores.is_empty();
    let want_store = |cat: &str| stores_empty || req.stores.iter().any(|s| s.eq_ignore_ascii_case(cat));

    let plat_empty = req.platforms.is_empty();
    let want_pc = plat_empty
        || req
            .platforms
            .iter()
            .any(|p| matches!(p.to_lowercase().as_str(), "pc" | "windows" | "macos" | "linux"));
    let want_mobile = plat_empty
        || req
            .platforms
            .iter()
            .any(|p| matches!(p.to_lowercase().as_str(), "mobile" | "android" | "ios"));
    let want_android = plat_empty
        || req
            .platforms
            .iter()
            .any(|p| matches!(p.to_lowercase().as_str(), "mobile" | "android"));
    let want_ios = plat_empty
        || req
            .platforms
            .iter()
            .any(|p| matches!(p.to_lowercase().as_str(), "mobile" | "ios"));
    let want_web = plat_empty
        || req.platforms.iter().any(|p| p.eq_ignore_ascii_case("web"));

    type TaskOutput = (SourceStatus, Vec<GameResult>);
    let mut handles: Vec<tokio::task::JoinHandle<TaskOutput>> = Vec::new();

    // 1. Steam
    if want_store("steam") && want_pc {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match steam::search_steam(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Steam".into(),
                        category: "steam".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Steam".into(),
                        category: "steam".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 2. GOG
    if want_store("gog") && want_pc {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match gog::search_gog(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "GOG".into(),
                        category: "gog".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "GOG".into(),
                        category: "gog".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 3. Epic Games
    if want_store("epic") && want_pc {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match epic::search_epic(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Epic Games".into(),
                        category: "epic".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Epic Games".into(),
                        category: "epic".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 4. CheapShark (Multi-store PC deals: Humble, Fanatical, GMG, Steam, GOG, Epic...)
    if (want_store("other_pc") || want_store("steam") || want_store("epic") || want_store("gog"))
        && want_pc
    {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        let mp = req.max_price_usd;
        let only_sale = req.price_mode == "sale";
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match cheapshark::search_cheapshark(&c, &q, &t, mp, only_sale).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Çoklu PC Mağazaları".into(),
                        category: "other_pc".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Çoklu PC Mağazaları".into(),
                        category: "other_pc".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 5. Apple App Store (iOS)
    if want_store("appstore") && want_ios {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match appstore::search_appstore(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "App Store (iOS)".into(),
                        category: "appstore".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "App Store (iOS)".into(),
                        category: "appstore".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 6. Google Play (Android)
    if want_store("googleplay") && want_android {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match googleplay::search_google_play(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Google Play".into(),
                        category: "googleplay".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Google Play".into(),
                        category: "googleplay".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 7. F-Droid (Open-Source / Niche Android)
    if (want_store("fdroid") || (stores_empty && req.include_niche_news)) && want_android {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match googleplay::search_fdroid(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "F-Droid (FOSS)".into(),
                        category: "fdroid".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "F-Droid (FOSS)".into(),
                        category: "fdroid".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 8. itch.io (Indie / Niche / Web / Freeware)
    if want_store("itchio") && (want_pc || want_web || want_android) {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match itchio::search_itchio(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "itch.io".into(),
                        category: "itchio".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "itch.io".into(),
                        category: "itchio".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 9. FreeToGame (Official Website / Browser F2P)
    if want_store("website") && (want_pc || want_web) && req.price_mode != "paid" {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match freetogame::search_freetogame(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Websitesi / F2P".into(),
                        category: "website".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Websitesi / F2P".into(),
                        category: "website".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 10. GamerPower (Cross-platform free giveaways)
    if req.price_mode != "paid" && (want_pc || want_mobile) {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match freetogame::search_gamerpower(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "GamerPower Fırsatlar".into(),
                        category: "website".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "GamerPower Fırsatlar".into(),
                        category: "website".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 11. Niche / Indie / Gaming News RSS & Custom Feeds
    if req.include_niche_news || want_store("news") || !req.custom_feeds.is_empty() {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        let cf = req.custom_feeds.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match news_niche::search_news_and_niche(&c, &q, &t, &cf).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Kıyı Kenar & Haber Siteleri".into(),
                        category: "news".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Kıyı Kenar & Haber Siteleri".into(),
                        category: "news".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 12. VNDB (Visual Novels & exact language verification)
    if want_store("vndb") || (stores_empty && req.include_niche_news) {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match niche_platforms::search_vndb(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "VNDB".into(),
                        category: "vndb".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "VNDB".into(),
                        category: "vndb".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 13. GameJolt (Indie / Experimental / Freeware)
    if (want_store("gamejolt") || (stores_empty && req.include_niche_news)) && (want_pc || want_web) {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match niche_platforms::search_gamejolt(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "GameJolt".into(),
                        category: "gamejolt".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "GameJolt".into(),
                        category: "gamejolt".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 14. Archive.org (Retro / Freeware / Browser-playable Niche Games)
    if (want_store("archive") || (stores_empty && req.include_niche_news)) && (want_pc || want_web) {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match niche_platforms::search_archive_org(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "Archive.org".into(),
                        category: "archive".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "Archive.org".into(),
                        category: "archive".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 15. TapTap (Global & Asian niche mobile games)
    if (want_store("taptap") || (stores_empty && req.include_niche_news)) && want_mobile {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match niche_platforms::search_taptap(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "TapTap".into(),
                        category: "taptap".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "TapTap".into(),
                        category: "taptap".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    // 16. SteamDB / SteamSpy (Tag-based Steam analytics)
    if (want_store("steamdb") || (stores_empty && req.include_niche_news)) && want_pc {
        let c = client.clone();
        let q = req.query.clone();
        let t = req.tags.clone();
        handles.push(tokio::spawn(async move {
            let st = Instant::now();
            match niche_platforms::search_steamspy_steamdb(&c, &q, &t).await {
                Ok(items) => (
                    SourceStatus {
                        name: "SteamDB / SteamSpy".into(),
                        category: "steamdb".into(),
                        count: items.len(),
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: true,
                        error: None,
                    },
                    items,
                ),
                Err(e) => (
                    SourceStatus {
                        name: "SteamDB / SteamSpy".into(),
                        category: "steamdb".into(),
                        count: 0,
                        elapsed_ms: st.elapsed().as_millis() as u64,
                        ok: false,
                        error: Some(e),
                    },
                    vec![],
                ),
            }
        }));
    }

    let joined = join_all(handles).await;
    let mut statuses = Vec::new();
    let mut raw_results = Vec::new();

    for res in joined {
        if let Ok((status, mut items)) = res {
            statuses.push(status);
            raw_results.append(&mut items);
        }
    }

    let total_found = raw_results.len();

    // Deduplicate by (normalized title, store_category), keeping the richer entry
    let mut dedup: HashMap<String, GameResult> = HashMap::new();
    for item in raw_results {
        let key = format!(
            "{}::{}",
            item.title.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>(),
            item.store_category
        );
        if let Some(existing) = dedup.get_mut(&key) {
            // Keep whichever has more metadata (languages, description, image)
            let new_richness = item.languages.len() * 3 + item.tags.len() + if item.description.len() > 40 { 2 } else { 0 };
            let old_richness = existing.languages.len() * 3 + existing.tags.len() + if existing.description.len() > 40 { 2 } else { 0 };
            if new_richness > old_richness {
                *existing = item;
            }
        } else {
            dedup.insert(key, item);
        }
    }

    let mut filtered: Vec<GameResult> = dedup
        .into_values()
        .filter(|g| passes_filters(g, req))
        .map(|mut g| {
            g.relevance_score = compute_relevance(&g, req);
            g
        })
        .collect();

    // Sort results
    match req.sort_by.as_str() {
        "price_asc" => filtered.sort_by(|a, b| {
            let pa = a.price_usd.unwrap_or(9999.0);
            let pb = b.price_usd.unwrap_or(9999.0);
            pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
        }),
        "price_desc" => filtered.sort_by(|a, b| {
            let pa = a.price_usd.unwrap_or(-1.0);
            let pb = b.price_usd.unwrap_or(-1.0);
            pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
        }),
        "discount" => filtered.sort_by(|a, b| {
            b.discount_percent.unwrap_or(0).cmp(&a.discount_percent.unwrap_or(0))
        }),
        "rating" => filtered.sort_by(|a, b| {
            let ra = a.rating.unwrap_or(-1.0);
            let rb = b.rating.unwrap_or(-1.0);
            rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal)
        }),
        _ => filtered.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }

    let filtered_count = filtered.len();

    SearchResponse {
        total_found,
        filtered_count,
        elapsed_ms: total_start.elapsed().as_millis() as u64,
        sources: statuses,
        results: filtered,
    }
}

fn passes_filters(g: &GameResult, req: &SearchQuery) -> bool {
    // 1. Store filter
    if !req.stores.is_empty() {
        if !req.stores.iter().any(|s| s.eq_ignore_ascii_case(&g.store_category)) {
            return false;
        }
    }

    // 2. Platform filter
    if !req.platforms.is_empty() {
        let matched = req.platforms.iter().any(|wanted| {
            let w = wanted.to_lowercase();
            g.platforms.iter().any(|gp| {
                let p = gp.to_lowercase();
                p == w
                    || (w == "pc" && matches!(p.as_str(), "windows" | "macos" | "linux"))
                    || (w == "mobile" && matches!(p.as_str(), "android" | "ios"))
                    || (w == "konsol" && matches!(p.as_str(), "playstation" | "xbox" | "switch"))
            })
        });
        if !matched {
            return false;
        }
    }

    // 3. Price mode filter
    match req.price_mode.as_str() {
        "free" => {
            if !g.is_free {
                return false;
            }
        }
        "paid" => {
            if g.is_free {
                return false;
            }
        }
        "sale" => {
            if g.discount_percent.unwrap_or(0) == 0 {
                return false;
            }
        }
        _ => {}
    }

    // 4. Max price filter
    if let Some(max_p) = req.max_price_usd {
        if let Some(p) = g.price_usd {
            if p > max_p {
                return false;
            }
        } else if !g.is_free && req.price_mode == "paid" {
            return false;
        }
    }

    // 5. Language filter
    if !req.languages.is_empty() {
        if g.languages.is_empty() {
            if !req.include_unknown_lang {
                return false;
            }
        } else {
            let has_all_or_any = req.languages.iter().any(|wanted| {
                g.languages.iter().any(|gl| gl.eq_ignore_ascii_case(wanted))
            });
            if !has_all_or_any {
                return false;
            }
        }
    }

    true
}

fn compute_relevance(g: &GameResult, req: &SearchQuery) -> f64 {
    let mut score = 10.0;
    let q = req.query.trim().to_lowercase();
    let title_low = g.title.to_lowercase();
    let desc_low = g.description.to_lowercase();
    let tags_joined = g.tags.join(" ").to_lowercase();

    if !q.is_empty() {
        if title_low == q {
            score += 60.0;
        } else if title_low.starts_with(&q) {
            score += 42.0;
        } else if title_low.contains(&q) {
            score += 28.0;
        } else if desc_low.contains(&q) || tags_joined.contains(&q) {
            score += 12.0;
        }
    }

    for tag in &req.tags {
        let tl = tag.to_lowercase();
        if tags_joined.contains(&tl) {
            score += 15.0;
        } else if title_low.contains(&tl) || desc_low.contains(&tl) {
            score += 8.0;
        }
    }

    // Slight boost for rich metadata
    if g.image_url.is_some() {
        score += 3.0;
    }
    if !g.languages.is_empty() {
        score += 4.0;
    }
    if let Some(r) = g.rating {
        score += (r / 100.0) * 6.0;
    }
    if g.discount_percent.unwrap_or(0) > 0 {
        score += 2.0;
    }

    score
}

pub async fn find_similar_games(client: &Client, req: &SimilarRequest) -> SimilarResponse {
    let start = Instant::now();

    let stop_tags: &[&str] = &[
        "steam", "epic games", "gog", "itch.io", "gamejolt", "archive.org", "taptap",
        "vndb", "steamdb", "f-droid", "pc", "windows", "macos", "linux", "mobile",
        "mobil", "android", "ios", "web", "ücretsiz", "free", "f2p", "indirimde",
        "kıyı kenar", "haber", "single-player", "steam achievements", "steam cloud",
        "full controller support", "family sharing", "remote play", "steam trading cards",
        "games", "entertainment", "açık kaynak", "promosyon", "topluluk paylaşımı",
    ];

    let mut meaningful_tags: Vec<String> = Vec::new();
    for t in &req.tags {
        let low = t.trim().to_lowercase();
        if low.len() >= 2
            && !stop_tags.iter().any(|s| *s == low)
            && !low.starts_with("r/")
            && !meaningful_tags.iter().any(|x| x.eq_ignore_ascii_case(t))
        {
            meaningful_tags.push(t.trim().to_string());
        }
    }

    // Also pull distinctive gameplay/genre keywords from title & description if we have fewer than 3 tags
    let known_genres = [
        "roguelike", "roguelite", "rpg", "jrpg", "action", "adventure", "strategy",
        "puzzle", "horror", "survival", "sandbox", "metroidvania", "platformer",
        "shooter", "fps", "simulation", "racing", "fighting", "stealth", "card",
        "deckbuilder", "tactical", "turn-based", "open world", "soulslike", "pixel",
        "zombie", "dungeon", "anime", "visual novel", "co-op", "multiplayer", "mmorpg",
        "tower defense", "city builder", "management", "cyberpunk", "fantasy", "sci-fi",
        "mystery", "detective", "chess", "rhythm", "fighting", "arcade", "fighting",
    ];
    let text_hay = format!("{} {}", req.title.to_lowercase(), req.description.to_lowercase());
    for kw in known_genres {
        if meaningful_tags.len() >= 4 {
            break;
        }
        if text_hay.contains(kw)
            && !meaningful_tags.iter().any(|x| x.eq_ignore_ascii_case(kw))
        {
            meaningful_tags.push(kw.to_string());
        }
    }

    // Fallback to a meaningful word from the title if still empty
    if meaningful_tags.is_empty() {
        let title_stop = ["the", "of", "and", "in", "to", "a", "an", "for", "with", "edition", "game", "ii", "iii"];
        for word in req.title.split(|c: char| !c.is_alphanumeric()) {
            let lw = word.to_lowercase();
            if lw.len() >= 4 && !title_stop.contains(&lw.as_str()) {
                meaningful_tags.push(word.to_string());
                if meaningful_tags.len() >= 2 {
                    break;
                }
            }
        }
    }
    if meaningful_tags.is_empty() {
        meaningful_tags.push("indie".to_string());
    }

    let used_tags: Vec<String> = meaningful_tags.iter().take(4).cloned().collect();
    let search_term = used_tags.iter().take(2).cloned().collect::<Vec<_>>().join(" ");

    // Query diverse sources + English screenshots concurrently!
    let (steam_r, gog_r, itch_r, gj_r, app_r, f2g_r, cs_r, vndb_r, screenshots) = tokio::join!(
        steam::search_steam(client, &search_term, &used_tags),
        gog::search_gog(client, &search_term, &used_tags),
        itchio::search_itchio(client, "", &used_tags),
        niche_platforms::search_gamejolt(client, &search_term, &used_tags),
        appstore::search_appstore(client, &search_term, &used_tags),
        freetogame::search_freetogame(client, "", &used_tags),
        cheapshark::search_cheapshark(client, &search_term, &used_tags, None, false),
        niche_platforms::search_vndb(client, &search_term, &used_tags),
        fetch_english_screenshots(client, req),
    );

    let mut pool = Vec::new();
    for res in [steam_r, gog_r, itch_r, gj_r, app_r, f2g_r, cs_r, vndb_r] {
        if let Ok(mut items) = res {
            pool.append(&mut items);
        }
    }

    let base_norm: String = req
        .title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();

    let mut dedup: HashMap<String, GameResult> = HashMap::new();
    for mut g in pool {
        if g.id == req.id {
            continue;
        }
        let g_norm: String = g
            .title
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        // Exclude exact same game (even on another store)
        if g_norm == base_norm || g_norm.is_empty() {
            continue;
        }

        // Compute similarity score
        let mut sim_score = 10.0;
        let g_tags_low: Vec<String> = g.tags.iter().map(|t| t.to_lowercase()).collect();
        let g_hay = format!(
            "{} {} {}",
            g.title.to_lowercase(),
            g.description.to_lowercase(),
            g_tags_low.join(" ")
        );

        for ut in &used_tags {
            let utl = ut.to_lowercase();
            if g_tags_low.iter().any(|gt| gt.contains(&utl) || utl.contains(gt)) {
                sim_score += 20.0;
            } else if g_hay.contains(&utl) {
                sim_score += 10.0;
            }
        }

        // Platform affinity bonus
        for bp in &req.platforms {
            if g.platforms.iter().any(|gp| gp.eq_ignore_ascii_case(bp)) {
                sim_score += 4.0;
            }
        }
        if g.image_url.is_some() {
            sim_score += 4.0;
        }
        if let Some(r) = g.rating {
            sim_score += (r / 100.0) * 6.0;
        }
        g.relevance_score = sim_score;

        if let Some(existing) = dedup.get_mut(&g_norm) {
            if g.relevance_score > existing.relevance_score {
                *existing = g;
            }
        } else {
            dedup.insert(g_norm, g);
        }
    }

    let mut results: Vec<GameResult> = dedup.into_values().collect();
    results.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(16);

    let riotpixels_url = format!(
        "https://en.riotpixels.com/search/{}/",
        urlencoding::encode(req.title.trim())
    );

    SimilarResponse {
        base_title: req.title.clone(),
        used_tags,
        elapsed_ms: start.elapsed().as_millis() as u64,
        screenshots,
        riotpixels_url,
        results,
    }
}

/// Fetches high-resolution English screenshots for the selected game from open APIs
/// (Steam Store EN, GOG EN, App Store US, VNDB).
async fn fetch_english_screenshots(client: &Client, req: &SimilarRequest) -> Vec<ScreenshotItem> {
    let mut shots = Vec::new();

    // 1. Try Steam (English) — best source for high-res PC/indie/multiplatform screenshots
    let mut steam_id: Option<u64> = req
        .id
        .strip_prefix("steam-")
        .or_else(|| req.id.strip_prefix("steamdb-"))
        .and_then(|s| s.parse::<u64>().ok());

    if steam_id.is_none() && req.store_category != "appstore" && req.store_category != "googleplay" {
        let search_url = format!(
            "https://store.steampowered.com/api/storesearch/?term={}&l=english&cc=US",
            urlencoding::encode(req.title.trim())
        );
        if let Ok(resp) = client
            .get(&search_url)
            .timeout(std::time::Duration::from_secs(4))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                steam_id = json
                    .pointer("/items/0/id")
                    .and_then(|v| v.as_u64());
            }
        }
    }

    if let Some(appid) = steam_id {
        let detail_url = format!(
            "https://store.steampowered.com/api/appdetails?appids={}&l=english&cc=US",
            appid
        );
        if let Ok(resp) = client
            .get(&detail_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json
                    .get(appid.to_string())
                    .and_then(|e| e.pointer("/data/screenshots"))
                    .and_then(|v| v.as_array())
                {
                    for s in arr.iter().take(10) {
                        let thumb = s.get("path_thumbnail").and_then(|v| v.as_str()).unwrap_or("");
                        let full = s.get("path_full").and_then(|v| v.as_str()).unwrap_or(thumb);
                        if !thumb.is_empty() {
                            shots.push(ScreenshotItem {
                                thumb_url: thumb.to_string(),
                                full_url: full.to_string(),
                                source: "Steam (EN)".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. If still empty (or mobile game), try Apple App Store (US / EN) screenshots
    if shots.is_empty() {
        let itunes_url = format!(
            "https://itunes.apple.com/search?term={}&entity=software&genreId=6014&limit=1&country=US",
            urlencoding::encode(req.title.trim())
        );
        if let Ok(resp) = client
            .get(&itunes_url)
            .timeout(std::time::Duration::from_secs(4))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json
                    .pointer("/results/0/screenshotUrls")
                    .and_then(|v| v.as_array())
                {
                    for s in arr.iter().take(10) {
                        if let Some(u) = s.as_str() {
                            shots.push(ScreenshotItem {
                                thumb_url: u.to_string(),
                                full_url: u.to_string(),
                                source: "App Store (EN)".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 3. Fallback: GOG Catalog (en-US) screenshots
    if shots.is_empty() {
        let gog_url = format!(
            "https://catalog.gog.com/v1/catalog?limit=1&query=like:{}&productType=in:game&locale=en-US",
            urlencoding::encode(req.title.trim())
        );
        if let Ok(resp) = client
            .get(&gog_url)
            .timeout(std::time::Duration::from_secs(4))
            .send()
            .await
        {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if let Some(arr) = json
                    .pointer("/products/0/screenshots")
                    .and_then(|v| v.as_array())
                {
                    for s in arr.iter().take(10) {
                        if let Some(tpl) = s.as_str() {
                            let full = tpl.replace("{formatter}", "ggvgm_2x");
                            let thumb = tpl.replace("{formatter}", "ggvgm");
                            shots.push(ScreenshotItem {
                                thumb_url: thumb,
                                full_url: full,
                                source: "GOG (EN)".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    shots
}
