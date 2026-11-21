use chrono::{DateTime, Utc};
use futures::future::join_all;
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize)]
pub struct ForeignStory {
    pub id: String,
    pub headline: String,
    pub explanation: String,
    pub link: String,
    pub published_at: Option<String>,
    pub published_ts: i64,
    pub publisher: String,
    pub country: String,
    pub flag: String,
    pub medium: String, // "Foreign Press" | "Social Media"
    pub topics: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutsideLensResponse {
    pub stories: Vec<ForeignStory>,
    pub press_count: usize,
    pub social_count: usize,
    pub blocked_tr_count: usize,
    pub cached: bool,
    pub updated_at: String,
}

struct CachedLens {
    data: OutsideLensResponse,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct OutsideLensCache {
    inner: Arc<RwLock<Option<CachedLens>>>,
}

impl OutsideLensCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get_or_fetch(&self, client: &Client, force_refresh: bool) -> OutsideLensResponse {
        if !force_refresh {
            let guard = self.inner.read().await;
            if let Some(c) = guard.as_ref() {
                if c.fetched_at.elapsed() < Duration::from_secs(300) {
                    let mut resp = c.data.clone();
                    resp.cached = true;
                    return resp;
                }
            }
        }

        let fresh = fetch_all_outside_lens(client).await;
        let mut guard = self.inner.write().await;
        *guard = Some(CachedLens {
            data: fresh.clone(),
            fetched_at: Instant::now(),
        });
        fresh
    }
}

struct RssFeedDef {
    name: &'static str,
    url: &'static str,
    country: &'static str,
    flag: &'static str,
    turkey_dedicated: bool,
}

const RSS_FEEDS: &[RssFeedDef] = &[
    RssFeedDef { name: "The Guardian — Turkey", url: "https://www.theguardian.com/world/turkey/rss", country: "UK", flag: "🇬🇧", turkey_dedicated: true },
    RssFeedDef { name: "BBC News — Europe", url: "https://feeds.bbci.co.uk/news/world/europe/rss.xml", country: "UK", flag: "🇬🇧", turkey_dedicated: false },
    RssFeedDef { name: "BBC News — Middle East", url: "https://feeds.bbci.co.uk/news/world/middle_east/rss.xml", country: "UK", flag: "🇬🇧", turkey_dedicated: false },
    RssFeedDef { name: "Middle East Eye", url: "https://www.middleeasteye.net/rss", country: "UK", flag: "🇬🇧", turkey_dedicated: false },
    RssFeedDef { name: "Deutsche Welle (DW)", url: "https://rss.dw.com/xml/rss-en-world", country: "Germany", flag: "🇩🇪", turkey_dedicated: false },
    RssFeedDef { name: "France 24", url: "https://www.france24.com/en/rss", country: "France", flag: "🇫🇷", turkey_dedicated: false },
    RssFeedDef { name: "Euronews", url: "https://www.euronews.com/rss?format=mrss&level=theme&name=news", country: "EU", flag: "🇪🇺", turkey_dedicated: false },
    RssFeedDef { name: "Politico Europe", url: "https://www.politico.eu/feed/", country: "EU", flag: "🇪🇺", turkey_dedicated: false },
    RssFeedDef { name: "New York Times — Europe", url: "https://rss.nytimes.com/services/xml/rss/nyt/Europe.xml", country: "USA", flag: "🇺🇸", turkey_dedicated: false },
    RssFeedDef { name: "NPR World", url: "https://feeds.npr.org/1004/rss.xml", country: "USA", flag: "🇺🇸", turkey_dedicated: false },
    RssFeedDef { name: "Al-Monitor", url: "https://www.al-monitor.com/rss", country: "USA", flag: "🇺🇸", turkey_dedicated: false },
    RssFeedDef { name: "Al Jazeera", url: "https://www.aljazeera.com/xml/rss/all.xml", country: "Qatar", flag: "🇶🇦", turkey_dedicated: false },
    RssFeedDef { name: "Balkan Insight (BIRN)", url: "https://balkaninsight.com/feed/", country: "Balkans", flag: "🌍", turkey_dedicated: false },
    RssFeedDef {
        name: "Global Press Wire",
        url: "https://news.google.com/rss/search?q=Turkiye+OR+(Turkey+(Ankara+OR+Istanbul+OR+Erdogan+OR+NATO+OR+lira+OR+EU+OR+Syria+OR+economy))+when:7d&hl=en-US&gl=US&ceid=US:en",
        country: "International",
        flag: "🌐",
        turkey_dedicated: false,
    },
];

const BLOCKED_DOMAINS: &[&str] = &[
    "dailysabah", "hurriyetdailynews", "hurriyet", "trtworld", "trtglobal", "trthaber",
    "aa.com.tr", "anadolu", "bianet", "duvarenglish", "gazeteduvar", "turkishminute",
    "ahvalnews", "yenisafak", "sabah.com", "milliyet", "cumhuriyet", "sozcu", "haberturk",
    "ntv.com", "cnnturk", "t24.com", "medyascope", "birgun", "evrensel", "turkiyetoday",
    "turkiyegazetesi", "ekonomim", "dunya.com", "bloomberght", "ensonhaber", "mynet",
    "iha.com", "dha.com", "karar.com", "aksam.com", "star.com.tr", "takvim.com",
    "turkishpress", "istanbulgazette",
];

const BLOCKED_PUBLISHERS: &[&str] = &[
    "daily sabah", "hürriyet", "hurriyet", "trt world", "trt global", "anadolu agency",
    "anadolu ajansı", "bianet", "duvar english", "gazete duvar", "turkish minute",
    "yeni şafak", "yeni safak", "türkiye today", "turkiye today", "ahval", "medyascope",
    "cumhuriyet", "milliyet", "sözcü", "sozcu", "habertürk", "haberturk", "cnn türk",
    "cnn turk", "bloomberg ht",
];

fn is_turkey_based(url: &str, publisher: &str) -> bool {
    let lu = url.to_lowercase();
    let lp = publisher.to_lowercase();

    if let Some(host_start) = lu.find("://").map(|i| i + 3) {
        let rest = &lu[host_start..];
        let host = rest.split('/').next().unwrap_or("");
        if host.ends_with(".tr") {
            return true;
        }
        if BLOCKED_DOMAINS.iter().any(|kw| host.contains(kw)) {
            return true;
        }
    } else if lu.contains(".tr/") {
        return true;
    }

    BLOCKED_PUBLISHERS.iter().any(|p| lp.contains(p))
}

fn is_about_turkey_country(headline: &str, summary: &str) -> bool {
    let combined = format!("{} {}", headline, summary);
    let lower = combined.to_lowercase();

    let strong = [
        "türkiye", "turkiye", "turkish", "ankara", "istanbul", "erdoğan", "erdogan",
        "bosphorus", "anatolia", "izmir", "antalya", "cappadocia", "turkish lira",
        "bayraktar", "pkk", "imamoğlu", "imamoglu", "akp", "chp", "diyarbakir",
        "gaziantep", "trabzon", "aegean", "dardanelles",
    ];
    let has_strong = strong.iter().any(|k| lower.contains(k));

    let animal_food = [
        "thanksgiving", "turkey day", "turkey trot", "roast turkey", "roasted turkey",
        "turkey recipe", "stuffing", "cranberry", "gravy", "butterball", "brine", "brining",
        "mashed potato", "leftover turkey", "turkey fryer", "deep-fry", "deep fry",
        "carve a turkey", "carving a turkey", "baste", "basting", "giblet", "drumstick",
        "turkey breast", "turkey leg", "turkey thigh", "turkey wing", "turkey dinner",
        "turkey meal", "turkey sandwich", "ground turkey", "turkey burger", "turkey bacon",
        "turkey chili", "turkey soup", "turkey stock", "turkey carcass", "turkey meat",
        "smoked turkey", "smoking a turkey", "frozen turkey", "thaw a turkey", "defrost",
        "turkey prices", "price of a turkey", "free turkey", "turkeys for families",
        "pumpkin pie", "pecan pie", "sweet potato", "green bean", "casserole", "turducken",
        "poultry", "white meat", "dark meat", "butcher", "oven", "recipe", "cookbook",
        "pardon", "pardons", "pardoned", "gobble", "wild turkey", "wild turkeys",
        "turkey vulture", "turkey hunting", "turkey hunter", "turkey season", "flock of turkeys",
        "bird flu", "avian flu", "cold turkey", "talk turkey", "jive turkey", "turkeys",
    ];
    if !has_strong && animal_food.iter().any(|k| lower.contains(k)) {
        return false;
    }
    if has_strong {
        return true;
    }

    static CAP_TURKEY_RE: OnceLock<Regex> = OnceLock::new();
    let re = CAP_TURKEY_RE.get_or_init(|| Regex::new(r"\bTurkey\b").unwrap());
    if !re.is_match(&combined) {
        return false;
    }

    let ctx = [
        "nato", "europe", "european", "eu ", "middle east", "syria", "greece", "cyprus",
        "russia", "ukraine", "israel", "gaza", "iran", "iraq", "armenia", "azerbaijan",
        "kurd", "kurdish", "president", "minister", "government", "parliament", "election",
        "opposition", "embassy", "diplomat", "sanction", "summit", "treaty", "border",
        "military", "defense", "defence", "missile", "drone", "f-16", "eurofighter",
        "economy", "inflation", "lira", "central bank", "gdp", "trade", "export", "tariff",
        "court", "arrest", "protest", "journalist", "earthquake", "refugee", "migrant",
        "airline", "flight", "tourism", "visa", "black sea", "mediterranean",
    ];
    ctx.iter().any(|k| lower.contains(k))
}

fn detect_topics(headline: &str, summary: &str) -> Vec<String> {
    let lower = format!("{} {}", headline, summary).to_lowercase();
    let mut out = Vec::new();
    if ["lira", "inflation", "central bank", "economy", "trade", "tariff", "interest rate", "imf"].iter().any(|k| lower.contains(k)) {
        out.push("Economy".to_string());
    }
    if ["nato", "eu ", "european union", "greece", "cyprus", "syria", "russia", "ukraine", "israel", "gaza", "summit", "diploma"].iter().any(|k| lower.contains(k)) {
        out.push("Diplomacy & NATO".to_string());
    }
    if ["drone", "bayraktar", "f-16", "eurofighter", "military", "defense", "defence", "missile", "army"].iter().any(|k| lower.contains(k)) {
        out.push("Defense".to_string());
    }
    if ["erdoğan", "erdogan", "parliament", "election", "opposition", "chp", "mayor", "imamoglu", "court", "protest"].iter().any(|k| lower.contains(k)) {
        out.push("Politics".to_string());
    }
    if out.is_empty() {
        out.push("Society & Culture".to_string());
    }
    out
}

pub fn clean_html(raw: &str) -> String {
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    static WS_RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());
    let ws_re = WS_RE.get_or_init(|| Regex::new(r"\s+").unwrap());

    let no_cdata = raw.replace("<![CDATA[", "").replace("]]>", "");
    let stripped = tag_re.replace_all(&no_cdata, " ");
    let decoded = stripped
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    ws_re.replace_all(&decoded, " ").trim().to_string()
}

pub fn extract_tag(block: &str, tag: &str) -> String {
    let pattern = format!(r"(?is)<{}(?:\s[^>]*)?>(.*?)</{}>", tag, tag);
    if let Ok(re) = Regex::new(&pattern) {
        if let Some(cap) = re.captures(block) {
            return cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        }
    }
    String::new()
}

pub fn extract_attr(block: &str, tag: &str, attr: &str) -> String {
    let pattern = format!(r#"(?is)<{}\b[^>]*\b{}=["']([^"']+)["']"#, tag, attr);
    if let Ok(re) = Regex::new(&pattern) {
        if let Some(cap) = re.captures(block) {
            return cap.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        }
    }
    String::new()
}

pub fn parse_date_ts(raw: &str) -> (Option<String>, i64) {
    let s = raw.trim();
    if s.is_empty() {
        return (None, 0);
    }
    if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
        let utc = dt.with_timezone(&Utc);
        return (Some(utc.to_rfc3339()), utc.timestamp());
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        let utc = dt.with_timezone(&Utc);
        return (Some(utc.to_rfc3339()), utc.timestamp());
    }
    (None, 0)
}

async fn fetch_rss_feed(client: &Client, feed: &RssFeedDef) -> (Vec<ForeignStory>, usize) {
    let Ok(resp) = client.get(feed.url).timeout(Duration::from_secs(9)).send().await else {
        return (vec![], 0);
    };
    if !resp.status().is_success() {
        return (vec![], 0);
    }
    let Ok(body) = resp.text().await else {
        return (vec![], 0);
    };

    static ITEM_RE: OnceLock<Regex> = OnceLock::new();
    let item_re = ITEM_RE.get_or_init(|| Regex::new(r"(?is)<(item|entry)\b[^>]*>.*?</(item|entry)>").unwrap());

    let mut stories = Vec::new();
    let mut blocked = 0usize;

    for m in item_re.find_iter(&body).take(40) {
        let block = m.as_str();
        let raw_title = clean_html(&extract_tag(block, "title"));
        let raw_desc = {
            let d = extract_tag(block, "description");
            if !d.is_empty() { d } else { extract_tag(block, "summary") }
        };
        let summary = clean_html(&raw_desc);
        let link = {
            let l = clean_html(&extract_tag(block, "link"));
            if !l.is_empty() { l } else { extract_attr(block, "link", "href") }
        };
        if raw_title.is_empty() || link.is_empty() {
            continue;
        }

        let source_tag = clean_html(&extract_tag(block, "source"));
        let source_url = extract_attr(block, "source", "url");
        let publisher = if source_tag.is_empty() { feed.name.to_string() } else { source_tag.clone() };

        let headline = if !source_tag.is_empty() && raw_title.ends_with(&format!(" - {}", source_tag)) {
            raw_title[..raw_title.len() - source_tag.len() - 3].to_string()
        } else {
            raw_title
        };

        if is_turkey_based(&link, &publisher) || is_turkey_based(&source_url, &publisher) {
            blocked += 1;
            continue;
        }

        if !feed.turkey_dedicated && !is_about_turkey_country(&headline, &summary) {
            continue;
        }

        let date_raw = {
            let p = extract_tag(block, "pubDate");
            if !p.is_empty() { p } else {
                let u = extract_tag(block, "updated");
                if !u.is_empty() { u } else { extract_tag(block, "published") }
            }
        };
        let (published_at, published_ts) = parse_date_ts(&date_raw);

        let explanation = if summary.is_empty() || summary == headline {
            format!("International report from {}.", publisher)
        } else {
            summary.chars().take(260).collect()
        };

        let topics = detect_topics(&headline, &explanation);
        stories.push(ForeignStory {
            id: format!("rss-{:x}", md5_like_hash(&link)),
            headline,
            explanation,
            link,
            published_at,
            published_ts,
            publisher,
            country: feed.country.to_string(),
            flag: feed.flag.to_string(),
            medium: "Foreign Press".to_string(),
            topics,
        });
    }

    (stories, blocked)
}

async fn fetch_bluesky(client: &Client, handle: &str, country: &str, flag: &str) -> (Vec<ForeignStory>, usize) {
    let url = format!("https://public.api.bsky.app/xrpc/app.bsky.feed.getAuthorFeed?actor={}&limit=30", handle);
    let Ok(resp) = client.get(&url).timeout(Duration::from_secs(9)).send().await else { return (vec![], 0); };
    let Ok(val) = resp.json::<serde_json::Value>().await else { return (vec![], 0); };

    let mut stories = Vec::new();
    let mut blocked = 0usize;

    if let Some(arr) = val.get("feed").and_then(|v| v.as_array()) {
        for item in arr {
            let Some(post) = item.get("post") else { continue };
            let text = clean_html(post.pointer("/record/text").and_then(|v| v.as_str()).unwrap_or(""));
            if text.is_empty() || !is_about_turkey_country(&text, "") {
                continue;
            }
            let author_handle = post.pointer("/author/handle").and_then(|v| v.as_str()).unwrap_or(handle);
            let display = post.pointer("/author/displayName").and_then(|v| v.as_str()).unwrap_or(author_handle);
            let uri = post.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let rkey = uri.rsplit('/').next().unwrap_or("");
            let link = format!("https://bsky.app/profile/{}/post/{}", author_handle, rkey);
            let publisher = format!("Bluesky • {}", display);

            if is_turkey_based(&link, &publisher) || author_handle.ends_with(".tr") {
                blocked += 1;
                continue;
            }

            let (published_at, published_ts) = parse_date_ts(post.pointer("/record/createdAt").and_then(|v| v.as_str()).unwrap_or(""));
            let headline: String = text.chars().take(135).collect();
            let explanation: String = text.chars().take(250).collect();
            let topics = detect_topics(&headline, &explanation);

            stories.push(ForeignStory {
                id: format!("bsky-{:x}", md5_like_hash(&link)),
                headline,
                explanation,
                link,
                published_at,
                published_ts,
                publisher,
                country: country.to_string(),
                flag: flag.to_string(),
                medium: "Social Media".to_string(),
                topics,
            });
        }
    }
    (stories, blocked)
}

async fn fetch_mastodon(client: &Client) -> (Vec<ForeignStory>, usize) {
    let url = "https://mastodon.social/api/v1/timelines/tag/Turkey?limit=25";
    let Ok(resp) = client.get(url).timeout(Duration::from_secs(9)).send().await else { return (vec![], 0); };
    let Ok(arr) = resp.json::<Vec<serde_json::Value>>().await else { return (vec![], 0); };

    let mut stories = Vec::new();
    let mut blocked = 0usize;

    for st in arr {
        if st.get("language").and_then(|v| v.as_str()).unwrap_or("").eq_ignore_ascii_case("tr") {
            blocked += 1;
            continue;
        }
        let text = clean_html(st.get("content").and_then(|v| v.as_str()).unwrap_or(""));
        if text.len() < 25 || !is_about_turkey_country(&text, "") {
            continue;
        }
        let link = st.get("url").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let acct = st.pointer("/account/acct").and_then(|v| v.as_str()).unwrap_or("mastodon");
        let name = st.pointer("/account/display_name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or(acct);
        let publisher = format!("Mastodon • {}", name);

        if is_turkey_based(&link, &publisher) || acct.ends_with(".tr") {
            blocked += 1;
            continue;
        }

        let (published_at, published_ts) = parse_date_ts(st.get("created_at").and_then(|v| v.as_str()).unwrap_or(""));
        let headline: String = text.chars().take(130).collect();
        let explanation: String = text.chars().take(250).collect();
        let topics = detect_topics(&headline, &explanation);

        stories.push(ForeignStory {
            id: format!("masto-{:x}", md5_like_hash(&link)),
            headline,
            explanation,
            link,
            published_at,
            published_ts,
            publisher,
            country: "International".to_string(),
            flag: "🌐".to_string(),
            medium: "Social Media".to_string(),
            topics,
        });
    }
    (stories, blocked)
}

async fn fetch_lemmy(client: &Client) -> (Vec<ForeignStory>, usize) {
    let url = "https://lemmy.world/api/v3/search?q=Turkey&type_=Posts&sort=New&limit=20";
    let Ok(resp) = client.get(url).timeout(Duration::from_secs(9)).send().await else { return (vec![], 0); };
    let Ok(val) = resp.json::<serde_json::Value>().await else { return (vec![], 0); };

    let mut stories = Vec::new();
    let mut blocked = 0usize;

    if let Some(arr) = val.get("posts").and_then(|v| v.as_array()) {
        for pv in arr {
            let headline = clean_html(pv.pointer("/post/name").and_then(|v| v.as_str()).unwrap_or(""));
            let body = clean_html(pv.pointer("/post/body").and_then(|v| v.as_str()).unwrap_or(""));
            if headline.is_empty() || !is_about_turkey_country(&headline, &body) {
                continue;
            }
            let link = pv.pointer("/post/url").and_then(|v| v.as_str())
                .or_else(|| pv.pointer("/post/ap_id").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            let comm = pv.pointer("/community/name").and_then(|v| v.as_str()).unwrap_or("world");
            let publisher = format!("Lemmy • c/{}", comm);

            if is_turkey_based(&link, &publisher) {
                blocked += 1;
                continue;
            }

            let explanation = if body.is_empty() {
                format!("Community discussion on Lemmy (c/{}).", comm)
            } else {
                body.chars().take(240).collect()
            };
            let (published_at, published_ts) = parse_date_ts(pv.pointer("/post/published").and_then(|v| v.as_str()).unwrap_or(""));
            let topics = detect_topics(&headline, &explanation);

            stories.push(ForeignStory {
                id: format!("lemmy-{:x}", md5_like_hash(&link)),
                headline,
                explanation,
                link,
                published_at,
                published_ts,
                publisher,
                country: "International".to_string(),
                flag: "🌐".to_string(),
                medium: "Social Media".to_string(),
                topics,
            });
        }
    }
    (stories, blocked)
}

async fn fetch_hn(client: &Client) -> (Vec<ForeignStory>, usize) {
    let url = "https://hn.algolia.com/api/v1/search_by_date?query=Turkey&tags=story&hitsPerPage=20";
    let Ok(resp) = client.get(url).timeout(Duration::from_secs(9)).send().await else { return (vec![], 0); };
    let Ok(val) = resp.json::<serde_json::Value>().await else { return (vec![], 0); };

    let mut stories = Vec::new();
    let mut blocked = 0usize;

    if let Some(arr) = val.get("hits").and_then(|v| v.as_array()) {
        for hit in arr {
            let headline = clean_html(hit.get("title").and_then(|v| v.as_str()).unwrap_or(""));
            if headline.is_empty() || !is_about_turkey_country(&headline, "") {
                continue;
            }
            let obj_id = hit.get("objectID").and_then(|v| v.as_str()).unwrap_or("");
            let link = hit.get("url").and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", obj_id));
            let publisher = "Hacker News".to_string();

            if is_turkey_based(&link, &publisher) {
                blocked += 1;
                continue;
            }

            let pts = hit.get("points").and_then(|v| v.as_i64()).unwrap_or(0);
            let cmts = hit.get("num_comments").and_then(|v| v.as_i64()).unwrap_or(0);
            let explanation = format!("Global discussion on Hacker News ({} points, {} comments).", pts, cmts);
            let (published_at, published_ts) = parse_date_ts(hit.get("created_at").and_then(|v| v.as_str()).unwrap_or(""));
            let topics = detect_topics(&headline, &explanation);

            stories.push(ForeignStory {
                id: format!("hn-{}", obj_id),
                headline,
                explanation,
                link,
                published_at,
                published_ts,
                publisher,
                country: "USA".to_string(),
                flag: "🇺🇸".to_string(),
                medium: "Social Media".to_string(),
                topics,
            });
        }
    }
    (stories, blocked)
}

pub fn md5_like_hash(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    h
}

async fn fetch_all_outside_lens(client: &Client) -> OutsideLensResponse {
    let rss_futs = RSS_FEEDS.iter().map(|f| fetch_rss_feed(client, f));
    let (rss_results, b1, b2, b3, masto, lemmy, hn) = tokio::join!(
        join_all(rss_futs),
        fetch_bluesky(client, "reuters.com", "UK", "🇬🇧"),
        fetch_bluesky(client, "theguardian.com", "UK", "🇬🇧"),
        fetch_bluesky(client, "france24.com", "France", "🇫🇷"),
        fetch_mastodon(client),
        fetch_lemmy(client),
        fetch_hn(client)
    );

    let mut all = Vec::new();
    let mut blocked = 0usize;

    for (list, b) in rss_results {
        all.extend(list);
        blocked += b;
    }
    for (list, b) in [b1, b2, b3, masto, lemmy, hn] {
        all.extend(list);
        blocked += b;
    }

    let mut seen_links = HashSet::new();
    let mut seen_titles = HashSet::new();
    let mut deduped: Vec<ForeignStory> = all
        .into_iter()
        .filter(|s| seen_links.insert(s.link.clone()) && seen_titles.insert(s.headline.to_lowercase()))
        .collect();

    deduped.sort_by(|a, b| b.published_ts.cmp(&a.published_ts));

    let press_count = deduped.iter().filter(|s| s.medium == "Foreign Press").count();
    let social_count = deduped.iter().filter(|s| s.medium == "Social Media").count();

    OutsideLensResponse {
        stories: deduped,
        press_count,
        social_count,
        blocked_tr_count: blocked,
        cached: false,
        updated_at: Utc::now().to_rfc3339(),
    }
}
