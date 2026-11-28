//! Package boundaries for compile-time source and data inclusion.
use std::path::{Component, Path, PathBuf};

use syn::visit::Visit;

fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        if part == Component::ParentDir {
            out.pop();
        } else if part != Component::CurDir {
            out.push(part);
        }
    }
    out
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
}

// Resolve the two supported build-time roots. OUT_DIR contains generated Rust; its
// generator's literal inputs are checked by scanning build.rs as well.
fn operand(expr: &syn::Expr, package: &Path) -> Result<Option<PathBuf>, String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Ok(Some(PathBuf::from(s.value()))),
        syn::Expr::Macro(m) if m.mac.path.is_ident("env") => {
            let name = syn::parse2::<syn::LitStr>(m.mac.tokens.clone()).map_err(|e| e.to_string())?;
            match name.value().as_str() {
                "CARGO_MANIFEST_DIR" => Ok(Some(package.to_owned())),
                "OUT_DIR" => Ok(None),
                other => Err(format!("unsupported include root {other}")),
            }
        }
        syn::Expr::Macro(m) if m.mac.path.is_ident("concat") => {
            use syn::parse::Parser;
            let args =
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated.parse2(m.mac.tokens.clone()).map_err(|e| e.to_string())?;
            let mut text = String::new();
            for arg in args {
                let Some(part) = operand(&arg, package)? else { return Ok(None) };
                text.push_str(&part.to_string_lossy());
            }
            Ok(Some(PathBuf::from(text)))
        }
        _ => Err("unresolved include operand".into()),
    }
}

struct Scanner<'a> {
    package: &'a Path,
    file: &'a Path,
    errors: Vec<String>,
}
impl Scanner<'_> {
    fn check(&mut self, expr: &syn::Expr, line: usize) {
        let result = operand(expr, self.package).and_then(|path| {
            if let Some(path) = path {
                let base = self.file.parent().unwrap_or(self.package);
                let resolved = normal(&base.join(path));
                let resolved = resolved.canonicalize().unwrap_or(resolved);
                if !resolved.starts_with(self.package) {
                    return Err(format!("include resolves outside package: {}", resolved.display()));
                }
            }
            Ok(())
        });
        if let Err(error) = result {
            self.errors.push(format!("{}:{line}: {error}", self.file.display()));
        }
    }
    fn tokens(&mut self, tokens: proc_macro2::TokenStream) {
        use proc_macro2::TokenTree;
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (i, token) in tokens.iter().enumerate() {
            if let TokenTree::Ident(name) = token {
                if (name == "include_str" || name == "include_bytes" || name == "include")
                    && matches!(tokens.get(i + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!')
                {
                    if let Some(TokenTree::Group(group)) = tokens.get(i + 2) {
                        match syn::parse2::<syn::Expr>(group.stream()) {
                            Ok(expr) => self.check(&expr, name.span().start().line),
                            Err(e) => self.errors.push(format!("{}:{}: {e}", self.file.display(), name.span().start().line)),
                        }
                    }
                }
            }
            if let TokenTree::Group(group) = token {
                self.tokens(group.stream());
            }
        }
    }
}
impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !test_only(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        if attr.path().is_ident("path") {
            if let syn::Meta::NameValue(value) = &attr.meta {
                use syn::spanned::Spanned;
                self.check(&value.value, attr.span().start().line);
            }
        }
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("include_str") || mac.path.is_ident("include_bytes") || mac.path.is_ident("include") {
            use syn::spanned::Spanned;
            match syn::parse2::<syn::Expr>(mac.tokens.clone()) {
                Ok(expr) => self.check(&expr, mac.span().start().line),
                Err(e) => self.errors.push(e.to_string()),
            }
        } else {
            self.tokens(mac.tokens.clone());
        }
    }
}

pub(super) fn scan(package: &Path, file: &Path, source: &str) -> Result<Vec<String>, String> {
    let syntax = syn::parse_file(source).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut scanner = Scanner { package, file, errors: Vec::new() };
    scanner.visit_file(&syntax);
    Ok(scanner.errors)
}

pub fn run(root: &Path) -> Result<(), String> {
    fn walk(dir: &Path, package: Option<&Path>, errors: &mut Vec<String>) -> Result<(), String> {
        let manifest = dir.join("Cargo.toml");
        let package = if manifest.exists() {
            let text = std::fs::read_to_string(&manifest).map_err(|e| e.to_string())?;
            let value: toml::Value = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
            if let Some(info) = value.get("package") {
                if info.get("publish").and_then(toml::Value::as_bool) == Some(false) {
                    return Ok(());
                }
                Some(dir)
            } else {
                package
            }
        } else {
            package
        };
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                if !name.starts_with('.')
                    && !["target", "vendor", "solidcore", "tests", "examples", "benches", "embedded"].contains(&name.as_ref())
                    && !name.starts_with("target-")
                {
                    walk(&path, package, errors)?;
                }
            } else if path.extension().is_some_and(|e| e == "rs") && !name.contains("test") {
                if let Some(package) = package {
                    errors.extend(scan(package, &path, &std::fs::read_to_string(&path).map_err(|e| e.to_string())?)?);
                }
            }
        }
        Ok(())
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let mut errors = Vec::new();
    walk(&root, None, &mut errors)?;
    errors.sort();
    if errors.is_empty() {
        println!("package-includes: all production includes stay inside their package");
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}
