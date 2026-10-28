use std::collections::HashMap;
use std::ops::Range;

/// Local bindings by scope.
#[derive(Default)]
pub(super) struct Locals {
    scopes: Vec<Range<usize>>,
    /// Per scope index: binding name → start bytes of its definitions.
    definitions: HashMap<usize, HashMap<String, Vec<usize>>>,
    /// Initialisers, each with the start of the statement that owns it.
    exclusions: Vec<(Range<usize>, usize)>,
}

impl Locals {
    pub fn new(
        mut scopes: Vec<Range<usize>>,
        definitions: Vec<(Range<usize>, String)>,
        exclusions: Vec<(Range<usize>, usize)>,
    ) -> Self {
        scopes.sort_by_key(|scope| (scope.start, std::cmp::Reverse(scope.end)));
        scopes.dedup();
        let mut locals = Self {
            scopes,
            definitions: HashMap::new(),
            exclusions,
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
    /// later than the site, ignoring definitions an enclosing initialiser hides from it.
    pub fn is_local(&self, name: &str, site: &Range<usize>) -> bool {
        self.scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| contains(scope, site))
            .filter_map(|(index, _)| self.definitions.get(&index)?.get(name))
            .flatten()
            .any(|&start| start <= site.start && !self.hidden(start, site))
    }

    /// A definition made in a statement before that statement's initialiser is not visible
    /// inside the initialiser (`let x = x();`, `for x in x() {}`).
    fn hidden(&self, definition: usize, site: &Range<usize>) -> bool {
        self.exclusions.iter().any(|(initialiser, statement)| {
            contains(initialiser, site)
                && *statement <= definition
                && definition < initialiser.start
        })
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
