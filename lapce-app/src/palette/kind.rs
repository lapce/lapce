#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKind {
    File,
    Line,
    Command,
    Workspace,
    Reference,
    DocumentSymbol,
    WorkspaceSymbol,
    SshHost,
    RunAndDebug,
    ColorTheme,
    IconTheme,
}

impl PaletteKind {
    /// The symbol/prefix that is used to signify the behavior of the palette.
    pub fn symbol(&self) -> &'static str {
        match &self {
            PaletteKind::Line => "/",
            PaletteKind::DocumentSymbol => "@",
            PaletteKind::WorkspaceSymbol => "#",
            // PaletteKind::GlobalSearch => "?",
            PaletteKind::Workspace => ">",
            PaletteKind::Command => ":",
            PaletteKind::File
            | PaletteKind::Reference
            | PaletteKind::ColorTheme
            | PaletteKind::IconTheme
            | PaletteKind::SshHost
            | PaletteKind::RunAndDebug
            // | PaletteKind::Language 
              => "",
        }
    }

    /// Extract the palette kind from the input string. This is most often a prefix.
    pub fn from_input(input: &str) -> PaletteKind {
        match input {
            _ if input.starts_with('/') => PaletteKind::Line,
            _ if input.starts_with('@') => PaletteKind::DocumentSymbol,
            _ if input.starts_with('#') => PaletteKind::WorkspaceSymbol,
            _ if input.starts_with('>') => PaletteKind::Workspace,
            _ if input.starts_with(':') => PaletteKind::Command,
            _ => PaletteKind::File,
        }
    }

    // pub fn has_preview(&self) -> bool {
    //     matches!(
    //         self,
    //         PaletteType::Line
    //             | PaletteType::DocumentSymbol
    //             | PaletteType::WorkspaceSymbol
    //             | PaletteType::GlobalSearch
    //             | PaletteType::Reference
    //     )
    // }

    pub fn get_input<'a>(&self, input: &'a str) -> &'a str {
        match self {
            PaletteKind::File
            | PaletteKind::Reference
            | PaletteKind::ColorTheme
            | PaletteKind::IconTheme
            // | PaletteKind::Language
            | PaletteKind::RunAndDebug
            | PaletteKind::SshHost
             => input,
            PaletteKind::Command
            | PaletteKind::Workspace
            | PaletteKind::DocumentSymbol
            | PaletteKind::WorkspaceSymbol
            | PaletteKind::Line
            // | PaletteType::GlobalSearch
             => if !input.is_empty() {&input[1..]} else {input},
        }
    }

    /// Get the palette kind that it should be considered as based on the current
    /// [`PaletteKind`] and the current input.
    pub fn get_palette_kind(&self, input: &str) -> PaletteKind {
        if self != &PaletteKind::File && self.symbol() == "" {
            return *self;
        }
        PaletteKind::from_input(input)
    }
}
