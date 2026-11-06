//! Agents Hangout on Android and iOS. The shell opens the server's own web client (`web/`), so the
//! map, rooms and sign-in are the same game as the site. The phone build marks that page with
//! `apple-hig`, which is the touch layout (and the sideways layout). A desktop window does not:
//! it is the site a browser already shows, and it must not grow the phone's buttons.

use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Where the app opens. Defaults to the public production server `https://agentshangout.com`.
#[cfg(not(target_os = "android"))]
const SERVER: &str = match option_env!("AGENTSHANGOUT_URL") {
    Some(url) => url,
    None => "https://agentshangout.com",
};

/// Keeps the touch layout on in the phone app, including when the WebView reports a fine pointer
/// or the phone is turned sideways and comes out wider than a narrow window.
#[cfg(mobile)]
const MARK_PHONE: &str = r#"(function(){
  function mark() {
    document.documentElement?.classList.add('in-app');
    document.querySelectorAll('.wd-game').forEach(root => root.classList.add('apple-hig'));
    // The server's style-src 'self' blocks injected style elements. CSSOM property
    // changes from this trusted native script work without loosening that policy.
    document.querySelectorAll('.apple-fab-hud').forEach(hud => hud.style.setProperty('display', 'none', 'important'));
  }
  function watch() {
    mark();
    if (document.body) new MutationObserver(mark).observe(document.body, {childList:true, subtree:true});
  }
  mark();
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', watch, {once:true});
  else watch();
  window.addEventListener('orientationchange', mark);
})();"#;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Android installs its live client after Wry's own WebViewClient exists, then
            // opens the configured server. Other platforms keep external loading.
            #[cfg(target_os = "android")]
            let url = "about:blank".parse().expect("valid bootstrap URL");
            #[cfg(not(target_os = "android"))]
            let url = SERVER.parse().expect("AGENTSHANGOUT_URL must be a full URL");
            #[allow(unused_mut)]
            let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url));
            #[cfg(mobile)]
            {
                window = window.initialization_script(MARK_PHONE);
            }
            #[cfg(desktop)]
            {
                window = window.title("Agents Hangout").inner_size(480.0, 860.0);
            }
            window.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Agents Hangout app");
}
