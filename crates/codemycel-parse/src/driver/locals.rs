use std::collections::HashMap;
use std::ops::Range;

/// Local bindings by scope (spec §6.5).
#[derive(Default)]
pub(super) struct Locals {
    scopes: Vec<Range<usize>>,
    /// Per scope index: binding name → start bytes of its definitions.
    definitions: HashMap<usize, HashMap<String, Vec<usize>>>,
}

impl Locals {
    pub fn new(mut scopes: Vec<Range<usize>>, definitions: Vec<(Range<usize>, String)>) -> Self {
        scopes.sort_by_key(|scope| (scope.start, std::cmp::Reverse(scope.end)));
        scopes.dedup();
        let mut locals = Self {
            scopes,
            definitions: HashMap::new(),
        };
        for (range, name) in definitions {
            if let Some(scope) = locals.innermost(&range) {
                locals
                    .definitions
                    .entry(scope)
                    .or_default()
                    .entry(name)
                    .or_default()
                    .push(range.start);
            }
        }
        locals
    }

    /// Whether `name` at `site` resolves to a definition in a scope enclosing it that starts no
    /// later than the site.
    pub fn is_local(&self, name: &str, site: &Range<usize>) -> bool {
        self.scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| contains(scope, site))
            .filter_map(|(index, _)| self.definitions.get(&index)?.get(name))
            .any(|starts| starts.iter().any(|&start| start <= site.start))
    }

    fn innermost(&self, range: &Range<usize>) -> Option<usize> {
        self.scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| contains(scope, range))
            .max_by_key(|(_, scope)| (scope.start, std::cmp::Reverse(scope.end)))
            .map(|(index, _)| index)
    }
}

fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
