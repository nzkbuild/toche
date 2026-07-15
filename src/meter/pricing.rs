use std::collections::HashMap;

/// Per-token pricing in USD.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_create: f64,
}

impl Pricing {
    #[allow(dead_code)]
    pub const fn empty() -> Self {
        Self {
            input: 0.0,
            output: 0.0,
            cache_read: 0.0,
            cache_create: 0.0,
        }
    }
}

/// Embedded pricing map with exact-match + date-suffix-stripping lookup.
pub struct PricingMap {
    entries: HashMap<String, Pricing>,
}

const MODEL_DATE_SUFFIX_DIGITS: usize = 8;

/// Built-in pricing data — common Anthropic and OpenAI-compatible models.
/// Prices are per 1M tokens (standard format), converted to per-token below.
fn builtin_pricing() -> HashMap<String, Pricing> {
    let mut m = HashMap::new();

    // Anthropic models (per-token USD)
    m.insert(
        "claude-sonnet-5".into(),
        Pricing {
            input: 3.0 / 1_000_000.0,
            output: 15.0 / 1_000_000.0,
            cache_read: 0.30 / 1_000_000.0,
            cache_create: 3.75 / 1_000_000.0,
        },
    );
    m.insert(
        "claude-sonnet-4.7".into(),
        Pricing {
            input: 3.0 / 1_000_000.0,
            output: 15.0 / 1_000_000.0,
            cache_read: 0.30 / 1_000_000.0,
            cache_create: 3.75 / 1_000_000.0,
        },
    );
    m.insert(
        "claude-opus-4.8".into(),
        Pricing {
            input: 15.0 / 1_000_000.0,
            output: 75.0 / 1_000_000.0,
            cache_read: 1.50 / 1_000_000.0,
            cache_create: 18.75 / 1_000_000.0,
        },
    );
    m.insert(
        "claude-haiku-4.5".into(),
        Pricing {
            input: 1.0 / 1_000_000.0,
            output: 5.0 / 1_000_000.0,
            cache_read: 0.10 / 1_000_000.0,
            cache_create: 1.25 / 1_000_000.0,
        },
    );
    m.insert(
        "claude-fable-5".into(),
        Pricing {
            input: 3.0 / 1_000_000.0,
            output: 15.0 / 1_000_000.0,
            cache_read: 0.30 / 1_000_000.0,
            cache_create: 3.75 / 1_000_000.0,
        },
    );

    // ChatGPT-style models through proxies
    m.insert(
        "cx/gpt-5.6-sol".into(),
        Pricing {
            input: 1.10 / 1_000_000.0,
            output: 4.40 / 1_000_000.0,
            cache_read: 0.0,
            cache_create: 0.0,
        },
    );
    m.insert(
        "cx/gpt-5.6-luna".into(),
        Pricing {
            input: 0.75 / 1_000_000.0,
            output: 3.00 / 1_000_000.0,
            cache_read: 0.0,
            cache_create: 0.0,
        },
    );
    m.insert(
        "glm-5.2".into(),
        Pricing {
            input: 0.50 / 1_000_000.0,
            output: 2.00 / 1_000_000.0,
            cache_read: 0.0,
            cache_create: 0.0,
        },
    );

    m
}

impl PricingMap {
    pub fn load_embedded() -> Self {
        Self {
            entries: builtin_pricing(),
        }
    }

    /// Look up pricing for a model ID. Resolution order:
    /// 1. Exact match
    /// 2. Strip trailing date suffix (YYYYMMDD), retry exact match
    /// 3. Return None (unknown)
    pub fn find(&self, model: &str) -> Option<Pricing> {
        if let Some(p) = self.entries.get(model) {
            return Some(*p);
        }
        let stripped = strip_date_suffix(model);
        if stripped != model {
            if let Some(p) = self.entries.get(stripped) {
                return Some(*p);
            }
        }
        None
    }
}

/// If the model name ends with `-YYYYMMDD` (8 digits), strip it.
/// Returns a borrowed slice of the original string.
fn strip_date_suffix(model: &str) -> &str {
    if model.len() > MODEL_DATE_SUFFIX_DIGITS + 1 {
        let prefix_len = model.len() - MODEL_DATE_SUFFIX_DIGITS - 1;
        if model.as_bytes()[prefix_len] == b'-'
            && model[prefix_len + 1..]
                .bytes()
                .all(|b| b.is_ascii_digit())
        {
            return &model[..prefix_len];
        }
    }
    model
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let map = PricingMap::load_embedded();
        let p = map.find("claude-sonnet-5").unwrap();
        assert!(p.input > 0.0);
        assert!(p.output > 0.0);
        assert!(p.cache_read > 0.0);
        assert!(p.cache_create > 0.0);
    }

    #[test]
    fn test_date_suffix_stripping() {
        let map = PricingMap::load_embedded();
        let p = map.find("claude-sonnet-5-20251001").unwrap();
        assert!(p.input > 0.0);
        let base = map.find("claude-sonnet-5").unwrap();
        assert_eq!(p.input, base.input);
    }

    #[test]
    fn test_unknown_model_returns_none() {
        let map = PricingMap::load_embedded();
        assert!(map.find("nonexistent-model-v42").is_none());
    }

    #[test]
    fn test_strip_date_suffix_basic() {
        assert_eq!(
            strip_date_suffix("claude-sonnet-5-20251001"),
            "claude-sonnet-5"
        );
        assert_eq!(strip_date_suffix("claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(strip_date_suffix("gpt-4"), "gpt-4");
        assert_eq!(strip_date_suffix("model-123"), "model-123");
    }

    #[test]
    fn test_cx_models_have_pricing() {
        let map = PricingMap::load_embedded();
        assert!(map.find("cx/gpt-5.6-sol").is_some());
        assert!(map.find("cx/gpt-5.6-luna").is_some());
    }

    #[test]
    fn test_glm_model_has_pricing() {
        let map = PricingMap::load_embedded();
        let p = map.find("glm-5.2").unwrap();
        assert!(p.input > 0.0);
        assert!(p.output > 0.0);
    }
}
