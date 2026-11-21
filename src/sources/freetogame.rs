use crate::models::GameResult;
use reqwest::Client;
use serde_json::Value;

pub async fn search_freetogame(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let url = "https://www.freetogame.com/api/games";
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("FreeToGame istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("FreeToGame HTTP {}", resp.status()));
    }

    let items: Vec<Value> = resp
        .json()
        .await
        .map_err(|e| format!("FreeToGame JSON hatası: {}", e))?;

    let q_lower = query.trim().to_lowercase();
    let tag_lowers: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();

    let mut results = Vec::new();
    for item in items {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let desc = item
            .get("short_description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let genre = item.get("genre").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let plat_str = item.get("platform").and_then(|v| v.as_str()).unwrap_or("PC (Windows)");

        let hay = format!(
            "{} {} {} {}",
            title.to_lowercase(),
            desc.to_lowercase(),
            genre.to_lowercase(),
            plat_str.to_lowercase()
        );

        if !q_lower.is_empty() && !hay.contains(&q_lower) {
            continue;
        }
        if !tag_lowers.is_empty() && !tag_lowers.iter().any(|t| hay.contains(t)) {
            continue;
        }

        let mut platforms = Vec::new();
        let p_low = plat_str.to_lowercase();
        if p_low.contains("pc") || p_low.contains("windows") {
            platforms.push("PC".to_string());
            platforms.push("Windows".to_string());
        }
        if p_low.contains("browser") || p_low.contains("web") {
            platforms.push("Web".to_string());
        }
        if platforms.is_empty() {
            platforms.push("PC".to_string());
        }

        let is_web = platforms.contains(&"Web".to_string());
        let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let game_url = item
            .get("game_url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://www.freetogame.com/")
            .to_string();
        let thumb = item
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let release_date = item
            .get("release_date")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let mut tags_vec = vec!["Ücretsiz".to_string(), "F2P".to_string()];
        if !genre.is_empty() {
            tags_vec.push(genre);
        }
        if is_web {
            tags_vec.push("Tarayıcı Oyunu".to_string());
        }

        results.push(GameResult {
            id: format!("f2g-{}", id),
            title,
            description: desc,
            url: game_url,
            image_url: thumb,
            store: if is_web {
                "Websitesi / Tarayıcı".to_string()
            } else {
                "Resmi Websitesi".to_string()
            },
            store_category: "website".to_string(),
            platforms,
            tags: tags_vec,
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free: true,
            price_usd: Some(0.0),
            original_price_usd: None,
            price_display: "Ücretsiz (F2P)".to_string(),
            discount_percent: None,
            rating: None,
            release_date,
            source_name: "FreeToGame (Web & Resmi Site)".to_string(),
            is_niche_or_news: false,
            relevance_score: 0.0,
        });

        if results.len() >= 15 {
            break;
        }
    }

    Ok(results)
}

pub async fn search_gamerpower(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let url = "https://www.gamerpower.com/api/giveaways?type=game";
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("GamerPower istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("GamerPower HTTP {}", resp.status()));
    }

    let items: Vec<Value> = resp
        .json()
        .await
        .map_err(|e| format!("GamerPower JSON hatası: {}", e))?;

    let q_lower = query.trim().to_lowercase();
    let tag_lowers: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();

    let mut results = Vec::new();
    for item in items {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let desc = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plats = item.get("platforms").and_then(|v| v.as_str()).unwrap_or("");
        let hay = format!(
            "{} {} {}",
            title.to_lowercase(),
            desc.to_lowercase(),
            plats.to_lowercase()
        );

        if !q_lower.is_empty() && !hay.contains(&q_lower) {
            continue;
        }
        if !tag_lowers.is_empty() && !tag_lowers.iter().any(|t| hay.contains(t)) {
            continue;
        }

        let p_low = plats.to_lowercase();
        let mut platforms = Vec::new();
        if p_low.contains("pc") {
            platforms.push("PC".to_string());
        }
        if p_low.contains("android") {
            platforms.push("Mobile".to_string());
            platforms.push("Android".to_string());
        }
        if p_low.contains("ios") {
            if !platforms.contains(&"Mobile".to_string()) {
                platforms.push("Mobile".to_string());
            }
            platforms.push("iOS".to_string());
        }
        if p_low.contains("playstation") || p_low.contains("ps4") || p_low.contains("ps5") {
            platforms.push("Konsol".to_string());
            platforms.push("PlayStation".to_string());
        }
        if p_low.contains("xbox") {
            if !platforms.contains(&"Konsol".to_string()) {
                platforms.push("Konsol".to_string());
            }
            platforms.push("Xbox".to_string());
        }
        if p_low.contains("switch") {
            if !platforms.contains(&"Konsol".to_string()) {
                platforms.push("Konsol".to_string());
            }
            platforms.push("Switch".to_string());
        }
        if platforms.is_empty() {
            platforms.push("PC".to_string());
        }

        let (store, store_cat) = if p_low.contains("steam") {
            ("Steam", "steam")
        } else if p_low.contains("epic") {
            ("Epic Games", "epic")
        } else if p_low.contains("gog") {
            ("GOG", "gog")
        } else if p_low.contains("itch") {
            ("itch.io", "itchio")
        } else if p_low.contains("android") {
            ("Google Play", "googleplay")
        } else if p_low.contains("ios") {
            ("App Store", "appstore")
        } else {
            ("Websitesi", "website")
        };

        let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let worth = item.get("worth").and_then(|v| v.as_str()).unwrap_or("N/A");
        let orig_price = worth
            .trim_start_matches('$')
            .parse::<f64>()
            .ok()
            .filter(|&v| v > 0.0);

        results.push(GameResult {
            id: format!("gp-{}", id),
            title,
            description: desc,
            url: item
                .get("open_giveaway_url")
                .and_then(|v| v.as_str())
                .unwrap_or("https://www.gamerpower.com/")
                .to_string(),
            image_url: item.get("thumbnail").and_then(|v| v.as_str()).map(|s| s.to_string()),
            store: store.to_string(),
            store_category: store_cat.to_string(),
            platforms,
            tags: vec!["Ücretsiz Dağıtım".to_string(), plats.to_string()],
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free: true,
            price_usd: Some(0.0),
            original_price_usd: orig_price,
            price_display: if orig_price.is_some() {
                format!("Ücretsiz (Normal: {})", worth)
            } else {
                "Ücretsiz".to_string()
            },
            discount_percent: orig_price.map(|_| 100),
            rating: None,
            release_date: item.get("published_date").and_then(|v| v.as_str()).map(|s| s.chars().take(10).collect()),
            source_name: "GamerPower (Ücretsiz Fırsatlar)".to_string(),
            is_niche_or_news: false,
            relevance_score: 0.0,
        });

        if results.len() >= 10 {
            break;
        }
    }

    Ok(results)
}
