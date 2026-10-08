//! Where CivilOps PDF lives on the web. One table, so the Help menu, the About dialog, the home
//! screen, the CLI and the README agree.

/// The app's name.
pub const APP: &str = "civilops-pdf";
/// CivilOps PDF's version (the crates keep their own numbering).
pub const VERSION: &str = "0.1.5";

pub const WEBSITE: &str = "https://civilops.de";
pub const APP_PAGE: &str = "https://civilops.de";
/// CivilOps PDF's source code (developer tools only, not in the app).
pub const GITHUB: &str = "https://github.com/Checkov23/civilops-pdf";

/// A link and the registry command that opens it.
#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub command: &'static str,
    pub label: &'static str,
    pub url: &'static str,
    /// Lucide icon name.
    pub icon: &'static str,
}

/// In the order they are shown.
pub const LINKS: &[Link] = &[Link { command: "help.website", label: "CivilOps website", url: WEBSITE, icon: "globe" }];

pub fn for_command(id: &str) -> Option<&'static Link> {
    LINKS.iter().find(|l| l.command == id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_are_registered_commands() {
        for l in super::LINKS {
            assert!(l.url.starts_with("https://"), "{}", l.url);
            assert!(crate::commands::command(l.command).is_some(), "{} is a registered command", l.command);
        }
    }
}
