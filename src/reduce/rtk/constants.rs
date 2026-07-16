/// RTK-only subcommands that should never be treated as filterable commands.
/// Used by `is_rtk_reserved_command` to skip filter matching for meta-commands.
#[allow(dead_code)] // vendored, kept for upstream sync
pub const RTK_META_COMMANDS: &[&str] = &[
    "gain",
    "discover",
    "learn",
    "init",
    "config",
    "proxy",
    "run",
    "hook",
    "hook-audit",
    "pipe",
    "cc-economics",
    "verify",
    "trust",
    "untrust",
    "session",
    "rewrite",
    "telemetry",
    "smart",
    "deps",
    "json",
];
