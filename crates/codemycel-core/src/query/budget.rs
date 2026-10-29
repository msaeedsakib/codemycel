use super::answer::{Answer, SectionCount, Truncation};
use super::format::TextFormatter;

/// A token allowance for one answer, estimated as characters ÷ 4 of its text rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    max_tokens: u32,
}

impl Budget {
    pub fn new(max_tokens: u32) -> Self {
        Self { max_tokens }
    }

    pub fn max_tokens(self) -> u32 {
        self.max_tokens
    }

    pub fn tokens(text: &str) -> usize {
        text.chars().count().div_ceil(4)
    }

    /// Cuts whole entries until the text rendering, without the truncation footer, fits:
    /// the last entry of the highest-numbered priority first, the later section on a tie.
    /// Then records every section showing fewer entries than it has, with `hint`.
    pub fn apply(self, answer: &mut Answer, hint: &str) {
        let limit = self.max_tokens as usize * 4;
        let mixed = TextFormatter::mixed_sources(answer);
        let chars = |text: &str| text.chars().count() + 1;
        let mut costs: Vec<Vec<usize>> = answer
            .sections
            .iter()
            .map(|section| {
                let indent = TextFormatter::indent(section);
                section
                    .lines
                    .iter()
                    .map(|line| chars(&TextFormatter::line(line, indent, mixed)))
                    .collect()
            })
            .collect();
        let headers: Vec<usize> = answer
            .sections
            .iter()
            .map(|s| {
                if s.header {
                    chars(&TextFormatter::header(s))
                } else {
                    0
                }
            })
            .collect();
        let mut total = chars(&TextFormatter::freshness(&answer.freshness))
            + costs
                .iter()
                .zip(&headers)
                .filter(|(lines, _)| !lines.is_empty())
                .map(|(lines, header)| header + lines.iter().sum::<usize>())
                .sum::<usize>();
        while total > limit {
            let Some(index) = (0..answer.sections.len())
                .filter(|&i| !costs[i].is_empty())
                .max_by_key(|&i| (answer.sections[i].priority, i))
            else {
                break;
            };
            answer.sections[index].lines.pop();
            total -= costs[index].pop().unwrap_or(0);
            if costs[index].is_empty() {
                total -= headers[index];
            }
        }
        let cut: Vec<SectionCount> = answer
            .sections
            .iter()
            .filter(|section| section.lines.len() < section.total)
            .map(|section| SectionCount {
                name: section.name.clone(),
                shown: section.lines.len(),
                total: section.total,
            })
            .collect();
        answer.truncated = (!cut.is_empty()).then(|| Truncation {
            sections: cut,
            hint: hint.to_owned(),
        });
    }
}
