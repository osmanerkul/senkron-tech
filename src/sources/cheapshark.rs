use crate::models::GameResult;
use reqwest::Client;
use serde_json::Value;

fn map_store(store_id: &str) -> (&'static str, &'static str) {
    match store_id {
        "1" => ("Steam", "steam"),
        "2" => ("GamersGate", "other_pc"),
        "3" => ("GreenManGaming", "other_pc"),
        "7" => ("GOG", "gog"),
        "8" => ("EA / Origin", "other_pc"),
        "11" => ("Humble Store", "other_pc"),
        "13" => ("Ubisoft Connect", "other_pc"),
        "15" => ("Fanatical", "other_pc"),
        "21" => ("WinGameStore", "other_pc"),
        "23" => ("GameBillet", "other_pc"),
        "25" => ("Epic Games", "epic"),
        "27" => ("Gamesplanet", "other_pc"),
        "28" => ("Gamesload", "other_pc"),
        "30" => ("IndieGala", "other_pc"),
        "31" => ("Blizzard", "other_pc"),
        "33" => ("DLGamer", "other_pc"),
        "34" => ("Noctre", "other_pc"),
        "35" => ("DreamGame", "other_pc"),
        _ => ("PC Mağazası", "other_pc"),
    }
}

pub async fn search_cheapshark(
    client: &Client,
    query: &str,
    tags: &[String],
    max_price: Option<f64>,
    only_sale: bool,
) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        String::new()
    };

    let mut url = format!(
        "https://www.cheapshark.com/api/1.0/deals?pageSize=25&sortBy={}",
        if only_sale { "Savings" } else { "Deal%20Rating" }
    );
    if !term.is_empty() {
        url.push_str(&format!("&title={}", urlencoding::encode(&term)));
    }
    if let Some(mp) = max_price {
        url.push_str(&format!("&upperPrice={:.0}", mp.ceil()));
    }
    if only_sale {
        url.push_str("&onSale=1");
    }

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("CheapShark istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("CheapShark HTTP {}", resp.status()));
    }

    let items: Vec<Value> = resp
        .json()
        .await
        .map_err(|e| format!("CheapShark JSON hatası: {}", e))?;

    let mut results = Vec::new();
    for item in items {
        let deal_id = item.get("dealID").and_then(|v| v.as_str()).unwrap_or("");
        let title = item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Bilinmeyen Oyun")
            .to_string();
        let store_id = item.get("storeID").and_then(|v| v.as_str()).unwrap_or("0");
        let (store_name, store_cat) = map_store(store_id);

        let sale_price = item
            .get("salePrice")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok());
        let normal_price = item
            .get("normalPrice")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok());
        let savings = item
            .get("savings")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .map(|f| f.round() as u32)
            .filter(|&d| d > 0);

        let is_free = sale_price == Some(0.0);
        let price_display = if is_free {
            "Ücretsiz".to_string()
        } else if let Some(p) = sale_price {
            format!("${:.2}", p)
        } else {
            "Bilinmiyor".to_string()
        };

        let thumb = item
            .get("thumb")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let rating = item
            .get("steamRatingPercent")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|&r| r > 0.0)
            .or_else(|| {
                item.get("metacriticScore")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|&r| r > 0.0)
            });

        let steam_rating_text = item
            .get("steamRatingText")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let description = if !steam_rating_text.is_empty() {
            format!("{} mağazasında • Değerlendirme: {}", store_name, steam_rating_text)
        } else {
            format!("{} üzerinden PC dijital sürüm", store_name)
        };

        let deal_url = format!("https://www.cheapshark.com/redirect?dealID={}", deal_id);

        let mut tags_vec = vec!["PC".to_string(), store_name.to_string()];
        if savings.is_some() {
            tags_vec.push("İndirimde".to_string());
        }

        results.push(GameResult {
            id: format!("cs-{}-{}", store_id, deal_id),
            title,
            description,
            url: deal_url,
            image_url: thumb,
            store: store_name.to_string(),
            store_category: store_cat.to_string(),
            platforms: vec!["PC".to_string(), "Windows".to_string()],
            tags: tags_vec,
            languages: vec![],
            has_english: None,
            has_turkish: None,
            is_free,
            price_usd: sale_price,
            original_price_usd: normal_price,
            price_display,
            discount_percent: savings,
            rating,
            release_date: None,
            source_name: "CheapShark (Çoklu Mağaza)".to_string(),
            is_niche_or_news: false,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}
