use naming;

pub struct Options {
    pub id_converter: Option<Box<naming::Naming>>,
    pub modules: Vec<String>,
}
