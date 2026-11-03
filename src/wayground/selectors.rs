//! Every selector lives here, with the fallbacks documented in the research.

pub const JOIN_INPUT: &str = "input[data-cy=gamecode-field]";
pub const JOIN_BUTTON: &str = "button[data-cy=joinGame-button]";
pub const NAME_INPUT: &str = "input[data-cy=enter-name-field]";
pub const START_BUTTON: &str = "button[data-cy=start-game-button]";
pub const SUBMIT_BUTTON: &str = "button[data-cy=submit-button]";
pub const QUESTION_CONTAINER: &str = ".quiz-container[data-quesid]";
pub const FULLSCREEN_BUTTON: &str = "button[aria-label=Fullscreen]";
pub const BLANK_INPUT: &str = "input[data-cy=fib-text-input]";
pub const OPEN_EDITOR: &str = "div.ProseMirror[contenteditable=true]";
pub const DROPDOWN_INPUT: &str = "input[data-cy=dropdown-input]";
pub const REORDER_ITEM: &str = "[data-testid^=reorder-option-]";
pub const MATCH_CARD: &str = "[data-testid=match-option-card-text]";

pub fn option(index: u32) -> String {
    format!("button[data-cy='option-{index}'], button.option[data-cy='option-{index}']")
}

pub fn option_any() -> &'static str {
    "button.option[data-cy^=option-], button[data-cy^=option-]"
}

pub fn resume_yes_button() -> String {
    "[data-cy=resume-button], button[data-cy='Yes']".to_string()
}
