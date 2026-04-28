//! Embedded built-in specs. Compiled into the binary via `include_str!`.

pub struct Builtin {
    pub catalog_path: &'static str, // e.g. "trusted/security"
    pub body: &'static str,
}

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        catalog_path: "trusted/security",
        body: include_str!("../.oaudit/auditors/trusted/security.md"),
    },
    Builtin {
        catalog_path: "trusted/supply-chain",
        body: include_str!("../.oaudit/auditors/trusted/supply-chain.md"),
    },
    Builtin {
        catalog_path: "trusted/infra",
        body: include_str!("../.oaudit/auditors/trusted/infra.md"),
    },
    Builtin {
        catalog_path: "trusted/llm-security",
        body: include_str!("../.oaudit/auditors/trusted/llm-security.md"),
    },
    Builtin {
        catalog_path: "trusted/privacy",
        body: include_str!("../.oaudit/auditors/trusted/privacy.md"),
    },
    Builtin {
        catalog_path: "untrusted/security",
        body: include_str!("../.oaudit/auditors/untrusted/security.md"),
    },
    Builtin {
        catalog_path: "untrusted/supply-chain",
        body: include_str!("../.oaudit/auditors/untrusted/supply-chain.md"),
    },
    Builtin {
        catalog_path: "untrusted/infra",
        body: include_str!("../.oaudit/auditors/untrusted/infra.md"),
    },
    Builtin {
        catalog_path: "untrusted/llm-security",
        body: include_str!("../.oaudit/auditors/untrusted/llm-security.md"),
    },
    Builtin {
        catalog_path: "untrusted/privacy",
        body: include_str!("../.oaudit/auditors/untrusted/privacy.md"),
    },
];

pub fn all() -> &'static [Builtin] {
    BUILTINS
}
