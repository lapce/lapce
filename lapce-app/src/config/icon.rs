pub struct LapceIcons {}

impl LapceIcons {
    pub const WINDOW_CLOSE: &'static str = "window.close";
    pub const WINDOW_RESTORE: &'static str = "window.restore";
    pub const WINDOW_MAXIMIZE: &'static str = "window.maximize";
    pub const WINDOW_MINIMIZE: &'static str = "window.minimize";

    pub const LOGO: &'static str = "logo";
    pub const MENU: &'static str = "menu";
    pub const LINK: &'static str = "link";
    pub const ERROR: &'static str = "error";
    pub const ADD: &'static str = "add";
    pub const CLOSE: &'static str = "close";
    pub const REMOTE: &'static str = "remote";
    pub const PROBLEM: &'static str = "error";
    pub const DEBUG: &'static str = "debug";
    pub const DEBUG_ALT: &'static str = "debug_alt";
    pub const DEBUG_BREAKPOINT: &'static str = "debug_breakpoint";
    pub const DEBUG_SMALL: &'static str = "debug_small";
    pub const DEBUG_RESTART: &'static str = "debug_restart";
    pub const DEBUG_CONTINUE: &'static str = "debug_continue";
    pub const DEBUG_STEP_OVER: &'static str = "debug_step_over";
    pub const DEBUG_STEP_INTO: &'static str = "debug_step_into";
    pub const DEBUG_STEP_OUT: &'static str = "debug_step_out";
    pub const DEBUG_PAUSE: &'static str = "debug_pause";
    pub const DEBUG_STOP: &'static str = "debug_stop";
    pub const DEBUG_CONSOLE: &'static str = "debug_console";
    pub const DEBUG_DISCONNECT: &'static str = "debug_disconnect";
    pub const START: &'static str = "start";
    pub const RUN_ERRORS: &'static str = "run_errors";
    pub const UNSAVED: &'static str = "unsaved";
    pub const WARNING: &'static str = "warning";
    pub const TERMINAL: &'static str = "terminal";
    pub const SETTINGS: &'static str = "settings";
    pub const LIGHTBULB: &'static str = "lightbulb";
    pub const EXTENSIONS: &'static str = "extensions";
    pub const KEYBOARD: &'static str = "keyboard";
    pub const BREADCRUMB_SEPARATOR: &'static str = "breadcrumb_separator";
    pub const SYMBOL_COLOR: &'static str = "symbol_color";
    pub const TYPE_HIERARCHY: &'static str = "type_hierarchy";

    pub const FILE: &'static str = "file";
    pub const FILE_EXPLORER: &'static str = "file_explorer";
    pub const FILE_PICKER_UP: &'static str = "file_picker_up";

    pub const IMAGE_LOADING: &'static str = "image_loading";
    pub const IMAGE_ERROR: &'static str = "image_error";

    pub const SCM: &'static str = "scm.icon";
    pub const SCM_DIFF_MODIFIED: &'static str = "scm.diff.modified";
    pub const SCM_DIFF_ADDED: &'static str = "scm.diff.added";
    pub const SCM_DIFF_REMOVED: &'static str = "scm.diff.removed";
    pub const SCM_DIFF_RENAMED: &'static str = "scm.diff.renamed";
    pub const SCM_CHANGE_ADD: &'static str = "scm.change.add";
    pub const SCM_CHANGE_REMOVE: &'static str = "scm.change.remove";

    pub const FOLD: &'static str = "fold";
    pub const FOLD_UP: &'static str = "fold.up";
    pub const FOLD_DOWN: &'static str = "fold.down";

    pub const PALETTE_MENU: &'static str = "palette.menu";

    pub const DROPDOWN_ARROW: &'static str = "dropdown.arrow";

    pub const PANEL_FOLD_UP: &'static str = "panel.fold-up";
    pub const PANEL_FOLD_DOWN: &'static str = "panel.fold-down";

    pub const LOCATION_BACKWARD: &'static str = "location.backward";
    pub const LOCATION_FORWARD: &'static str = "location.forward";

    pub const ITEM_OPENED: &'static str = "item.opened";
    pub const ITEM_CLOSED: &'static str = "item.closed";

    pub const DIRECTORY_CLOSED: &'static str = "directory.closed";
    pub const DIRECTORY_OPENED: &'static str = "directory.opened";

    pub const PANEL_RESTORE: &'static str = "panel.restore";
    pub const PANEL_MAXIMISE: &'static str = "panel.maximise";

    pub const SPLIT_HORIZONTAL: &'static str = "split.horizontal";

    pub const TAB_PREVIOUS: &'static str = "tab.previous";
    pub const TAB_NEXT: &'static str = "tab.next";

    pub const SIDEBAR_LEFT: &'static str = "sidebar.left.on";
    pub const SIDEBAR_LEFT_OFF: &'static str = "sidebar.left.off";
    pub const SIDEBAR_RIGHT: &'static str = "sidebar.right.on";
    pub const SIDEBAR_RIGHT_OFF: &'static str = "sidebar.right.off";

    pub const LAYOUT_PANEL: &'static str = "layout.panel.on";
    pub const LAYOUT_PANEL_OFF: &'static str = "layout.panel.off";

    pub const SEARCH: &'static str = "search.icon";
    pub const SEARCH_CLEAR: &'static str = "search.clear";
    pub const SEARCH_FORWARD: &'static str = "search.forward";
    pub const SEARCH_BACKWARD: &'static str = "search.backward";
    pub const SEARCH_CASE_SENSITIVE: &'static str = "search.case_sensitive";
    pub const SEARCH_WHOLE_WORD: &'static str = "search.whole_word";
    pub const SEARCH_REGEX: &'static str = "search.regex";
    pub const SEARCH_REPLACE: &'static str = "search.replace";
    pub const SEARCH_REPLACE_ALL: &'static str = "search.replace_all";

    pub const FILE_TYPE_CODE: &'static str = "file-code";
    pub const FILE_TYPE_MEDIA: &'static str = "file-media";
    pub const FILE_TYPE_BINARY: &'static str = "file-binary";
    pub const FILE_TYPE_ARCHIVE: &'static str = "file-zip";
    pub const FILE_TYPE_SUBMODULE: &'static str = "file-submodule";
    pub const FILE_TYPE_SYMLINK_FILE: &'static str = "file-symlink-file";
    pub const FILE_TYPE_SYMLINK_DIRECTORY: &'static str = "file-symlink-directory";

    pub const DOCUMENT_SYMBOL: &'static str = "document_symbol";

    pub const REFERENCES: &'static str = "references";

    pub const IMPLEMENTATION: &'static str = "implementation";

    pub const SYMBOL_KIND_ARRAY: &'static str = "symbol_kind.array";
    pub const SYMBOL_KIND_BOOLEAN: &'static str = "symbol_kind.boolean";
    pub const SYMBOL_KIND_CLASS: &'static str = "symbol_kind.class";
    pub const SYMBOL_KIND_CONSTANT: &'static str = "symbol_kind.constant";
    pub const SYMBOL_KIND_ENUM_MEMBER: &'static str = "symbol_kind.enum_member";
    pub const SYMBOL_KIND_ENUM: &'static str = "symbol_kind.enum";
    pub const SYMBOL_KIND_EVENT: &'static str = "symbol_kind.event";
    pub const SYMBOL_KIND_FIELD: &'static str = "symbol_kind.field";
    pub const SYMBOL_KIND_FILE: &'static str = "symbol_kind.file";
    pub const SYMBOL_KIND_FUNCTION: &'static str = "symbol_kind.function";
    pub const SYMBOL_KIND_INTERFACE: &'static str = "symbol_kind.interface";
    pub const SYMBOL_KIND_KEY: &'static str = "symbol_kind.key";
    pub const SYMBOL_KIND_METHOD: &'static str = "symbol_kind.method";
    pub const SYMBOL_KIND_NAMESPACE: &'static str = "symbol_kind.namespace";
    pub const SYMBOL_KIND_NUMBER: &'static str = "symbol_kind.number";
    pub const SYMBOL_KIND_OBJECT: &'static str = "symbol_kind.namespace";
    pub const SYMBOL_KIND_OPERATOR: &'static str = "symbol_kind.operator";
    pub const SYMBOL_KIND_PROPERTY: &'static str = "symbol_kind.property";
    pub const SYMBOL_KIND_STRING: &'static str = "symbol_kind.string";
    pub const SYMBOL_KIND_STRUCT: &'static str = "symbol_kind.struct";
    pub const SYMBOL_KIND_TYPE_PARAMETER: &'static str =
        "symbol_kind.type_parameter";
    pub const SYMBOL_KIND_VARIABLE: &'static str = "symbol_kind.variable";

    pub const COMPLETION_ITEM_KIND_CLASS: &'static str =
        "completion_item_kind.class";
    pub const COMPLETION_ITEM_KIND_CONSTANT: &'static str =
        "completion_item_kind.constant";
    pub const COMPLETION_ITEM_KIND_ENUM_MEMBER: &'static str =
        "completion_item_kind.enum_member";
    pub const COMPLETION_ITEM_KIND_ENUM: &'static str = "completion_item_kind.enum";
    pub const COMPLETION_ITEM_KIND_FIELD: &'static str =
        "completion_item_kind.field";
    pub const COMPLETION_ITEM_KIND_FUNCTION: &'static str =
        "completion_item_kind.function";
    pub const COMPLETION_ITEM_KIND_INTERFACE: &'static str =
        "completion_item_kind.interface";
    pub const COMPLETION_ITEM_KIND_KEYWORD: &'static str =
        "completion_item_kind.keyword";
    pub const COMPLETION_ITEM_KIND_METHOD: &'static str =
        "completion_item_kind.method";
    pub const COMPLETION_ITEM_KIND_MODULE: &'static str =
        "completion_item_kind.module";
    pub const COMPLETION_ITEM_KIND_PROPERTY: &'static str =
        "completion_item_kind.property";
    pub const COMPLETION_ITEM_KIND_SNIPPET: &'static str =
        "completion_item_kind.snippet";
    pub const COMPLETION_ITEM_KIND_STRING: &'static str =
        "completion_item_kind.string";
    pub const COMPLETION_ITEM_KIND_STRUCT: &'static str =
        "completion_item_kind.struct";
    pub const COMPLETION_ITEM_KIND_VARIABLE: &'static str =
        "completion_item_kind.variable";
}
