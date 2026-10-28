/// A declaration's text up to its body on one line: whitespace runs become one space, and
/// trailing `{`, `=>`, `=`, `;`, and `,` are dropped.
pub fn normalise_signature(text: &str) -> String {
    let mut signature = collapse_whitespace(text);
    loop {
        let trimmed = signature.trim_end();
        let stripped = ["{", "=>", "=", ";", ","]
            .iter()
            .find_map(|suffix| trimmed.strip_suffix(suffix));
        match stripped {
            Some(rest) => signature = rest.to_owned(),
            None => return trimmed.to_owned(),
        }
    }
}

/// A type's text in one canonical spelling: no whitespace inside brackets or before `,`, one
/// space after `,`, single spaces elsewhere. `Vec< Foo >` and `Vec<Foo>` agree.
pub fn normalise_type(text: &str) -> String {
    let collapsed = collapse_whitespace(text);
    let chars: Vec<char> = collapsed.chars().collect();
    let mut out = String::with_capacity(collapsed.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == ' ' {
            let prev = out.chars().last();
            let next = chars.get(i + 1).copied();
            let glued = matches!(prev, Some('<' | '(' | '[' | ' '))
                || matches!(next, Some('>' | ')' | ']' | ','));
            if !glued {
                out.push(' ');
            }
            continue;
        }
        if matches!(c, '>' | ')' | ']') && out.ends_with(' ') {
            out.pop();
        }
        out.push(c);
        if c == ',' {
            out.push(' ');
        }
    }
    out.trim().to_owned()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_drops_trailing_body_openers() {
        assert_eq!(
            normalise_signature("pub fn f(\r\n    a: u32,\r\n) -> u32 {"),
            "pub fn f( a: u32, ) -> u32"
        );
        assert_eq!(
            normalise_signature("export const f = (a: number): string =>"),
            "export const f = (a: number): string"
        );
    }

    #[test]
    fn type_spacing_is_canonical() {
        assert_eq!(
            normalise_type("HashMap< K ,V >"),
            normalise_type("HashMap<K, V>")
        );
        assert_eq!(normalise_type("HashMap<K,V>"), "HashMap<K, V>");
        assert_eq!(normalise_type("&'a  mut dyn  Tr"), "&'a mut dyn Tr");
    }
}
