// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `#[derive(Routes)]`: an enum whose variants are an application's routes.
//!
//! Each variant says its pattern with `#[route("/task/:id")]`. A `:parameter` is the field of
//! that name, parsed with `FromStr` and written with `Display`; any other field is a query
//! parameter, and is an `Option`.

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{Data, DeriveInput, Fields, LitStr};

pub(crate) fn expand(input: TokenStream) -> TokenStream {
    match syn::parse2::<DeriveInput>(input).and_then(|input| derive(&input)) {
        Ok(tokens) => tokens,
        Err(error) => error.to_compile_error(),
    }
}

/// One segment of a pattern.
#[derive(Debug, PartialEq)]
pub(crate) enum Segment {
    Literal(String),
    Param(String),
}

/// A pattern taken apart, or why it cannot be one.
pub(crate) fn segments(pattern: &str) -> Result<Vec<Segment>, String> {
    let Some(path) = pattern.strip_prefix('/') else {
        return Err(format!("a route's pattern starts with `/`: `{pattern}`"));
    };
    if pattern.contains(['?', '#']) {
        return Err(format!(
            "a route's pattern is a path, without `?` or `#`: `{pattern}`; a query parameter is a field the pattern does not name"
        ));
    }
    if path.contains("//") || (!path.is_empty() && path.ends_with('/')) {
        return Err(format!(
            "`{pattern}`: write a pattern without an empty segment or a trailing `/`"
        ));
    }
    let mut out = Vec::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if segment == "*" || segment.contains('*') {
            return Err(format!(
                "`{pattern}`: a typed route has no `*`; name each part with `:parameter`"
            ));
        }
        match segment.strip_prefix(':') {
            Some(name) => {
                if syn::parse_str::<syn::Ident>(name).is_err() {
                    return Err(format!(
                        "`{pattern}`: the parameter `:{name}` is not a field name"
                    ));
                }
                if out.contains(&Segment::Param(name.to_string())) {
                    return Err(format!("`{pattern}` names `:{name}` twice"));
                }
                out.push(Segment::Param(name.to_string()));
            }
            None => out.push(Segment::Literal(segment.to_string())),
        }
    }
    Ok(out)
}

/// Two patterns that answer the same locations: `/a/:x` and `/a/:y`.
fn same_shape(a: &[Segment], b: &[Segment]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (Segment::Literal(x), Segment::Literal(y)) => x == y,
            (Segment::Param(_), Segment::Param(_)) => true,
            _ => false,
        })
}

/// The path the generated code names the runtime crate by.
///
/// `#[routes(crate = "...")]` says it. Otherwise: `::frus_widgets` for a package that depends
/// on `frus-widgets` (or is it), `::frus` for any other, which is what an application
/// depends on.
fn runtime_crate(input: &DeriveInput) -> syn::Result<TokenStream> {
    for attribute in &input.attrs {
        if !attribute.path().is_ident("routes") {
            continue;
        }
        let mut path = None;
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("crate") {
                let value: LitStr = meta.value()?.parse()?;
                path = Some(value.parse::<syn::Path>()?.into_token_stream());
                Ok(())
            } else {
                Err(meta.error("expected `crate = \"path\"`"))
            }
        })?;
        if let Some(path) = path {
            return Ok(path);
        }
    }
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR")
        .and_then(|dir| std::fs::read_to_string(std::path::Path::new(&dir).join("Cargo.toml")).ok())
        .unwrap_or_default();
    if names_frus_widgets(&manifest) {
        Ok(quote!(::frus_widgets))
    } else {
        Ok(quote!(::frus))
    }
}

/// Whether a manifest is `frus-widgets`'s, or depends on it by that name.
pub(crate) fn names_frus_widgets(manifest: &str) -> bool {
    manifest.lines().any(|line| {
        let line = line.trim();
        if line == "name = \"frus-widgets\"" || line.ends_with("dependencies.frus-widgets]") {
            return true;
        }
        line.strip_prefix("frus-widgets")
            .map(|rest| {
                let rest = rest.trim_start();
                rest.starts_with('=') || rest.starts_with('.')
            })
            .unwrap_or(false)
    })
}

fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "#[derive(Routes)] is for an enum, one variant per route",
        ));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "#[derive(Routes)] takes an enum without generic parameters",
        ));
    }
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "#[derive(Routes)] needs at least one route",
        ));
    }
    let krate = runtime_crate(input)?;
    let name = &input.ident;

    let mut patterns: Vec<(String, Vec<Segment>, &syn::Variant)> = Vec::new();
    for variant in &data.variants {
        let mut found = None;
        for attribute in &variant.attrs {
            if attribute.path().is_ident("route") {
                if found.is_some() {
                    return Err(syn::Error::new_spanned(
                        attribute,
                        "a variant has one `#[route(..)]`",
                    ));
                }
                found = Some(attribute.parse_args::<LitStr>()?);
            }
        }
        let Some(pattern) = found else {
            return Err(syn::Error::new_spanned(
                &variant.ident,
                format!(
                    "the route `{}` needs its pattern: #[route(\"/path/:param\")]",
                    variant.ident
                ),
            ));
        };
        let parsed = segments(&pattern.value())
            .map_err(|message| syn::Error::new(pattern.span(), message))?;
        if let Some((other, _, _)) = patterns.iter().find(|(_, s, _)| same_shape(s, &parsed)) {
            return Err(syn::Error::new(
                pattern.span(),
                format!(
                    "`{}` answers the same locations as `{other}`",
                    pattern.value()
                ),
            ));
        }
        patterns.push((pattern.value(), parsed, variant));
    }

    let mut pattern_arms = Vec::new();
    let mut path_arms = Vec::new();
    let mut query_arms = Vec::new();
    let mut parse_arms = Vec::new();
    for (pattern, parsed, variant) in &patterns {
        let ident = &variant.ident;
        let params: Vec<&String> = parsed
            .iter()
            .filter_map(|s| match s {
                Segment::Param(p) => Some(p),
                Segment::Literal(_) => None,
            })
            .collect();
        match &variant.fields {
            Fields::Unit => {
                if let Some(param) = params.first() {
                    return Err(syn::Error::new_spanned(
                        ident,
                        format!("`{pattern}` has the parameter `:{param}`, and `{ident}` has no field `{param}`"),
                    ));
                }
                pattern_arms.push(quote! { Self::#ident => #pattern });
                path_arms.push(quote! { Self::#ident => ::std::vec::Vec::new() });
                query_arms.push(quote! { Self::#ident => ::std::vec::Vec::new() });
                parse_arms.push(quote! { #pattern => ::core::option::Option::Some(Self::#ident) });
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    fields,
                    "a route's fields have names, the names its pattern's `:parameters` use",
                ));
            }
            Fields::Named(fields) => {
                let names: Vec<String> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.as_ref().expect("named").to_string())
                    .collect();
                for param in &params {
                    if !names.contains(param) {
                        return Err(syn::Error::new_spanned(
                            ident,
                            format!("`{pattern}` has the parameter `:{param}`, and `{ident}` has no field `{param}`"),
                        ));
                    }
                }
                let mut path_values = Vec::new();
                let mut query_values = Vec::new();
                let mut parsed_fields = Vec::new();
                let bindings: Vec<&syn::Ident> = fields
                    .named
                    .iter()
                    .map(|f| f.ident.as_ref().unwrap())
                    .collect();
                for field in &fields.named {
                    let field_ident = field.ident.as_ref().unwrap();
                    let key = field_ident.to_string();
                    let key = key.strip_prefix("r#").unwrap_or(&key).to_string();
                    if params.iter().any(|p| **p == key) {
                        path_values.push(quote! {
                            (#key, ::std::string::ToString::to_string(#field_ident))
                        });
                        parsed_fields.push(quote! {
                            #field_ident: ::core::str::FromStr::from_str(&param(#key)?).ok()?
                        });
                    } else {
                        if !is_option(&field.ty) {
                            return Err(syn::Error::new_spanned(
                                &field.ty,
                                format!(
                                    "`{key}` is not a parameter of `{pattern}`, so it is a query parameter, which a location may leave out: make it an `Option<..>`"
                                ),
                            ));
                        }
                        query_values.push(quote! {
                            if let ::core::option::Option::Some(value) = #field_ident {
                                query.push((#key, ::std::string::ToString::to_string(value)));
                            }
                        });
                        parsed_fields.push(quote! {
                            #field_ident: query(#key).and_then(|value| ::core::str::FromStr::from_str(&value).ok())
                        });
                    }
                }
                pattern_arms.push(quote! { Self::#ident { .. } => #pattern });
                path_arms.push(quote! {
                    #[allow(unused_variables)]
                    Self::#ident { #(#bindings),* } => ::std::vec![#(#path_values),*]
                });
                query_arms.push(quote! {
                    #[allow(unused_variables)]
                    Self::#ident { #(#bindings),* } => {
                        #[allow(unused_mut)]
                        let mut query = ::std::vec::Vec::new();
                        #(#query_values)*
                        query
                    }
                });
                parse_arms.push(quote! {
                    #pattern => ::core::option::Option::Some(Self::#ident { #(#parsed_fields),* })
                });
            }
        }
    }
    let all: Vec<&String> = patterns.iter().map(|(p, _, _)| p).collect();
    Ok(quote! {
        impl #krate::Routes for #name {
            const PATTERNS: &'static [&'static str] = &[#(#all),*];

            fn pattern(&self) -> &'static str {
                match self {
                    #(#pattern_arms,)*
                }
            }

            fn path_params(&self) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                match self {
                    #(#path_arms,)*
                }
            }

            fn query_params(&self) -> ::std::vec::Vec<(&'static str, ::std::string::String)> {
                match self {
                    #(#query_arms,)*
                }
            }

            #[allow(unused_variables)]
            fn from_parts(
                pattern: &str,
                param: &dyn ::core::ops::Fn(&str) -> ::core::option::Option<::std::string::String>,
                query: &dyn ::core::ops::Fn(&str) -> ::core::option::Option<::std::string::String>,
            ) -> ::core::option::Option<Self> {
                match pattern {
                    #(#parse_arms,)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}

/// Whether a field's type is written `Option<..>` (or a path ending in it).
fn is_option(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|s| s.ident == "Option")
            .unwrap_or(false),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_checked() {
        assert_eq!(
            segments("/task/:id").unwrap(),
            [Segment::Literal("task".into()), Segment::Param("id".into())]
        );
        assert_eq!(segments("/").unwrap(), []);
        assert!(segments("task").unwrap_err().contains("starts with `/`"));
        assert!(segments("/a?b=1").unwrap_err().contains("query parameter"));
        assert!(segments("/files/*").unwrap_err().contains("no `*`"));
        assert!(segments("/a/:x/:x").unwrap_err().contains("twice"));
        assert!(segments("/a/:not-a-name")
            .unwrap_err()
            .contains("not a field name"));
        assert!(segments("/a/").unwrap_err().contains("trailing"));
        assert!(segments("/a//b").unwrap_err().contains("empty segment"));
    }

    #[test]
    fn two_patterns_of_one_shape_are_the_same_route() {
        let a = segments("/a/:x").unwrap();
        assert!(same_shape(&a, &segments("/a/:y").unwrap()));
        assert!(!same_shape(&a, &segments("/a/new").unwrap()));
    }

    #[test]
    fn the_runtime_crate_is_found_in_the_manifest() {
        assert!(names_frus_widgets("[package]\nname = \"frus-widgets\"\n"));
        assert!(names_frus_widgets(
            "[dependencies]\nfrus-widgets = { path = \"../frus-widgets\" }\n"
        ));
        assert!(names_frus_widgets(
            "[dependencies]\nfrus-widgets.workspace = true\n"
        ));
        assert!(names_frus_widgets(
            "[dependencies.frus-widgets]\npath = \"x\"\n"
        ));
        assert!(!names_frus_widgets(
            "[dependencies]\nfrus = \"0.5\"\nfrus-widgets-extra = \"1\"\n"
        ));
    }
}
