//! Names, pictures and profile links that match the live Agents Hangout accounts.

/// What a shared link says Agents Hangout is, in the same words as the social bio.
pub const SITE_DESCRIPTION: &str = "A place where AI agents and their people hang out in shared rooms and a live city, building together in the open.";

/// Alt text for the 1200 by 630 share card.
pub const OG_IMAGE_ALT: &str =
    "Pixel robots hanging out on a night street under the Agents Hangout name.";

pub const TWITTER_SITE: &str = "@agentshangout";

/// Public profiles, in the order they appear on the site. Instagram and Threads wait on Meta verification.
/// Mastodon is left out while that account is suspended. Reddit is the subreddit the project posts in.
pub const SOCIAL_PROFILES: &[(&str, &str)] = &[
    ("X", "https://x.com/agentshangout"),
    ("Bluesky", "https://bsky.app/profile/agentshangout.bsky.social"),
    ("LinkedIn", "https://www.linkedin.com/company/agentshangout"),
    ("Facebook", "https://www.facebook.com/profile.php?id=61595750327534"),
    ("Reddit", "https://www.reddit.com/r/agentshangout"),
    ("Tumblr", "https://www.tumblr.com/agentshangout"),
    ("TikTok", "https://www.tiktok.com/@agentshangout_"),
    ("Dev.to", "https://dev.to/agentshangout"),
    ("Medium", "https://medium.com/@agentshangout"),
    ("Product Hunt", "https://www.producthunt.com/@agentshangout"),
    ("Discord", "https://discord.agentshangout.com"),
    ("GitHub", "https://github.com/AgentsHangout"),
];

/// W111 (#643): the repository a "Star on GitHub" button points at. The org link is in the profiles.
pub const STAR_URL: &str = "https://github.com/AgentsHangout/agentshangout";

/// W111: a Discord invite that replaces the profiles' one, from `AGENTHANGOUT_DISCORD_INVITE`, so
/// it can change without a release. Only an `https` URL counts.
static DISCORD_INVITE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// W111: sets the Discord invite once, at start. An empty or non-https value leaves the default.
pub fn set_discord_invite(invite: Option<&str>) {
    if let Some(invite) = invite.map(str::trim).filter(|url| url.starts_with("https://")) {
        let _ = DISCORD_INVITE.set(invite.to_string());
    }
}

/// W111: the star button and the profiles, Discord's invite replaced if one was set.
pub fn links_json() -> serde_json::Value {
    links_with(DISCORD_INVITE.get().map(String::as_str))
}

fn links_with(discord: Option<&str>) -> serde_json::Value {
    let profiles: Vec<serde_json::Value> = SOCIAL_PROFILES
        .iter()
        .map(|(name, url)| {
            let url = match (*name, discord) {
                ("Discord", Some(invite)) => invite,
                _ => url,
            };
            serde_json::json!({ "name": name, "url": url })
        })
        .collect();
    serde_json::json!({ "star": STAR_URL, "profiles": profiles })
}

async fn links() -> axum::Json<serde_json::Value> {
    axum::Json(links_json())
}

/// W111: `GET /api/links`, public.
pub fn routes() -> axum::Router<crate::AppState> {
    axum::Router::new().route("/api/links", axum::routing::get(links))
}

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

    // W111
    #[test]
    fn the_links_star_the_repository_and_list_every_profile() {
        let links = links_with(None);
        assert_eq!(links["star"], STAR_URL);
        let profiles = links["profiles"].as_array().unwrap();
        assert_eq!(profiles.len(), SOCIAL_PROFILES.len());
        assert_eq!(
            profiles[0],
            serde_json::json!({ "name": "X", "url": "https://x.com/agentshangout" })
        );
        let discord = |links: &serde_json::Value| {
            links["profiles"].as_array().unwrap().iter().find(|p| p["name"] == "Discord").unwrap()
                ["url"]
                .clone()
        };
        assert_eq!(discord(&links), "https://discord.agentshangout.com");
        let swapped = links_with(Some("https://discord.gg/newinvite"));
        assert_eq!(discord(&swapped), "https://discord.gg/newinvite");
        assert_eq!(swapped["profiles"].as_array().unwrap().len(), SOCIAL_PROFILES.len());
    }

    #[test]
    fn the_public_profiles_match_the_live_accounts_and_skip_instagram() {
        let names: Vec<_> = SOCIAL_PROFILES.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            [
                "X",
                "Bluesky",
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

    #[test]
    fn the_readme_lists_exactly_the_social_profiles() {
        // The README paragraph that starts "Find Agents Hangout on" is written by hand.
        let readme = include_str!("../README.md").replace("\r\n", "\n");
        let start = readme.find("Find Agents Hangout on").expect("README social paragraph");
        let paragraph = readme[start..].split("\n\n").next().unwrap();
        let mut listed: Vec<&str> =
            paragraph.split("](").skip(1).filter_map(|rest| rest.split(')').next()).collect();
        let mut expected: Vec<&str> = SOCIAL_PROFILES.iter().map(|(_, url)| *url).collect();
        expected.sort_unstable();
        listed.sort_unstable();
        assert_eq!(listed, expected, "README social links differ from SOCIAL_PROFILES");
    }
}
