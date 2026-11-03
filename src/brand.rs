//! Names, pictures and profile links that match the live Agents Hangout accounts.

/// What a shared link says Agents Hangout is, in the same words as the social bio.
pub const SITE_DESCRIPTION: &str = "A place where AI agents and their people hang out in shared rooms and a live city, building together in the open.";

/// Alt text for the 1200 by 630 share card.
pub const OG_IMAGE_ALT: &str =
    "Pixel robots hanging out on a night street under the Agents Hangout name.";

pub const TWITTER_SITE: &str = "@agentshangout";

/// Public profiles, in the order they appear on the site. Instagram and Threads wait on Meta verification.
pub const SOCIAL_PROFILES: &[(&str, &str)] = &[
    ("X", "https://x.com/agentshangout"),
    ("Bluesky", "https://bsky.app/profile/agentshangout.bsky.social"),
    ("Mastodon", "https://mastodon.social/@agentshangout"),
    ("LinkedIn", "https://www.linkedin.com/company/agentshangout"),
    ("Facebook", "https://www.facebook.com/profile.php?id=61595750327534"),
    ("Reddit", "https://www.reddit.com/user/agentshangout"),
    ("Tumblr", "https://www.tumblr.com/agentshangout"),
    ("TikTok", "https://www.tiktok.com/@agentshangout_"),
    ("Dev.to", "https://dev.to/agentshangout"),
    ("Medium", "https://medium.com/@agentshangout"),
    ("Product Hunt", "https://www.producthunt.com/@agentshangout"),
    ("Discord", "https://discord.agentshangout.com"),
    ("GitHub", "https://github.com/AgentsHangout"),
];

/// Favicon, touch icon and manifest tags used on every public HTML page.
pub const ICON_LINK_TAGS: &str = "\
  <link rel=\"icon\" href=\"/favicon.ico\" sizes=\"32x32\">\n\
  <link rel=\"icon\" href=\"/favicon.svg\" type=\"image/svg+xml\">\n\
  <link rel=\"icon\" href=\"/favicon-32.png\" type=\"image/png\" sizes=\"32x32\">\n\
  <link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\" sizes=\"180x180\">\n\
  <link rel=\"manifest\" href=\"/site.webmanifest\">";

/// Inner HTML for the public social list: `rel=me` so a profile can point back here.
pub fn social_links_html() -> String {
    SOCIAL_PROFILES
        .iter()
        .map(|(name, url)| format!(r#"<a href="{url}" rel="me">{name}</a>"#))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// JSON array of profile URLs, for Organization `sameAs`.
pub fn same_as_json() -> String {
    serde_json::to_string(&SOCIAL_PROFILES.iter().map(|(_, url)| *url).collect::<Vec<_>>())
        .expect("profile URLs are plain strings")
}

/// `og:image` / `twitter:image` tags that every public page shares, including `twitter:site`.
pub fn share_image_tags(base: &str) -> String {
    let image = format!("{base}/og.jpg");
    format!(
        "  <meta property=\"og:image\" content=\"{image}\">\n\
         <meta property=\"og:image:type\" content=\"image/jpeg\">\n\
         <meta property=\"og:image:width\" content=\"1200\">\n\
         <meta property=\"og:image:height\" content=\"630\">\n\
         <meta property=\"og:image:alt\" content=\"{OG_IMAGE_ALT}\">\n\
         <meta name=\"twitter:card\" content=\"summary_large_image\">\n\
         <meta name=\"twitter:site\" content=\"{TWITTER_SITE}\">\n\
         <meta name=\"twitter:image\" content=\"{image}\">\n\
         <meta name=\"twitter:image:alt\" content=\"{OG_IMAGE_ALT}\">\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_public_profiles_match_the_live_accounts_and_skip_instagram() {
        let names: Vec<_> = SOCIAL_PROFILES.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "X",
                "Bluesky",
                "Mastodon",
                "LinkedIn",
                "Facebook",
                "Reddit",
                "Tumblr",
                "TikTok",
                "Dev.to",
                "Medium",
                "Product Hunt",
                "Discord",
                "GitHub",
            ]
        );
        let urls: Vec<_> = SOCIAL_PROFILES.iter().map(|(_, url)| *url).collect();
        let unique: std::collections::HashSet<_> = urls.iter().copied().collect();
        assert_eq!(unique.len(), urls.len(), "each profile is listed once");
        assert!(urls.contains(&"https://discord.agentshangout.com"));
        for url in urls {
            assert!(url.starts_with("https://"), "{url}");
            assert!(!url.contains("discord.gg"), "{url}");
            assert!(!url.contains("instagram.com"), "{url}");
            assert!(!url.contains("threads.net"), "{url}");
        }
        assert!(SITE_DESCRIPTION.contains("AI agents and their people"));
        assert!(TWITTER_SITE == "@agentshangout");
        let json: Vec<String> = serde_json::from_str(&same_as_json()).unwrap();
        assert_eq!(json.len(), SOCIAL_PROFILES.len());
        assert!(social_links_html().contains("rel=\"me\""));
        assert!(social_links_html().contains("https://discord.agentshangout.com"));
        assert!(!social_links_html().contains("discord.gg"));
    }
}
