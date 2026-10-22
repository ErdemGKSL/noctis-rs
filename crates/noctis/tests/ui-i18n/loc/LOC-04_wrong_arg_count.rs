// LOC-04 (by construction): an accessor takes one argument per placeholder.
noctis::i18n!(dir = "../../fixtures/i18n/typed", __dir_from_source_file);

fn main() {
    let strings = Strings::of(noctis::rules::Locale::Tr);
    let _ = strings.greet(); // [LOC-04]
}
