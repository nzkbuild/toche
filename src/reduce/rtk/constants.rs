/// RTK-only subcommands that should never be treated as filterable commands.
/// Used by `is_rtk_reserved_command` to skip filter matching for meta-commands.
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
