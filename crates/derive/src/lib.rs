//! `#[derive(Walk)]`: the walks over a feature (`crate::walk::Walk`), generated from the fields of
//! a struct or an enum's variants. Each walk visits the fields in declaration order, and comes in
//! `&` and `&mut`:
//!
//! - expressions (`walk_exprs`): each with its label, kind and role; the inputs' order names their
//!   parameters (`dN`);
//! - references (`walk_refs`): sketches by id, features and bodies by name;
//! - geometry (`walk_geometry`): points with their roles, planes, construction references,
//!   placement points and profile selections.
//!
//! What a field is, is written once, on the field, with `#[walk(...)]`:
//!
//! - no attribute: the field's own type walks itself (`impl Walk`): a reference (`SketchId`,
//!   `BodyName`, `FeatureName`), a point with its role (`FacePick`, `Position`, `Dir`), a plane, a
//!   nested struct, a list of them, or nothing for names, flags and numbers. An `Expr` or a bare
//!   `Vec3` has no walk, so an expression or a point that does not say what it is does not
//!   compile;
//! - `length = label`, `angle = label`, `unitless = label`: an expression input (an `Expr`, an
//!   optional one, a list or an array of them, or `(f64, Expr)` pairs). The label is any
//!   expression: a `&'static str`, an array of them (one per element), or a closure
//!   `|i: usize, n: usize| -> &'static str`; it may read the other fields;
//! - `pinned`: a face pick that stays where it is when the feature moves;
//!   `pick_if = cond`: a face pick when `cond` holds, else a position;
//! - `placement`: a primitive's placement point, or an optional one (its X, Y and Z expressions
//!   are not inputs);
//! - `direction`: a primitive's axis as unitless expressions (a `DirExpr`; its X, Y and Z are
//!   not inputs either), which turns with the feature but never moves;
//! - `profiles_of = sketch_field`, optionally with `when = cond`: a profile selection;
//! - `after = field`: visit this field right after `field` instead of in declaration order (where
//!   the parameter names already given need it);
//! - `picks_faces`, on a nested value: its positions pick faces here (a `FaceAt` that names the
//!   face it is on).
//!
//! Conditions and labels are worked out before the walk starts. The generated code names
//! `crate::walk`, so the derive is for `solidcraft-doc`.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as Tokens};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Expr, Fields, Ident, Member, parse_macro_input};

#[proc_macro_derive(Walk, attributes(walk))]
pub fn derive_walk(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(t) => t.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// A field's role in the geometry walk.
enum Geo {
    Points(&'static str),
    PickIf(Expr),
    Placement,
    Direction,
    Profiles { sketch: Ident, when: Option<Expr> },
}

/// What a field is, from its `#[walk(...)]` attributes.
#[derive(Default)]
struct Roles {
    geo: Option<Geo>,
    /// The kind (`Length`, `Angle`, `Unitless`) and the label.
    expr: Option<(&'static str, Expr)>,
    after: Option<Ident>,
    /// The positions inside this (nested) value pick faces here.
    picks_faces: bool,
}

impl Roles {
    /// No role given: the field's type walks itself.
    fn nested(&self) -> bool {
        self.geo.is_none() && self.expr.is_none()
    }
}

fn roles(field: &syn::Field) -> syn::Result<Roles> {
    let mut r = Roles::default();
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("walk")) {
        let mut sketch = None;
        let mut when = None;
        attr.parse_nested_meta(|meta| {
            let name = meta.path.get_ident().map(Ident::to_string).unwrap_or_default();
            match name.as_str() {
                "pinned" => r.geo = Some(Geo::Points("Pinned")),
                "placement" => r.geo = Some(Geo::Placement),
                "direction" => r.geo = Some(Geo::Direction),
                "pick_if" => r.geo = Some(Geo::PickIf(meta.value()?.parse()?)),
                "profiles_of" => sketch = Some(meta.value()?.parse::<Ident>()?),
                "when" => when = Some(meta.value()?.parse::<Expr>()?),
                "length" => r.expr = Some(("Length", meta.value()?.parse()?)),
                "angle" => r.expr = Some(("Angle", meta.value()?.parse()?)),
                "unitless" => r.expr = Some(("Unitless", meta.value()?.parse()?)),
                "after" => r.after = Some(meta.value()?.parse()?),
                "picks_faces" => r.picks_faces = true,
                _ => return Err(meta.error("unknown walk role")),
            }
            Ok(())
        })?;
        if let Some(sketch) = sketch {
            r.geo = Some(Geo::Profiles { sketch, when });
        } else if when.is_some() {
            return Err(syn::Error::new_spanned(attr, "`when` goes with `profiles_of`"));
        }
    }
    Ok(r)
}

#[derive(Clone, Copy, PartialEq)]
enum Which {
    Exprs,
    Refs,
    Geometry,
}

/// A value worked out before the walk (a condition, a label, a sketch id) for field `b`.
fn pre(b: &Ident, what: &str) -> Ident {
    format_ident!("__{}_{}", what, b)
}

/// The statements that walk field `b` in walk `which`.
fn walk_field(b: &Ident, r: &Roles, which: Which, mutable: bool) -> Tokens {
    let w = quote!(crate::walk);
    let each = if mutable { quote!(each_mut) } else { quote!(each) };
    if r.nested() {
        let m = match which {
            Which::Exprs => "walk_exprs",
            Which::Refs => "walk_refs",
            Which::Geometry => "walk_geometry",
        };
        let m = if mutable { format_ident!("{}_mut", m) } else { format_ident!("{}", m) };
        if r.picks_faces && which == Which::Geometry {
            return quote! {
                #w::Walk::#m(#b, &mut |g| f(match g { #w::Geo::Pos(p) => #w::Geo::Pick(p), g => g }));
            };
        }
        return quote! { #w::Walk::#m(#b, f); };
    }
    match which {
        Which::Exprs => {
            let label = pre(b, "label");
            match (&r.expr, &r.geo) {
                (Some((kind, _)), _) => {
                    let kind = Ident::new(kind, Span::call_site());
                    quote! {
                        #w::ExprField::#each(#b, &mut |i, n, e| f((#w::Label::at(&#label, i, n), e, #w::Kind::#kind, #w::ExprRole::Input)));
                    }
                }
                (None, Some(Geo::Placement)) => quote! {
                    #w::ExprField::#each(#b, &mut |i, n, e| f((#w::Label::at(&["X", "Y", "Z"], i, n), e, #w::Kind::Length, #w::ExprRole::Placement)));
                },
                (None, Some(Geo::Direction)) => quote! {
                    #w::ExprField::#each(#b, &mut |i, n, e| f((#w::Label::at(&["Axis X", "Axis Y", "Axis Z"], i, n), e, #w::Kind::Unitless, #w::ExprRole::Placement)));
                },
                _ => quote!(),
            }
        }
        Which::Refs => quote!(),
        Which::Geometry => match &r.geo {
            Some(Geo::Points(v)) => {
                let v = Ident::new(v, Span::call_site());
                quote! { #w::Each::<#w::Vec3>::#each(#b, &mut |p| f(#w::Geo::#v(p))); }
            }
            Some(Geo::PickIf(_)) => {
                let pick = pre(b, "pick");
                quote! {
                    #w::Each::<#w::Vec3>::#each(#b, &mut |p| f(if #pick { #w::Geo::Pick(p) } else { #w::Geo::Pos(p) }));
                }
            }
            // A point or an optional one (a hole's `at`, #1678).
            Some(Geo::Placement) => quote! { #w::Each::<crate::PointExpr>::#each(#b, &mut |p| f(#w::Geo::Placement(p))); },
            Some(Geo::Direction) => quote! { #w::Each::<crate::DirExpr>::#each(#b, &mut |d| f(#w::Geo::DirExpr(d))); },
            Some(Geo::Profiles { .. }) => {
                let (id, when) = (pre(b, "sketch"), pre(b, "when"));
                quote! {
                    if #when && let Some(id) = #id {
                        #w::Each::<#w::ProfileSel>::#each(#b, &mut |sel| f(#w::Geo::Profiles(id, sel)));
                    }
                }
            }
            None => quote!(),
        },
    }
}

/// The values field `b`'s walk needs, worked out before the walk starts (the walk may hand out
/// the fields they read).
fn pre_values(b: &Ident, r: &Roles, which: Which) -> Tokens {
    let w = quote!(crate::walk);
    match (which, &r.expr, &r.geo) {
        (Which::Exprs, Some((_, label)), _) => {
            let l = pre(b, "label");
            quote! { let #l = #label; }
        }
        (Which::Geometry, _, Some(Geo::PickIf(cond))) => {
            let p = pre(b, "pick");
            quote! { let #p: bool = #cond; }
        }
        (Which::Geometry, _, Some(Geo::Profiles { sketch, when })) => {
            let (id, wh) = (pre(b, "sketch"), pre(b, "when"));
            let when = when.as_ref().map_or(quote!(true), |c| quote!(#c));
            quote! {
                let #wh: bool = #when;
                let mut #id: Option<u64> = None;
                #w::Each::<crate::SketchId>::each(&*#sketch, &mut |s| #id = Some(s.0));
            }
        }
        _ => quote!(),
    }
}

/// A pattern that binds every field of `fields` by name (`_0`, `_1`… for tuple fields), and the
/// body of walk `which` over them.
fn arm(fields: &Fields, which: Which, mutable: bool) -> syn::Result<(Tokens, Tokens)> {
    let mut members = Vec::new();
    let mut walked: Vec<(Ident, Roles)> = Vec::new();
    for (i, field) in fields.iter().enumerate() {
        let b = field.ident.clone().unwrap_or_else(|| format_ident!("_{}", i));
        members.push(field.ident.clone().map_or_else(|| Member::Unnamed(i.into()), Member::Named));
        walked.push((b, roles(field)?));
    }
    let bindings: Vec<Ident> = walked.iter().map(|(b, _)| b.clone()).collect();
    // `after = x` moves a field to just after `x`.
    for b in &bindings {
        let Some(pos) = walked.iter().position(|(x, _)| x == b) else { continue };
        let Some(after) = walked.get(pos).and_then(|(_, r)| r.after.clone()) else { continue };
        let moved = walked.remove(pos);
        let Some(at) = walked.iter().position(|(x, _)| *x == after) else {
            return Err(syn::Error::new_spanned(after, "no such field"));
        };
        walked.insert(at + 1, moved);
    }
    let mut body = Tokens::new();
    for (b, r) in &walked {
        body.extend(pre_values(b, r, which));
    }
    for (b, r) in &walked {
        body.extend(walk_field(b, r, which, mutable));
    }
    let pattern = match fields {
        Fields::Named(_) => quote!({ #(#bindings),* }),
        Fields::Unnamed(_) => quote!({ #(#members: #bindings),* }),
        Fields::Unit => quote!(),
    };
    Ok((pattern, body))
}

fn expand(input: &DeriveInput) -> syn::Result<Tokens> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let w = quote!(crate::walk);
    let mut methods = Tokens::new();
    for (which, method, item) in
        [(Which::Exprs, "walk_exprs", "ExprItem"), (Which::Refs, "walk_refs", "RefItem"), (Which::Geometry, "walk_geometry", "GeoItem")]
    {
        for mutable in [false, true] {
            let body = match &input.data {
                Data::Struct(s) => {
                    let (pattern, body) = arm(&s.fields, which, mutable)?;
                    quote! { let Self #pattern = self; #body }
                }
                Data::Enum(e) => {
                    let mut arms = Tokens::new();
                    for v in &e.variants {
                        let ident = &v.ident;
                        let (pattern, body) = arm(&v.fields, which, mutable)?;
                        arms.extend(quote! { Self::#ident #pattern => { #body } });
                    }
                    quote! { match self { #arms } }
                }
                Data::Union(u) => return Err(syn::Error::new_spanned(u.union_token, "unions are not walked")),
            };
            let (method, item, r) = if mutable {
                (format_ident!("{}_mut", method), format_ident!("{}Mut", item), quote!(&'a mut))
            } else {
                (format_ident!("{}", method), format_ident!("{}", item), quote!(&'a))
            };
            methods.extend(quote! {
                #[allow(unused_variables, unused_mut, clippy::needless_borrow)]
                fn #method<'a>(#r self, f: &mut dyn FnMut(#w::#item<'a>)) { #body }
            });
        }
    }
    Ok(quote! {
        impl #impl_generics #w::Walk for #name #ty_generics #where_clause { #methods }
    })
}
