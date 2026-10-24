//! The `RemoteImage` constructors, URL grammar, size normalization, CDN
//! URLs and cache keys (IMG-03).

use twilight_model::id::Id;

use super::*;

const CDN: &str = "https://cdn.discordapp.com";
/// A still and an animated avatar hash.
const STILL: &str = "1269e74af4df7417b13759eae50c83dc";
const ANIMATED: &str = "a_1269e74af4df7417b13759eae50c83dc";

fn user(id: u64, avatar: Option<&str>, discriminator: u16) -> User {
    User {
        accent_color: None,
        avatar: avatar.map(|hash| hash.parse().unwrap()),
        avatar_decoration: None,
        avatar_decoration_data: None,
        banner: None,
        bot: false,
        discriminator,
        email: None,
        flags: None,
        global_name: None,
        id: Id::new(id),
        locale: None,
        mfa_enabled: None,
        name: "u".to_owned(),
        premium_type: None,
        primary_guild: None,
        public_flags: None,
        system: None,
        verified: None,
    }
}

fn guild(id: u64, icon: Option<&str>) -> Guild {
    let icon = icon.map_or_else(|| "null".to_owned(), |hash| format!("\"{hash}\""));
    serde_json::from_str(&format!(
        r#"{{"afk_timeout":60,"default_message_notifications":0,"explicit_content_filter":0,
        "features":[],"id":"{id}","mfa_level":0,"name":"g","owner_id":"2",
        "preferred_locale":"en-US","roles":[],"system_channel_flags":0,
        "premium_progress_bar_enabled":false,"verification_level":0,"nsfw_level":0,
        "icon":{icon}}}"#
    ))
    .unwrap()
}

fn target(image: &RemoteImage, animated: bool) -> Target {
    image.target(CDN, animated).unwrap().unwrap()
}

#[test]
fn img_03_url_ok_and_canonical() {
    let image = RemoteImage::url("https://cdn.discordapp.com/a/b.png?x=1").unwrap();
    assert_eq!(
        target(&image, false).url,
        "https://cdn.discordapp.com/a/b.png?x=1"
    );
    assert_eq!(image.to_string(), "https://cdn.discordapp.com/a/b.png");
    let upper = RemoteImage::url("HTTPS://CDN.DiscordApp.com/a").unwrap();
    assert_eq!(target(&upper, false).url, "https://cdn.discordapp.com/a");
    let port = RemoteImage::url("https://h:8443?q=1").unwrap();
    assert_eq!(target(&port, false).url, "https://h:8443/?q=1");
    assert_eq!(port.to_string(), "https://h:8443/");
}

#[test]
fn img_03_url_bounds_and_bytes() {
    assert_eq!(RemoteImage::url(""), Err(RemoteUrlError::Empty));
    let base = "https://h/";
    let ok = format!("{base}{}", "a".repeat(2048 - base.len()));
    assert!(RemoteImage::url(&ok).is_ok());
    let long = format!("{ok}a");
    assert_eq!(
        RemoteImage::url(&long),
        Err(RemoteUrlError::TooLong { len: 2049 })
    );
    for (url, index) in [
        ("https://h/\u{e9}", 10),
        ("https://h/a b", 11),
        ("https://h/\u{7}", 10),
    ] {
        assert_eq!(
            RemoteImage::url(url),
            Err(RemoteUrlError::InvalidCharacter { index }),
            "{url}"
        );
    }
}

#[test]
fn img_03_url_canonical_form_respects_the_length_limit() {
    // An empty path gains a `/`, so a 2048-byte input becomes 2049 bytes.
    let base = "https://h?q=";
    let input = format!("{base}{}", "a".repeat(2048 - base.len()));
    assert_eq!(input.len(), 2048);
    assert_eq!(
        RemoteImage::url(&input),
        Err(RemoteUrlError::TooLong { len: 2049 })
    );
    let fits = &input[..2047];
    let image = RemoteImage::url(fits).unwrap();
    assert_eq!(target(&image, false).url.len(), 2048);
}

#[test]
fn img_03_url_shape_and_parts() {
    for url in [
        "ftp://x/y",
        "file:///etc/passwd",
        "data:image/png;base64,AAAA",
        "//h/x",
        "cdn.discordapp.com/x",
    ] {
        assert_eq!(RemoteImage::url(url), Err(RemoteUrlError::Scheme), "{url}");
    }
    for (url, error) in [
        ("https://u:p@h/x", RemoteUrlError::UserInfo),
        ("https://h/x#f", RemoteUrlError::Fragment),
        ("https:///x", RemoteUrlError::NoHost),
        ("https://:443/x", RemoteUrlError::NoHost),
        ("https://h:99999/x", RemoteUrlError::Malformed),
        ("https://h:0/x", RemoteUrlError::Malformed),
        ("https://h*/x", RemoteUrlError::InvalidHost),
    ] {
        assert_eq!(RemoteImage::url(url), Err(error), "{url}");
    }
    assert!(RemoteImage::url("http://[::1]:8080/x").is_ok());
    assert!(RemoteImage::url("http://127.0.0.1:1/x").is_ok());
}

#[test]
fn img_03_sizes_normalize_to_cdn_powers_of_two() {
    for (size, expected) in [
        (0, 16),
        (15, 16),
        (16, 16),
        (17, 32),
        (256, 256),
        (257, 512),
        (4096, 4096),
        (4097, 4096),
        (u16::MAX, 4096),
    ] {
        assert_eq!(normalize_size(size), expected, "{size}");
    }
}

#[test]
fn img_03_avatar_urls_and_keys() {
    let still = RemoteImage::avatar(&user(80_351_110_224_678_912, Some(STILL), 0), 256);
    assert_eq!(
        target(&still, true),
        Target {
            url: format!("{CDN}/avatars/80351110224678912/{STILL}.webp?size=256"),
            key: format!("img:v1:avatar:80351110224678912:{STILL}:256:s"),
        }
    );
    assert_eq!(
        still.to_string(),
        format!("avatar/80351110224678912/{STILL}@256")
    );
    let animated = RemoteImage::avatar(&user(1, Some(ANIMATED), 0), 200);
    assert_eq!(
        target(&animated, true),
        Target {
            url: format!("{CDN}/avatars/1/{ANIMATED}.webp?size=256&animated=true"),
            key: format!("img:v1:avatar:1:{ANIMATED}:256:a"),
        }
    );
    // A still job requests the still frame, under its own key.
    let still_job = target(&animated, false);
    assert_eq!(
        still_job.url,
        format!("{CDN}/avatars/1/{ANIMATED}.webp?size=256")
    );
    assert!(still_job.key.ends_with(":s"));
    // A changed hash is a new key.
    let other = RemoteImage::avatar(&user(1, Some("0269e74af4df7417b13759eae50c83dc"), 0), 256);
    assert_ne!(target(&other, false).key, target(&animated, false).key);
    assert_eq!(
        format!("{animated:?}"),
        format!("RemoteImage(avatar/1/{ANIMATED}@256)")
    );
}

#[test]
fn img_03_default_avatars() {
    let migrated = RemoteImage::avatar(&user(80_351_110_224_678_912, None, 0), 256);
    let index = (80_351_110_224_678_912_u64 >> 22) % 6;
    assert_eq!(
        target(&migrated, false),
        Target {
            url: format!("{CDN}/embed/avatars/{index}.png"),
            key: format!("img:v1:default-avatar:{index}"),
        }
    );
    let legacy = RemoteImage::avatar(&user(1, None, 1234), 256);
    assert_eq!(
        target(&legacy, false).url,
        format!("{CDN}/embed/avatars/4.png")
    );
    assert_eq!(legacy.to_string(), "default-avatar/4");
}

#[test]
fn img_03_guild_icons() {
    let icon = RemoteImage::guild_icon(&guild(41_771_983_423_143_937, Some(ANIMATED)), 128);
    assert_eq!(
        target(&icon, true),
        Target {
            url: format!("{CDN}/icons/41771983423143937/{ANIMATED}.webp?size=128&animated=true"),
            key: format!("img:v1:icon:41771983423143937:{ANIMATED}:128:a"),
        }
    );
    assert_eq!(
        icon.to_string(),
        format!("icon/41771983423143937/{ANIMATED}@128")
    );
    let none = RemoteImage::guild_icon(&guild(1, None), 128);
    assert_eq!(none, RemoteImage::placeholder());
    assert_eq!(none.target(CDN, false).unwrap(), None);
}

#[test]
fn img_03_url_keys_hash_the_canonical_url() {
    let a = RemoteImage::url("HTTPS://H/x.png?q=1").unwrap();
    let b = RemoteImage::url("https://h/x.png?q=1").unwrap();
    let key = target(&a, false).key;
    assert_eq!(key, target(&b, false).key);
    assert_eq!(
        key,
        format!(
            "img:v1:url:{}",
            blake3::hash(b"https://h/x.png?q=1").to_hex()
        )
    );
}

#[test]
fn img_03_decoded_data_is_revalidated() {
    // Template data is untrusted: a forged job value is checked again.
    let forged = RemoteImage {
        source: Source::Url("https://u@h/x".to_owned()),
    };
    assert_eq!(
        forged.target(CDN, false),
        Err(RemoteUrlError::UserInfo.to_string())
    );
    assert_eq!(forged.to_string(), "invalid-url");
    let index = RemoteImage {
        source: Source::DefaultAvatar { index: 9 },
    };
    assert!(index.target(CDN, false).is_err());
    let size = RemoteImage {
        source: Source::Avatar {
            user: 1,
            hash: [0; 16],
            animated: false,
            size: 9999,
        },
    };
    assert!(target(&size, false).url.ends_with("?size=4096"));
    // Round trip through postcard, as Template data does.
    let image = RemoteImage::avatar(&user(7, Some(ANIMATED), 0), 64);
    let bytes = postcard_round_trip(&image);
    assert_eq!(bytes, image);
}

fn postcard_round_trip(image: &RemoteImage) -> RemoteImage {
    let job = noctis_ports::encode_value(1, image).unwrap();
    noctis_ports::decode_value(1, &job).unwrap().unwrap()
}
