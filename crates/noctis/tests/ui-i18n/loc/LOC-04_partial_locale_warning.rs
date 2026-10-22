// LOC-04: a key missing in a non-default locale is a `deprecated` build
// warning; denied here, it fails the build.
#![deny(deprecated)]

noctis::i18n!(dir = "../../fixtures/i18n/partial", __dir_from_source_file); // [LOC-04]

fn main() {
    assert_eq!(Strings::of(noctis::rules::Locale::Tr).a(), "A");
}
