use crate::driver::LanguageHooks;

pub(super) struct RustHooks;

impl LanguageHooks for RustHooks {
    fn impl_container(&self, self_type: &str, trait_text: Option<&str>) -> String {
        match trait_text {
            Some(trait_text) => format!("<{self_type} as {trait_text}>"),
            None if self_type.contains("::") => format!("<{self_type}>"),
            None => self_type.to_owned(),
        }
    }

    fn is_doc_comment(&self, text: &str) -> bool {
        (text.starts_with("///") && !text.starts_with("////"))
            || (text.starts_with("/**") && !text.starts_with("/***") && !text.starts_with("/**/"))
    }

    fn doc_skip_kinds(&self) -> &'static [&'static str] {
        &["attribute_item"]
    }
}
