use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct LanguageId(Arc<str>);

impl LanguageId {
    pub fn new(s: impl Into<Arc<str>>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LanguageId {
    fn from(s: &str) -> Self {
        Self(Arc::from(s))
    }
}

impl std::fmt::Display for LanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct LanguageInfo {
    pub id: &'static str,
    pub name: &'static str,
}

pub const ALL: &[LanguageInfo] = &[
    LanguageInfo { id: "asciidoc", name: "AsciiDoc" },
    LanguageInfo { id: "astro", name: "Astro" },
    LanguageInfo { id: "batch", name: "Batch" },
    LanguageInfo { id: "bazel", name: "Bazel" },
    LanguageInfo { id: "c", name: "C" },
    LanguageInfo { id: "clojure", name: "Clojure" },
    LanguageInfo { id: "cmake", name: "CMake" },
    LanguageInfo { id: "cpp", name: "C++" },
    LanguageInfo { id: "csharp", name: "C#" },
    LanguageInfo { id: "css", name: "CSS" },
    LanguageInfo { id: "d", name: "D" },
    LanguageInfo { id: "dart", name: "Dart" },
    LanguageInfo { id: "dockerfile", name: "Dockerfile" },
    LanguageInfo { id: "dotenv", name: "dotenv" },
    LanguageInfo { id: "editorconfig", name: "EditorConfig" },
    LanguageInfo { id: "elixir", name: "Elixir" },
    LanguageInfo { id: "elm", name: "Elm" },
    LanguageInfo { id: "erlang", name: "Erlang" },
    LanguageInfo { id: "fish", name: "Fish" },
    LanguageInfo { id: "fsharp", name: "F#" },
    LanguageInfo { id: "gdscript", name: "GDScript" },
    LanguageInfo { id: "gitignore", name: "Git Ignore" },
    LanguageInfo { id: "go", name: "Go" },
    LanguageInfo { id: "graphql", name: "GraphQL" },
    LanguageInfo { id: "groovy", name: "Groovy" },
    LanguageInfo { id: "haskell", name: "Haskell" },
    LanguageInfo { id: "hcl", name: "HCL" },
    LanguageInfo { id: "html", name: "HTML" },
    LanguageInfo { id: "ini", name: "INI" },
    LanguageInfo { id: "java", name: "Java" },
    LanguageInfo { id: "javascript", name: "JavaScript" },
    LanguageInfo { id: "javascriptreact", name: "JavaScript JSX" },
    LanguageInfo { id: "json", name: "JSON" },
    LanguageInfo { id: "json5", name: "JSON5" },
    LanguageInfo { id: "jsonc", name: "JSON with Comments" },
    LanguageInfo { id: "julia", name: "Julia" },
    LanguageInfo { id: "kotlin", name: "Kotlin" },
    LanguageInfo { id: "latex", name: "LaTeX" },
    LanguageInfo { id: "less", name: "Less" },
    LanguageInfo { id: "lua", name: "Lua" },
    LanguageInfo { id: "makefile", name: "Makefile" },
    LanguageInfo { id: "markdown", name: "Markdown" },
    LanguageInfo { id: "mdx", name: "MDX" },
    LanguageInfo { id: "nim", name: "Nim" },
    LanguageInfo { id: "nix", name: "Nix" },
    LanguageInfo { id: "ocaml", name: "OCaml" },
    LanguageInfo { id: "org", name: "Org Mode" },
    LanguageInfo { id: "perl", name: "Perl" },
    LanguageInfo { id: "php", name: "PHP" },
    LanguageInfo { id: "powershell", name: "PowerShell" },
    LanguageInfo { id: "properties", name: "Properties" },
    LanguageInfo { id: "proto", name: "Protocol Buffers" },
    LanguageInfo { id: "python", name: "Python" },
    LanguageInfo { id: "r", name: "R" },
    LanguageInfo { id: "rst", name: "reStructuredText" },
    LanguageInfo { id: "ruby", name: "Ruby" },
    LanguageInfo { id: "rust", name: "Rust" },
    LanguageInfo { id: "sass", name: "Sass" },
    LanguageInfo { id: "scala", name: "Scala" },
    LanguageInfo { id: "scss", name: "SCSS" },
    LanguageInfo { id: "shellscript", name: "Shell" },
    LanguageInfo { id: "solidity", name: "Solidity" },
    LanguageInfo { id: "sql", name: "SQL" },
    LanguageInfo { id: "stylus", name: "Stylus" },
    LanguageInfo { id: "svelte", name: "Svelte" },
    LanguageInfo { id: "swift", name: "Swift" },
    LanguageInfo { id: "terraform", name: "Terraform" },
    LanguageInfo { id: "toml", name: "TOML" },
    LanguageInfo { id: "typescript", name: "TypeScript" },
    LanguageInfo { id: "typescriptreact", name: "TypeScript JSX" },
    LanguageInfo { id: "v", name: "V" },
    LanguageInfo { id: "vb", name: "Visual Basic" },
    LanguageInfo { id: "vue", name: "Vue" },
    LanguageInfo { id: "xml", name: "XML" },
    LanguageInfo { id: "yaml", name: "YAML" },
    LanguageInfo { id: "zig", name: "Zig" },
];

pub fn info(id: &str) -> Option<&'static LanguageInfo> {
    ALL.iter().find(|l| l.id == id)
}

pub fn from_extension(ext: &str) -> Option<LanguageId> {
    let id = match ext.to_ascii_lowercase().as_str() {
        // ─── Web / JS family ──────────────────────────────────────────
        "ts" => "typescript",
        "tsx" => "typescriptreact",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascriptreact",
        "vue" => "vue",
        "svelte" => "svelte",
        "astro" => "astro",

        // ─── Systems ──────────────────────────────────────────────────
        "rs" => "rust",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "zig" => "zig",
        "d" => "d",
        "v" => "v",
        "nim" => "nim",

        // ─── JVM / .NET ───────────────────────────────────────────────
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "scala" | "sc" => "scala",
        "groovy" => "groovy",
        "cs" => "csharp",
        "fs" | "fsx" => "fsharp",
        "vb" => "vb",

        // ─── Functional ───────────────────────────────────────────────
        "hs" | "lhs" => "haskell",
        "ml" | "mli" => "ocaml",
        "elm" => "elm",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "clj" | "cljs" | "cljc" => "clojure",

        // ─── Scripting ────────────────────────────────────────────────
        "py" | "pyi" | "pyw" => "python",
        "rb" => "ruby",
        "php" => "php",
        "pl" | "pm" => "perl",
        "lua" => "lua",
        "r" => "r",
        "jl" => "julia",
        "dart" => "dart",
        "swift" => "swift",

        // ─── Shell ────────────────────────────────────────────────────
        "sh" | "bash" | "zsh" => "shellscript",
        "fish" => "fish",
        "ps1" => "powershell",
        "bat" | "cmd" => "batch",

        // ─── Markup / styles ──────────────────────────────────────────
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "sass" => "sass",
        "less" => "less",
        "styl" => "stylus",

        // ─── Data / config ────────────────────────────────────────────
        "json" => "json",
        "jsonc" => "jsonc",
        "json5" => "json5",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "xml" => "xml",
        "ini" | "cfg" | "conf" => "ini",
        "env" => "dotenv",
        "properties" => "properties",

        // ─── Docs ─────────────────────────────────────────────────────
        "md" | "markdown" => "markdown",
        "mdx" => "mdx",
        "rst" => "rst",
        "tex" | "latex" => "latex",
        "adoc" | "asciidoc" => "asciidoc",
        "org" => "org",

        // ─── IaC / build ──────────────────────────────────────────────
        "tf" | "tfvars" => "terraform",
        "hcl" => "hcl",
        "nix" => "nix",
        "dockerfile" => "dockerfile",

        // ─── Other ────────────────────────────────────────────────────
        "sql" => "sql",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        "sol" => "solidity",
        "gd" => "gdscript",

        _ => return None,
    };
    Some(LanguageId::from(id))
}

pub fn from_path(path: &Path) -> Option<LanguageId> {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let by_name = match name.to_ascii_lowercase().as_str() {
            "dockerfile" | "containerfile" => Some("dockerfile"),
            "makefile" | "gnumakefile" => Some("makefile"),
            "cmakelists.txt" => Some("cmake"),
            "build" | "build.bazel" | "workspace" | "workspace.bazel" => Some("bazel"),
            ".gitignore" | ".gitattributes" | ".gitmodules" => Some("gitignore"),
            ".env" => Some("dotenv"),
            ".editorconfig" => Some("editorconfig"),
            _ => None,
        };
        if let Some(id) = by_name {
            return Some(LanguageId::from(id));
        }
    }
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(from_extension)
}
