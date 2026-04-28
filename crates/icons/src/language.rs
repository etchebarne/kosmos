use crate::IconName;

pub(crate) fn icon_for_file_name(file_name: &str) -> Option<IconName> {
    let lower = file_name.to_ascii_lowercase();
    let icon = match lower.as_str() {
        "cargo.toml" | "cargo.lock" => IconName::LangRust,
        "bun.lock" | "bun.lockb" | "bunfig.toml" => IconName::LangBun,
        "package.json" | "package-lock.json" => IconName::LangJavascript,
        "tsconfig.json" => IconName::LangTypescript,
        "go.mod" | "go.sum" => IconName::LangGo,
        "gemfile" | "gemfile.lock" => IconName::LangRuby,
        "dockerfile" | "containerfile" | ".dockerignore" => IconName::LangDocker,
        ".gitignore" | ".gitattributes" | ".gitmodules" => IconName::LangGit,
        _ => return None,
    };
    Some(icon)
}

pub(crate) fn icon_for_language(id: &str) -> Option<IconName> {
    let icon = match id {
        "astro" => IconName::LangAstro,
        "c" => IconName::LangC,
        "cpp" => IconName::LangCpp,
        "csharp" => IconName::LangCsharp,
        "css" => IconName::LangCss,
        "dart" => IconName::LangDart,
        "dockerfile" => IconName::LangDocker,
        "dotenv" => IconName::LangDotenv,
        "gitignore" => IconName::LangGit,
        "go" => IconName::LangGo,
        "graphql" => IconName::LangGraphql,
        "haskell" => IconName::LangHaskell,
        "hcl" | "terraform" => IconName::LangTerraform,
        "html" => IconName::LangHtml,
        "java" => IconName::LangJava,
        "javascript" => IconName::LangJavascript,
        "javascriptreact" | "typescriptreact" => IconName::LangReact,
        "json" | "json5" | "jsonc" => IconName::LangJson,
        "julia" => IconName::LangJulia,
        "kotlin" => IconName::LangKotlin,
        "lua" => IconName::LangLua,
        "markdown" | "mdx" => IconName::LangMarkdown,
        "php" => IconName::LangPhp,
        "powershell" => IconName::LangPowershell,
        "python" => IconName::LangPython,
        "r" => IconName::LangR,
        "ruby" => IconName::LangRuby,
        "rust" => IconName::LangRust,
        "sass" | "scss" => IconName::LangSass,
        "scala" => IconName::LangScala,
        "shellscript" => IconName::LangBash,
        "solidity" => IconName::LangSolidity,
        "sql" => IconName::LangSql,
        "svelte" => IconName::LangSvelte,
        "swift" => IconName::LangSwift,
        "typescript" => IconName::LangTypescript,
        "vue" => IconName::LangVue,
        "zig" => IconName::LangZig,
        _ => return None,
    };
    Some(icon)
}
