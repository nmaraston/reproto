use backend::*;
use super::processor;

pub struct Module {
}

impl Module {
    pub fn new() -> Module {
        Module {}
    }
}

impl processor::Listeners for Module {
    fn configure(&self, options: &mut processor::ProcessorOptions) -> Result<()> {
        options.nullable = true;
        Ok(())
    }
}
