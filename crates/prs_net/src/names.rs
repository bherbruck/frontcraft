//! What players call themselves, cleaned up by the server, which is the only one whose say counts. A
//! name is only ever shown as text, but it still must not pass for a bot, the server or somebody else.

/// What a server lets a name be.
#[derive(Clone, Debug)]
pub struct NameRules {
    /// The most characters a name can have, after cleaning.
    pub max_chars: usize,
    /// Letters and digits from any alphabet, or only a to z and digits. Other alphabets have letters
    /// that look exactly like Latin ones, which makes "Bot" easy to fake, and the game's font only has
    /// Latin anyway.
    pub ascii_only: bool,
    /// Allowed besides letters, digits and single spaces.
    pub punctuation: String,
    /// Words a name can't start with, in lower case, compared without anything but letters.
    pub reserved: Vec<String>,
    /// Given to anyone whose name comes to nothing, followed by their number.
    pub fallback: String,
}

impl Default for NameRules {
    fn default() -> Self {
        Self {
            max_chars: 16,
            ascii_only: true,
            punctuation: "-_.'".into(),
            reserved: ["bot", "server", "admin", "player", "you", "system"].map(String::from).to_vec(),
            fallback: "Player".into(),
        }
    }
}

/// The name a player asked for as the server will have it. `id` is their number, for when nothing
/// is left, and `taken` the names everyone else already has, compared ignoring case.
pub fn clean_name(asked: &str, id: u16, taken: &[&str], rules: &NameRules) -> String {
    let allowed = |c: char| {
        let letter = if rules.ascii_only { c.is_ascii_alphanumeric() } else { c.is_alphanumeric() };
        letter || rules.punctuation.contains(c)
    };
    let mut name = String::new();
    for word in asked.split(|c: char| c.is_whitespace()) {
        let word: String = word.chars().filter(|&c| allowed(c)).collect();
        if word.is_empty() {
            continue;
        }
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(&word);
    }
    let mut name: String = name.chars().take(rules.max_chars).collect::<String>().trim_end().to_owned();
    // The letters the first word starts with, so that "B-o-t 3" and "Bot_7" are both "bot".
    let first_word = name.split(' ').next().unwrap_or_default();
    let leading: String = first_word.chars().filter(|c| !rules.punctuation.contains(*c)).take_while(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect();
    if name.is_empty() || rules.reserved.iter().any(|word| *word == leading) {
        name = format!("{} {id}", rules.fallback);
    }
    let is_taken = |name: &str| taken.iter().any(|other| other.to_lowercase() == name.to_lowercase());
    if !is_taken(&name) {
        return name;
    }
    (2..)
        .map(|n| {
            let tail = format!(" {n}");
            let room = rules.max_chars.saturating_sub(tail.chars().count()).max(1);
            format!("{}{tail}", name.chars().take(room).collect::<String>().trim_end())
        })
        .find(|candidate| !is_taken(candidate))
        .expect("there are always more numbers")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(asked: &str) -> String {
        clean_name(asked, 7, &[], &NameRules::default())
    }

    #[test]
    fn a_plain_name_comes_through() {
        assert_eq!(clean("Hawk"), "Hawk");
        assert_eq!(clean("Sgt. O'Neil-2"), "Sgt. O'Neil-2");
    }

    #[test]
    fn spaces_are_trimmed_and_run_together() {
        assert_eq!(clean("  Big \t  Bear  "), "Big Bear");
    }

    #[test]
    fn it_is_cut_to_length() {
        assert_eq!(clean("Abcdefghijklmnopqrstuvwxyz"), "Abcdefghijklmnop");
        assert_eq!(clean("Abcdefghijklmno pq"), "Abcdefghijklmno", "and loses a space left at the end");
    }

    #[test]
    fn control_zero_width_and_markup_characters_are_gone() {
        assert_eq!(clean("Ha\u{200B}wk\u{0007}\u{202E}"), "Hawk");
        assert_eq!(clean("<b>Hawk</b>"), "bHawkb");
        assert_eq!(clean("Hawk\nServer: you are banned"), "Hawk Server you", "a new line is a space");
    }

    #[test]
    fn other_alphabets_only_where_allowed() {
        assert_eq!(clean("Вот"), "Player 7", "Cyrillic that reads as Bot");
        let open = NameRules { ascii_only: false, ..Default::default() };
        assert_eq!(clean_name("Jürgen", 7, &[], &open), "Jürgen");
        assert_eq!(clean_name("Ju\u{200D}rgen", 7, &[], &open), "Jurgen");
    }

    #[test]
    fn nothing_left_gets_the_fallback() {
        assert_eq!(clean(""), "Player 7");
        assert_eq!(clean("   "), "Player 7");
        assert_eq!(clean("\u{200B}\u{200B}"), "Player 7");
    }

    #[test]
    fn it_cannot_pass_for_a_bot_the_server_or_another_player_number() {
        for fake in ["Bot 3", "bot3", "B-o-t 3", "BOT", "Server", "[Server]", "Player 2", "you", "Admin"] {
            assert_eq!(clean(fake), "Player 7", "{fake}");
        }
        assert_eq!(clean("Botany"), "Botany", "only the word itself");
        assert_eq!(clean("Bot_7"), "Player 7");
        assert_eq!(clean("Bot Bob"), "Player 7");
    }

    #[test]
    fn a_name_already_taken_gets_a_number() {
        let rules = NameRules::default();
        assert_eq!(clean_name("Hawk", 7, &["hawk"], &rules), "Hawk 2");
        assert_eq!(clean_name("Hawk", 7, &["Hawk", "Hawk 2"], &rules), "Hawk 3");
        assert_eq!(clean_name("Abcdefghijklmnop", 7, &["Abcdefghijklmnop"], &rules), "Abcdefghijklmn 2", "still within the limit");
    }
}
