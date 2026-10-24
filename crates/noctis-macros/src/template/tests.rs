//! The `#[template(..)]` grammar rows, the item shape, the `HashMap` scan and
//! `#[asset]`.

#![allow(
    clippy::needless_pass_by_value,
    reason = "the helpers take `quote!` output by value at every call site"
)]

use quote::quote;

use super::*;

fn settings(args: &TokenStream) -> Result<Settings, Vec<String>> {
    let input: DeriveInput = syn::parse2(quote! {
        #[template(#args)]
        struct Card { name: String }
    })
    .unwrap();
    let attr = &input.attrs[0];
    match attr.parse_args_with(|s: ParseStream<'_>| parse_settings(s, attr)) {
        Ok(Ok(settings)) => Ok(settings),
        Ok(Err(errors)) => Err(errors.iter().map(ToString::to_string).collect()),
        Err(err) => Err(vec![err.to_string()]),
    }
}

fn base(extra: TokenStream) -> TokenStream {
    quote!(id = "card", version = 1, size = (10, 20), format = Png #extra)
}

#[track_caller]
fn fails(args: TokenStream, needle: &str) {
    let errors = settings(&args).expect_err(&args.to_string());
    assert!(
        errors.iter().any(|e| e.contains(needle)),
        "{args}: {errors:?} lacks {needle:?}"
    );
}

#[test]
fn template_ok() {
    let s = settings(&quote!(
        id = "profile.card",
        version = 1,
        size = (934, 282),
        format = Png
    ))
    .unwrap();
    assert_eq!(
        s,
        Settings {
            id: "profile.card".into(),
            version: 1,
            size: (934, 282),
            format: Still::Png,
            animation: None,
        }
    );
    // Any key order.
    let s = settings(&quote!(
        format = WebpLossless,
        size = (1, 1),
        version = 0u32,
        id = "a"
    ))
    .unwrap();
    assert_eq!(
        (s.format, s.size, s.version),
        (Still::WebpLossless, (1, 1), 0)
    );
}

#[test]
fn template_animation() {
    let s = settings(&base(
        quote!(, animation(duration_ms = 2000, fps = 20, format = Webp { quality: 80 })),
    ))
    .unwrap();
    assert_eq!(
        s.animation,
        Some(Animation {
            duration_ms: 2000,
            fps: 20,
            format: Animated::Webp(80)
        })
    );
    let s = settings(&base(
        quote!(, animation(format = Apng, fps = 90, duration_ms = 1)),
    ))
    .unwrap();
    assert_eq!(s.animation.map(|a| a.format), Some(Animated::Apng));
}

#[test]
fn template_animation_format_defaults_to_webp_80() {
    let s = settings(&base(quote!(, animation(duration_ms = 1000, fps = 10)))).unwrap();
    assert_eq!(s.animation.map(|a| a.format), Some(Animated::Webp(80)));
}

#[test]
fn template_id_bounds() {
    let long = "a".repeat(64);
    let too_long = "a".repeat(65);
    assert!(settings(&quote!(id = #long, version = 1, size = (1, 1), format = Png)).is_ok());
    for bad in ["", too_long.as_str(), "Profile", "a:b", "\u{fc}"] {
        fails(
            quote!(id = #bad, version = 1, size = (1, 1), format = Png),
            "`id` takes a string literal of 1 to 64 characters",
        );
    }
    fails(
        quote!(id = card, version = 1, size = (1, 1), format = Png),
        "found `card`",
    );
}

#[test]
fn template_version_bounds() {
    for ok in [quote!(0), quote!(4294967295), quote!(7u32)] {
        assert!(settings(&quote!(id = "a", version = #ok, size = (1, 1), format = Png)).is_ok());
    }
    for (bad, shown) in [
        (quote!(4294967296), "4294967296"),
        (quote!(-1), "-1"),
        (quote!(1u8), "1u8"),
        (quote!("1"), "\"1\""),
    ] {
        fails(
            quote!(id = "a", version = #bad, size = (1, 1), format = Png),
            &format!("`version` takes an integer literal 0..=4294967295 (`u32`), found `{shown}`"),
        );
    }
}

#[test]
fn template_size_bounds() {
    assert!(settings(&quote!(id = "a", version = 1, size = (1, 1), format = Png)).is_ok());
    fails(
        quote!(id = "a", version = 1, size = (0, 5), format = Png),
        "the `size` width takes an integer literal 1..=4294967295",
    );
    fails(
        quote!(id = "a", version = 1, size = (5, 0), format = Png),
        "the `size` height takes",
    );
    for bad in [quote!((1, 2, 3)), quote!([1, 2]), quote!(5)] {
        fails(
            quote!(id = "a", version = 1, size = #bad, format = Png),
            "`size` takes `(width, height)`",
        );
    }
}

#[test]
fn template_quality_bounds() {
    for q in [1u8, 100] {
        let lit = Literal::u8_unsuffixed(q);
        let s = settings(
            &quote!(id = "a", version = 1, size = (1, 1), format = Jpeg { quality: #lit }),
        )
        .unwrap();
        assert_eq!(s.format, Still::Jpeg(q));
    }
    for q in [0u16, 101] {
        let lit = Literal::u16_unsuffixed(q);
        fails(
            quote!(id = "a", version = 1, size = (1, 1), format = Webp { quality: #lit }),
            "`quality` takes an integer literal 1..=100",
        );
    }
    fails(
        quote!(id = "a", version = 1, size = (1, 1), format = Jpeg),
        "`format` takes `Png`",
    );
    fails(
        quote!(
            id = "a",
            version = 1,
            size = (1, 1),
            format = Png { quality: 5 }
        ),
        "`format` takes `Png`",
    );
}

#[test]
fn template_animated_format_is_not_still() {
    for animated in [quote!(Gif), quote!(Apng)] {
        fails(
            quote!(id = "a", version = 1, size = (1, 1), format = #animated),
            "is not a still format",
        );
    }
    fails(
        base(quote!(, animation(duration_ms = 1, fps = 1, format = Png))),
        "`animation(format = ..)` takes `Webp { quality: 1..=100 }`, `Gif` or `Apng`",
    );
}

#[test]
fn template_fps_bounds() {
    let anim = |fps: u16, format: TokenStream| {
        let fps = Literal::u16_unsuffixed(fps);
        base(quote!(, animation(duration_ms = 1000, fps = #fps, format = #format)))
    };
    for fps in [1, 90] {
        assert!(settings(&anim(fps, quote!(Webp { quality: 80 }))).is_ok());
    }
    fails(
        anim(91, quote!(Webp { quality: 80 })),
        "`fps` takes an integer literal 1..=90",
    );
    fails(
        anim(0, quote!(Apng)),
        "`fps` takes an integer literal 1..=90",
    );
    assert!(settings(&anim(50, quote!(Gif))).is_ok());
    fails(
        anim(51, quote!(Gif)),
        "`fps` for `Gif` must be 1..=50, found `51`",
    );
}

#[test]
fn template_duration_bounds() {
    fails(
        base(quote!(, animation(duration_ms = 0, fps = 10, format = Gif))),
        "`duration_ms` takes an integer literal 1..=4294967295",
    );
    assert!(
        settings(&base(
            quote!(, animation(duration_ms = 1, fps = 10, format = Gif))
        ))
        .is_ok()
    );
}

#[test]
fn template_animation_keys() {
    fails(
        base(quote!(, animation(fps = 20, format = Gif))),
        "`animation(..)` is missing `duration_ms = <milliseconds>`",
    );
    fails(
        base(quote!(, animation(duration_ms = 1, fps = 2, fps = 3, format = Gif))),
        "duplicate key `fps` in `animation(..)`",
    );
    fails(
        base(quote!(, animation(duration_ms = 1, fps = 2, format = Gif, loop = 1))),
        "unknown key `loop` in `animation(..)`",
    );
    fails(
        base(quote!(, animation = 5)),
        "`animation` takes `(duration_ms",
    );
}

#[test]
fn template_keys() {
    fails(
        quote!(id = "a", version = 1, format = Png),
        "`#[template(..)]` is missing `size = (<width>, <height>)`",
    );
    fails(
        quote!(id = "a", id = "b", version = 1, size = (1, 1), format = Png),
        "duplicate key `id` in `#[template(..)]`",
    );
    fails(
        base(quote!(, colour = 1)),
        "unknown key `colour` in `#[template(..)]`: it takes `id`, `version`, `size`, `format` and `animation(..)`",
    );
    fails(quote!(), "is missing `id = ");
}

fn derive_errors(item: &TokenStream) -> Vec<String> {
    let input: DeriveInput = syn::parse2(item.clone()).unwrap();
    match expand(&input) {
        Ok(_) => Vec::new(),
        Err(err) => err.into_iter().map(|e| e.to_string()).collect(),
    }
}

#[test]
fn template_item_shape() {
    let attr = quote!(#[template(id = "a", version = 1, size = (1, 1), format = Png)]);
    for item in [
        quote!(#attr enum E { A }),
        quote!(#attr struct S<T> { t: T }),
        quote!(#attr struct S<'a> { t: &'a str }),
    ] {
        let errors = derive_errors(&item);
        assert!(
            errors
                .iter()
                .any(|e| e.starts_with("`#[derive(Template)]` needs a struct")),
            "{item}: {errors:?}"
        );
    }
    assert!(derive_errors(&quote!(#attr struct Unit;)).is_empty());
    assert!(derive_errors(&quote!(#attr struct Tuple(u8);)).is_empty());
    let missing = derive_errors(&quote!(
        struct S {
            a: u8,
        }
    ));
    assert!(
        missing[0].starts_with("missing `#[template("),
        "{missing:?}"
    );
    let twice = derive_errors(&quote!(#attr #attr struct S { a: u8 }));
    assert!(twice[0].contains("appears twice"), "{twice:?}");
}

#[test]
fn template_rejects_hash_collections() {
    let attr = quote!(#[template(id = "a", version = 1, size = (1, 1), format = Png)]);
    let errors = derive_errors(&quote! {
        #attr
        struct S {
            a: HashMap<String, u8>,
            b: Vec<HashMap<u8, u8>>,
            c: Option<std::collections::HashSet<u8>>,
            d: (u8, &'static [HashSet<u8>]),
            ok: std::collections::BTreeMap<String, Vec<u8>>,
        }
    });
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert!(errors.iter().all(|e| e.starts_with(
        "Template data must serialize deterministically (it is part of the render cache key): `Hash"
    )));
    assert!(
        errors[0]
            .ends_with("`HashMap` iterates in random order; use `BTreeMap`, `BTreeSet` or `Vec`")
    );
}

#[test]
fn img_03_asset_attribute_errors() {
    let attr = quote!(#[template(id = "a", version = 1, size = (1, 1), format = Png)]);
    for field in [
        quote!(#[asset(optional)] a: RemoteImage),
        quote!(#[asset = 1] a: RemoteImage),
        quote!(#[asset(required, required)] a: RemoteImage),
        quote!(#[asset()] a: RemoteImage),
    ] {
        let errors = derive_errors(&quote!(#attr struct S { #field }));
        assert_eq!(
            errors,
            ["[IMG-03] `#[asset]` takes no argument or `required`: `#[asset(required)]`"],
            "{field}"
        );
    }
    let errors = derive_errors(&quote!(#attr struct S { #[asset] #[asset] a: RemoteImage }));
    assert_eq!(errors, ["[IMG-03] `#[asset]` appears twice on this field"]);
}

/// The expansion of a Template whose fields are `fields`.
fn expanded(fields: TokenStream) -> String {
    let input: DeriveInput = syn::parse2(quote! {
        #[template(id = "a", version = 1, size = (1, 1), format = Png)]
        struct S #fields
    })
    .unwrap();
    expand(&input).unwrap().to_string()
}

#[test]
fn img_03_asset_fields_emit_template_assets() {
    let out = expanded(quote!({
        #[asset] a: RemoteImage,
        #[asset(required)] b: Option<RemoteImage>,
        c: String,
        #[asset] r#type: Vec<RemoteImage>,
    }));
    for needle in [
        "impl :: noctis :: __private :: image :: TemplateAssets for S",
        "const DECLARES : bool = true ;",
        "< RemoteImage as :: noctis :: __private :: image :: AssetField > :: declare (& self . a , \"a\" , false , out) ;",
        "< Option < RemoteImage > as :: noctis :: __private :: image :: AssetField > :: declare (& self . b , \"b\" , true , out) ;",
        "declare (& self . r#type , \"type\" , false , out) ;",
    ] {
        assert!(out.contains(needle), "{needle} not in {out}");
    }
    assert!(!out.contains("self . c"), "{out}");
    let tuple = expanded(quote!((#[asset] RemoteImage, u8);));
    assert!(
        tuple.contains("declare (& self . 0 , \"0\" , false , out) ;"),
        "{tuple}"
    );
    let plain = expanded(quote!({ name: String }));
    assert!(
        plain.contains("impl :: noctis :: __private :: image :: TemplateAssets for S { }"),
        "{plain}"
    );
    assert!(!plain.contains("DECLARES"), "{plain}");
}

#[test]
fn template_emits_the_ports_impl() {
    let input: DeriveInput = syn::parse2(quote! {
        #[template(id = "card", version = 2, size = (600, 200), format = Jpeg { quality: 90 },
                   animation(duration_ms = 2000, fps = 20, format = Gif))]
        struct Card { name: String }
    })
    .unwrap();
    let out = expand(&input).unwrap().to_string();
    for needle in [
        "impl :: noctis :: __private :: ports :: Template for Card",
        "const ID : & 'static str = \"card\"",
        "const VERSION : u32 = 2",
        "Size :: new (600 , 200)",
        "Format :: Jpeg { quality : 90 }",
        "Duration :: from_millis (2000)",
        "AnimatedFormat :: Gif",
        ":: noctis :: __private :: image :: erased_view :: < Self > ()",
    ] {
        assert!(out.contains(needle), "{needle} not in {out}");
    }
}

#[test]
fn template_ids() {
    assert!(is_template_id("profile.card_v2-x"));
    assert!(!is_template_id(""));
    assert!(!is_template_id("A"));
}

#[test]
fn gif_and_apng_emit_the_feature_gate() {
    let expanded = |anim: TokenStream| {
        let input: DeriveInput = syn::parse2(quote! {
            #[template(id = "card", version = 1, size = (10, 20), format = Png #anim)]
            struct Card { name: String }
        })
        .unwrap();
        derive(&input).to_string()
    };
    for format in [quote!(Gif), quote!(Apng)] {
        let out = expanded(quote!(, animation(duration_ms = 1, fps = 10, format = #format)));
        assert!(out.contains("require_gif_output"), "{out}");
    }
    for anim in [
        quote!(),
        quote!(, animation(duration_ms = 1, fps = 10)),
        quote!(, animation(duration_ms = 1, fps = 10, format = Webp { quality: 50 })),
    ] {
        let out = expanded(anim);
        assert!(!out.contains("require_gif_output"), "{out}");
    }
}
