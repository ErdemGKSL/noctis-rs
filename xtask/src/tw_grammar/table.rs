//! The hand-curated half of the Tailwind grammar.
//!
//! - [`VALUE_GRAMMARS`]: the closed suffix table, one entry per takumi value
//!   parse type (the 49 distinct parse types of `property_parsers!` plus the
//!   hard-coded `TwGradientPosition`). Each entry is a Rust expression over
//!   `crate::tw::suffix` items, emitted verbatim into `generated.rs`. It was
//!   written from the type's `parse_tw` / `FromCss` impl in the pinned
//!   takumi-core and may narrow what takumi accepts, never widen it; the
//!   `tw_crosscheck` test proves every sample against takumi.
//! - [`FINGERPRINTS`]: blake3 hashes of takumi's lookup algorithm and of every
//!   `impl TailwindPropertyParser`. A mismatch means takumi changed code the
//!   matcher or a table entry copies: re-audit, then update the hash by hand.
//! - The v3 -> v4 renames and hints.

/// The rules every `Length` suffix grammar shares (`auto` is not negatable).
macro_rules! length_rules {
    () => {
        "Rule::new(SuffixRule::Spacing), \
         Rule::new(SuffixRule::Fraction), \
         Rule::new(SuffixRule::Keywords(LENGTH_KEYWORDS)), \
         Rule::no_neg(SuffixRule::Keywords(&[\"auto\"])), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))"
    };
}

/// `Size`: `min`/`max`/`fit` plus every `Length` rule.
macro_rules! size_rules {
    () => {
        concat!(
            "Rule::new(SuffixRule::Keywords(&[\"min\", \"max\", \"fit\"])), ",
            length_rules!()
        )
    };
}

/// The closed suffix table: takumi parse type -> value grammar expression.
pub(crate) const VALUE_GRAMMARS: &[(&str, &str)] = &[
    (
        "Angle",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\", \"to-t\", \"to-tr\", \"to-r\", \"to-br\", \
         \"to-b\", \"to-bl\", \"to-l\", \"to-tl\"])), \
         Rule::new(SuffixRule::Integer { min: 0, max: 360 }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Angle))]",
    ),
    (
        "AlignItems",
        "&[Rule::new(SuffixRule::Keywords(&[\"start\", \"end\", \"center\", \"baseline\", \
         \"stretch\"]))]",
    ),
    (
        "AspectRatio",
        "&[Rule::new(SuffixRule::Keywords(&[\"square\", \"video\"])), \
         Rule::new(SuffixRule::Fraction), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Ratio))]",
    ),
    (
        "BackgroundClip",
        "&[Rule::new(SuffixRule::Keywords(&[\"border\", \"padding\", \"content\", \"text\"]))]",
    ),
    (
        "BackgroundImage",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\"]))]",
    ),
    (
        "BackgroundSize",
        "&[Rule::new(SuffixRule::Keywords(&[\"auto\", \"cover\", \"contain\"]))]",
    ),
    (
        "BlendMode",
        "&[Rule::new(SuffixRule::Keywords(&[\"normal\", \"multiply\", \"screen\", \"overlay\", \
         \"darken\", \"lighten\", \"color-dodge\", \"color-burn\", \"hard-light\", \
         \"soft-light\", \"difference\", \"exclusion\", \"hue\", \"saturation\", \"color\", \
         \"luminosity\", \"plus-lighter\", \"plus-darker\"]))]",
    ),
    (
        "BorderStyle",
        "&[Rule::new(SuffixRule::Keywords(&[\"solid\", \"dashed\", \"dotted\", \"double\", \
         \"hidden\", \"none\"]))]",
    ),
    // Shadow layers have no suffix form; `shadow-*` presets are fixed utilities.
    ("BoxShadow", "&[]"),
    (
        "ColorInput",
        "&[Rule::new(SuffixRule::Color { current: true }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Color))]",
    ),
    ("Filters", "&[Rule::new(SuffixRule::Keywords(&[\"none\"]))]"),
    (
        "Flex",
        "&[Rule::new(SuffixRule::Keywords(&[\"auto\", \"none\", \"initial\"])), \
         Rule::new(SuffixRule::Integer { min: 1, max: 99 }), \
         Rule::new(SuffixRule::Fraction)]",
    ),
    (
        "FlexBasis",
        concat!(
            "&[Rule::new(SuffixRule::Keywords(&[\"content\"])), ",
            size_rules!(),
            "]"
        ),
    ),
    (
        "FlexGrow",
        "&[Rule::new(SuffixRule::Number { max: 99, decimals: 2 }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Number))]",
    ),
    (
        "FontFamily",
        "&[Rule::new(SuffixRule::Keywords(&[\"sans\", \"serif\", \"mono\"])), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Keyword))]",
    ),
    (
        "FontStretch",
        "&[Rule::new(SuffixRule::Keywords(&[\"ultra-condensed\", \"extra-condensed\", \
         \"condensed\", \"semi-condensed\", \"normal\", \"semi-expanded\", \"expanded\", \
         \"extra-expanded\", \"ultra-expanded\"])), \
         Rule::new(SuffixRule::Percent)]",
    ),
    (
        "FontWeight",
        "&[Rule::new(SuffixRule::Keywords(&[\"thin\", \"extralight\", \"light\", \"normal\", \
         \"medium\", \"semibold\", \"bold\", \"extrabold\", \"black\"])), \
         Rule::new(SuffixRule::Integer { min: 1, max: 1000 })]",
    ),
    (
        "GridLine",
        "&[Rule::no_neg(SuffixRule::Keywords(&[\"auto\"])), \
         Rule::new(SuffixRule::Integer { min: 1, max: 999 })]",
    ),
    (
        "GridPlacement",
        "&[Rule::new(SuffixRule::Integer { min: 1, max: 999 }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Integer))]",
    ),
    (
        "GridPlacementSpan",
        "&[Rule::new(SuffixRule::Integer { min: 1, max: 999 })]",
    ),
    (
        "GridTrackSize",
        "&[Rule::new(SuffixRule::Keywords(&[\"auto\", \"fr\"]))]",
    ),
    (
        "JustifyContent",
        "&[Rule::new(SuffixRule::Keywords(&[\"start\", \"end\", \"center\", \"between\", \
         \"around\", \"evenly\", \"stretch\", \"normal\"]))]",
    ),
    ("Length", concat!("&[", length_rules!(), "]")),
    (
        "LineClamp",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\"])), \
         Rule::new(SuffixRule::Integer { min: 1, max: 99 }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Integer))]",
    ),
    (
        "LineHeight",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\", \"tight\", \"snug\", \"normal\", \
         \"relaxed\", \"loose\"])), \
         Rule::new(SuffixRule::Spacing), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    (
        "LineWidth",
        "&[Rule::new(SuffixRule::Number { max: 99, decimals: 2 }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    (
        "ListStyleImage",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\"]))]",
    ),
    (
        "ListStylePosition",
        "&[Rule::new(SuffixRule::Keywords(&[\"inside\", \"outside\"]))]",
    ),
    (
        "ListStyleType",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\", \"disc\", \"decimal\"]))]",
    ),
    (
        "ObjectFit",
        "&[Rule::new(SuffixRule::Keywords(&[\"contain\", \"cover\", \"fill\", \"none\", \
         \"scale-down\"]))]",
    ),
    (
        "Overflow",
        "&[Rule::new(SuffixRule::Keywords(&[\"visible\", \"clip\", \"hidden\"]))]",
    ),
    (
        "OverflowWrap",
        "&[Rule::new(SuffixRule::Keywords(&[\"normal\", \"anywhere\", \"break-word\"]))]",
    ),
    (
        "PercentageNumber",
        "&[Rule::new(SuffixRule::Integer { min: 0, max: 1000 }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Percentage)), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Number))]",
    ),
    (
        "PositionValue",
        "&[Rule::new(SuffixRule::Keywords(&[\"top-left\", \"top\", \"top-right\", \"left\", \
         \"center\", \"right\", \"bottom-left\", \"bottom\", \"bottom-right\"]))]",
    ),
    ("Size", concat!("&[", size_rules!(), "]")),
    (
        "TextAlign",
        "&[Rule::new(SuffixRule::Keywords(&[\"left\", \"center\", \"right\", \"justify\", \
         \"start\", \"end\"]))]",
    ),
    (
        "TextDecorationThickness",
        "&[Rule::new(SuffixRule::Integer { min: 0, max: 99 }), \
         Rule::new(SuffixRule::Keywords(&[\"auto\", \"from-font\"])), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    // Text-shadow layers have no suffix form; presets are fixed utilities.
    ("TextShadow", "&[]"),
    (
        "TextWrap",
        "&[Rule::new(SuffixRule::Keywords(&[\"wrap\", \"nowrap\", \"balance\", \"pretty\"]))]",
    ),
    (
        "TwAnimation",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\", \"spin\", \"ping\", \"pulse\", \
         \"bounce\"]))]",
    ),
    (
        "TwBlur",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\", \"xs\", \"sm\", \"md\", \"lg\", \"xl\", \
         \"2xl\", \"3xl\"])), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    // takumi's `parse_tw` accepts nothing; presets are fixed utilities.
    ("TwDropShadow", "&[]"),
    (
        "TwFontSize",
        "&[Rule::new(SuffixRule::Keywords(FONT_SIZES)), \
         Rule::new(SuffixRule::Slash { base: FONT_SIZES, modifier: &SuffixRule::Spacing }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    ("TwGradientPosition", "&[Rule::new(SuffixRule::Percent)]"),
    (
        "TwGridTemplate",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\"])), \
         Rule::new(SuffixRule::Integer { min: 1, max: 100 })]",
    ),
    (
        "TwLetterSpacing",
        "&[Rule::new(SuffixRule::Keywords(&[\"tighter\", \"tight\", \"normal\", \"wide\", \
         \"wider\", \"widest\"])), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    (
        "TwRounded",
        "&[Rule::new(SuffixRule::Keywords(&[\"none\", \"xs\", \"sm\", \"md\", \"lg\", \"xl\", \
         \"2xl\", \"3xl\", \"4xl\", \"full\"])), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Length))]",
    ),
    (
        "TwVarColor",
        "&[Rule::new(SuffixRule::Color { current: true }), \
         Rule::new(SuffixRule::Arbitrary(ArbitraryKind::Color))]",
    ),
    (
        "VerticalAlign",
        "&[Rule::new(SuffixRule::Keywords(&[\"baseline\", \"top\", \"middle\", \"bottom\", \
         \"text-top\", \"text-bottom\", \"sub\", \"super\"]))]",
    ),
    (
        "WhiteSpace",
        "&[Rule::new(SuffixRule::Keywords(&[\"normal\", \"nowrap\", \"pre\", \"pre-wrap\", \
         \"pre-line\"]))]",
    ),
];

/// blake3 (first 16 hex digits) of each fingerprinted item's token string.
pub(crate) const FINGERPRINTS: &[(&str, &str)] = &[
    ("Breakpoint::parse", "e97764a60788f28b"),
    ("TailwindProperty::parse", "3db361a3474a25a4"),
    ("TailwindProperty::parse_prefix_suffix", "6352172a5cedb7bf"),
    ("TailwindProperty::try_neg", "b3ed25d93c6b959a"),
    ("TailwindValue::parse", "fdfba100222177db"),
    ("extract_arbitrary_value", "5dead4c3011710c6"),
    (
        "impl TailwindPropertyParser for AlignItems",
        "d925ffcb7c38a180",
    ),
    ("impl TailwindPropertyParser for Angle", "dbaf308a67576979"),
    (
        "impl TailwindPropertyParser for Animations",
        "a86be8f7a3949777",
    ),
    (
        "impl TailwindPropertyParser for AspectRatio",
        "3998e67657a01c78",
    ),
    (
        "impl TailwindPropertyParser for BackgroundClip",
        "685b237d056732cb",
    ),
    (
        "impl TailwindPropertyParser for BackgroundImage",
        "b0980a4715904b91",
    ),
    (
        "impl TailwindPropertyParser for BackgroundOrigin",
        "e12190edee1ec86d",
    ),
    (
        "impl TailwindPropertyParser for BackgroundSize",
        "4a8c0d6cc6ff346a",
    ),
    (
        "impl TailwindPropertyParser for BlendMode",
        "9d619dbd67cccd01",
    ),
    (
        "impl TailwindPropertyParser for BorderStyle",
        "e2d25ced971e8ed1",
    ),
    (
        "impl TailwindPropertyParser for BoxShadow",
        "22e0b99e32dd3f8c",
    ),
    ("impl TailwindPropertyParser for Color", "74c5aeced9cac002"),
    (
        "impl TailwindPropertyParser for ColorInput",
        "9ba35778453e3b45",
    ),
    (
        "impl TailwindPropertyParser for ContentValue",
        "d8b1b5715b4e0f96",
    ),
    (
        "impl TailwindPropertyParser for Filters",
        "e9ae3f8768abed12",
    ),
    ("impl TailwindPropertyParser for Flex", "da9e1e105be14413"),
    (
        "impl TailwindPropertyParser for FlexBasis",
        "644d4fae81b17b72",
    ),
    (
        "impl TailwindPropertyParser for FlexGrow",
        "f2f0bec052c89c3f",
    ),
    (
        "impl TailwindPropertyParser for FontFamily",
        "f0fbf4651c310b36",
    ),
    (
        "impl TailwindPropertyParser for FontStretch",
        "c73b442996f8afa9",
    ),
    (
        "impl TailwindPropertyParser for FontWeight",
        "fdcf17e168071b33",
    ),
    (
        "impl TailwindPropertyParser for GridLine",
        "dcd7fe4db525d83d",
    ),
    (
        "impl TailwindPropertyParser for GridPlacement",
        "14a24497dd81e532",
    ),
    (
        "impl TailwindPropertyParser for GridPlacementSpan",
        "dc4655eefa5d81c1",
    ),
    (
        "impl TailwindPropertyParser for GridTrackSize",
        "708c6da271af06ce",
    ),
    (
        "impl TailwindPropertyParser for JustifyContent",
        "746de90503a54f42",
    ),
    ("impl TailwindPropertyParser for Length", "414a4a34fa0f75ac"),
    (
        "impl TailwindPropertyParser for LineClamp",
        "6835e7a4ca4a95c3",
    ),
    (
        "impl TailwindPropertyParser for LineHeight",
        "8a98da5c35fb1b9e",
    ),
    (
        "impl TailwindPropertyParser for LineJoin",
        "c4e8208f7b229dde",
    ),
    (
        "impl TailwindPropertyParser for LineWidth",
        "65de3041ecfe3f44",
    ),
    (
        "impl TailwindPropertyParser for ListStyleImage",
        "dccc2037578c2a9c",
    ),
    (
        "impl TailwindPropertyParser for ListStylePosition",
        "f61903f2f4941bb0",
    ),
    (
        "impl TailwindPropertyParser for ListStyleType",
        "b7c3e7ecb93a985d",
    ),
    (
        "impl TailwindPropertyParser for ObjectFit",
        "f4c6bbb1e010bddd",
    ),
    (
        "impl TailwindPropertyParser for Overflow",
        "8dd91a6760a5a759",
    ),
    (
        "impl TailwindPropertyParser for OverflowWrap",
        "9fe1907fc2a7ff0a",
    ),
    (
        "impl TailwindPropertyParser for PercentageNumber",
        "4394bba100f4319a",
    ),
    (
        "impl TailwindPropertyParser for PositionValue",
        "a03b9ed8ecb1f810",
    ),
    ("impl TailwindPropertyParser for Size", "a77f54835839333b"),
    (
        "impl TailwindPropertyParser for TextAlign",
        "754fc8c2e5a316aa",
    ),
    (
        "impl TailwindPropertyParser for TextDecorationThickness",
        "e48b98c9f2a8873a",
    ),
    (
        "impl TailwindPropertyParser for TextShadow",
        "1d70af3c24748a21",
    ),
    (
        "impl TailwindPropertyParser for TextWrap",
        "3ab0cdbc951abb04",
    ),
    (
        "impl TailwindPropertyParser for TwAnimation",
        "5d5336f7a8638e99",
    ),
    ("impl TailwindPropertyParser for TwBlur", "d20178a4dc2a7cea"),
    (
        "impl TailwindPropertyParser for TwDropShadow",
        "3186a38813d6606b",
    ),
    (
        "impl TailwindPropertyParser for TwFontSize",
        "61190398ec7b4df0",
    ),
    (
        "impl TailwindPropertyParser for TwGradientPosition",
        "5a20a6f858f6f028",
    ),
    (
        "impl TailwindPropertyParser for TwGridTemplate",
        "b420de26affeac7e",
    ),
    (
        "impl TailwindPropertyParser for TwLetterSpacing",
        "8fac2458dd4bccfa",
    ),
    (
        "impl TailwindPropertyParser for TwRounded",
        "6bd3cfaade551d84",
    ),
    (
        "impl TailwindPropertyParser for TwVarColor",
        "d4ae188ed8a22176",
    ),
    (
        "impl TailwindPropertyParser for VerticalAlign",
        "326de5d612d81b5a",
    ),
    (
        "impl TailwindPropertyParser for WebkitLineClamp",
        "d95d6f41ff52f7be",
    ),
    (
        "impl TailwindPropertyParser for WhiteSpace",
        "7bc116d9af1b0b5a",
    ),
    ("is_ident", "8b3514ed7e071e05"),
    ("is_ident_byte", "7d053af486050818"),
    ("macro_rules! property_parsers", "22cc219e065bc6c4"),
    ("macro_rules! try_neg", "712225e5e9552ce4"),
    ("parse_opacity_modifier", "b7a6070c1564c3ce"),
    ("split_variant", "c297dfbd2404208e"),
    ("var_expression", "46cb67a785e73254"),
];

/// Prefix renames: `(v3 prefix, v4 prefix, the suffixes renamed)`.
pub(crate) const PREFIX_RENAMES: &[(&str, &str, &[&str])] = &[(
    "bg-gradient-to-",
    "bg-linear-to-",
    &["t", "tr", "r", "br", "b", "bl", "l", "tl"],
)];

/// Exact renames: `(v3 class, v4 class)`.
pub(crate) const EXACT_RENAMES: &[(&str, &str)] = &[("overflow-ellipsis", "text-ellipsis")];

/// Hints for v3 utilities v4 dropped: `(prefix, hint)`.
pub(crate) const HINTS: &[(&str, &str)] = &[
    ("bg-opacity-", OPACITY_HINT),
    ("text-opacity-", OPACITY_HINT),
    ("border-opacity-", OPACITY_HINT),
    ("space-x-", SPACE_HINT),
    ("space-y-", SPACE_HINT),
];

const OPACITY_HINT: &str = "use an opacity modifier, e.g. `bg-red-500/50`";
const SPACE_HINT: &str = "use `gap-x-*`/`gap-y-*` on a flex or grid parent";
