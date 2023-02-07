pub struct LapceIcons {}

impl LapceIcons {
    pub const WINDOW_CLOSE: &str = "window.close";
    pub const WINDOW_RESTORE: &str = "window.restore";
    pub const WINDOW_MAXIMIZE: &str = "window.maximize";
    pub const WINDOW_MINIMIZE: &str = "window.minimize";

    pub const LINK: &str = "link";
    pub const ERROR: &str = "error";
    pub const ADD: &str = "add";
    pub const CLOSE: &str = "close";
    pub const REMOTE: &str = "remote";
    pub const PROBLEM: &str = "error";
    pub const DEBUG: &str = "debug";
    pub const DEBUG_BREAKPOINT: &str = "debug_breakpoint";
    pub const DEBUG_SMALL: &str = "debug_small";
    pub const DEBUG_RESTART: &str = "debug_restart";
    pub const DEBUG_CONTINUE: &str = "debug_continue";
    pub const DEBUG_PAUSE: &str = "debug_pause";
    pub const DEBUG_STOP: &str = "debug_stop";
    pub const START: &str = "start";
    pub const RUN_ERRORS: &str = "run_errors";
    pub const UNSAVED: &str = "unsaved";
    pub const WARNING: &str = "warning";
    pub const TERMINAL: &str = "terminal";
    pub const SETTINGS: &str = "settings";
    pub const LIGHTBULB: &str = "lightbulb";
    pub const EXTENSIONS: &str = "extensions";
    pub const BREADCRUMB_SEPARATOR: &str = "breadcrumb_separator";

    pub const FILE: &str = "file";
    pub const FILE_EXPLORER: &str = "file_explorer";
    pub const FILE_PICKER_UP: &str = "file_picker_up";

    pub const IMAGE_LOADING: &str = "image_loading";
    pub const IMAGE_ERROR: &str = "image_error";

    pub const SCM: &str = "scm.icon";
    pub const SCM_DIFF_MODIFIED: &str = "scm.diff.modified";
    pub const SCM_DIFF_ADDED: &str = "scm.diff.added";
    pub const SCM_DIFF_REMOVED: &str = "scm.diff.removed";
    pub const SCM_DIFF_RENAMED: &str = "scm.diff.renamed";
    pub const SCM_CHANGE_ADD: &str = "scm.change.add";
    pub const SCM_CHANGE_REMOVE: &str = "scm.change.remove";

    pub const PALETTE_MENU: &str = "palette.menu";

    pub const DROPDOWN_ARROW: &str = "dropdown.arrow";

    pub const LOCATION_BACKWARD: &str = "location.backward";
    pub const LOCATION_FORWARD: &str = "location.forward";

    pub const ITEM_OPENED: &str = "item.opened";
    pub const ITEM_CLOSED: &str = "item.closed";

    pub const DIRECTORY_CLOSED: &str = "directory.closed";
    pub const DIRECTORY_OPENED: &str = "directory.opened";

    pub const PANEL_RESTORE: &str = "panel.restore";
    pub const PANEL_MAXIMISE: &str = "panel.maximise";

    pub const SPLIT_HORIZONTAL: &str = "split.horizontal";

    pub const TAB_PREVIOUS: &str = "tab.previous";
    pub const TAB_NEXT: &str = "tab.next";

    pub const SIDEBAR_LEFT: &str = "sidebar.left.on";
    pub const SIDEBAR_LEFT_OFF: &str = "sidebar.left.off";
    pub const SIDEBAR_RIGHT: &str = "sidebar.right.on";
    pub const SIDEBAR_RIGHT_OFF: &str = "sidebar.right.off";

    pub const LAYOUT_PANEL: &str = "layout.panel.on";
    pub const LAYOUT_PANEL_OFF: &str = "layout.panel.off";

    pub const SEARCH: &'static str = "search.icon";
    pub const SEARCH_CLEAR: &'static str = "search.clear";
    pub const SEARCH_FORWARD: &'static str = "search.forward";
    pub const SEARCH_BACKWARD: &'static str = "search.backward";
    pub const SEARCH_CASE_SENSITIVE: &'static str = "search.case_sensitive";

    pub const FILE_TYPE_CODE: &str = "file-code";
    pub const FILE_TYPE_MEDIA: &str = "file-media";
    pub const FILE_TYPE_BINARY: &str = "file-binary";
    pub const FILE_TYPE_ARCHIVE: &str = "file-zip";
    pub const FILE_TYPE_SUBMODULE: &str = "file-submodule";
    pub const FILE_TYPE_SYMLINK_FILE: &str = "file-symlink-file";
    pub const FILE_TYPE_SYMLINK_DIRECTORY: &str = "file-symlink-directory";

    pub const SYMBOL_KIND_ARRAY: &str = "symbol_kind.array";
    pub const SYMBOL_KIND_BOOLEAN: &str = "symbol_kind.boolean";
    pub const SYMBOL_KIND_CLASS: &str = "symbol_kind.class";
    pub const SYMBOL_KIND_CONSTANT: &str = "symbol_kind.constant";
    pub const SYMBOL_KIND_ENUM_MEMBER: &str = "symbol_kind.enum_member";
    pub const SYMBOL_KIND_ENUM: &str = "symbol_kind.enum";
    pub const SYMBOL_KIND_EVENT: &str = "symbol_kind.event";
    pub const SYMBOL_KIND_FIELD: &str = "symbol_kind.field";
    pub const SYMBOL_KIND_FILE: &str = "symbol_kind.file";
    pub const SYMBOL_KIND_FUNCTION: &str = "symbol_kind.function";
    pub const SYMBOL_KIND_INTERFACE: &str = "symbol_kind.interface";
    pub const SYMBOL_KIND_KEY: &str = "symbol_kind.key";
    pub const SYMBOL_KIND_METHOD: &str = "symbol_kind.method";
    pub const SYMBOL_KIND_NAMESPACE: &str = "symbol_kind.namespace";
    pub const SYMBOL_KIND_NUMBER: &str = "symbol_kind.number";
    pub const SYMBOL_KIND_OBJECT: &str = "symbol_kind.namespace";
    pub const SYMBOL_KIND_OPERATOR: &str = "symbol_kind.operator";
    pub const SYMBOL_KIND_PROPERTY: &str = "symbol_kind.property";
    pub const SYMBOL_KIND_STRING: &str = "symbol_kind.string";
    pub const SYMBOL_KIND_STRUCT: &str = "symbol_kind.struct";
    pub const SYMBOL_KIND_TYPE_PARAMETER: &str = "symbol_kind.type_parameter";
    pub const SYMBOL_KIND_VARIABLE: &str = "symbol_kind.variable";

    pub const COMPLETION_ITEM_KIND_CLASS: &str = "completion_item_kind.class";
    pub const COMPLETION_ITEM_KIND_CONSTANT: &str = "completion_item_kind.constant";
    pub const COMPLETION_ITEM_KIND_ENUM_MEMBER: &str =
        "completion_item_kind.enum_member";
    pub const COMPLETION_ITEM_KIND_ENUM: &str = "completion_item_kind.enum";
    pub const COMPLETION_ITEM_KIND_FIELD: &str = "completion_item_kind.field";
    pub const COMPLETION_ITEM_KIND_FUNCTION: &str = "completion_item_kind.function";
    pub const COMPLETION_ITEM_KIND_INTERFACE: &str =
        "completion_item_kind.interface";
    pub const COMPLETION_ITEM_KIND_KEYWORD: &str = "completion_item_kind.keyword";
    pub const COMPLETION_ITEM_KIND_METHOD: &str = "completion_item_kind.method";
    pub const COMPLETION_ITEM_KIND_MODULE: &str = "completion_item_kind.module";
    pub const COMPLETION_ITEM_KIND_PROPERTY: &str = "completion_item_kind.property";
    pub const COMPLETION_ITEM_KIND_SNIPPET: &str = "completion_item_kind.snippet";
    pub const COMPLETION_ITEM_KIND_STRING: &str = "completion_item_kind.string";
    pub const COMPLETION_ITEM_KIND_STRUCT: &str = "completion_item_kind.struct";
    pub const COMPLETION_ITEM_KIND_VARIABLE: &str = "completion_item_kind.variable";
}
