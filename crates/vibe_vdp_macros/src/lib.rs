//! `#[vdp_methods]` — generate VDP method dispatch from a tagged `impl` block.
//!
//! Annotate an inherent `impl` block and tag the methods you want exposed over
//! the Vibe Debug Protocol with `#[vdp("game.methodName")]`. The macro keeps
//! those methods (stripping the `#[vdp(...)]` marker) and generates an extra
//! method:
//!
//! ```ignore
//! fn dispatch_vdp(&mut self, method: &str, params: &serde_json::Value)
//!     -> Option<Result<serde_json::Value, String>>
//! ```
//!
//! which returns `Some(..)` when `method` matches a tagged method (deserializing
//! the typed params, calling the method, serializing its result) and `None`
//! otherwise — so the caller can fall back or forward to another namespace.
//!
//! Supported method shapes (both return `Result<R, String>` with `R: Serialize`):
//! - `fn foo(&mut self) -> Result<R, String>` — no params
//! - `fn foo(&mut self, p: P) -> Result<R, String>` — `P: DeserializeOwned`

use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ImplItem, ItemImpl, LitStr, parse_macro_input};

/// Attribute macro applied to an inherent `impl` block. See crate docs.
#[proc_macro_attribute]
pub fn vdp_methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);

    // Collected dispatch arms: (vdp method name, method ident, has typed param).
    let mut arms = Vec::new();
    let mut errors: Vec<syn::Error> = Vec::new();

    for item in &mut input.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        // Find and remove the #[vdp("...")] marker attribute, capturing its name.
        let mut vdp_name: Option<LitStr> = None;
        let mut kept = Vec::with_capacity(method.attrs.len());
        for attr in method.attrs.drain(..) {
            if attr.path().is_ident("vdp") {
                match attr.parse_args::<LitStr>() {
                    Ok(name) => vdp_name = Some(name),
                    Err(e) => errors.push(e),
                }
            } else {
                kept.push(attr);
            }
        }
        method.attrs = kept;

        let Some(name) = vdp_name else {
            continue;
        };

        let ident = method.sig.ident.clone();

        // Count typed (non-receiver) arguments to pick the dispatch shape.
        let typed_args = method
            .sig
            .inputs
            .iter()
            .filter(|a| matches!(a, FnArg::Typed(_)))
            .count();

        arms.push((name, ident, typed_args));
    }

    if let Some(err) = errors.into_iter().reduce(|mut a, b| {
        a.combine(b);
        a
    }) {
        return err.to_compile_error().into();
    }

    let dispatch_arms = arms.iter().map(|(name, ident, typed_args)| {
        if *typed_args == 0 {
            quote! {
                #name => Some(
                    self.#ident().and_then(|r| ::vibe2d::vdp::to_result(&r))
                ),
            }
        } else {
            quote! {
                #name => Some(
                    ::vibe2d::vdp::from_params(params)
                        .and_then(|p| self.#ident(p))
                        .and_then(|r| ::vibe2d::vdp::to_result(&r))
                ),
            }
        }
    });

    let self_ty = &input.self_ty;
    let (impl_generics, _, where_clause) = input.generics.split_for_impl();

    let dispatch_impl = quote! {
        impl #impl_generics #self_ty #where_clause {
            /// Route a VDP method call to a `#[vdp(..)]`-tagged method.
            ///
            /// Returns `Some(result)` when `method` matches, `None` otherwise.
            #[allow(dead_code)]
            fn dispatch_vdp(
                &mut self,
                method: &str,
                params: &::serde_json::Value,
            ) -> Option<Result<::serde_json::Value, String>> {
                match method {
                    #(#dispatch_arms)*
                    _ => None,
                }
            }
        }
    };

    quote! {
        #input
        #dispatch_impl
    }
    .into()
}
