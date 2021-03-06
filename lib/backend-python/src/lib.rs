#[macro_use]
extern crate genco;
#[macro_use]
extern crate log;
extern crate reproto_backend as backend;
extern crate reproto_core as core;
#[macro_use]
extern crate reproto_manifest as manifest;
extern crate reproto_naming as naming;
extern crate reproto_trans as trans;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate toml;

mod codegen;
mod compiler;
mod python_field;
mod python_file_spec;
mod module;
mod utils;

use backend::Initializer;
use codegen::ServiceCodegen;
use compiler::Compiler;
use core::Context;
use core::errors::*;
use manifest::{Lang, Manifest, NoModule, TryFromToml};
use std::any::Any;
use std::path::Path;
use std::rc::Rc;
use trans::Environment;

const TYPE_SEP: &str = "_";
const INIT_PY: &str = "__init__.py";
const EXT: &str = "py";
const PYTHON_CONTEXT: &str = "python";

#[derive(Clone, Copy, Default, Debug)]
pub struct PythonLang;

impl Lang for PythonLang {
    lang_base!(PythonModule, compile);

    fn comment(&self, input: &str) -> Option<String> {
        Some(format!("# {}", input))
    }

    fn keywords(&self) -> Vec<(&'static str, &'static str)> {
        // NB: combined set of keywords for Python 2/3 to avoid having two codegen implementations.
        vec![
            ("and", "_and"),
            ("as", "_as"),
            ("assert", "_assert"),
            ("break", "_break"),
            ("class", "_class"),
            ("continue", "_continue"),
            ("def", "_def"),
            ("del", "_del"),
            ("elif", "_elif"),
            ("else", "_else"),
            ("except", "_except"),
            ("exec", "_exec"),
            ("finally", "_finally"),
            ("for", "_for"),
            ("from", "_from"),
            ("global", "_global"),
            ("if", "_if"),
            ("import", "_import"),
            ("in", "_in"),
            ("is", "_is"),
            ("lambda", "_lambda"),
            ("nonlocal", "_nonlocal"),
            ("not", "_not"),
            ("or", "_or"),
            ("pass", "_pass"),
            ("print", "_print"),
            ("raise", "_raise"),
            ("return", "_return"),
            ("try", "_try"),
            ("while", "_while"),
            ("with", "_with"),
            ("yield", "_yield"),
        ]
    }
}

#[derive(Debug)]
pub enum PythonModule {
    Requests(module::RequestsConfig),
}

impl TryFromToml for PythonModule {
    fn try_from_string(path: &Path, id: &str, value: String) -> Result<Self> {
        use self::PythonModule::*;

        let result = match id {
            "requests" => Requests(module::RequestsConfig::default()),
            _ => return NoModule::illegal(path, id, value),
        };

        Ok(result)
    }

    fn try_from_value(path: &Path, id: &str, value: toml::Value) -> Result<Self> {
        use self::PythonModule::*;

        let result = match id {
            "requests" => Requests(value.try_into()?),
            _ => return NoModule::illegal(path, id, value),
        };

        Ok(result)
    }
}

pub struct Options {
    pub build_getters: bool,
    pub build_constructor: bool,
    pub service_generators: Vec<Box<ServiceCodegen>>,
}

impl Options {
    pub fn new() -> Options {
        Options {
            build_getters: true,
            build_constructor: true,
            service_generators: Vec::new(),
        }
    }
}

pub fn setup_options(modules: Vec<PythonModule>) -> Result<Options> {
    use self::PythonModule::*;

    let mut options = Options::new();

    for module in modules {
        let initializer: Box<Initializer<Options = Options>> = match module {
            Requests(config) => Box::new(module::Requests::new(config)),
        };

        initializer.initialize(&mut options)?;
    }

    Ok(options)
}

fn compile(ctx: Rc<Context>, env: Environment, manifest: Manifest) -> Result<()> {
    let modules = manifest::checked_modules(manifest.modules)?;
    let options = setup_options(modules)?;
    let handle = ctx.filesystem(manifest.output.as_ref().map(AsRef::as_ref))?;
    Compiler::new(&env, options, handle.as_ref()).compile()
}
