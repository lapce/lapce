use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Returns the first item yielded from `items` if at least one item is yielded, all yielded items
/// are `Some`, and all yielded items compare equal, else returns `None`.
fn try_all_equal_value<T: PartialEq, I: IntoIterator<Item = Option<T>>>(
    items: I,
) -> Option<T> {
    let mut items = items.into_iter();
    let first = items.next().flatten()?;

    items.try_fold(first, |initial_item, item| {
        item.and_then(|item| (item == initial_item).then_some(initial_item))
    })
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct IconThemeConfig {
    #[serde(skip)]
    pub path: PathBuf,
    pub name: String,
    pub use_editor_color: Option<bool>,
    pub ui: IndexMap<String, String>,
    pub foldername: IndexMap<String, String>,
    pub filename: IndexMap<String, String>,
    pub extension: IndexMap<String, String>,
}

impl IconThemeConfig {
    /// If all paths in `paths` have the same file type (as determined by the file name or
    /// extension), and there is an icon associated with that file type, returns the path of the
    /// icon.
    pub fn resolve_path_to_icon(&self, paths: &[&Path]) -> Option<PathBuf> {
        let file_names = paths
            .iter()
            .map(|path| path.file_name().and_then(OsStr::to_str));
        let file_name_icon = try_all_equal_value(file_names)
            .and_then(|file_name| self.filename.get(file_name));

        file_name_icon
            .or_else(|| {
                let extensions = paths
                    .iter()
                    .map(|path| path.extension().and_then(OsStr::to_str));

                try_all_equal_value(extensions)
                    .and_then(|extension| self.extension.get(extension))
            })
            .map(|icon| self.path.join(icon))
    }
}

#[cfg(test)]
mod tests {
    use crate::config::icon_theme::try_all_equal_value;

    use super::IconThemeConfig;

    #[test]
    fn try_all_equal_value_empty_none() {
        assert_eq!(Option::<u32>::None, try_all_equal_value([]));
    }

    #[test]
    fn try_all_equal_value_any_none_none() {
        assert_eq!(Option::<u32>::None, try_all_equal_value([None]));
        assert_eq!(
            Option::<i32>::None,
            try_all_equal_value([None, Some(1), Some(1)])
        );
        assert_eq!(Option::<u64>::None, try_all_equal_value([Some(0), None]));
        assert_eq!(
            Option::<u8>::None,
            try_all_equal_value([Some(3), Some(3), None, Some(3)])
        );
    }

    #[test]
    fn try_all_equal_value_any_different_none() {
        assert_eq!(Option::<u32>::None, try_all_equal_value([Some(1), Some(2)]));
        assert_eq!(
            Option::<u128>::None,
            try_all_equal_value([Some(1), Some(10), Some(1)])
        );
        assert_eq!(
            Option::<i16>::None,
            try_all_equal_value([Some(3), Some(3), Some(3), Some(3), Some(2)])
        );
        assert_eq!(
            Option::<i64>::None,
            try_all_equal_value([Some(5), Some(4), Some(4), Some(4), Some(4)])
        );
        assert_eq!(
            Option::<i128>::None,
            try_all_equal_value([Some(3), Some(0), Some(9), Some(20), Some(1)])
        );
    }

    #[test]
    fn try_all_equal_value_all_same_some() {
        assert_eq!(Option::<u32>::Some(1), try_all_equal_value([Some(1)]));
        assert_eq!(Option::<i16>::Some(-2), try_all_equal_value([Some(-2); 2]));
        assert_eq!(Option::<i128>::Some(0), try_all_equal_value([Some(0); 3]));
        assert_eq!(Option::<u8>::Some(30), try_all_equal_value([Some(30); 57]));
    }

    fn get_icon_theme_config() -> IconThemeConfig {
        IconThemeConfig {
            path: "icons".to_owned().into(),
            filename: [("Makefile", "makefile.svg"), ("special.rs", "special.svg")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .into(),
            extension: [("rs", "rust.svg"), ("c", "c.svg"), ("py", "python.svg")]
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .into(),
            ..Default::default()
        }
    }

    #[test]
    fn resolve_path_to_icon_no_paths_none() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(None, icon_theme_config.resolve_path_to_icon(&[]));
    }

    #[test]
    fn resolve_path_to_icon_different_none() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            None,
            icon_theme_config
                .resolve_path_to_icon(&["foo.rs", "bar.c"].map(AsRef::as_ref))
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["/some/path/main.py", "other/path.py", "dir1/./dir2/file.rs"]
                    .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["/root/Makefile", "dir/dir/special.rs", "../../main.rs"]
                    .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config
                .resolve_path_to_icon(&["main.c", "foo.txt"].map(AsRef::as_ref))
        );
    }

    #[test]
    fn resolve_path_to_icon_no_match_none() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(&["foo"].map(AsRef::as_ref))
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["/some/path/file.txt", "other/path.txt"].map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(
                &["folder/file", "/home/user/file", "../../file"].map(AsRef::as_ref)
            )
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(&[".."].map(AsRef::as_ref))
        );
        assert_eq!(
            None,
            icon_theme_config.resolve_path_to_icon(&["."].map(AsRef::as_ref))
        );
    }

    #[test]
    fn resolve_path_to_icon_file_name_match_some() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            Some("icons/makefile.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(&["Makefile"].map(AsRef::as_ref))
        );
        assert_eq!(
            Some("icons/makefile.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &[
                    "baz/Makefile",
                    "/foo/bar/dir/Makefile",
                    ".././/././Makefile"
                ]
                .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            Some("icons/special.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &["dir/special.rs", "/dir1/dir2/..//./special.rs"]
                    .map(AsRef::as_ref)
            )
        );
    }

    #[test]
    fn resolve_path_to_icon_extension_match_some() {
        let icon_theme_config = get_icon_theme_config();

        assert_eq!(
            Some("icons/python.svg".to_owned().into()),
            icon_theme_config
                .resolve_path_to_icon(&["source.py"].map(AsRef::as_ref))
        );
        assert_eq!(
            Some("icons/rust.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &["/home/user/main.rs", "../../special.rs.rs", "special.rs"]
                    .map(AsRef::as_ref)
            )
        );
        assert_eq!(
            Some("icons/c.svg".to_owned().into()),
            icon_theme_config.resolve_path_to_icon(
                &["/dir1/Makefile.c", "../main.c"].map(AsRef::as_ref)
            )
        );
    }
}
