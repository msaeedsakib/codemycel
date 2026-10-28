use crate::geometry::Area;

pub trait Render {
    fn render(&self) -> String;
}

pub trait Labelled: Render + Area {
    fn label(&self) -> String;
}

impl<T: Area> Render for T {
    fn render(&self) -> String {
        self.describe()
    }
}

pub struct Canvas {
    pub width: u32,
}

pub mod layout {
    pub mod grid {
        pub(super) const COLUMNS: u32 = 12;

        pub(in crate::render) fn columns() -> u32 {
            COLUMNS
        }
    }

    pub fn total_columns() -> u32 {
        grid::columns() * 2
    }

    impl super::Canvas {
        pub fn cells(&self) -> u32 {
            self.width * grid::COLUMNS
        }
    }
}
