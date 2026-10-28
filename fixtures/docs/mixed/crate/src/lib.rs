pub struct Store {
    pub path: String,
}

pub fn open_store(path: &str) -> Store {
    Store {
        path: path.to_owned(),
    }
}

pub fn shared() {}
