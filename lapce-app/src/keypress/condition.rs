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
    #[strum(serialize = "list_focus")]
    ListFocus,
}

#[cfg(test)]
mod test {
    use floem::{app::AppContext, glazier::Modifiers};
    use lapce_core::mode::Mode;

    use crate::keypress::{condition::CheckCondition, KeyPressData, KeyPressFocus};

    use super::Condition;

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
            &mut self,
            _ctx: AppContext,
            _command: &crate::command::LapceCommand,
            _count: Option<usize>,
            _mods: Modifiers,
        ) -> crate::command::CommandExecuted {
            unimplemented!()
        }

        fn receive_char(&mut self, _ctx: AppContext, _c: &str) {
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
