//! URL builders.

pub const ORIGIN: &str = "https://wayground.com";
pub const JOIN: &str = "https://wayground.com/join";

pub fn check_room() -> String {
    format!("{ORIGIN}/play-api/v5/checkRoom")
}

pub fn get_questions() -> String {
    format!("{ORIGIN}/play-api/v4/getQuestions")
}

pub fn quiz_data(quiz_id: &str) -> String {
    format!("{ORIGIN}/_quizserver/main/v2/quiz/{quiz_id}?source=join")
}

pub fn game_info(hash: &str) -> String {
    format!("{ORIGIN}/_gameapi/main/public/v1/games/{hash}")
}
