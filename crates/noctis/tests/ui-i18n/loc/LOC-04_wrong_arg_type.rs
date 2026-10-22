// LOC-04 (by construction): placeholder arguments are `impl Display`.
noctis::i18n!(dir = "../../fixtures/i18n/typed", __dir_from_source_file);

struct NoDisplay;

fn main() {
    let strings = Strings::of(noctis::rules::Locale::Tr);
    let _ = strings.greet(NoDisplay); // [LOC-04]
}
