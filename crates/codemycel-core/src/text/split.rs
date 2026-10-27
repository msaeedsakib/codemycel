/// Splits an identifier into lowercase words joined by single spaces (spec §9, §10.1):
/// `HTTPServer` → `http server`, `rate_limiter` → `rate limiter`, `http2Server` → `http 2 server`.
pub fn name_split(name: &str) -> String {
    let mut words: Vec<String> = Vec::new();
    for token in name.split(|c: char| !c.is_alphanumeric()) {
        let chars: Vec<char> = token.chars().collect();
        let mut word = String::new();
        for (i, &c) in chars.iter().enumerate() {
            if i > 0 && is_boundary(chars[i - 1], c, chars.get(i + 1).copied()) {
                words.push(std::mem::take(&mut word));
            }
            word.extend(c.to_lowercase());
        }
        if !word.is_empty() {
            words.push(word);
        }
    }
    words.join(" ")
}

fn is_boundary(prev: char, cur: char, next: Option<char>) -> bool {
    (prev.is_lowercase() && cur.is_uppercase())
        || (prev.is_uppercase() && cur.is_uppercase() && next.is_some_and(char::is_lowercase))
        || prev.is_numeric() != cur.is_numeric()
}
