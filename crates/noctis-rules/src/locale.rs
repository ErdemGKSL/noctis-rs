//! Discord's 32 locale codes (LOC-01): [`Locale`] and [`ParseLocaleError`].

use std::error::Error;
use std::fmt;
use std::str::FromStr;

macro_rules! locales {
    ($($variant:ident = $code:literal, $language:literal;)*) => {
        /// One of Discord's 32 locale codes (LOC-01), named after its code.
        ///
        /// The set is closed and listed in Discord's order by [`Locale::ALL`].
        /// Parsing is exact and case-sensitive: `"en-US"` parses, `"en"`,
        /// `"en-us"` and `"en_US"` do not (see [`ParseLocaleError`]).
        ///
        /// | Variant | Code | Language |
        /// |---|---|---|
        $(#[doc = concat!("| `", stringify!($variant), "` | `", $code, "` | ", $language, " |")])*
        ///
        /// ```
        /// use noctis_rules::Locale;
        ///
        /// assert_eq!("en-US".parse::<Locale>(), Ok(Locale::EnUs));
        /// assert_eq!(Locale::EnUs.sibling(), Some(Locale::EnGb));
        /// assert!("en".parse::<Locale>().is_err());
        /// ```
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Locale {
            $(
                #[doc = concat!("`", $code, "`: ", $language, ".")]
                $variant,
            )*
        }

        impl Locale {
            /// Every locale, in Discord's documented order.
            pub const ALL: [Locale; 32] = [$(Locale::$variant,)*];

            /// The Discord locale code, for example `"en-US"` or `"tr"`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Locale::$variant => $code,)*
                }
            }
        }
    };
}

locales! {
    Id = "id", "Indonesian";
    Da = "da", "Danish";
    De = "de", "German";
    EnGb = "en-GB", "English, UK";
    EnUs = "en-US", "English, US";
    EsEs = "es-ES", "Spanish";
    Es419 = "es-419", "Spanish, LATAM";
    Fr = "fr", "French";
    Hr = "hr", "Croatian";
    It = "it", "Italian";
    Lt = "lt", "Lithuanian";
    Hu = "hu", "Hungarian";
    Nl = "nl", "Dutch";
    No = "no", "Norwegian";
    Pl = "pl", "Polish";
    PtBr = "pt-BR", "Portuguese, Brazilian";
    Ro = "ro", "Romanian, Romania";
    Fi = "fi", "Finnish";
    SvSe = "sv-SE", "Swedish";
    Vi = "vi", "Vietnamese";
    Tr = "tr", "Turkish";
    Cs = "cs", "Czech";
    El = "el", "Greek";
    Bg = "bg", "Bulgarian";
    Ru = "ru", "Russian";
    Uk = "uk", "Ukrainian";
    Hi = "hi", "Hindi";
    Th = "th", "Thai";
    ZhCn = "zh-CN", "Chinese, China";
    Ja = "ja", "Japanese";
    ZhTw = "zh-TW", "Chinese, Taiwan";
    Ko = "ko", "Korean";
}

impl Locale {
    /// The same-language fallback locale: `en-US` ↔ `en-GB` and
    /// `es-ES` ↔ `es-419`. Every other locale has none.
    pub const fn sibling(self) -> Option<Locale> {
        match self {
            Self::EnUs => Some(Self::EnGb),
            Self::EnGb => Some(Self::EnUs),
            Self::EsEs => Some(Self::Es419),
            Self::Es419 => Some(Self::EsEs),
            _ => None,
        }
    }

    /// The language part of the code, before any `-`.
    fn language(self) -> &'static str {
        let code = self.as_str();
        code.split_once('-').map_or(code, |(language, _)| language)
    }
}

impl fmt::Display for Locale {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Locale {
    type Err = ParseLocaleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(locale) = Self::ALL.into_iter().find(|l| l.as_str() == s) {
            return Ok(locale);
        }
        let language = s.split(['-', '_']).next().unwrap_or_default();
        let suggestions = if language.is_empty() {
            Vec::new()
        } else {
            Self::ALL
                .into_iter()
                .filter(|l| l.language().eq_ignore_ascii_case(language))
                .collect()
        };
        Err(ParseLocaleError {
            input: s.to_owned(),
            suggestions,
        })
    }
}

/// The error for a string that is not exactly one of Discord's locale codes.
///
/// Displays as `` [LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US` ``;
/// the `; matching codes` part is left out when there are no suggestions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseLocaleError {
    input: String,
    suggestions: Vec<Locale>,
}

impl ParseLocaleError {
    /// The string that failed to parse.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The codes with the same language part as the input (ASCII case-insensitive),
    /// in [`Locale::ALL`] order; empty when nothing matches.
    pub fn suggestions(&self) -> &[Locale] {
        &self.suggestions
    }
}

impl fmt::Display for ParseLocaleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[LOC-01] `{}` is not a Discord locale code",
            self.input.escape_debug()
        )?;
        for (i, locale) in self.suggestions.iter().enumerate() {
            let sep = if i == 0 { "; matching codes: " } else { ", " };
            write!(f, "{sep}`{locale}`")?;
        }
        Ok(())
    }
}

impl Error for ParseLocaleError {}

#[cfg(test)]
mod tests {
    use super::*;
    use Locale::{EnGb, EnUs, Es419, EsEs, No, PtBr, SvSe, Tr, ZhCn, ZhTw};

    fn err(s: &str) -> ParseLocaleError {
        s.parse::<Locale>().unwrap_err()
    }

    #[test]
    fn all_is_loc_01_in_order() {
        let codes: Vec<_> = Locale::ALL.iter().map(|l| l.as_str()).collect();
        assert_eq!(
            codes.join(", "),
            "id, da, de, en-GB, en-US, es-ES, es-419, fr, hr, it, lt, hu, nl, no, pl, pt-BR, \
             ro, fi, sv-SE, vi, tr, cs, el, bg, ru, uk, hi, th, zh-CN, ja, zh-TW, ko"
        );
    }

    #[test]
    fn every_code_round_trips() {
        for locale in Locale::ALL {
            assert_eq!(locale.as_str().parse::<Locale>(), Ok(locale));
            assert_eq!(locale.to_string(), locale.as_str());
        }
    }

    #[test]
    fn exact_codes_parse() {
        for (code, want) in [
            ("en-US", EnUs),
            ("es-419", Es419),
            ("no", No),
            ("tr", Tr),
            ("zh-TW", ZhTw),
        ] {
            assert_eq!(code.parse::<Locale>(), Ok(want));
        }
    }

    #[test]
    fn bare_language_suggests_codes() {
        let e = err("en");
        assert_eq!(e.input(), "en");
        assert_eq!(e.suggestions(), [EnGb, EnUs]);
        assert_eq!(
            e.to_string(),
            "[LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US`"
        );
    }

    #[test]
    fn wrong_case_separator_or_space_fails() {
        for input in ["en-us", "EN-US", "en_US", "en-US "] {
            assert_eq!(err(input).suggestions(), [EnGb, EnUs], "{input:?}");
        }
        assert_eq!(err(" tr").suggestions(), []);
    }

    #[test]
    fn other_region_suggests_same_language() {
        assert_eq!(err("es-MX").suggestions(), [EsEs, Es419]);
        assert_eq!(err("pt-PT").suggestions(), [PtBr]);
        assert_eq!(err("zh").suggestions(), [ZhCn, ZhTw]);
        assert_eq!(err("sv").suggestions(), [SvSe]);
    }

    #[test]
    fn unknown_input_has_no_suggestions() {
        for input in ["xx", "", "-US", "türkçe", "nb"] {
            let e = err(input);
            assert_eq!(e.suggestions(), [], "{input:?}");
            assert_eq!(
                e.to_string(),
                format!("[LOC-01] `{input}` is not a Discord locale code")
            );
            let _: &dyn Error = &e;
        }
    }

    #[test]
    fn sibling_pairs() {
        assert_eq!(EnUs.sibling(), Some(EnGb));
        assert_eq!(EnGb.sibling(), Some(EnUs));
        assert_eq!(EsEs.sibling(), Some(Es419));
        assert_eq!(Es419.sibling(), Some(EsEs));
    }

    #[test]
    fn other_locales_have_no_sibling_and_sibling_is_an_involution() {
        let paired = [EnUs, EnGb, EsEs, Es419];
        for locale in Locale::ALL {
            match locale.sibling() {
                Some(s) => assert_eq!(s.sibling(), Some(locale)),
                None => assert!(!paired.contains(&locale), "{locale}"),
            }
        }
        let unpaired = Locale::ALL.iter().filter(|l| l.sibling().is_none()).count();
        assert_eq!(unpaired, 28);
    }
}
