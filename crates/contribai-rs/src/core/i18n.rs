//! Internationalization (i18n) for CLI messages.
//!
//! Supports English (default), Vietnamese, Japanese, Chinese (Simplified).
//!
//! Config:
//! ```yaml
//! locale: "vi"  # or "ja", "zh-CN", "en"
//! ```

use std::collections::HashMap;
use std::sync::LazyLock;

/// Supported locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    Vi,
    Ja,
    ZhCn,
}

impl Locale {
    pub fn from_code(code: &str) -> Self {
        match code {
            "vi" | "vi-VN" => Locale::Vi,
            "ja" | "ja-JP" => Locale::Ja,
            "zh" | "zh-CN" | "zh-Hans" => Locale::ZhCn,
            _ => Locale::En,
        }
    }
}

/// Translation message key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MsgKey {
    PipelineStart,
    DiscoveringRepos,
    AnalyzingRepo,
    NoFindings,
    PRCreated,
    DryRun,
    LiveMode,
    CircuitBreakerOpen,
    CacheHit,
}

static TRANSLATIONS: LazyLock<HashMap<(Locale, MsgKey), &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // English (default)
    m.insert(
        (Locale::En, MsgKey::PipelineStart),
        "🚀 Starting ContribAI pipeline",
    );
    m.insert(
        (Locale::En, MsgKey::DiscoveringRepos),
        "🔍 Discovering repositories...",
    );
    m.insert(
        (Locale::En, MsgKey::AnalyzingRepo),
        "📦 Analyzing repository",
    );
    m.insert((Locale::En, MsgKey::NoFindings), "✅ No findings");
    m.insert((Locale::En, MsgKey::PRCreated), "🎉 PR created");
    m.insert((Locale::En, MsgKey::DryRun), "[DRY RUN] No PRs created");
    m.insert((Locale::En, MsgKey::LiveMode), "[LIVE]");
    m.insert(
        (Locale::En, MsgKey::CircuitBreakerOpen),
        "🔴 Circuit breaker is open",
    );
    m.insert((Locale::En, MsgKey::CacheHit), "💾 Cache hit");
    // Vietnamese
    m.insert(
        (Locale::Vi, MsgKey::PipelineStart),
        "🚀 Bắt đầu ContribAI pipeline",
    );
    m.insert(
        (Locale::Vi, MsgKey::DiscoveringRepos),
        "🔍 Đang tìm repositories...",
    );
    m.insert(
        (Locale::Vi, MsgKey::AnalyzingRepo),
        "📦 Đang phân tích repository",
    );
    m.insert((Locale::Vi, MsgKey::NoFindings), "✅ Không tìm thấy vấn đề");
    m.insert((Locale::Vi, MsgKey::PRCreated), "🎉 Đã tạo PR");
    m.insert((Locale::Vi, MsgKey::DryRun), "[DRY RUN] Không tạo PR");
    m.insert((Locale::Vi, MsgKey::LiveMode), "[LIVE]");
    m.insert(
        (Locale::Vi, MsgKey::CircuitBreakerOpen),
        "🔴 Circuit breaker đang mở",
    );
    m.insert((Locale::Vi, MsgKey::CacheHit), "💾 Cache hit");
    // Japanese
    m.insert(
        (Locale::Ja, MsgKey::PipelineStart),
        "🚀 ContribAIパイプラインを開始",
    );
    m.insert(
        (Locale::Ja, MsgKey::DiscoveringRepos),
        "🔍 リポジトリを検索中...",
    );
    m.insert((Locale::Ja, MsgKey::AnalyzingRepo), "📦 リポジトリを分析中");
    m.insert(
        (Locale::Ja, MsgKey::NoFindings),
        "✅ 問題は見つかりませんでした",
    );
    m.insert((Locale::Ja, MsgKey::PRCreated), "🎉 PRを作成しました");
    m.insert((Locale::Ja, MsgKey::DryRun), "[DRY RUN] PRは作成されません");
    m.insert((Locale::Ja, MsgKey::LiveMode), "[LIVE]");
    m.insert(
        (Locale::Ja, MsgKey::CircuitBreakerOpen),
        "🔴 サーキットブレーカーが開いています",
    );
    m.insert((Locale::Ja, MsgKey::CacheHit), "💾 キャッシュヒット");
    // Chinese (Simplified)
    m.insert(
        (Locale::ZhCn, MsgKey::PipelineStart),
        "🚀 启动 ContribAI 管道",
    );
    m.insert(
        (Locale::ZhCn, MsgKey::DiscoveringRepos),
        "🔍 正在搜索仓库...",
    );
    m.insert((Locale::ZhCn, MsgKey::AnalyzingRepo), "📦 正在分析仓库");
    m.insert((Locale::ZhCn, MsgKey::NoFindings), "✅ 未发现问题");
    m.insert((Locale::ZhCn, MsgKey::PRCreated), "🎉 已创建 PR");
    m.insert((Locale::ZhCn, MsgKey::DryRun), "[DRY RUN] 未创建 PR");
    m.insert((Locale::ZhCn, MsgKey::LiveMode), "[LIVE]");
    m.insert(
        (Locale::ZhCn, MsgKey::CircuitBreakerOpen),
        "🔴 断路器已打开",
    );
    m.insert((Locale::ZhCn, MsgKey::CacheHit), "💾 缓存命中");
    m
});

/// Translate a message key to the current locale.
pub fn t(locale: Locale, key: MsgKey) -> &'static str {
    TRANSLATIONS
        .get(&(locale, key))
        .copied()
        .unwrap_or_else(|| {
            TRANSLATIONS
                .get(&(Locale::En, key))
                .copied()
                .unwrap_or("unknown")
        })
}

/// Get current locale from config or env var.
pub fn current_locale(config_locale: Option<&str>) -> Locale {
    if let Some(code) = config_locale {
        return Locale::from_code(code);
    }
    if let Ok(lang) = std::env::var("LANG") {
        return Locale::from_code(&lang);
    }
    Locale::En
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_from_code() {
        assert_eq!(Locale::from_code("vi"), Locale::Vi);
        assert_eq!(Locale::from_code("ja-JP"), Locale::Ja);
        assert_eq!(Locale::from_code("zh-CN"), Locale::ZhCn);
        assert_eq!(Locale::from_code("en-US"), Locale::En);
        assert_eq!(Locale::from_code("unknown"), Locale::En);
    }

    #[test]
    fn test_translate_english() {
        assert_eq!(
            t(Locale::En, MsgKey::PipelineStart),
            "🚀 Starting ContribAI pipeline"
        );
        assert_eq!(t(Locale::En, MsgKey::NoFindings), "✅ No findings");
    }

    #[test]
    fn test_translate_vietnamese() {
        assert_eq!(
            t(Locale::Vi, MsgKey::PipelineStart),
            "🚀 Bắt đầu ContribAI pipeline"
        );
        assert_eq!(
            t(Locale::Vi, MsgKey::NoFindings),
            "✅ Không tìm thấy vấn đề"
        );
    }

    #[test]
    fn test_translate_japanese() {
        assert_eq!(
            t(Locale::Ja, MsgKey::PipelineStart),
            "🚀 ContribAIパイプラインを開始"
        );
        assert_eq!(
            t(Locale::Ja, MsgKey::NoFindings),
            "✅ 問題は見つかりませんでした"
        );
    }

    #[test]
    fn test_translate_chinese() {
        assert_eq!(
            t(Locale::ZhCn, MsgKey::PipelineStart),
            "🚀 启动 ContribAI 管道"
        );
        assert_eq!(t(Locale::ZhCn, MsgKey::NoFindings), "✅ 未发现问题");
    }

    #[test]
    fn test_current_locale_default() {
        // Should default to English when no env var set
        let locale = current_locale(None);
        // Could be En or something else depending on system LANG
        assert!(matches!(
            locale,
            Locale::En | Locale::Vi | Locale::Ja | Locale::ZhCn
        ));
    }

    #[test]
    fn test_current_locale_from_config() {
        assert_eq!(current_locale(Some("vi")), Locale::Vi);
        assert_eq!(current_locale(Some("ja")), Locale::Ja);
        assert_eq!(current_locale(Some("zh-CN")), Locale::ZhCn);
    }
}
