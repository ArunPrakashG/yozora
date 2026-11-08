//! The icons a row can ask for when it has no image of its own. The names are the ones the
//! GPUI launcher's icon files use; a UI maps them onto whatever set it draws.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Glyph {
    Search,
    Window,
    Clipboard,
    Folder,
    Smile,
    Calculator,
    Shell,
    Globe,
    Code,
    Terminal,
    Star,
    Clock,
    Image,
    Link,
    Lock,
    Moon,
    Settings,
    Power,
    Reset,
    Logout,
    Music,
    Package,
    File,
}

impl Glyph {
    pub fn name(self) -> &'static str {
        match self {
            Glyph::Search => "search",
            Glyph::Window => "window",
            Glyph::Clipboard => "clipboard",
            Glyph::Folder => "folder",
            Glyph::Smile => "smile",
            Glyph::Calculator => "calculator",
            Glyph::Shell => "shell",
            Glyph::Globe => "globe",
            Glyph::Code => "code",
            Glyph::Terminal => "terminal",
            Glyph::Star => "star",
            Glyph::Clock => "clock",
            Glyph::Image => "image",
            Glyph::Link => "link",
            Glyph::Lock => "lock",
            Glyph::Moon => "moon",
            Glyph::Settings => "settings",
            Glyph::Power => "power",
            Glyph::Reset => "reset",
            Glyph::Logout => "logout",
            Glyph::Music => "music",
            Glyph::Package => "package",
            Glyph::File => "file",
        }
    }
}
