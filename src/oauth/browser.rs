//! Open the system browser for the authorization URL.

use std::io::{self, Write};

/// Print the auth URL to stderr and attempt to open it in a browser.
pub fn open_authorization_url(url: &str) {
    let _ = writeln!(
        io::stderr(),
        "Open this URL in your browser to authorize:\n\n{url}\n"
    );
    if let Err(e) = open::that(url) {
        let _ = writeln!(
            io::stderr(),
            "(could not open browser automatically: {e}; open the URL manually)"
        );
    }
}
