//! Efficiency profile instruction texts.
//!
//! Concision rules adapted from `caveman-activate.md` in vendor_reuse/caveman-claude/ (MIT).
//! Careful-work rules adapted from `SKILL.md` in vendor_reuse/andrej-karpathy-skills/ (MIT).

use super::config::EfficiencyMode;

/// Concision instruction adapted from caveman `caveman-activate.md` (MIT-licensed).
pub const CONCISE_INSTRUCTION: &str = "\
Respond with concision. Drop filler words, pleasantries, and hedging.\n\
Keep all technical substance, code, errors, and diagnostics verbatim.\n\
Fragments OK where meaning stays clear.\n\
Boundaries: code, commits, and PRs written normally.\n\
Auto-clarity: normal prose for security warnings and irreversible actions.";

/// Careful-work instruction adapted from karpathy-guidelines `SKILL.md` (MIT-licensed).
pub const CAREFUL_INSTRUCTION: &str = "\
Before implementing:\n\
- State assumptions explicitly. If uncertain, ask.\n\
- If multiple interpretations exist, present them \u{2014} don't pick silently.\n\
- If a simpler approach exists, say so.\n\
\n\
Simplicity first: minimum code that solves the problem. Nothing speculative.\n\
Surgical changes: touch only what you must. Match existing style.\n\
Goal-driven: transform tasks into verifiable success criteria. Loop until verified.";

/// Map an efficiency mode to its system-prompt instruction text.
/// Returns `None` for `Normal` (no injection needed).
pub fn instruction_for_mode(mode: &EfficiencyMode) -> Option<&'static str> {
    match mode {
        EfficiencyMode::Normal => None,
        EfficiencyMode::Concise => Some(CONCISE_INSTRUCTION),
        EfficiencyMode::Careful => Some(CAREFUL_INSTRUCTION),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_returns_none() {
        assert!(instruction_for_mode(&EfficiencyMode::Normal).is_none());
    }

    #[test]
    fn concise_instruction_contains_key_terms() {
        let text = instruction_for_mode(&EfficiencyMode::Concise).unwrap();
        assert!(text.contains("concision"));
        assert!(text.contains("Fragments"));
    }

    #[test]
    fn careful_instruction_contains_key_terms() {
        let text = instruction_for_mode(&EfficiencyMode::Careful).unwrap();
        assert!(text.contains("assumptions"));
        assert!(text.contains("Surgical"));
    }
}
