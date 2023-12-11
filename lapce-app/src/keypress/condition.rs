use strum_macros::EnumString;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CheckCondition<'a> {
    Single(&'a str),
    Or(&'a str, &'a str),
    And(&'a str, &'a str),
}

impl<'a> CheckCondition<'a> {
    pub(super) fn parse_first(condition: &'a str) -> Self {
        let or = condition.match_indices("||").next();
        let and = condition.match_indices("&&").next();

        match (or, and) {
            (None, None) => CheckCondition::Single(condition),
            (Some((pos, _)), None) => {
                CheckCondition::Or(&condition[..pos], &condition[pos + 2..])
            }
            (None, Some((pos, _))) => {
                CheckCondition::And(&condition[..pos], &condition[pos + 2..])
            }
            (Some((or_pos, _)), Some((and_pos, _))) => {
                if or_pos < and_pos {
                    CheckCondition::Or(
                        &condition[..or_pos],
                        &condition[or_pos + 2..],
                    )
                } else {
                    CheckCondition::And(
                        &condition[..and_pos],
                        &condition[and_pos + 2..],
                    )
                }
            }
        }
    }
}

#[derive(EnumString, PartialEq, Eq)]
pub enum Condition {
    #[strum(serialize = "editor_focus")]
    EditorFocus,
    #[strum(serialize = "input_focus")]
    InputFocus,
    #[strum(serialize = "list_focus")]
    ListFocus,
    #[strum(serialize = "palette_focus")]
    PaletteFocus,
    #[strum(serialize = "completion_focus")]
    CompletionFocus,
    #[strum(serialize = "inline_completion_visible")]
    InlineCompletionVisible,
    #[strum(serialize = "modal_focus")]
    ModalFocus,
    #[strum(serialize = "in_snippet")]
    InSnippet,
    #[strum(serialize = "terminal_focus")]
    TerminalFocus,
    #[strum(serialize = "source_control_focus")]
    SourceControlFocus,
    #[strum(serialize = "panel_focus")]
    PanelFocus,
    #[strum(serialize = "rename_focus")]
    RenameFocus,
    #[strum(serialize = "search_active")]
    SearchActive,
    #[strum(serialize = "search_focus")]
    SearchFocus,
    #[strum(serialize = "replace_focus")]
    ReplaceFocus,
}

#[cfg(test)]
mod test {
    use floem::keyboard::ModifiersState;
    use lapce_core::mode::Mode;

    use super::Condition;
    use crate::keypress::{condition::CheckCondition, KeyPressData, KeyPressFocus};

    struct MockFocus {
        accepted_conditions: &'static [Condition],
    }

    impl KeyPressFocus for MockFocus {
        fn check_condition(&self, condition: Condition) -> bool {
            self.accepted_conditions.contains(&condition)
        }

        fn get_mode(&self) -> Mode {
            unimplemented!()
        }

        fn run_command(
            &self,
            _command: &crate::command::LapceCommand,
            _count: Option<usize>,
            _mods: ModifiersState,
        ) -> crate::command::CommandExecuted {
            unimplemented!()
        }

        fn receive_char(&self, _c: &str) {
            unimplemented!()
        }
    }

    #[test]
    fn test_parse() {
        assert_eq!(
            CheckCondition::Or("foo", "bar"),
            CheckCondition::parse_first("foo||bar")
        );
        assert_eq!(
            CheckCondition::And("foo", "bar"),
            CheckCondition::parse_first("foo&&bar")
        );
        assert_eq!(
            CheckCondition::And("foo", "bar||baz"),
            CheckCondition::parse_first("foo&&bar||baz")
        );
        assert_eq!(
            CheckCondition::And("foo ", " bar || baz"),
            CheckCondition::parse_first("foo && bar || baz")
        );
    }

    #[test]
    fn test_check_condition() {
        let focus = MockFocus {
            accepted_conditions: &[Condition::EditorFocus, Condition::ListFocus],
        };

        let test_cases = [
            ("editor_focus", true),
            ("list_focus", true),
            ("!editor_focus", false),
            ("!list_focus", false),
            ("editor_focus || list_focus", true),
            ("editor_focus || !list_focus", true),
            ("!editor_focus || list_focus", true),
            ("editor_focus && list_focus", true),
            ("editor_focus && !list_focus", false),
            ("!editor_focus && list_focus", false),
            ("editor_focus && list_focus || baz", true),
            ("editor_focus && list_focus && baz", false),
            ("editor_focus && list_focus && !baz", true),
        ];

        for (condition, should_accept) in test_cases.into_iter() {
            assert_eq!(
                should_accept,
                KeyPressData::check_condition(condition, &focus),
                "Condition check failed. Condition: {condition}. Expected result: {should_accept}",
            );
        }
    }
}
