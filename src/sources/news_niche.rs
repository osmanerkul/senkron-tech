use crate::models::{compute_lang_flags, GameResult};
use futures::future::join_all;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;

struct BuiltinFeed {
    name: &'static str,
    url: &'static str,
    default_tags: &'static [&'static str],
}

const BUILTIN_FEEDS: &[BuiltinFeed] = &[
    BuiltinFeed {
        name: "Alpha Beta Gamer",
        url: "https://www.alphabetagamer.com/feed/",
        default_tags: &["Kıyı Kenar", "Indie", "Beta / Ücretsiz"],
    },
    BuiltinFeed {
        name: "Gematsu",
        url: "https://www.gematsu.com/feed",
        default_tags: &["Haber", "Japonya / Indie", "Lokalizasyon"],
    },
    BuiltinFeed {
        name: "Rock Paper Shotgun",
        url: "https://www.rockpapershotgun.com/feed",
        default_tags: &["Haber", "PC", "Keşif"],
    },
    BuiltinFeed {
        name: "PC Gamer",
        url: "https://www.pcgamer.com/rss/",
        default_tags: &["Haber", "PC"],
    },
    BuiltinFeed {
        name: "Siliconera",
        url: "https://www.siliconera.com/feed/",
        default_tags: &["Haber", "JRPG / Indie", "Lokalizasyon"],
    },
    BuiltinFeed {
        name: "GamingOnLinux",
        url: "https://www.gamingonlinux.com/article_rss.php",
        default_tags: &["Haber", "Linux / Steam Deck", "Indie"],
    },
    BuiltinFeed {
        name: "PocketGamer",
        url: "https://www.pocketgamer.com/rss/",
        default_tags: &["Haber", "Mobil", "iOS / Android"],
    },
];

pub async fn search_news_and_niche(
    client: &Client,
    query: &str,
    tags: &[String],
    custom_feeds: &[String],
) -> Result<Vec<GameResult>, String> {
    let q_lower = query.trim().to_lowercase();
    let tag_lowers: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();

    let mut feed_tasks = Vec::new();

    for feed in BUILTIN_FEEDS {
        let client = client.clone();
        let q = q_lower.clone();
        let tl = tag_lowers.clone();
        feed_tasks.push(tokio::spawn(async move {
            scan_rss_or_html_feed(&client, feed.name, feed.url, feed.default_tags, &q, &tl).await
        }));
    }

    for custom_url in custom_feeds {
        let trimmed = custom_url.trim().to_string();
        if trimmed.is_empty() || !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            continue;
        }
        let client = client.clone();
        let q = q_lower.clone();
        let tl = tag_lowers.clone();
        feed_tasks.push(tokio::spawn(async move {
            let host = trimmed
                .split("://")
                .nth(1)
                .and_then(|s| s.split('/').next())
                .unwrap_or("Özel Kaynak")
                .to_string();
            let label = format!("Özel: {}", host);
            scan_rss_or_html_feed(&client, &label, &trimmed, &["Özel Kaynak", "Haber"], &q, &tl).await
        }));
    }

    // Also query Reddit r/indiegaming & r/WebGames for fresh niche/indie/web games
    let reddit_client = client.clone();
    let reddit_q = query.trim().to_string();
    let reddit_tags = tags.to_vec();
    let reddit_handle = tokio::spawn(async move {
        search_reddit_indie(&reddit_client, &reddit_q, &reddit_tags).await
    });

    let mut all_results = Vec::new();
    for res in join_all(feed_tasks).await {
        if let Ok(Ok(mut items)) = res {
            all_results.append(&mut items);
        }
    }
    if let Ok(Ok(mut reddit_items)) = reddit_handle.await {
        all_results.append(&mut reddit_items);
    }

    Ok(all_results)
}

async fn scan_rss_or_html_feed(
    client: &Client,
    source_name: &str,
    url: &str,
    default_tags: &[&str],
    q_lower: &str,
    tag_lowers: &[String],
) -> Result<Vec<GameResult>, String> {
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(7))
        .send()
        .await
        .map_err(|e| format!("{} hata: {}", source_name, e))?;

    if !resp.status().is_success() {
        return Err(format!("{} HTTP {}", source_name, resp.status()));
    }

    let text = resp.text().await.map_err(|e| e.to_string())?;
    let mut results = Vec::new();

    // Detect RSS/Atom XML vs plain HTML news page
    let is_xml = text.contains("<item") || text.contains("<entry");

    if is_xml {
        let items = parse_rss_items(&text);
        for (idx, item) in items.into_iter().enumerate() {
            let hay = format!(
                "{} {} {}",
                item.title.to_lowercase(),
                item.description.to_lowercase(),
                item.categories.join(" ").to_lowercase()
            );

            if !q_lower.is_empty() && !matches_query(&hay, q_lower) {
                continue;
            }
            if !tag_lowers.is_empty()
                && !tag_lowers.iter().any(|t| hay.contains(t))
                && !q_lower.is_empty()
            {
                continue;
            }

            let mut platforms = detect_platforms(&hay);
            if platforms.is_empty() {
                platforms.push("PC".to_string());
            }

            let languages = detect_languages(&hay);
            let (has_english, has_turkish) = compute_lang_flags(&languages);

            let is_free = hay.contains("free")
                || hay.contains("freeware")
                || hay.contains("demo")
                || hay.contains("open beta")
                || hay.contains("ücretsiz")
                || source_name.contains("Alpha Beta Gamer");

            let mut item_tags: Vec<String> = default_tags.iter().map(|s| s.to_string()).collect();
            for cat in item.categories.into_iter().take(4) {
                if !item_tags.iter().any(|x| x.eq_ignore_ascii_case(&cat)) {
                    item_tags.push(cat);
                }
            }

            let (store_label, store_cat) = detect_store_from_text(&hay, source_name);

            results.push(GameResult {
                id: format!("news-{}-{}", slugify(source_name), idx),
                title: item.title,
                description: item.description.chars().take(260).collect(),
                url: item.link,
                image_url: item.image,
                store: store_label,
                store_category: store_cat,
                platforms,
                tags: item_tags,
                languages,
                has_english,
                has_turkish,
                is_free,
                price_usd: if is_free { Some(0.0) } else { None },
                original_price_usd: None,
                price_display: if is_free {
                    "Ücretsiz / Demo / Beta".to_string()
                } else {
                    "Haber / Kaynakta Gör".to_string()
                },
                discount_percent: None,
                rating: None,
                release_date: item.pub_date,
                source_name: source_name.to_string(),
                is_niche_or_news: true,
                relevance_score: 0.0,
            });

            if results.len() >= 10 {
                break;
            }
        }
    } else {
        // Fallback HTML scraper for custom news websites without RSS!
        let doc = Html::parse_document(&text);
        let link_sel = Selector::parse("article a[href], h2 a[href], h3 a[href]").unwrap();
        let mut seen = std::collections::HashSet::new();

        for (idx, el) in doc.select(&link_sel).enumerate() {
            let title = el.text().collect::<String>().trim().to_string();
            let href = el.value().attr("href").unwrap_or("").trim().to_string();
            if title.len() < 8 || href.is_empty() || !seen.insert(title.clone()) {
                continue;
            }
            let full_url = if href.starts_with("http") {
                href
            } else {
                let base = url.trim_end_matches('/');
                format!("{}/{}", base, href.trim_start_matches('/'))
            };
            let hay = title.to_lowercase();
            if !q_lower.is_empty() && !matches_query(&hay, q_lower) {
                continue;
            }
            results.push(GameResult {
                id: format!("html-{}-{}", slugify(source_name), idx),
                title,
                description: format!("{} web sayfasından tarandı", source_name),
                url: full_url,
                image_url: None,
                store: source_name.to_string(),
                store_category: "news".to_string(),
                platforms: vec!["PC".to_string(), "Web".to_string()],
                tags: default_tags.iter().map(|s| s.to_string()).collect(),
                languages: vec!["EN".to_string()],
                has_english: Some(true),
                has_turkish: None,
                is_free: false,
                price_usd: None,
                original_price_usd: None,
                price_display: "Sitede Gör".to_string(),
                discount_percent: None,
                rating: None,
                release_date: None,
                source_name: source_name.to_string(),
                is_niche_or_news: true,
                relevance_score: 0.0,
            });
            if results.len() >= 8 {
                break;
            }
        }
    }

    Ok(results)
}

async fn search_reddit_indie(
    client: &Client,
    query: &str,
    tags: &[String],
) -> Result<Vec<GameResult>, String> {
    let term = if !query.trim().is_empty() {
        query.trim().to_string()
    } else if !tags.is_empty() {
        tags.join(" ")
    } else {
        String::new()
    };

    let url = if term.is_empty() {
        "https://www.reddit.com/r/indiegaming+WebGames/hot.json?limit=12".to_string()
    } else {
        format!(
            "https://www.reddit.com/r/indiegaming+WebGames/search.json?q={}&restrict_sr=1&sort=relevance&limit=12",
            urlencoding::encode(&term)
        )
    };

    let resp = client
        .get(&url)
        .header("User-Agent", "OyunBulucuRust/0.1 (Game Discovery Tool)")
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| format!("Reddit hata: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Reddit HTTP {}", resp.status()));
    }

    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    let children = json
        .pointer("/data/children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut results = Vec::new();
    for child in children {
        let data = match child.get("data") {
            Some(d) => d,
            None => continue,
        };
        if data.get("stickied").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let title = data.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if title.is_empty() {
            continue;
        }
        let sub = data.get("subreddit").and_then(|v| v.as_str()).unwrap_or("indiegaming");
        let permalink = data.get("permalink").and_then(|v| v.as_str()).unwrap_or("");
        let ext_url = data.get("url").and_then(|v| v.as_str()).unwrap_or("");
        let final_url = if ext_url.starts_with("http") && !ext_url.contains("redd.it") {
            ext_url.to_string()
        } else {
            format!("https://www.reddit.com{}", permalink)
        };
        let selftext = data.get("selftext").and_then(|v| v.as_str()).unwrap_or("");
        let thumb = data
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .filter(|s| s.starts_with("http"))
            .map(|s| s.to_string());

        let hay = format!("{} {} {}", title.to_lowercase(), selftext.to_lowercase(), ext_url.to_lowercase());
        let mut platforms = detect_platforms(&hay);
        if sub.eq_ignore_ascii_case("WebGames") && !platforms.contains(&"Web".to_string()) {
            platforms.push("Web".to_string());
        }
        if platforms.is_empty() {
            platforms.push("PC".to_string());
        }

        let is_free = sub.eq_ignore_ascii_case("WebGames") || hay.contains("free") || hay.contains("demo");
        let (store_label, store_cat) = detect_store_from_text(&hay, &format!("Reddit r/{}", sub));

        let desc = if !selftext.is_empty() {
            selftext.chars().take(220).collect()
        } else {
            format!("Reddit r/{} topluluğunda paylaşılan bağımsız oyun", sub)
        };

        results.push(GameResult {
            id: format!("reddit-{}", data.get("id").and_then(|v| v.as_str()).unwrap_or("0")),
            title,
            description: desc,
            url: final_url,
            image_url: thumb,
            store: store_label,
            store_category: store_cat,
            platforms,
            tags: vec!["Kıyı Kenar".to_string(), "Indie".to_string(), format!("r/{}", sub)],
            languages: vec!["EN".to_string()],
            has_english: Some(true),
            has_turkish: None,
            is_free,
            price_usd: if is_free { Some(0.0) } else { None },
            original_price_usd: None,
            price_display: if is_free {
                "Ücretsiz / Tarayıcı".to_string()
            } else {
                "Topluluk Paylaşımı".to_string()
            },
            discount_percent: None,
            rating: None,
            release_date: None,
            source_name: format!("Reddit r/{}", sub),
            is_niche_or_news: true,
            relevance_score: 0.0,
        });
    }

    Ok(results)
}

struct RssItem {
    title: String,
    link: String,
    description: String,
    pub_date: Option<String>,
    image: Option<String>,
    categories: Vec<String>,
}

fn parse_rss_items(xml: &str) -> Vec<RssItem> {
    let item_re = Regex::new(r"(?s)<(?:item|entry)\b[^>]*>(.*?)</(?:item|entry)>").unwrap();
    let title_re = Regex::new(r"(?s)<title[^>]*>(.*?)</title>").unwrap();
    let link_re = Regex::new(r"(?s)<link[^>]*>(.*?)</link>").unwrap();
    let link_href_re = Regex::new(r#"<link[^>]*href=["']([^"']+)["']"#).unwrap();
    let desc_re = Regex::new(r"(?s)<(?:description|summary|content:encoded)[^>]*>(.*?)</(?:description|summary|content:encoded)>").unwrap();
    let date_re = Regex::new(r"(?s)<(?:pubDate|published|updated)[^>]*>(.*?)</(?:pubDate|published|updated)>").unwrap();
    let cat_re = Regex::new(r"(?s)<category[^>]*>(.*?)</category>").unwrap();
    let img_re = Regex::new(r#"<img[^>]+src=["']([^"']+)["']"#).unwrap();
    let media_re = Regex::new(r#"<(?:media:content|media:thumbnail|enclosure)[^>]+url=["']([^"']+)["']"#).unwrap();

    let mut out = Vec::new();
    for cap in item_re.captures_iter(xml) {
        let block = &cap[1];
        let title = title_re
            .captures(block)
            .map(|c| clean_xml_text(&c[1]))
            .unwrap_or_default();
        if title.is_empty() {
            continue;
        }
        let link = link_re
            .captures(block)
            .map(|c| clean_xml_text(&c[1]))
            .filter(|s| s.starts_with("http"))
            .or_else(|| link_href_re.captures(block).map(|c| c[1].to_string()))
            .unwrap_or_default();
        if link.is_empty() {
            continue;
        }
        let raw_desc = desc_re
            .captures(block)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let image = media_re
            .captures(block)
            .map(|c| c[1].to_string())
            .or_else(|| img_re.captures(&raw_desc).map(|c| c[1].to_string()));
        let description = clean_xml_text(&raw_desc);
        let pub_date = date_re
            .captures(block)
            .map(|c| clean_xml_text(&c[1]).chars().take(16).collect());
        let categories = cat_re
            .captures_iter(block)
            .map(|c| clean_xml_text(&c[1]))
            .filter(|s| !s.is_empty())
            .collect();

        out.push(RssItem {
            title,
            link,
            description,
            pub_date,
            image,
            categories,
        });
    }
    out
}

fn clean_xml_text(raw: &str) -> String {
    let no_cdata = raw
        .trim()
        .trim_start_matches("<![CDATA[")
        .trim_end_matches("]]>");
    // Strip HTML tags
    let mut out = String::with_capacity(no_cdata.len());
    let mut inside = false;
    for ch in no_cdata.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#8217;", "'")
        .replace("&#8216;", "'")
        .replace("&#8220;", "\"")
        .replace("&#8221;", "\"")
        .replace("&#038;", "&")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn matches_query(hay: &str, q_lower: &str) -> bool {
    if hay.contains(q_lower) {
        return true;
    }
    let words: Vec<&str> = q_lower.split_whitespace().filter(|w| w.len() >= 2).collect();
    !words.is_empty() && words.iter().all(|w| hay.contains(w))
}

fn detect_platforms(hay: &str) -> Vec<String> {
    let mut p = Vec::new();
    if hay.contains("pc") || hay.contains("steam") || hay.contains("windows") || hay.contains("gog") {
        p.push("PC".to_string());
    }
    if hay.contains("browser") || hay.contains("web game") || hay.contains("html5") || hay.contains("play in browser") {
        p.push("Web".to_string());
    }
    if hay.contains("android") || hay.contains("google play") {
        p.push("Mobile".to_string());
        p.push("Android".to_string());
    }
    if hay.contains("ios") || hay.contains("iphone") || hay.contains("ipad") || hay.contains("app store") {
        if !p.contains(&"Mobile".to_string()) {
            p.push("Mobile".to_string());
        }
        p.push("iOS".to_string());
    }
    if hay.contains("ps5") || hay.contains("ps4") || hay.contains("playstation") {
        p.push("Konsol".to_string());
        p.push("PlayStation".to_string());
    }
    if hay.contains("xbox") {
        if !p.contains(&"Konsol".to_string()) {
            p.push("Konsol".to_string());
        }
        p.push("Xbox".to_string());
    }
    if hay.contains("switch") || hay.contains("nintendo") {
        if !p.contains(&"Konsol".to_string()) {
            p.push("Konsol".to_string());
        }
        p.push("Switch".to_string());
    }
    p
}

fn detect_languages(hay: &str) -> Vec<String> {
    let mut langs = Vec::new();
    if hay.contains("japanese only") || hay.contains("japan-only") || hay.contains("no english") {
        langs.push("JA".to_string());
        return langs;
    }
    // English news sites usually cover English-available or English-announced games
    langs.push("EN".to_string());
    if hay.contains("turkish") || hay.contains("türkçe") {
        langs.push("TR".to_string());
    }
    if hay.contains("japanese") {
        langs.push("JA".to_string());
    }
    langs
}

fn detect_store_from_text(hay: &str, fallback_source: &str) -> (String, String) {
    if hay.contains("itch.io") {
        ("itch.io".to_string(), "itchio".to_string())
    } else if hay.contains("store.steampowered.com") {
        ("Steam".to_string(), "steam".to_string())
    } else if hay.contains("epicgames.com") {
        ("Epic Games".to_string(), "epic".to_string())
    } else if hay.contains("gog.com") {
        ("GOG".to_string(), "gog".to_string())
    } else if hay.contains("play.google.com") {
        ("Google Play".to_string(), "googleplay".to_string())
    } else if hay.contains("apps.apple.com") {
        ("App Store".to_string(), "appstore".to_string())
    } else {
        (fallback_source.to_string(), "news".to_string())
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect()
}
