use crate::models::GameResult;
use reqwest::Client;
use scraper::{Html, Selector};

pub async fn search_itchio(client: &Client, query: &str, tags: &[String]) -> Result<Vec<GameResult>, String> {
    let url = if !query.trim().is_empty() {
        format!(
            "https://itch.io/search?q={}",
            urlencoding::encode(query.trim())
        )
    } else if let Some(first_tag) = tags.first() {
        let slug = first_tag.trim().to_lowercase().replace(' ', "-");
        format!("https://itch.io/games/tag-{}", urlencoding::encode(&slug))
    } else {
        "https://itch.io/games/newest".to_string()
    };

    let resp = client
        .get(&url)
        .header("Accept", "text/html,application/xhtml+xml")
        .send()
        .await
        .map_err(|e| format!("itch.io istek hatası: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("itch.io HTTP {}", resp.status()));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("itch.io okuma hatası: {}", e))?;

    let doc = Html::parse_document(&body);
    let cell_sel = Selector::parse("div.game_cell").unwrap();
    let title_sel = Selector::parse("a.title.game_link, a.game_link").unwrap();
    let text_sel = Selector::parse("div.game_text").unwrap();
    let genre_sel = Selector::parse("div.game_genre").unwrap();
    let price_sel = Selector::parse("div.price_value").unwrap();
    let img_sel = Selector::parse("img").unwrap();
    let web_flag_sel = Selector::parse("span.web_flag").unwrap();
    let win_sel = Selector::parse("span.icon-windows8").unwrap();
    let mac_sel = Selector::parse("span.icon-apple").unwrap();
    let lin_sel = Selector::parse("span.icon-tux").unwrap();
    let and_sel = Selector::parse("span.icon-android").unwrap();

    let mut results = Vec::new();
    for (idx, cell) in doc.select(&cell_sel).enumerate() {
        if idx >= 16 {
            break;
        }
        let link_el = match cell.select(&title_sel).next() {
            Some(el) => el,
            None => continue,
        };
        let title = link_el.text().collect::<String>().trim().to_string();
        let game_url = link_el
            .value()
            .attr("href")
            .unwrap_or("https://itch.io")
            .to_string();
        if title.is_empty() {
            continue;
        }

        let description = cell
            .select(&text_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "Bağımsız / Indie oyun (itch.io)".to_string());

        let genre = cell
            .select(&genre_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let price_raw = cell
            .select(&price_sel)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let (is_free, price_usd, price_display) = if price_raw.is_empty() || price_raw.eq_ignore_ascii_case("free") {
            (true, Some(0.0), "Ücretsiz".to_string())
        } else {
            let digits: String = price_raw
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let val = digits.parse::<f64>().ok();
            let free = val == Some(0.0);
            (free, val, price_raw)
        };

        let image_url = cell
            .select(&img_sel)
            .next()
            .and_then(|img| {
                img.value()
                    .attr("data-lazy_src")
                    .or_else(|| img.value().attr("src"))
            })
            .map(|s| s.to_string());

        let mut platforms = Vec::new();
        if cell.select(&web_flag_sel).next().is_some() {
            platforms.push("Web".to_string());
        }
        if cell.select(&win_sel).next().is_some() {
            platforms.push("PC".to_string());
            platforms.push("Windows".to_string());
        }
        if cell.select(&mac_sel).next().is_some() {
            if !platforms.contains(&"PC".to_string()) {
                platforms.push("PC".to_string());
            }
            platforms.push("macOS".to_string());
        }
        if cell.select(&lin_sel).next().is_some() {
            if !platforms.contains(&"PC".to_string()) {
                platforms.push("PC".to_string());
            }
            platforms.push("Linux".to_string());
        }
        if cell.select(&and_sel).next().is_some() {
            platforms.push("Mobile".to_string());
            platforms.push("Android".to_string());
        }
        if platforms.is_empty() {
            platforms.push("PC".to_string());
        }

        let mut tags_vec = vec!["Indie".to_string(), "itch.io".to_string()];
        if !genre.is_empty() {
            tags_vec.push(genre);
        }
        if platforms.contains(&"Web".to_string()) {
            tags_vec.push("Tarayıcı Oyunu".to_string());
        }

        let game_id = cell
            .value()
            .attr("data-game_id")
            .map(|s| s.to_string())
            .unwrap_or_else(|| idx.to_string());

        results.push(GameResult {
            id: format!("itch-{}", game_id),
            title,
            description,
            url: game_url,
            image_url,
            store: "itch.io".to_string(),
            store_category: "itchio".to_string(),
            platforms,
            tags: tags_vec,
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free,
            price_usd,
            original_price_usd: None,
            price_display,
            discount_percent: None,
            rating: None,
            release_date: None,
            source_name: "itch.io (Indie / Kıyı Kenar)".to_string(),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}
