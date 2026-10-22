// LOC-04 (by construction): a key the default locale lacks has no accessor
// and no `keys::` const.
noctis::i18n!(dir = "../../fixtures/i18n/typed", __dir_from_source_file);

fn main() {
    let strings = Strings::of(noctis::rules::Locale::Tr);
    let _ = strings.shop.missing(); // [LOC-04]
    let _ = keys::SHOP_MISSING; // [LOC-04]
}
