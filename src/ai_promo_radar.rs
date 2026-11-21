use crate::outside_lens::{clean_html, extract_attr, extract_tag, md5_like_hash, parse_date_ts};
use chrono::Utc;
use futures::future::join_all;
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize)]
pub struct AiTool {
    pub id: &'static str,
    pub name: &'static str,
    pub company: &'static str,
    pub country: &'static str,
    pub flag: &'static str,
    pub category: &'static str,
    pub platforms: &'static [&'static str],
    pub free_tier: &'static str,
    pub reset_cycle: &'static str,
    pub io_ratio: &'static str,
    pub multiplier: &'static str,
    pub burn_rate: &'static str,
    pub plain_estimate: &'static str,
    pub website: &'static str,
    pub note: &'static str,
    pub hot: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PromoArticle {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub link: String,
    pub published_at: Option<String>,
    pub published_ts: i64,
    pub source_name: String,
    pub flag: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AiPromoResponse {
    pub tools: Vec<AiTool>,
    pub articles: Vec<PromoArticle>,
    pub promo_signal_count: usize,
    pub cached: bool,
    pub updated_at: String,
}

struct CachedPromo {
    data: AiPromoResponse,
    fetched_at: Instant,
}

#[derive(Clone)]
pub struct AiPromoCache {
    inner: Arc<RwLock<Option<CachedPromo>>>,
}

impl AiPromoCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn get_or_fetch(&self, client: &Client, force_refresh: bool) -> AiPromoResponse {
        if !force_refresh {
            let guard = self.inner.read().await;
            if let Some(c) = guard.as_ref() {
                if c.fetched_at.elapsed() < Duration::from_secs(600) {
                    let mut resp = c.data.clone();
                    resp.cached = true;
                    return resp;
                }
            }
        }

        let fresh = fetch_all_promos(client).await;
        let mut guard = self.inner.write().await;
        *guard = Some(CachedPromo {
            data: fresh.clone(),
            fetched_at: Instant::now(),
        });
        fresh
    }
}

fn curated_tools() -> Vec<AiTool> {
    vec![
        AiTool {
            id: "qoder", name: "Qoder", company: "Alibaba", country: "China", flag: "🇨🇳",
            category: "Coding", platforms: &["macOS", "Windows"],
            free_tier: "Free starter & periodic promo credits", reset_cycle: "Launch Promo",
            io_ratio: "1 : 4", multiplier: "1× Standard / 2×–3× Quest", burn_rate: "Medium Burn",
            plain_estimate: "≈ 30–60 agentic coding prompts on starter credits",
            website: "https://qoder.com", note: "Agentic IDE with multi-file Quest mode and periodic free credit campaigns.", hot: true,
        },
        AiTool {
            id: "qronos", name: "Qronos AI", company: "Qronos", country: "Global", flag: "🌍",
            category: "API Gateway", platforms: &["Web", "CLI/API"],
            free_tier: "10M free promo tokens (Playground + API)", reset_cycle: "Launch Promo",
            io_ratio: "1 : 5", multiplier: "OpenAI & Anthropic compatible", burn_rate: "Low Burn",
            plain_estimate: "≈ 100 novels of chat, or 3–10 heavy agentic coding sessions",
            website: "https://qronos.me", note: "Universal endpoint (api.qronos.me/v1) + browser Playground.", hot: true,
        },
        AiTool {
            id: "trae", name: "Trae", company: "ByteDance", country: "Singapore", flag: "🇸🇬",
            category: "Coding", platforms: &["macOS", "Windows"],
            free_tier: "Free fast requests + monthly allowance", reset_cycle: "Monthly Reset",
            io_ratio: "1 : 4", multiplier: "1× Fast / Queue when depleted", burn_rate: "Medium Burn",
            plain_estimate: "≈ 50+ fast coding tasks/month + unlimited slow queue",
            website: "https://www.trae.ai", note: "AI-native VS Code fork with Builder mode.", hot: true,
        },
        AiTool {
            id: "kiro", name: "Kiro", company: "AWS", country: "USA", flag: "🇺🇸",
            category: "Coding", platforms: &["macOS", "Windows", "Linux"],
            free_tier: "Monthly free spec & vibe-coding credits", reset_cycle: "Monthly Reset",
            io_ratio: "1 : 5", multiplier: "1× Vibe / 2× Spec-Driven", burn_rate: "Medium Burn",
            plain_estimate: "≈ 25–50 spec-driven coding tasks per month",
            website: "https://kiro.dev", note: "Spec-driven agentic IDE from AWS.", hot: true,
        },
        AiTool {
            id: "google-ai-studio", name: "Google AI Studio", company: "Google", country: "USA", flag: "🇺🇸",
            category: "API Gateway", platforms: &["Web", "CLI/API"],
            free_tier: "Generous daily free Gemini API & Studio quota", reset_cycle: "Daily Reset",
            io_ratio: "1 : 4", multiplier: "Free daily RPM/RPD on Flash & Pro", burn_rate: "Low Burn",
            plain_estimate: "≈ 100–1,000+ daily prompts with 1M+ context window",
            website: "https://aistudio.google.com", note: "Free daily Gemini API requests and web playground.", hot: true,
        },
        AiTool {
            id: "openrouter", name: "OpenRouter", company: "OpenRouter", country: "USA", flag: "🇺🇸",
            category: "API Gateway", platforms: &["Web", "CLI/API"],
            free_tier: "Daily free requests on :free open models", reset_cycle: "Daily Reset",
            io_ratio: "1 : 3", multiplier: "0× on :free models (50–1,000/day)", burn_rate: "Low Burn",
            plain_estimate: "≈ 50 free requests/day (1,000/day with $10+ top-up)",
            website: "https://openrouter.ai", note: "Single API key for 200+ models including free DeepSeek, Llama, Qwen.", hot: false,
        },
        AiTool {
            id: "groq", name: "Groq Cloud", company: "Groq", country: "USA", flag: "🇺🇸",
            category: "API Gateway", platforms: &["Web", "CLI/API"],
            free_tier: "Free daily tokens (up to 500k tokens/day)", reset_cycle: "Daily Reset",
            io_ratio: "1 : 1", multiplier: "1× (ultra-fast LPU hardware)", burn_rate: "Low Burn",
            plain_estimate: "≈ 200–500 ultra-fast chats/day on Llama & Whisper",
            website: "https://console.groq.com", note: "Blazing-fast inference for open-weight models.", hot: false,
        },
        AiTool {
            id: "windsurf", name: "Windsurf", company: "Codeium", country: "USA", flag: "🇺🇸",
            category: "Coding", platforms: &["macOS", "Windows", "Linux"],
            free_tier: "Monthly free Cascade credits + Autocomplete", reset_cycle: "Monthly Reset",
            io_ratio: "1 : 5", multiplier: "0.25×–1× Fast / 2×+ Reasoning", burn_rate: "Medium Burn",
            plain_estimate: "≈ 25 prompt credits/month + free tab autocomplete",
            website: "https://windsurf.com", note: "Agentic IDE with Cascade multi-file edits.", hot: false,
        },
        AiTool {
            id: "github-copilot", name: "GitHub Copilot Free", company: "GitHub / Microsoft", country: "USA", flag: "🇺🇸",
            category: "Coding", platforms: &["macOS", "Windows", "Linux", "Web"],
            free_tier: "2,000 completions + 50 agent/chat requests / mo", reset_cycle: "Monthly Reset",
            io_ratio: "1 : 4", multiplier: "0× Base / 1×–3× Premium", burn_rate: "Low Burn",
            plain_estimate: "≈ 50 chat/agent prompts + 2,000 inline completions monthly",
            website: "https://github.com/features/copilot", note: "Built into VS Code, Xcode, and JetBrains.", hot: false,
        },
        AiTool {
            id: "mistral", name: "Mistral Le Chat & API", company: "Mistral AI", country: "France", flag: "🇫🇷",
            category: "Chat", platforms: &["Web", "iOS", "Android", "CLI/API"],
            free_tier: "Free daily Le Chat + Experiment API tier", reset_cycle: "Daily Reset",
            io_ratio: "1 : 3", multiplier: "1× (Flash Answers ~1,000 tok/s)", burn_rate: "Low Burn",
            plain_estimate: "≈ Dozens of ultra-fast chats & web searches daily",
            website: "https://chat.mistral.ai", note: "European flagship AI lab with Canvas & free Experiment API.", hot: false,
        },
        AiTool {
            id: "deepseek", name: "DeepSeek", company: "DeepSeek", country: "China", flag: "🇨🇳",
            category: "Chat", platforms: &["Web", "iOS", "Android", "CLI/API"],
            free_tier: "Free web & mobile chat (V3 + R1 Reasoning)", reset_cycle: "Free Tier",
            io_ratio: "1 : 4", multiplier: "90% off with context cache on API", burn_rate: "Low Burn",
            plain_estimate: "Unlimited normal chat on web/app; pennies/1M on API",
            website: "https://chat.deepseek.com", note: "Open-weights frontier reasoning & coding models.", hot: false,
        },
        AiTool {
            id: "poe", name: "Poe", company: "Quora", country: "USA", flag: "🇺🇸",
            category: "Chat", platforms: &["macOS", "Windows", "iOS", "Android", "Web"],
            free_tier: "3,000 free compute points every day", reset_cycle: "Daily Reset",
            io_ratio: "1 : 4", multiplier: "10–350 pts/message by model", burn_rate: "Medium Burn",
            plain_estimate: "≈ 15–100 messages/day depending on model weight",
            website: "https://poe.com", note: "Single native app to test Claude, GPT, Gemini, Flux daily.", hot: false,
        },
        AiTool {
            id: "perplexity", name: "Perplexity", company: "Perplexity AI", country: "USA", flag: "🇺🇸",
            category: "Chat", platforms: &["macOS", "Windows", "iOS", "Android", "Web"],
            free_tier: "Unlimited quick search + daily Pro searches", reset_cycle: "Daily Reset",
            io_ratio: "1 : 3", multiplier: "1 Pro Search = multi-step research", burn_rate: "Low Burn",
            plain_estimate: "≈ 3–5 deep Pro searches every 4 hours + unlimited quick",
            website: "https://www.perplexity.ai", note: "Real-time cited web research.", hot: false,
        },
        AiTool {
            id: "ideogram", name: "Ideogram", company: "Ideogram", country: "Canada", flag: "🇨🇦",
            category: "Image", platforms: &["Web", "iOS"],
            free_tier: "Free slow image generation credits", reset_cycle: "Daily Reset",
            io_ratio: "Per Image", multiplier: "1–2 credits per 4-image batch", burn_rate: "Medium Burn",
            plain_estimate: "≈ 10–20 typography-sharp images/week free",
            website: "https://ideogram.ai", note: "Best-in-class text rendering inside images & logos.", hot: false,
        },
        AiTool {
            id: "suno", name: "Suno", company: "Suno AI", country: "USA", flag: "🇺🇸",
            category: "Audio", platforms: &["Web", "iOS", "Android"],
            free_tier: "50 free credits every day (10 songs/day)", reset_cycle: "Daily Reset",
            io_ratio: "Per Track", multiplier: "5 credits per song", burn_rate: "Low Burn",
            plain_estimate: "≈ 10 full AI songs generated every single day free",
            website: "https://suno.com", note: "Refreshes every 24 hours.", hot: false,
        },
        AiTool {
            id: "kling", name: "Kling AI", company: "Kuaishou", country: "China", flag: "🇨🇳",
            category: "Video", platforms: &["Web", "iOS", "Android"],
            free_tier: "Free login video generation credits", reset_cycle: "Daily Reset",
            io_ratio: "Per Clip", multiplier: "10–35 credits per 5s clip", burn_rate: "Heavy Burn",
            plain_estimate: "≈ 3–6 short video clips per cycle on free credits",
            website: "https://klingai.com", note: "High-motion text-to-video and image-to-video studio.", hot: false,
        },
        AiTool {
            id: "ollama", name: "Ollama + MLX Local", company: "Ollama", country: "USA", flag: "🇺🇸",
            category: "Chat", platforms: &["macOS", "Windows", "Linux", "CLI/API"],
            free_tier: "100% Free Forever (runs locally)", reset_cycle: "Free Tier",
            io_ratio: "0 : 0 (Local)", multiplier: "0× — runs on your own RAM/GPU", burn_rate: "Low Burn",
            plain_estimate: "Unlimited private tokens — zero cloud cost or leaks",
            website: "https://ollama.com", note: "Run Qwen, Llama, Gemma, DeepSeek locally.", hot: false,
        },
    ]
}

struct AiFeedDef {
    name: &'static str,
    url: &'static str,
    flag: &'static str,
}

const AI_FEEDS: &[AiFeedDef] = &[
    AiFeedDef { name: "Hugging Face Blog", url: "https://huggingface.co/blog/feed.xml", flag: "🇫🇷" },
    AiFeedDef { name: "OpenAI News", url: "https://openai.com/news/rss.xml", flag: "🇺🇸" },
    AiFeedDef { name: "Google AI Blog", url: "https://blog.google/technology/ai/rss/", flag: "🇺🇸" },
    AiFeedDef { name: "Qwen Official Blog", url: "https://qwenlm.github.io/blog/index.xml", flag: "🇨🇳" },
    AiFeedDef { name: "GitHub Blog", url: "https://github.blog/feed/", flag: "🇺🇸" },
    AiFeedDef { name: "Simon Willison", url: "https://simonwillison.net/atom/everything/", flag: "🇬🇧" },
    AiFeedDef { name: "Show HN — AI Launches", url: "https://hnrss.org/show?q=AI", flag: "🌍" },
    AiFeedDef { name: "HN — Top AI", url: "https://hnrss.org/frontpage?q=AI", flag: "🌍" },
    AiFeedDef { name: "Product Hunt", url: "https://www.producthunt.com/feed", flag: "🇺🇸" },
    AiFeedDef { name: "TechCrunch AI", url: "https://techcrunch.com/category/artificial-intelligence/feed/", flag: "🇺🇸" },
    AiFeedDef { name: "The Verge AI", url: "https://www.theverge.com/rss/ai-artificial-intelligence/index.xml", flag: "🇺🇸" },
    AiFeedDef { name: "Cline Releases", url: "https://github.com/cline/cline/releases.atom", flag: "🇺🇸" },
    AiFeedDef { name: "Roo Code Releases", url: "https://github.com/RooCodeInc/Roo-Code/releases.atom", flag: "🌍" },
    AiFeedDef { name: "Ollama Releases", url: "https://github.com/ollama/ollama/releases.atom", flag: "🇺🇸" },
    AiFeedDef { name: "Lobsters AI", url: "https://lobste.rs/t/ai.rss", flag: "🌍" },
];

fn detect_promo_tags(title: &str, summary: &str) -> Vec<String> {
    let hay = format!("{} {}", title, summary).to_lowercase();
    let mut tags = Vec::new();
    if ["free credit", "daily credit", "million token", "free token"].iter().any(|k| hay.contains(k)) {
        tags.push("Free Credits".to_string());
    }
    if ["promo", "campaign", "discount", "giveaway", "student"].iter().any(|k| hay.contains(k)) {
        tags.push("Promo".to_string());
    }
    if ["free tier", "for free", "free plan", "no credit card"].iter().any(|k| hay.contains(k)) {
        tags.push("Free Tier".to_string());
    }
    if ["agent", "coding ide", "copilot", "mcp"].iter().any(|k| hay.contains(k)) {
        tags.push("AI Agent".to_string());
    }
    if ["open weight", "open-weight", "open source", "apache 2", "gguf"].iter().any(|k| hay.contains(k)) {
        tags.push("Open Weights".to_string());
    }
    if ["launch", "introducing", "announcing", "beta", "waitlist"].iter().any(|k| hay.contains(k)) {
        tags.push("Launch / Beta".to_string());
    }
    tags
}

async fn fetch_ai_feed(client: &Client, feed: &AiFeedDef) -> Vec<PromoArticle> {
    let Ok(resp) = client.get(feed.url).timeout(Duration::from_secs(8)).send().await else { return vec![]; };
    if !resp.status().is_success() { return vec![]; }
    let Ok(body) = resp.text().await else { return vec![]; };

    static ITEM_RE: OnceLock<Regex> = OnceLock::new();
    let item_re = ITEM_RE.get_or_init(|| Regex::new(r"(?is)<(item|entry)\b[^>]*>.*?</(item|entry)>").unwrap());

    let mut out = Vec::new();
    for m in item_re.find_iter(&body).take(15) {
        let block = m.as_str();
        let title = clean_html(&extract_tag(block, "title"));
        let raw_sum = {
            let d = extract_tag(block, "description");
            if !d.is_empty() { d } else { extract_tag(block, "summary") }
        };
        let summary: String = clean_html(&raw_sum).chars().take(230).collect();
        let link = {
            let l = clean_html(&extract_tag(block, "link"));
            if !l.is_empty() { l } else { extract_attr(block, "link", "href") }
        };
        if title.is_empty() || link.is_empty() { continue; }

        let date_raw = {
            let p = extract_tag(block, "pubDate");
            if !p.is_empty() { p } else {
                let u = extract_tag(block, "updated");
                if !u.is_empty() { u } else { extract_tag(block, "published") }
            }
        };
        let (published_at, published_ts) = parse_date_ts(&date_raw);
        let tags = detect_promo_tags(&title, &summary);

        out.push(PromoArticle {
            id: format!("ai-{:x}", md5_like_hash(&link)),
            title,
            summary,
            link,
            published_at,
            published_ts,
            source_name: feed.name.to_string(),
            flag: feed.flag.to_string(),
            tags,
        });
    }
    out
}

async fn fetch_all_promos(client: &Client) -> AiPromoResponse {
    let results = join_all(AI_FEEDS.iter().map(|f| fetch_ai_feed(client, f))).await;
    let mut seen = HashSet::new();
    let mut articles: Vec<PromoArticle> = results
        .into_iter()
        .flatten()
        .filter(|a| seen.insert(a.link.clone()))
        .collect();
    articles.sort_by(|a, b| b.published_ts.cmp(&a.published_ts));

    let promo_signal_count = articles.iter().filter(|a| !a.tags.is_empty()).count();

    AiPromoResponse {
        tools: curated_tools(),
        articles,
        promo_signal_count,
        cached: false,
        updated_at: Utc::now().to_rfc3339(),
    }
}
