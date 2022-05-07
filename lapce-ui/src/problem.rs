use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::Arc,
};

use druid::{
    widget::{Click, CrossAxisAlignment, Flex, Label, List, ListIter},
    Command, Data, EventCtx, Insets, Lens, Target, Widget, WidgetExt, WidgetId,
};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, GetConfig, LapceTheme},
    data::{EditorDiagnostic, LapceTabData, LapceWorkspace, PanelKind},
    editor::EditorLocationNew,
    problem::ProblemData,
    proxy::path_from_url,
    split::SplitDirection,
};
use lsp_types::{DiagnosticRelatedInformation, DiagnosticSeverity};

use crate::{
    panel::{LapcePanel, PanelHeaderKind},
    scroll::LapcePadding,
    svg::file_svg_path,
    widgets::{
        background::Background, label_utils::TextColorWatcher,
        stretch::StretchVertical, svg::Svg, utils::hover::Hover,
    },
};

pub fn new_problem_panel(data: &ProblemData) -> LapcePanel {
    LapcePanel::new(
        PanelKind::Problem,
        data.widget_id,
        data.split_id,
        SplitDirection::Vertical,
        PanelHeaderKind::Simple("Problem".to_string()),
        vec![
            (
                data.error_widget_id,
                PanelHeaderKind::Simple("Errors".to_string()),
                problem_content(DiagnosticSeverity::Error).boxed(),
                None,
            ),
            (
                data.warning_widget_id,
                PanelHeaderKind::Simple("Warnings".to_string()),
                problem_content(DiagnosticSeverity::Warning).boxed(),
                None,
            ),
        ],
    )
}

#[derive(Clone)]
struct ListLens(DiagnosticSeverity);
impl Lens<LapceTabData, ListData> for ListLens {
    fn with<V, F: FnOnce(&ListData) -> V>(&self, data: &LapceTabData, f: F) -> V {
        let data = ListData {
            severity: self.0,
            config: data.config.clone(),
            items: data.main_split.diagnostics.clone(),
            workspace: data.workspace.clone(),
            widget_id: data.id,
        };
        f(&data)
    }

    fn with_mut<V, F: FnOnce(&mut ListData) -> V>(
        &self,
        data: &mut LapceTabData,
        f: F,
    ) -> V {
        let mut data = ListData {
            severity: self.0,
            config: data.config.clone(),
            items: data.main_split.diagnostics.clone(),
            workspace: data.workspace.clone(),
            widget_id: data.id,
        };
        f(&mut data)
    }
}

#[derive(Clone)]
struct ListData {
    severity: DiagnosticSeverity,
    config: Arc<Config>,
    workspace: Arc<LapceWorkspace>,
    items: im::HashMap<PathBuf, Arc<Vec<EditorDiagnostic>>>,
    widget_id: WidgetId,
}
impl Data for ListData {
    fn same(&self, other: &Self) -> bool {
        self.config.same(&other.config) && self.items.same(&other.items)
    }
}
impl GetConfig for ListData {
    fn get_config(&self) -> &Config {
        &self.config
    }
}
impl ListIter<FileData> for ListData {
    fn for_each(&self, mut cb: impl FnMut(&FileData, usize)) {
        for (idx, (path, problems)) in self
            .items
            .iter()
            .filter(|(_, problems)| {
                problems.iter().any(|problem| {
                    problem.diagnostic.severity == Some(self.severity)
                })
            })
            .enumerate()
        {
            let data = FileData {
                severity: self.severity,
                path: path.clone(),
                workspace: self.workspace.clone(),
                config: self.config.clone(),
                items: problems.clone(),
                widget_id: self.widget_id,
            };
            cb(&data, idx);
        }
    }

    fn for_each_mut(&mut self, mut cb: impl FnMut(&mut FileData, usize)) {
        for (idx, (path, problems)) in self
            .items
            .iter()
            .filter(|(_, problems)| {
                problems.iter().any(|problem| {
                    problem.diagnostic.severity == Some(self.severity)
                })
            })
            .enumerate()
        {
            let mut data = FileData {
                severity: self.severity,
                path: path.clone(),
                workspace: self.workspace.clone(),
                config: self.config.clone(),
                items: problems.clone(),
                widget_id: self.widget_id,
            };
            cb(&mut data, idx);
        }
    }

    fn data_len(&self) -> usize {
        self.items
            .iter()
            .filter(|(_, problems)| {
                problems.iter().any(|problem| {
                    problem.diagnostic.severity == Some(self.severity)
                })
            })
            .count()
    }
}

#[derive(Clone)]
struct FileData {
    severity: DiagnosticSeverity,
    path: PathBuf,
    workspace: Arc<LapceWorkspace>,
    config: Arc<Config>,
    items: Arc<Vec<EditorDiagnostic>>,
    widget_id: WidgetId,
}
impl FileData {
    fn file(&self) -> String {
        self.path
            .file_name()
            .map(OsStr::to_string_lossy)
            .unwrap_or_default()
            .to_string()
    }

    fn path(&self) -> String {
        self.workspace
            .path
            .as_ref()
            .and_then(|prefix| self.path.strip_prefix(prefix).ok())
            .unwrap_or(&self.path)
            .parent()
            .map(Path::to_string_lossy)
            .unwrap_or_default()
            .to_string()
    }

    fn icon(&self) -> String {
        file_svg_path(&self.path)
    }
}

impl Data for FileData {
    fn same(&self, other: &Self) -> bool {
        self.config.same(&other.config)
            && self.path == other.path
            && self.items == other.items
    }
}
impl GetConfig for FileData {
    fn get_config(&self) -> &Config {
        &self.config
    }
}
impl ListIter<ItemData> for FileData {
    fn for_each(&self, mut cb: impl FnMut(&ItemData, usize)) {
        // Clone path once, and we'll move in and out of this variable in the loop
        let mut path = self.path.clone();
        for (idx, problem_item) in self
            .items
            .iter()
            .filter(|item| item.diagnostic.severity == Some(self.severity))
            .enumerate()
        {
            let data = ItemData {
                path,
                config: self.config.clone(),
                item: problem_item.clone(),
                widget_id: self.widget_id,
            };
            cb(&data, idx);
            path = data.path;
        }
    }

    fn for_each_mut(&mut self, mut cb: impl FnMut(&mut ItemData, usize)) {
        // Clone path once, and we'll move in and out of this variable in the loop
        let mut path = self.path.clone();
        for (idx, problem_item) in self
            .items
            .iter()
            .filter(|item| item.diagnostic.severity == Some(self.severity))
            .enumerate()
        {
            let mut data = ItemData {
                path,
                config: self.config.clone(),
                item: problem_item.clone(),
                widget_id: self.widget_id,
            };
            cb(&mut data, idx);
            path = data.path;
        }
    }

    fn data_len(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.diagnostic.severity == Some(self.severity))
            .count()
    }
}

#[derive(Clone)]
struct ItemData {
    path: PathBuf,
    config: Arc<Config>,
    item: EditorDiagnostic,
    widget_id: WidgetId,
}
impl ItemData {
    fn on_click(&self, ctx: &mut EventCtx) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::JumpToLocation(
                None,
                EditorLocationNew {
                    path: self.path.clone(),
                    position: Some(self.item.diagnostic.range.start),
                    scroll_offset: None,
                    history: None,
                },
            ),
            Target::Widget(self.widget_id),
        ));
    }

    fn message(&self) -> String {
        self.item.diagnostic.message.clone()
    }
}
impl Data for ItemData {
    fn same(&self, other: &Self) -> bool {
        self.config.same(&other.config)
            && self.path == other.path
            && self.item == other.item
    }
}
impl GetConfig for ItemData {
    fn get_config(&self) -> &Config {
        &self.config
    }
}
impl ListIter<RelatedItemData> for ItemData {
    fn for_each(&self, mut cb: impl FnMut(&RelatedItemData, usize)) {
        for (idx, problem_item) in self
            .item
            .diagnostic
            .related_information
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .enumerate()
        {
            let data = RelatedItemData {
                config: self.config.clone(),
                data: problem_item.clone(),
                widget_id: self.widget_id,
            };
            cb(&data, idx);
        }
    }

    fn for_each_mut(&mut self, mut cb: impl FnMut(&mut RelatedItemData, usize)) {
        for (idx, problem_item) in self
            .item
            .diagnostic
            .related_information
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .enumerate()
        {
            let mut data = RelatedItemData {
                config: self.config.clone(),
                data: problem_item.clone(),
                widget_id: self.widget_id,
            };
            cb(&mut data, idx);
        }
    }

    fn data_len(&self) -> usize {
        self.item
            .diagnostic
            .related_information
            .as_ref()
            .map_or(0, Vec::len)
    }
}

#[derive(Clone)]
struct RelatedItemData {
    config: Arc<Config>,
    data: DiagnosticRelatedInformation,
    widget_id: WidgetId,
}
impl RelatedItemData {
    fn message(&self) -> String {
        format!(
            "{}[{}, {}]: {}",
            path_from_url(&self.data.location.uri)
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default(),
            self.data.location.range.start.line,
            self.data.location.range.start.character,
            self.data.message
        )
    }

    fn on_click(&self, ctx: &mut EventCtx) {
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::JumpToLocation(
                None,
                EditorLocationNew {
                    path: self.data.location.uri.to_file_path().unwrap(),
                    position: Some(self.data.location.range.start),
                    scroll_offset: None,
                    history: None,
                },
            ),
            Target::Widget(self.widget_id),
        ));
    }
}

impl Data for RelatedItemData {
    fn same(&self, other: &Self) -> bool {
        self.config.same(&other.config) && self.data == other.data
    }
}
impl GetConfig for RelatedItemData {
    fn get_config(&self) -> &Config {
        &self.config
    }
}

fn hoverable<T: Data + GetConfig, W: Widget<T> + 'static>(
    widget: W,
) -> impl Widget<T> {
    Background::new(widget).controller(Hover::new(
        |widget: &mut Background<W>, ctx, data: &T, _env| {
            if ctx.is_hot() {
                widget.set_background(
                    data.get_config()
                        .get_color_unchecked(LapceTheme::HOVER_BACKGROUND)
                        .clone(),
                );
            } else {
                widget.clear_background()
            }
        },
    ))
}

fn problem_content(severity: DiagnosticSeverity) -> impl Widget<LapceTabData> {
    let severity_icon =
        String::from(if let DiagnosticSeverity::Warning = severity {
            "warning.svg"
        } else {
            "error.svg"
        });

    StretchVertical::new(
        LapcePadding::new(
            Insets::new(0.0, 10.0, 0.0, 10.0),
            List::new(move || {
                let severity_icon = severity_icon.clone();
                Flex::column()
                    .with_child(hoverable(
                        Flex::row()
                            .cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_child(
                                Svg::new(String::from("default_file.svg"))
                                    .on_added(|widget, ctx, data: &FileData, _evt| {
                                        widget.set_svg_path(data.icon());
                                        ctx.request_paint();
                                    })
                                    .padding(Insets::new(12.0, 2.0, 4.0, 2.0)),
                            )
                            .with_child(
                                Label::dynamic(|data: &FileData, _env| data.file())
                                    .with_text_size(13.0)
                                    .controller(TextColorWatcher::new(
                                        LapceTheme::EDITOR_FOREGROUND,
                                    )),
                            )
                            .with_child(
                                Label::dynamic(|data: &FileData, _env| data.path())
                                    .with_text_size(13.0)
                                    .controller(TextColorWatcher::new(
                                        LapceTheme::EDITOR_DIM,
                                    )),
                            )
                            .expand_width(),
                    ))
                    .with_child(List::new(move || {
                        Flex::column()
                            .with_child(hoverable(
                                Flex::row()
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .with_child(
                                        LapcePadding::new(Insets::new(27.0, 2.0, 4.0, 2.0), Svg::new(severity_icon.clone()))
                                    )
                                    .with_child(
                                        Label::dynamic(|data: &ItemData, _env| {
                                            data.message()
                                        })
                                        .with_text_size(13.0)
                                        .controller(TextColorWatcher::new(
                                            LapceTheme::EDITOR_FOREGROUND,
                                        )),
                                    )
                                    .with_flex_spacer(1.0)
                                    .controller(Click::new(
                                        |ctx: &mut EventCtx,
                                        data: &mut ItemData,
                                        _env| {
                                            data.on_click(ctx)
                                        },
                                    )),
                            ))
                            .with_child(List::new(|| {
                                hoverable(
                                Flex::row()
                                    .cross_axis_alignment(CrossAxisAlignment::Start)
                                    .with_child(
                                        LapcePadding::new(Insets::new(2.0 * 27.0, 2.0, 4.0, 2.0), Svg::new(String::from("link.svg")))
                                    )
                                    .with_child(
                                        Label::dynamic(
                                            |data: &RelatedItemData, _env| {
                                                data.message()
                                            },
                                        )
                                        .with_text_size(13.0)
                                        .controller(TextColorWatcher::new(
                                            LapceTheme::EDITOR_FOREGROUND,
                                        )),
                                    )
                                    .with_flex_spacer(1.0)
                                    .controller(Click::new(
                                        |ctx: &mut EventCtx,
                                        data: &mut RelatedItemData,
                                        _env| {
                                            data.on_click(ctx)
                                        },
                                    )),
                            )
                            }))
                    }))
            })
        )
        .lens(ListLens(severity)),
    )
}
