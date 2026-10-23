//! Typography belongs to Tokens, and named font families belong to fonts.rs.
//! Like the colour guard, exceptions have a per-file count and an explicit reason.

use std::path::Path;
use syn::visit::{self, Visit};
use syn::{Expr, Item, Lit};

/// These `.size()` calls size spinner geometry, not text. No text sizes are exempt.
const ALLOWED_LITERALS: &[(&str, usize, &str)] = &[
    ("dialogs.rs", 2, "preview spinners: graphic diameter, not typography"),
    ("viewport.rs", 1, "viewport status spinner: graphic diameter, not typography"),
];

fn test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| a.path().is_ident("test") || (a.path().is_ident("cfg") && a.parse_args::<syn::Path>().is_ok_and(|p| p.is_ident("test"))))
}

fn number(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Lit(e) => match &e.lit {
            Lit::Int(n) => n.base10_parse().ok(),
            Lit::Float(n) => n.base10_parse().ok(),
            _ => None,
        },
        Expr::Paren(e) => number(&e.expr),
        Expr::Group(e) => number(&e.expr),
        Expr::Unary(e) => number(&e.expr),
        _ => None,
    }
}

/// Includes literals hidden in arithmetic such as `FontId::proportional(12.0 * zoom)`.
fn has_number(expr: &Expr) -> bool {
    struct Numbers(bool);
    impl<'ast> Visit<'ast> for Numbers {
        fn visit_lit(&mut self, lit: &'ast Lit) {
            self.0 |= matches!(lit, Lit::Int(_) | Lit::Float(_));
        }
    }
    let mut numbers = Numbers(false);
    numbers.visit_expr(expr);
    numbers.0
}

fn spacing(expr: &Expr) -> bool {
    match expr {
        Expr::Field(e) => spacing(&e.base),
        Expr::MethodCall(e) => e.method == "spacing_mut" || spacing(&e.receiver),
        _ => false,
    }
}

struct Guard<'a> {
    file: &'a str,
    found: Vec<String>,
}

impl<'ast> Visit<'ast> for Guard<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        let attrs = match item {
            Item::Mod(i) => &i.attrs,
            Item::Fn(i) => &i.attrs,
            Item::Impl(i) => &i.attrs,
            Item::Const(i) => &i.attrs,
            Item::Static(i) => &i.attrs,
            _ => return visit::visit_item(self, item),
        };
        if !test_only(attrs) {
            visit::visit_item(self, item);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(p) = call.func.as_ref() {
            let names: Vec<_> = p.path.segments.iter().map(|s| s.ident.to_string()).collect();
            let last = names.last().map(String::as_str).unwrap_or_default();
            let font_id = names.iter().any(|s| s == "FontId") && matches!(last, "proportional" | "monospace" | "new");
            let weight = matches!(last, "medium" | "semibold" | "bold");
            if (font_id || weight) && call.args.first().is_some_and(has_number) {
                self.found.push(format!("{last}(numeric size)"));
            }
            // Helpers that take the text size as their fourth argument.
            if matches!(last, "ribbon_text" | "face_text") && call.args.iter().nth(3).is_some_and(has_number) {
                self.found.push(format!("{last}(numeric size)"));
            }
            if self.file != "fonts.rs" && names.iter().any(|s| s == "FontFamily") && last == "Name" {
                self.found.push("FontFamily::Name outside fonts.rs".into());
            }
            if self.file.starts_with("ui_kit/")
                && (matches!(last, "vec2" | "pos2") || (names.iter().any(|s| s == "Margin") && matches!(last, "same" | "symmetric")))
                && call.args.iter().any(|e| number(e).is_some_and(|n| n > 1.0))
            {
                self.found.push(format!("{last}(numeric form metric)"));
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "size" && call.args.first().is_some_and(has_number) {
            self.found.push(".size(numeric size)".into());
        }
        if self.file.starts_with("ui_kit/")
            && matches!(call.method.to_string().as_str(), "margin" | "inner_margin" | "outer_margin" | "add_space" | "shrink" | "corner_radius")
            && call.args.iter().any(|e| number(e).is_some_and(|n| n > 1.0))
        {
            self.found.push(format!(".{}(numeric form metric)", call.method));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        if self.file.starts_with("ui_kit/") && spacing(&assign.left) && has_number(&assign.right) {
            // Zero disables spacing along an axis; it is not a design dimension.
            if number(&assign.right) != Some(0.0) && !matches!(assign.right.as_ref(), Expr::Call(_)) {
                self.found.push("numeric ui.spacing_mut assignment".into());
            }
        }
        if let Expr::Field(field) = assign.left.as_ref()
            && matches!(&field.member, syn::Member::Named(name) if name == "extra_letter_spacing")
            && has_number(&assign.right)
        {
            self.found.push("numeric extra_letter_spacing".into());
        }
        visit::visit_expr_assign(self, assign);
    }
}

fn scan(dir: &Path, prefix: &str, failures: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        let relative = format!("{prefix}{name}");
        if path.is_dir() {
            scan(&path, &format!("{relative}/"), failures);
        } else if name.ends_with(".rs") && !name.ends_with("_tests.rs") && name != "tests.rs" {
            let src = std::fs::read_to_string(&path).unwrap();
            let ast = syn::parse_file(&src).unwrap_or_else(|e| panic!("{relative}: {e}"));
            let mut guard = Guard { file: &relative, found: Vec::new() };
            guard.visit_file(&ast);
            let allowed = ALLOWED_LITERALS.iter().find(|(f, _, _)| *f == relative).map_or(0, |(_, n, _)| *n);
            if guard.found.len() > allowed {
                failures.push(format!("{relative}: {} literal(s), at most {allowed} allowed: {:?}", guard.found.len(), guard.found));
            }
        }
    }
}

#[test]
fn font_sizes_and_dense_form_metrics_come_from_tokens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut failures = Vec::new();
    scan(&root.join("src"), "", &mut failures);
    scan(&root.join("../../assets/icons/ui/source"), "icons/", &mut failures);
    failures.sort();
    assert!(
        failures.is_empty(),
        "literals outside theme::Tokens (add a token, or a small per-file allowance with a reason):\n{}",
        failures.join("\n")
    );
}

#[test]
fn guard_catches_multiline_sizes_and_continues_after_test_modules() {
    let src = r#"
        #[cfg(test)] mod tests { fn example() { label.size(42.0); } }
        fn render() {
            label.size(
                12.0
            );
            FontId::proportional(14.0 * zoom);
            crate::fonts::semibold(13.0);
            FontFamily::Name("private-family".into());
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.add_space(7.0);
            vec2(24.0, t.space_8);
            format.extra_letter_spacing = size * 0.04;
            label.size(t.font_body);
            FontId::monospace(t.font_body * zoom);
        }
    "#;
    let ast = syn::parse_file(src).unwrap();
    let mut guard = Guard { file: "ui_kit/layout.rs", found: Vec::new() };
    guard.visit_file(&ast);
    assert_eq!(guard.found.len(), 8, "{:?}", guard.found);
}
